//! The track synth (`src/utils/synthFactory.ts` `createSynthFromPreset`): engine → instrument.
//!
//! - subtractive / fm / pluck → [`PolyInstrument`] with the engine's voices;
//! - sampler (presets 0 and 15) → [`SamplerHost`]: the sampled piano lives behind the
//!   [`SampledPianoSlot`] trait (another module); until it reports samples ready, notes play on the
//!   synthesized `FALLBACK_PIANO` (FM) and their note-offs are routed back to it, like
//!   `SampledPiano`.
//!
//! Output is the RAW voice bus (before `CharacterChain`, which the integration chains):
//! [`Instrument::process`] adds it into the bus. For the sampler host the fallback's raw bus and
//! the sampled slot's output are summed; the Electron chain is fallback → its own
//! CharacterChain → ×2 (SAMPLER_MAKEUP) → the sampler's CharacterChain, see
//! [`SamplerHost::process_fallback`].

use crate::contract::Instrument;
use crate::core::SynthInstrument;

use super::bank::fallback_piano;
use super::common::{MIN_GATE, norm_velocity};
use super::fm::FmVoice;
use super::pluck::PluckVoice;
use super::poly::{PolyInstrument, PolyPreset};
use super::presets::{
    FmPreset, LfoParams, PluckPreset, SamplerPreset, SubtractivePreset, SynthPreset,
};
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

/// The sampled piano (another module implements it; `SampledPiano`'s sampler side).
pub trait SampledPianoSlot: Send {
    /// True once real samples are in use (`isSampled`).
    fn is_sampled(&self) -> bool;
    fn note_on(&mut self, midi: i32, velocity: f64, time: f64);
    /// Called for every note-off, also for notes the fallback played (the TS releases the
    /// allocator either way).
    fn note_off(&mut self, midi: i32, time: f64);
    fn trigger_attack_release(&mut self, midi: i32, duration: f64, time: f64, velocity: f64);
    fn release_all(&mut self, time: f64);
    fn active_voice_count(&self, time: f64) -> usize;
    /// Adds the sampled voices into the bus (before the sampler makeup and CharacterChain).
    fn process(&mut self, frame: u64, left: &mut [f32], right: &mut [f32]);
    fn set_preset(&mut self, _preset: &SamplerPreset) {}
}

/// `SampledPiano`: the FALLBACK_PIANO FM instrument until the slot is sampled.
pub struct SamplerHost {
    preset: SamplerPreset,
    fallback: PolyInstrument<FmPreset>,
    slot: Option<Box<dyn SampledPianoSlot>>,
    on_fallback: [bool; 128],
}

impl SamplerHost {
    pub fn new(preset: SamplerPreset, sr: f64, now: f64, noise_seed: u32) -> Self {
        Self {
            preset,
            fallback: PolyInstrument::new(fallback_piano().clone(), sr, now, noise_seed),
            slot: None,
            on_fallback: [false; 128],
        }
    }

    /// Install the sampled piano (control side).
    pub fn set_slot(
        &mut self,
        slot: Box<dyn SampledPianoSlot>,
    ) -> Option<Box<dyn SampledPianoSlot>> {
        self.slot.replace(slot)
    }

    pub fn is_sampled(&self) -> bool {
        self.slot.as_ref().is_some_and(|s| s.is_sampled())
    }

    pub fn preset(&self) -> &SamplerPreset {
        &self.preset
    }

    fn sampled(&mut self) -> Option<&mut Box<dyn SampledPianoSlot>> {
        self.slot.as_mut().filter(|s| s.is_sampled())
    }

    pub fn note_on(&mut self, midi: i32, velocity: f64, time: f64) {
        if !(0..=127).contains(&midi) {
            return;
        }
        let vel = norm_velocity(Some(velocity));
        match self.sampled() {
            Some(s) => s.note_on(midi, vel, time),
            None => {
                self.on_fallback[midi as usize] = true;
                self.fallback.note_on(midi, vel, time);
            }
        }
    }

    pub fn note_off(&mut self, midi: i32, time: f64) {
        if (0..=127).contains(&midi) && std::mem::take(&mut self.on_fallback[midi as usize]) {
            self.fallback.note_off(midi, time);
        }
        if let Some(s) = self.slot.as_mut() {
            s.note_off(midi, time);
        }
    }

    pub fn trigger_attack_release(&mut self, midi: i32, duration: f64, time: f64, velocity: f64) {
        if !(0..=127).contains(&midi) {
            return;
        }
        let dur = duration.max(MIN_GATE * 2.0);
        let vel = norm_velocity(Some(velocity));
        match self.sampled() {
            Some(s) => s.trigger_attack_release(midi, dur, time, vel),
            None => self.fallback.trigger_attack_release(midi, dur, time, vel),
        }
    }

    pub fn release_all(&mut self, time: f64) {
        self.fallback.release_all(time);
        self.on_fallback = [false; 128];
        if let Some(s) = self.slot.as_mut() {
            s.release_all(time);
        }
    }

