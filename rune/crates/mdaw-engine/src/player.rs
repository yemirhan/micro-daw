//! The complete instrument of a synth track or the live keyboard (the TS `SynthPlayer` that
//! `createSynthFromPreset` returns), assembled from the parts of `synth`, `piano` and
//! `character`:
//!
//! ```text
//! subtractive / fm / pluck:  PolyInstrument voices ─→ CharacterChain(character, gain) ─→ out
//! sampler (presets 0, 15):   SampledPiano: Tone.Sampler sources ──────────────┐
//!                            FALLBACK_PIANO voices → its CharacterChain ──────┴→ ×2 → CharacterChain → out
//! ```
//!
//! - The preset is resolved by the caller (`resolve_preset(presetIndex, synthOverrides)`: an
//!   unknown index is preset 0, like `presetAt`); the chain uses the preset's `character` and
//!   `gain` ([`crate::character::CharacterParams`], the one type both sides use).
//! - A sampler preset plays the FM fallback piano until the shared [`PianoSlot`] holds decoded
//!   samples, then the samples (the slot is polled wait-free on every call; nothing to install).
//!   Note-offs of fallback notes go back to the fallback.
//! - `instrument.set(patch)` with the merged preset (voice parameters, chain and gain live,
//!   without rebuilding; same engine only) is split in two: [`SynthPatch::prepare`] on the
//!   control side builds everything that allocates (the preset, wave tables, noise buffers,
//!   fresh voices for the unused slots), [`SynthPlayer::apply_patch`] swaps it in on the audio
//!   thread and leaves what it replaced in the patch, which returns through the garbage ring
//!   (`Command::SynthSet`). [`SynthPlayer::set`] does both at once (control side, tests).
//! - Live status ([`SynthStatus`]): an instrument built with one publishes its active voice
//!   count, held notes and used voices after every quantum (a seqlock: one consistent
//!   snapshot), so the control side can answer `getActiveVoiceCount` / `getHeldNotes` while the
//!   instrument lives in the core. Only the slot's current instrument publishes: a replaced one
//!   stops at `retire` (it keeps ringing out on the slot). The held set is kept incrementally on
//!   note events (no per-quantum scan of the 128 notes).
//!
//! Note events, `apply_patch` and `process` never allocate (the voice pools, the chains' stages
//! and the piano's source pool are all allocated in `new`).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering, fence};

use crate::character::CharacterChain;
use crate::contract::{Instrument, RENDER_QUANTUM};
use crate::core::SynthInstrument;
use crate::piano::{PianoFallback, PianoParams, PianoSlot, SampledPiano};
use crate::synth::bank::fallback_piano;
use crate::synth::presets::SynthPreset;
use crate::synth::{DEFAULT_NOISE_SEED, EnginePatch, SynthEngine};

const Q: usize = RENDER_QUANTUM;

/// A voice engine followed by its `CharacterChain`.
struct Chained {
    engine: SynthEngine,
    chain: CharacterChain,
    bus: Box<[[f32; Q]; 2]>,
}

impl Chained {
    fn new(preset: &SynthPreset, sr: u32, now: f64, noise_seed: u32, offline: bool) -> Self {
        let m = preset.meta();
        let mut engine = SynthEngine::new(preset, sr as f64, now, noise_seed);
        if offline {
            engine.set_offline();
        }
        Self {
            engine,
            chain: CharacterChain::new(sr, m.character, m.gain, now),
            bus: Box::new([[0.0; Q]; 2]),
        }
    }

    fn process(&mut self, frame: u64, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len()).min(Q);
        let [bl, br] = &mut *self.bus;
        bl[..n].fill(0.0);
        br[..n].fill(0.0);
        self.engine.process(frame, &mut bl[..n], &mut br[..n]);
        self.chain.process(frame, &mut bl[..n], &mut br[..n]);
        for k in 0..n {
            left[k] += bl[k];
            right[k] += br[k];
        }
    }
}

