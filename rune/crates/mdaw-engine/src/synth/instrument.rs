//! The track synth (`src/utils/synthFactory.ts` `createSynthFromPreset`): engine → instrument.
//!
//! - subtractive / fm / pluck → [`PolyInstrument`] with the engine's voices;
//! - sampler (presets 0 and 15): the synthesized `FALLBACK_PIANO` (FM) on its own. The sampled
//!   piano itself (`SampledPiano`: samples, allocator, makeup, its chain, this fallback in front)
//!   is assembled by [`crate::player::SynthPlayer`].
//!
//! Output is the RAW voice bus (before `CharacterChain`, which [`crate::player::SynthPlayer`]
//! chains): [`Instrument::process`] adds it into the bus.

use std::sync::Arc;

use crate::contract::Instrument;
use crate::core::SynthInstrument;

use super::bank::fallback_piano;
use super::fm::{FmPrep, FmVoice};
use super::pluck::PluckVoice;
use super::poly::{PolyInstrument, PolyPatch, PolyPreset};
use super::presets::{FmPreset, LfoParams, PluckPreset, SubtractivePreset, SynthPreset};
use super::subtractive::{SubtractivePrep, SubtractiveVoice};
use super::wa::noise::NoiseSeeds;
use super::wa::param::Clock;

/// Default mulberry32 seed of an instrument's noise start offsets.
pub const DEFAULT_NOISE_SEED: u32 = 0x5eed_3000;

impl PolyPreset for SubtractivePreset {
    type V = SubtractiveVoice;
    fn polyphony(&self) -> f64 {
        self.meta.polyphony
    }
    fn lfo(&self) -> LfoParams {
        self.lfo.clone()
    }
    fn mono(&self) -> Option<bool> {
        self.mono.then_some(self.legato)
    }
    fn release_tail(&self) -> f64 {
        self.amp.release
    }
    fn prepare(&self, sr: f64) -> SubtractivePrep {
        SubtractivePrep::new(self, sr)
    }
    fn make_voice(p: &Arc<Self>, prep: &SubtractivePrep, sr: f64, cx: Clock) -> SubtractiveVoice {
        SubtractiveVoice::new(p, prep, sr, cx)
    }
}

impl PolyPreset for FmPreset {
    type V = FmVoice;
    fn polyphony(&self) -> f64 {
        self.meta.polyphony
    }
    fn lfo(&self) -> LfoParams {
        self.lfo.clone()
    }
    fn mono(&self) -> Option<bool> {
        None
    }
    fn release_tail(&self) -> f64 {
        self.amp.release
    }
    fn prepare(&self, sr: f64) -> FmPrep {
        FmPrep::new(self, sr)
    }
    fn make_voice(p: &Arc<Self>, _prep: &FmPrep, sr: f64, cx: Clock) -> FmVoice {
        FmVoice::new(p, sr, cx)
    }
}

impl PolyPreset for PluckPreset {
    type V = PluckVoice;
    fn polyphony(&self) -> f64 {
        self.meta.polyphony
    }
    /// Pluck presets have no LFO (`NO_LFO`).
    fn lfo(&self) -> LfoParams {
        LfoParams {
            rate: 5.0,
            vibrato: 0.0,
            filter: 0.0,
            delay: 0.0,
        }
    }
    fn mono(&self) -> Option<bool> {
        None
    }
    fn release_tail(&self) -> f64 {
        self.release
    }
    fn prepare(&self, _sr: f64) {}
    fn make_voice(p: &Arc<Self>, _prep: &(), sr: f64, cx: Clock) -> PluckVoice {
        PluckVoice::new(p, sr, NoiseSeeds::default(), cx)
    }
}

#[allow(clippy::large_enum_variant)] // one per track, built on the control side
enum Engine {
    Subtractive(PolyInstrument<SubtractivePreset>),
    Fm(PolyInstrument<FmPreset>),
    Pluck(PolyInstrument<PluckPreset>),
}

/// A prepared live `set(params)` for a [`SynthEngine`] (see [`SynthEngine::prepare_patch`]).
pub struct EnginePatch(PatchKind);

#[allow(clippy::large_enum_variant)] // control-side payload, boxed in its command
enum PatchKind {
    Subtractive(PolyPatch<SubtractivePreset>),
    Fm(PolyPatch<FmPreset>),
    Pluck(PolyPatch<PluckPreset>),
    /// A sampler preset: nothing for the voice engine (the fallback piano never changes).
    None,
}

/// One track synth: the instrument `createSynthFromPreset(preset)` builds.
pub struct SynthEngine(Engine);

impl SynthEngine {
    /// Build the instrument for `preset` at context time `now` (control side: allocates the
    /// voice pool and wave tables). `noise_seed` seeds the noise start offsets.
    pub fn new(preset: &SynthPreset, sr: f64, now: f64, noise_seed: u32) -> Self {
        SynthEngine(match preset {
            SynthPreset::Subtractive(p) => {
                Engine::Subtractive(PolyInstrument::new(p.clone(), sr, now, noise_seed))
            }
            SynthPreset::Fm(p) => Engine::Fm(PolyInstrument::new(p.clone(), sr, now, noise_seed)),
            SynthPreset::Pluck(p) => {
                Engine::Pluck(PolyInstrument::new(p.clone(), sr, now, noise_seed))
            }
            SynthPreset::Sampler(_) => Engine::Fm(PolyInstrument::new(
                fallback_piano().clone(),
                sr,
                now,
                noise_seed,
            )),
        })
    }

