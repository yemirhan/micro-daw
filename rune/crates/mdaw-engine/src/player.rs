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
//! - [`SynthPlayer::set`] is `instrument.set(patch)` with the merged preset: voice parameters,
//!   chain and gain live, without rebuilding (same engine only).
//! - Live status ([`SynthStatus`]): an instrument built with one publishes its active voice
//!   count and held notes after every quantum, so the control side can answer
//!   `getActiveVoiceCount` / `getHeldNotes` while the instrument lives in the core.
//!
//! Note events and `process` never allocate (the voice pools, the chains' stages and the piano's
//! source pool are all allocated in `new`); `set` may (control-side data, see `SynthSet`).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use crate::character::CharacterChain;
use crate::contract::{Instrument, RENDER_QUANTUM};
use crate::core::SynthInstrument;
use crate::piano::{PianoFallback, PianoParams, PianoSlot, SampledPiano};
use crate::synth::bank::fallback_piano;
use crate::synth::presets::SynthPreset;
use crate::synth::{DEFAULT_NOISE_SEED, SynthEngine};

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
    fn active_voice_count(&self, time: f64) -> usize {
        self.0.engine.active_voice_count(time)
    }
    fn is_held(&self, midi: i32) -> bool {
        self.0.engine.is_held(midi)
    }
}

/// What the control side can read about an instrument living in the core (published after
/// every rendered quantum by an instrument built with [`SynthPlayer::with_status`]).
#[derive(Debug, Default)]
pub struct SynthStatus {
    active: AtomicUsize,
    held: [AtomicU64; 2],
}

impl SynthStatus {
    /// Voices held or still releasing at the end of the last rendered quantum.
    pub fn active_voice_count(&self) -> usize {
        self.active.load(Ordering::Acquire)
    }

    /// Held MIDI notes, ascending (allocates: control side).
    pub fn held_notes(&self) -> Vec<i32> {
        let words = [
            self.held[0].load(Ordering::Acquire),
            self.held[1].load(Ordering::Acquire),
        ];
        (0..128)
            .filter(|&m| words[m / 64] >> (m % 64) & 1 == 1)
            .map(|m| m as i32)
            .collect()
    }

    fn publish(&self, active: usize, held: impl Fn(i32) -> bool) {
        let mut words = [0u64; 2];
        for m in 0..128usize {
            if held(m as i32) {
                words[m / 64] |= 1 << (m % 64);
            }
        }
        self.held[0].store(words[0], Ordering::Release);
        self.held[1].store(words[1], Ordering::Release);
        self.active.store(active, Ordering::Release);
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
    status: Option<Arc<SynthStatus>>,
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
        }
    }

    /// The same, publishing its status to `status` (the live instrument).
    pub fn with_status(mut self, status: Arc<SynthStatus>) -> Self {
        self.status = Some(status);
        self
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

    /// `instrument.set(patch)` with the merged preset at context time `now`. False (nothing
    /// changed) when the engine differs.
    pub fn set(&mut self, preset: &SynthPreset, now: f64) -> bool {
        let ok = match (&mut self.kind, preset) {
            (Kind::Piano(p), SynthPreset::Sampler(s)) => {
                p.set_params(PianoParams::from(s), now);
                true
            }
            (Kind::Poly(c), _) => {
                let ok = c.engine.set_preset(preset);
                if ok {
                    let m = preset.meta();
                    c.chain.update(m.character, m.gain, now);
                }
                ok
            }
            _ => false,
        };
        if ok {
            self.preset = preset.clone();
        }
        ok
    }

    pub fn note_on_midi(&mut self, midi: i32, velocity: f64, time: f64) {
        match &mut self.kind {
            Kind::Poly(c) => c.engine.note_on_midi(midi, velocity, time),
            Kind::Piano(p) => p.note_on(midi, velocity, time),
        }
    }

    pub fn note_off_midi(&mut self, midi: i32, time: f64) {
        match &mut self.kind {
            Kind::Poly(c) => c.engine.note_off_midi(midi, time),
            Kind::Piano(p) => p.note_off(midi, time),
        }
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
        if let Some(s) = &self.status {
            let now = (frame + left.len() as u64) as f64 / self.sr as f64;
            s.publish(self.active_voice_count(now), |m| self.is_held(m));
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
    }

    fn trigger_attack_release(&mut self, note: f64, duration: f64, time: f64, velocity: f64) {
        let midi = note.round() as i32;
        match &mut self.kind {
            Kind::Poly(c) => c
                .engine
                .trigger_attack_release(midi, duration, time, velocity),
            Kind::Piano(p) => p.trigger_attack_release(midi, duration, time, velocity),
        }
    }

    fn set_preset(&mut self, preset: &SynthPreset, time: f64) -> bool {
        self.set(preset, time)
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