/// `FALLBACK_PIANO` as `SampledPiano` builds it: a `PolyInstrument<FMPreset>` with its own
/// `CharacterChain` (gain −11 dB), feeding the sampled piano's makeup and chain.
pub struct FallbackPiano(Chained);

impl FallbackPiano {
    pub fn new(sr: u32, now: f64, noise_seed: u32) -> Self {
        Self::build(sr, now, noise_seed, false)
    }

    fn build(sr: u32, now: f64, noise_seed: u32, offline: bool) -> Self {
        let p = SynthPreset::Fm(fallback_piano().clone());
        Self(Chained::new(&p, sr, now, noise_seed, offline))
    }
}

impl Instrument for FallbackPiano {
    fn process(&mut self, frame: u64, left: &mut [f32], right: &mut [f32]) {
        self.0.process(frame, left, right);
    }
}

impl PianoFallback for FallbackPiano {
    fn note_on(&mut self, midi: i32, velocity: f64, time: f64) {
        self.0.engine.note_on_midi(midi, velocity, time);
    }
    fn note_off(&mut self, midi: i32, time: f64) {
        self.0.engine.note_off_midi(midi, time);
    }
    fn attack_release(&mut self, midi: i32, duration: f64, time: f64, velocity: f64) {
        self.0
            .engine
            .trigger_attack_release(midi, duration, time, velocity);
    }
    fn release_all(&mut self, time: f64) {
        SynthInstrument::release_all(&mut self.0.engine, time);
    }
    fn cut_scheduled(&mut self, midi: i32, time: f64) {
        self.0.engine.cut_scheduled_midi(midi, time);
    }
    fn active_voice_count(&self, time: f64) -> usize {
        self.0.engine.active_voice_count(time)
    }
    fn is_held(&self, midi: i32) -> bool {
        self.0.engine.is_held(midi)
    }
}

/// What the control side can read about an instrument living in the core (published after
/// every rendered quantum by the slot's current instrument, built with
/// [`SynthPlayer::with_status`]). A seqlock: [`SynthStatus::snapshot`] is always one quantum's
/// consistent state.
#[derive(Debug, Default)]
pub struct SynthStatus {
    seq: AtomicU64,
    instance: AtomicU64,
    active: AtomicU64,
    used: AtomicU64,
    held: [AtomicU64; 2],
}

/// One consistent [`SynthStatus`] reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StatusSnapshot {
    /// The publishing instrument (the id given to [`SynthPlayer::with_status`]); 0 before the
    /// first publish.
    pub instance: u64,
    /// Voices held or still releasing at the end of the last rendered quantum.
    pub active: usize,
    /// Voices of the engine used so far (`getPoolSize`).
    pub used: usize,
    /// Held MIDI notes as a bitset.
    pub held: [u64; 2],
}

impl StatusSnapshot {
    /// Held MIDI notes, ascending (allocates: control side).
    pub fn held_notes(&self) -> Vec<i32> {
        (0..128)
            .filter(|&m| self.held[m / 64] >> (m % 64) & 1 == 1)
            .map(|m| m as i32)
            .collect()
    }
}

impl SynthStatus {
    /// The last published state (wait-free for the writer; the reader retries while a publish
    /// is in progress).
    pub fn snapshot(&self) -> StatusSnapshot {
        loop {
            let s1 = self.seq.load(Ordering::Acquire);
            if s1 & 1 == 1 {
                std::hint::spin_loop();
                continue;
            }
            let snap = StatusSnapshot {
                instance: self.instance.load(Ordering::Relaxed),
                active: self.active.load(Ordering::Relaxed) as usize,
                used: self.used.load(Ordering::Relaxed) as usize,
                held: [
                    self.held[0].load(Ordering::Relaxed),
                    self.held[1].load(Ordering::Relaxed),
                ],
            };
            fence(Ordering::Acquire);
            if self.seq.load(Ordering::Relaxed) == s1 {
                return snap;
            }
        }
    }

    /// Voices held or still releasing at the end of the last rendered quantum.
    pub fn active_voice_count(&self) -> usize {
        self.snapshot().active
    }