    pub fn active_voice_count(&self, time: f64) -> usize {
        self.slot.as_ref().map_or(0, |s| s.active_voice_count(time))
            + self.fallback.active_voice_count(time)
    }

    pub fn held_notes(&self) -> Vec<i32> {
        self.fallback.held_notes()
    }

    /// The fallback piano's raw voice bus only.
    pub fn process_fallback(&mut self, frame: u64, left: &mut [f32], right: &mut [f32]) {
        self.fallback.process(frame, left, right);
    }

    /// The sampled slot's output only.
    pub fn process_sampled(&mut self, frame: u64, left: &mut [f32], right: &mut [f32]) {
        if let Some(s) = self.slot.as_mut() {
            s.process(frame, left, right);
        }
    }
}

enum Engine {
    Subtractive(PolyInstrument<SubtractivePreset>),
    Fm(PolyInstrument<FmPreset>),
    Pluck(PolyInstrument<PluckPreset>),
    Sampler(SamplerHost),
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
            SynthPreset::Sampler(p) => {
                Engine::Sampler(SamplerHost::new(p.clone(), sr, now, noise_seed))
            }
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

    pub fn note_on_midi(&mut self, midi: i32, velocity: f64, time: f64) {
        match &mut self.0 {
            Engine::Subtractive(i) => i.note_on(midi, velocity, time),
            Engine::Fm(i) => i.note_on(midi, velocity, time),
            Engine::Pluck(i) => i.note_on(midi, velocity, time),
            Engine::Sampler(h) => h.note_on(midi, velocity, time),
        }
    }

    pub fn note_off_midi(&mut self, midi: i32, time: f64) {
        match &mut self.0 {
            Engine::Subtractive(i) => i.note_off(midi, time),
            Engine::Fm(i) => i.note_off(midi, time),
            Engine::Pluck(i) => i.note_off(midi, time),
            Engine::Sampler(h) => h.note_off(midi, time),
        }
    }

    /// `triggerAttackRelease(midi, duration, time, velocity)`.
    pub fn trigger_attack_release(&mut self, midi: i32, duration: f64, time: f64, velocity: f64) {
        match &mut self.0 {
            Engine::Subtractive(i) => i.trigger_attack_release(midi, duration, time, velocity),
            Engine::Fm(i) => i.trigger_attack_release(midi, duration, time, velocity),
            Engine::Pluck(i) => i.trigger_attack_release(midi, duration, time, velocity),
            Engine::Sampler(h) => h.trigger_attack_release(midi, duration, time, velocity),
        }
    }

    pub fn active_voice_count(&self, time: f64) -> usize {
        match &self.0 {
            Engine::Subtractive(i) => i.active_voice_count(time),
            Engine::Fm(i) => i.active_voice_count(time),
            Engine::Pluck(i) => i.active_voice_count(time),
            Engine::Sampler(h) => h.active_voice_count(time),
        }
    }

    pub fn held_notes(&self) -> Vec<i32> {
        match &self.0 {
            Engine::Subtractive(i) => i.held_notes(),
            Engine::Fm(i) => i.held_notes(),
            Engine::Pluck(i) => i.held_notes(),
            Engine::Sampler(h) => h.held_notes(),
        }
    }

    /// Voices used so far (`getPoolSize`; the fallback's for a sampler).
    pub fn pool_size(&self) -> usize {
        match &self.0 {
            Engine::Subtractive(i) => i.pool_size(),
            Engine::Fm(i) => i.pool_size(),
            Engine::Pluck(i) => i.pool_size(),
            Engine::Sampler(h) => h.fallback.pool_size(),
        }
    }

    /// The sampler host of a sampler preset (to install the sampled piano).
    pub fn sampler_host(&mut self) -> Option<&mut SamplerHost> {
        match &mut self.0 {
            Engine::Sampler(h) => Some(h),
            _ => None,
        }
    }

    /// `set(params)` with a merged preset of the same engine (control side). Returns false when
    /// the engine differs (the caller rebuilds the instrument, as the app does).
    pub fn set_preset(&mut self, preset: &SynthPreset) -> bool {
        match (&mut self.0, preset) {
            (Engine::Subtractive(i), SynthPreset::Subtractive(p)) => i.set(p.clone()),
            (Engine::Fm(i), SynthPreset::Fm(p)) => i.set(p.clone()),
            (Engine::Pluck(i), SynthPreset::Pluck(p)) => i.set(p.clone()),
            (Engine::Sampler(h), SynthPreset::Sampler(p)) => {
                h.preset = p.clone();
                if let Some(s) = h.slot.as_mut() {
                    s.set_preset(p);
                }
            }
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
            Engine::Sampler(h) => {
                h.process_fallback(frame, left, right);
                h.process_sampled(frame, left, right);
            }
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

    fn release_all(&mut self, time: f64) {
        match &mut self.0 {
            Engine::Subtractive(i) => i.release_all(time),
            Engine::Fm(i) => i.release_all(time),
            Engine::Pluck(i) => i.release_all(time),
            Engine::Sampler(h) => h.release_all(time),
        }
    }
}
