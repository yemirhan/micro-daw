//! The track synth (`src/utils/synthFactory.ts` `createSynthFromPreset`): engine → instrument.
//!
//! - subtractive / fm / pluck → [`PolyInstrument`] with the engine's voices;
//! - sampler (presets 0 and 15): the synthesized `FALLBACK_PIANO` (FM) on its own. The sampled
//!   piano itself (`SampledPiano`: samples, allocator, makeup, its chain, this fallback in front)
//!   is assembled by [`crate::player::SynthPlayer`].
//!
//! Output is the RAW voice bus (before `CharacterChain`, which [`crate::player::SynthPlayer`]
//! chains): [`Instrument::process`] adds it into the bus.

use crate::contract::Instrument;
use crate::core::SynthInstrument;

use super::bank::fallback_piano;
use super::fm::FmVoice;
use super::pluck::PluckVoice;
use super::poly::{PolyInstrument, PolyPreset};
use super::presets::{FmPreset, LfoParams, PluckPreset, SubtractivePreset, SynthPreset};
use super::subtractive::SubtractiveVoice;
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
    fn make_voice(&self, sr: f64, cx: Clock) -> SubtractiveVoice {
        SubtractiveVoice::new(self, sr, NoiseSeeds::default(), cx)
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
    fn make_voice(&self, sr: f64, cx: Clock) -> FmVoice {
        FmVoice::new(self, sr, cx)
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
    fn make_voice(&self, sr: f64, cx: Clock) -> PluckVoice {
        PluckVoice::new(self, sr, NoiseSeeds::default(), cx)
    }
}

#[allow(clippy::large_enum_variant)] // one per track, built on the control side
enum Engine {
    Subtractive(PolyInstrument<SubtractivePreset>),
    Fm(PolyInstrument<FmPreset>),
    Pluck(PolyInstrument<PluckPreset>),
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
    /// times. Other engines: nothing to do.
    pub fn prepare_offline(&mut self, notes: &[(i32, f64, f64, f64)], sr: f64) {
        let Engine::Pluck(i) = &mut self.0 else {
            return;
        };
        let mut scratch = PolyInstrument::new(i.preset().clone(), sr, 0.0, 0);
        scratch.set_offline();
        for v in scratch.voices_mut() {
            v.record_attacks();
        }
        for &(midi, dur, time, vel) in notes {
            scratch.trigger_attack_release(midi, dur, time, vel);
        }
        for (v, s) in i.voices_mut().iter_mut().zip(scratch.used_voices()) {
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

    /// `set(params)` with a merged preset of the same engine (control side). Returns false when
    /// the engine differs (the caller rebuilds the instrument, as the app does).
    pub fn set_preset(&mut self, preset: &SynthPreset) -> bool {
        match (&mut self.0, preset) {
            (Engine::Subtractive(i), SynthPreset::Subtractive(p)) => i.set(p.clone()),
            (Engine::Fm(i), SynthPreset::Fm(p)) => i.set(p.clone()),
            (Engine::Pluck(i), SynthPreset::Pluck(p)) => i.set(p.clone()),
            _ => return false,
        }
        true
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

    fn release_all(&mut self, time: f64) {
        match &mut self.0 {
            Engine::Subtractive(i) => i.release_all(time),
            Engine::Fm(i) => i.release_all(time),
            Engine::Pluck(i) => i.release_all(time),
        }
    }
}