    /// Held MIDI notes, ascending (allocates: control side).
    pub fn held_notes(&self) -> Vec<i32> {
        self.snapshot().held_notes()
    }

    fn publish(&self, snap: StatusSnapshot) {
        let s = self.seq.load(Ordering::Relaxed);
        self.seq.store(s.wrapping_add(1), Ordering::Relaxed);
        fence(Ordering::Release);
        self.instance.store(snap.instance, Ordering::Relaxed);
        self.active.store(snap.active as u64, Ordering::Relaxed);
        self.used.store(snap.used as u64, Ordering::Relaxed);
        self.held[0].store(snap.held[0], Ordering::Relaxed);
        self.held[1].store(snap.held[1], Ordering::Relaxed);
        self.seq.store(s.wrapping_add(2), Ordering::Release);
    }
}

/// A prepared `instrument.set(patch)` (see the module docs and `Command::SynthSet`).
pub struct SynthPatch {
    preset: SynthPreset,
    engine: EnginePatch,
}

impl SynthPatch {
    /// Prepare `set(preset)` (control side: allocates) for an instrument at `sr` whose first
    /// `used` engine voices may be in use (a lower bound; 0 when unknown: every voice slot then
    /// gets a fresh voice). `now`: context time.
    pub fn prepare(preset: SynthPreset, sr: u32, now: f64, used: usize) -> Self {
        let engine = SynthEngine::prepare_patch(&preset, sr as f64, now, used);
        Self { preset, engine }
    }

    /// The merged preset (after an apply: the instrument's previous one).
    pub fn preset(&self) -> &SynthPreset {
        &self.preset
    }
}

enum Kind {
    Poly(Box<Chained>),
    Piano(Box<SampledPiano>),
}

/// See the module docs.
pub struct SynthPlayer {
    preset: SynthPreset,
    sr: u32,
    kind: Kind,
    /// The status it publishes to, and its instance id there (None once retired).
    status: Option<(Arc<SynthStatus>, u64)>,
    retired: bool,
    /// Held notes, kept on every note event.
    held: [u64; 2],
}

impl SynthPlayer {
    /// `createSynthFromPreset(preset)` at context time `now` (control side: allocates). A sampler
    /// preset reads its samples from `piano` (empty: the FM fallback plays). `noise_seed` seeds
    /// the instrument's noise start offsets ([`DEFAULT_NOISE_SEED`] for tracks).
    pub fn new(
        preset: &SynthPreset,
        sr: u32,
        now: f64,
        piano: &PianoSlot,
        noise_seed: u32,
    ) -> Self {
        Self::build(preset, sr, now, piano, noise_seed, false)
    }

    /// The same instrument for the WAV export (`Tone.Offline`): Tone's clock hands every note out
    /// at the next accumulated block time and every node exists before rendering starts (see
    /// the `synth::poly` docs). The sampled piano's own sources are not clamped by Tone
    /// (`ToneBufferSource` starts at the event time), so only the voices change.
    pub fn new_offline(preset: &SynthPreset, sr: u32, piano: &PianoSlot, noise_seed: u32) -> Self {
        Self::build(preset, sr, 0.0, piano, noise_seed, true)
    }

    fn build(
        preset: &SynthPreset,
        sr: u32,
        now: f64,
        piano: &PianoSlot,
        noise_seed: u32,
        offline: bool,
    ) -> Self {
        let kind = match preset {
            SynthPreset::Sampler(p) => Kind::Piano(Box::new(SampledPiano::new(
                sr,
                PianoParams::from(p),
                Box::new(FallbackPiano::build(sr, now, noise_seed, offline)),
                piano.clone(),
                now,
            ))),
            _ => Kind::Poly(Box::new(Chained::new(preset, sr, now, noise_seed, offline))),
        };
        Self {
            preset: preset.clone(),
            sr,
            kind,
            status: None,
            retired: false,
            held: [0; 2],
        }
    }

    /// The same, publishing its status to `status` as `instance` (the live instrument; give
    /// every instrument of one status its own non-zero id).
    pub fn with_status(mut self, status: Arc<SynthStatus>, instance: u64) -> Self {
        self.status = Some((status, instance));
        self
    }