    /// The FALLBACK_PIANO instrument on its own (what a sampler preset plays before its samples).
    pub fn fallback_piano(sr: f64, now: f64, noise_seed: u32) -> Self {
        SynthEngine(Engine::Fm(PolyInstrument::new(
            fallback_piano().clone(),
            sr,
            now,
            noise_seed,
        )))
    }

    /// Render like `Tone.Offline` (the WAV export: Tone's clock runs ahead of the render; see
    /// the `poly` module docs).
    pub fn set_offline(&mut self) {
        match &mut self.0 {
            Engine::Subtractive(i) => i.set_offline(),
            Engine::Fm(i) => i.set_offline(),
            Engine::Pluck(i) => i.set_offline(),
        }
    }

    /// Export pre-pass with every note the render will play (`(midi, duration, time,
    /// velocity)`, in time order). `Tone.Offline` builds the whole graph before rendering, so a
    /// pluck voice plays every note through the comb lowpass of its LAST attack, and an attack
    /// cancels the previous note's release ramp when that ramp ends later: replay the
    /// allocation on a scratch instrument and hand each voice its final dampening and attack
    /// times. Other engines: nothing to do. `dispatch`: the notes Tone's clock fires a tick
    /// early (`PolyInstrument::set_offline_dispatch`).
    pub fn prepare_offline(
        &mut self,
        notes: &[(i32, f64, f64, f64)],
        dispatch: &[(f64, f64)],
        sr: f64,
    ) {
        match &mut self.0 {
            Engine::Subtractive(i) => i.set_offline_dispatch(dispatch.to_vec()),
            Engine::Fm(i) => i.set_offline_dispatch(dispatch.to_vec()),
            Engine::Pluck(i) => i.set_offline_dispatch(dispatch.to_vec()),
        }
        let Engine::Pluck(i) = &mut self.0 else {
            return;
        };
        let mut scratch = PolyInstrument::new(i.preset().clone(), sr, 0.0, 0);
        scratch.set_offline();
        scratch.set_offline_dispatch(dispatch.to_vec());
        for v in scratch.voices_mut() {
            v.record_attacks();
        }
        for &(midi, dur, time, vel) in notes {
            scratch.trigger_attack_release(midi, dur, time, vel);
        }
        for (v, s) in i.voices_mut().zip(scratch.used_voices()) {
            v.fix_dampening(s.last_dampening());
            v.set_offline_attacks(s.recorded_attacks().to_vec());
        }
    }

    pub fn note_on_midi(&mut self, midi: i32, velocity: f64, time: f64) {
        match &mut self.0 {
            Engine::Subtractive(i) => i.note_on(midi, velocity, time),
            Engine::Fm(i) => i.note_on(midi, velocity, time),
            Engine::Pluck(i) => i.note_on(midi, velocity, time),
        }
    }

    pub fn note_off_midi(&mut self, midi: i32, time: f64) {
        match &mut self.0 {
            Engine::Subtractive(i) => i.note_off(midi, time),
            Engine::Fm(i) => i.note_off(midi, time),
            Engine::Pluck(i) => i.note_off(midi, time),
        }
    }

    /// See [`PolyInstrument::cut_scheduled`].
    pub fn cut_scheduled_midi(&mut self, midi: i32, started: f64, time: f64) {
        match &mut self.0 {
            Engine::Subtractive(i) => i.cut_scheduled(midi, started, time),
            Engine::Fm(i) => i.cut_scheduled(midi, started, time),
            Engine::Pluck(i) => i.cut_scheduled(midi, started, time),
        }
    }

    /// `triggerAttackRelease(midi, duration, time, velocity)`.
    pub fn trigger_attack_release(&mut self, midi: i32, duration: f64, time: f64, velocity: f64) {
        match &mut self.0 {
            Engine::Subtractive(i) => i.trigger_attack_release(midi, duration, time, velocity),
            Engine::Fm(i) => i.trigger_attack_release(midi, duration, time, velocity),
            Engine::Pluck(i) => i.trigger_attack_release(midi, duration, time, velocity),
        }
    }

    pub fn active_voice_count(&self, time: f64) -> usize {
        match &self.0 {
            Engine::Subtractive(i) => i.active_voice_count(time),
            Engine::Fm(i) => i.active_voice_count(time),
            Engine::Pluck(i) => i.active_voice_count(time),
        }
    }

    pub fn held_notes(&self) -> Vec<i32> {
        match &self.0 {
            Engine::Subtractive(i) => i.held_notes(),
            Engine::Fm(i) => i.held_notes(),
            Engine::Pluck(i) => i.held_notes(),
        }
    }