    /// Engine voices used so far (`getPoolSize`; 0 for a sampler preset).
    pub fn pool_size(&self) -> usize {
        match &self.kind {
            Kind::Poly(c) => c.engine.pool_size(),
            Kind::Piano(_) => 0,
        }
    }

    /// Update the held bitset after a note event on `midi`: its own bit, and every held note
    /// (a steal or a mono hand-over may have released one). Allocation-free.
    fn refresh_held(&mut self, midi: Option<i32>) {
        if let Some(m) = midi.filter(|m| (0..128).contains(m)) {
            let (w, b) = (m as usize / 64, m as usize % 64);
            if self.is_held(m) {
                self.held[w] |= 1 << b;
            } else {
                self.held[w] &= !(1 << b);
            }
        }
        for w in 0..2 {
            let mut bits = self.held[w];
            while bits != 0 {
                let b = bits.trailing_zeros() as usize;
                bits &= bits - 1;
                if !self.is_held((w * 64 + b) as i32) {
                    self.held[w] &= !(1 << b);
                }
            }
        }
    }

    /// Export only (after [`Self::new_offline`]): every note of the render, `(midi, duration,
    /// time, velocity)` in time order, and the ones Tone's clock fires a tick early, `(time,
    /// tick time)` (see `SynthEngine::prepare_offline`).
    pub fn prepare_offline(&mut self, notes: &[(i32, f64, f64, f64)], dispatch: &[(f64, f64)]) {
        if let Kind::Poly(c) = &mut self.kind {
            c.engine.prepare_offline(notes, dispatch, self.sr as f64);
        }
    }

    /// The preset the instrument plays (with every `set` merged in).
    pub fn preset(&self) -> &SynthPreset {
        &self.preset
    }

    pub fn sample_rate(&self) -> u32 {
        self.sr
    }

    /// True once a sampler preset plays real samples (`isSampled`); false for the other engines.
    pub fn is_sampled(&self) -> bool {
        matches!(&self.kind, Kind::Piano(p) if p.is_sampled())
    }

    /// `instrument.set(patch)` with the merged preset at context time `now`, prepared and
    /// applied at once (control side, tests). False (nothing changed) when the engine differs.
    pub fn set(&mut self, preset: &SynthPreset, now: f64) -> bool {
        let mut patch = SynthPatch::prepare(preset.clone(), self.sr, now, self.pool_size());
        self.apply_patch(&mut patch, now)
    }

    /// Apply a prepared `set` at context time `now` (audio thread: allocation-free; the patch
    /// then holds the previous preset and whatever voices it replaced). False when the engine
    /// differs (the patch is unchanged).
    pub fn apply_patch(&mut self, patch: &mut SynthPatch, now: f64) -> bool {
        let ok = match (&mut self.kind, &patch.preset) {
            (Kind::Piano(p), SynthPreset::Sampler(s)) => {
                p.set_params(PianoParams::from(s), now);
                true
            }
            (Kind::Poly(c), preset) => {
                let ok = c.engine.apply_patch(&mut patch.engine);
                if ok {
                    let m = preset.meta();
                    c.chain.update(m.character, m.gain, now);
                }
                ok
            }
            _ => false,
        };
        if ok {
            std::mem::swap(&mut self.preset, &mut patch.preset);
        }
        ok
    }

    /// Quanta whose voice output went non-finite and was cleared (the output guard).
    pub fn non_finite_quanta(&self) -> u64 {
        match &self.kind {
            Kind::Poly(c) => c.engine.non_finite_quanta(),
            // the fallback piano's guard is internal; the samples are finite buffers
            Kind::Piano(_) => 0,
        }
    }

    pub fn note_on_midi(&mut self, midi: i32, velocity: f64, time: f64) {
        match &mut self.kind {
            Kind::Poly(c) => c.engine.note_on_midi(midi, velocity, time),
            Kind::Piano(p) => p.note_on(midi, velocity, time),
        }
        self.refresh_held(Some(midi));
    }

    pub fn note_off_midi(&mut self, midi: i32, time: f64) {
        match &mut self.kind {
            Kind::Poly(c) => c.engine.note_off_midi(midi, time),
            Kind::Piano(p) => p.note_off(midi, time),
        }
        self.refresh_held(Some(midi));
    }

    pub fn active_voice_count(&self, time: f64) -> usize {
        match &self.kind {
            Kind::Poly(c) => c.engine.active_voice_count(time),
            Kind::Piano(p) => p.active_voice_count(time),
        }
    }

    pub fn is_held(&self, midi: i32) -> bool {
        match &self.kind {
            Kind::Poly(c) => c.engine.is_held(midi),
            Kind::Piano(p) => p.is_held(midi),
        }
    }

    /// `getHeldNotes()` ascending (control side: allocates).
    pub fn held_notes(&self) -> Vec<i32> {
        match &self.kind {
            Kind::Poly(c) => c.engine.held_notes(),
            Kind::Piano(p) => p.held_notes(),
        }
    }
}

impl Instrument for SynthPlayer {
    fn process(&mut self, frame: u64, left: &mut [f32], right: &mut [f32]) {
        match &mut self.kind {
            Kind::Poly(c) => c.process(frame, left, right),
            Kind::Piano(p) => p.process(frame, left, right),
        }
        if let Some((s, instance)) = self.status.as_ref().filter(|_| !self.retired) {
            let now = (frame + left.len() as u64) as f64 / self.sr as f64;
            s.publish(StatusSnapshot {
                instance: *instance,
                active: self.active_voice_count(now),
                used: self.pool_size(),
                held: self.held,
            });
        }
    }

    fn clear_non_finite(&mut self) {
        match &mut self.kind {
            Kind::Poly(c) => c.engine.clear_non_finite(),
            Kind::Piano(_) => {}
        }
    }
}

impl SynthInstrument for SynthPlayer {
    /// MIDI note numbers (rounded, like `midiToNoteName` → `noteToMidi`).
    fn note_on(&mut self, note: f64, velocity: f64, time: f64) {
        self.note_on_midi(note.round() as i32, velocity, time);
    }

    fn note_off(&mut self, note: f64, time: f64) {
        self.note_off_midi(note.round() as i32, time);
    }

    fn release_all(&mut self, time: f64) {
        match &mut self.kind {
            Kind::Poly(c) => SynthInstrument::release_all(&mut c.engine, time),
            Kind::Piano(p) => p.release_all(time),
        }
        self.refresh_held(None);
    }

    fn cut_scheduled(&mut self, note: f64, time: f64) {
        let midi = note.round() as i32;
        match &mut self.kind {
            Kind::Poly(c) => c.engine.cut_scheduled_midi(midi, time),
            Kind::Piano(p) => p.cut_scheduled(midi, time),
        }
        self.refresh_held(Some(midi));
    }

    fn trigger_attack_release(&mut self, note: f64, duration: f64, time: f64, velocity: f64) {
        let midi = note.round() as i32;
        match &mut self.kind {
            Kind::Poly(c) => c
                .engine
                .trigger_attack_release(midi, duration, time, velocity),
            Kind::Piano(p) => p.trigger_attack_release(midi, duration, time, velocity),
        }
        self.refresh_held(Some(midi));
    }

    fn apply_patch(&mut self, patch: &mut SynthPatch, time: f64) -> bool {
        SynthPlayer::apply_patch(self, patch, time)
    }

    fn retire(&mut self) {
        self.retired = true;
    }
}

/// A track's instrument: `resolvePreset(presetIndex, synthOverrides)` built at `now`.
pub fn track_synth(
    preset_index: i64,
    overrides: Option<&serde_json::Value>,
    sr: u32,
    now: f64,
    piano: &PianoSlot,
) -> SynthPlayer {
    let preset = crate::synth::presets::resolve_preset(preset_index as f64, overrides);
    SynthPlayer::new(&preset, sr, now, piano, DEFAULT_NOISE_SEED)
}

#[cfg(test)]
#[path = "player_tests.rs"]
mod tests;