    /// Whether `midi` is held (allocation-free).
    pub fn is_held(&self, midi: i32) -> bool {
        match &self.0 {
            Engine::Subtractive(i) => i.is_held(midi),
            Engine::Fm(i) => i.is_held(midi),
            Engine::Pluck(i) => i.is_held(midi),
        }
    }

    /// Voices used so far (`getPoolSize`; the fallback's for a sampler).
    pub fn pool_size(&self) -> usize {
        match &self.0 {
            Engine::Subtractive(i) => i.pool_size(),
            Engine::Fm(i) => i.pool_size(),
            Engine::Pluck(i) => i.pool_size(),
        }
    }

    /// `set(params)` with a merged preset of the same engine, prepared and applied at once
    /// (control side, tests). Returns false when the engine differs (the caller rebuilds the
    /// instrument, as the app does).
    pub fn set_preset(&mut self, preset: &SynthPreset) -> bool {
        let used = self.pool_size();
        let mut patch = Self::prepare_patch(preset, self.sample_rate(), self.now(), used);
        self.apply_patch(&mut patch)
    }

    fn sample_rate(&self) -> f64 {
        match &self.0 {
            Engine::Subtractive(i) => i.sample_rate(),
            Engine::Fm(i) => i.sample_rate(),
            Engine::Pluck(i) => i.sample_rate(),
        }
    }

    fn now(&self) -> f64 {
        match &self.0 {
            Engine::Subtractive(i) => i.now(),
            Engine::Fm(i) => i.now(),
            Engine::Pluck(i) => i.now(),
        }
    }

    /// Prepare `set(preset)` on the control side (allocates: the shared preset, its tables, and
    /// fresh voices for the slots from `used` on; see `PolyPatch::prepare`).
    pub fn prepare_patch(preset: &SynthPreset, sr: f64, now: f64, used: usize) -> EnginePatch {
        EnginePatch(match preset {
            SynthPreset::Subtractive(p) => {
                PatchKind::Subtractive(PolyPatch::prepare(p.clone(), sr, now, used))
            }
            SynthPreset::Fm(p) => PatchKind::Fm(PolyPatch::prepare(p.clone(), sr, now, used)),
            SynthPreset::Pluck(p) => PatchKind::Pluck(PolyPatch::prepare(p.clone(), sr, now, used)),
            SynthPreset::Sampler(_) => PatchKind::None,
        })
    }

    /// Apply a prepared patch (audio thread: allocation-free; what it replaces stays in the
    /// patch). False when the engine differs.
    pub fn apply_patch(&mut self, patch: &mut EnginePatch) -> bool {
        match (&mut self.0, &mut patch.0) {
            (Engine::Subtractive(i), PatchKind::Subtractive(p)) => i.apply(p),
            (Engine::Fm(i), PatchKind::Fm(p)) => i.apply(p),
            (Engine::Pluck(i), PatchKind::Pluck(p)) => i.apply(p),
            _ => return false,
        }
        true
    }

    /// Clear voice state that went non-finite.
    pub fn clear_non_finite(&mut self) {
        match &mut self.0 {
            Engine::Subtractive(i) => i.clear_non_finite(),
            Engine::Fm(i) => i.clear_non_finite(),
            Engine::Pluck(i) => i.clear_non_finite(),
        }
    }

    /// Quanta whose voice output went non-finite and was cleared.
    pub fn non_finite_quanta(&self) -> u64 {
        match &self.0 {
            Engine::Subtractive(i) => i.non_finite_quanta(),
            Engine::Fm(i) => i.non_finite_quanta(),
            Engine::Pluck(i) => i.non_finite_quanta(),
        }
    }
}

impl Instrument for SynthEngine {
    fn process(&mut self, frame: u64, left: &mut [f32], right: &mut [f32]) {
        match &mut self.0 {
            Engine::Subtractive(i) => i.process(frame, left, right),
            Engine::Fm(i) => i.process(frame, left, right),
            Engine::Pluck(i) => i.process(frame, left, right),
        }
    }
}

impl SynthInstrument for SynthEngine {
    /// MIDI note numbers (rounded, like the app's `noteToMidi` for numbers).
    fn note_on(&mut self, note: f64, velocity: f64, time: f64) {
        self.note_on_midi(note.round() as i32, velocity, time);
    }

    fn note_off(&mut self, note: f64, time: f64) {
        self.note_off_midi(note.round() as i32, time);
    }

    fn trigger_attack_release(&mut self, note: f64, duration: f64, time: f64, velocity: f64) {
        SynthEngine::trigger_attack_release(self, note.round() as i32, duration, time, velocity);
    }

    fn cut_scheduled(&mut self, note: f64, started: f64, time: f64) {
        self.cut_scheduled_midi(note.round() as i32, started, time);
    }

    fn release_all(&mut self, time: f64) {
        match &mut self.0 {
            Engine::Subtractive(i) => i.release_all(time),
            Engine::Fm(i) => i.release_all(time),
            Engine::Pluck(i) => i.release_all(time),
        }
    }
}
