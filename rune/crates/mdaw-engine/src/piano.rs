//! The sampled acoustic piano (port of `src/audio/SampledPiano.ts` + `src/audio/pianoSamples.ts`):
//! a `Tone.Sampler` over the bundled Salamander set ([`samples`], [`sampler`]) with note
//! bookkeeping by a `PolyVoiceAllocator` ([`allocator`]), then `Gain(2)` (makeup) and the
//! preset's [`CharacterChain`].
//!
//! ```text
//! Sampler sources ─┐
//!                  ├→ Gain(×2) → CharacterChain(character, gain dB) → out
//! fallback piano ──┘
//! ```
//!
//! - Velocity → gain `0.06 + 0.94·v^curve`, `curve = clamp(velocityCurve, 0.5, 3)`; velocities
//!   above 1 are MIDI (÷127).
//! - Release (fade-out of new sources) `clamp(release, 0.05, 5)`, exponential (`Sampler.curve`).
//! - Allocator size `max(8, round(polyphony))`. A re-strike releases the key's previous source; a
//!   steal releases the stolen key (both at the new note's time).
//! - **Before the samples are decoded** notes go to the fallback instrument (the FM
//!   `FALLBACK_PIANO`, which lives in the synth module: anything implementing [`PianoFallback`]),
//!   and their note-offs are routed back to it. The samples are polled wait-free on every call.
//!
//! [`SampledPiano`] implements [`SynthInstrument`]: `process` / note calls never allocate (fixed
//! source pool, fixed allocator capacity, the chain's stages allocated up front).

pub mod allocator;
pub mod sampler;
pub mod samples;

#[cfg(test)]
mod parity_tests;
#[cfg(test)]
mod tests;

use crate::character::{CharacterChain, CharacterParams};
use crate::contract::{Instrument, RENDER_QUANTUM};
use crate::core::SynthInstrument;
use crate::synth::presets::SamplerPreset;
use allocator::PolyVoiceAllocator;
use sampler::{ToneSampler, js_round};
pub use samples::{PianoSamples, PianoSlot};

/// `SAMPLER_MAKEUP`: the Salamander samples are mastered quietly; +6 dB.
pub const SAMPLER_MAKEUP: f32 = 2.0;
/// `MIN_GATE` (s).
pub const MIN_GATE: f64 = 0.012;

/// The parameters of a sampler preset the piano reads: a view of
/// [`crate::synth::presets::SamplerPreset`] (the one source of truth: [`PianoParams::from`] a
/// preset; the serde form is the preset's, so a sampler preset's JSON deserializes as this).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PianoParams {
    pub polyphony: f64,
    /// Output gain (dB).
    pub gain: f64,
    /// Damper release (s).
    pub release: f64,
    /// Velocity curve exponent.
    pub velocity_curve: f64,
    #[serde(default)]
    pub character: CharacterParams,
}

impl From<&SamplerPreset> for PianoParams {
    fn from(p: &SamplerPreset) -> Self {
        Self {
            polyphony: p.meta.polyphony,
            gain: p.meta.gain,
            release: p.release,
            velocity_curve: p.velocity_curve,
            character: p.meta.character,
        }
    }
}

impl PianoParams {
    /// Preset 0, `grand-piano` (equals `PianoParams::from` the bank's preset 0, see the tests).
    pub const GRAND: Self = Self {
        polyphony: 48.0,
        gain: 3.5,
        release: 0.5,
        velocity_curve: 1.5,
        character: CharacterParams {
            drive: 0.0,
            chorus: 0.0,
            tremolo: 0.0,
            tremolo_rate: 4.5,
            tone: 16000.0,
            presence: 0.0,
        },
    };
}

/// The synthesized piano used until the samples are decoded (the TS `PolyInstrument<FMPreset>`
/// built from `FALLBACK_PIANO`). Same timing rules as [`SynthInstrument`]; `process` ADDS its
/// output into the piano's bus (before the makeup gain and the character chain).
pub trait PianoFallback: Instrument {
    fn note_on(&mut self, midi: i32, velocity: f64, time: f64);
    fn note_off(&mut self, midi: i32, time: f64);
    /// `triggerAttackRelease(midi, duration, time, velocity)`.
    fn attack_release(&mut self, midi: i32, duration: f64, time: f64, velocity: f64);
    fn release_all(&mut self, time: f64);
    /// Voices held or releasing at `time`.
    fn active_voice_count(&self, time: f64) -> usize;
    /// Whether `midi` is held.
    fn is_held(&self, midi: i32) -> bool;
}

/// `normVelocity`: undefined / non-finite → 0.8, > 1 → MIDI (÷127), clamped to 0..1.
pub fn norm_velocity(v: f64) -> f64 {
    if !v.is_finite() {
        0.8
    } else if v > 1.0 {
        (v / 127.0).clamp(0.0, 1.0)
    } else {
        v.clamp(0.0, 1.0)
    }
}

fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    hi.min(lo.max(v))
}

/// The sampled piano (see the module docs).
pub struct SampledPiano {
    sr: u32,
    params: PianoParams,
    slot: PianoSlot,
    sampler: ToneSampler,
    fallback: Box<dyn PianoFallback>,
    on_fallback: [bool; 128],
    allocator: PolyVoiceAllocator,
    chain: CharacterChain,
    bus: [[f32; RENDER_QUANTUM]; 2],
}

impl SampledPiano {
    /// `new SampledPiano(preset, FALLBACK_PIANO, buffers)` in a context at `sr`, created at
    /// context time `now`. `slot` holds the decoded samples or is filled later
    /// ([`PianoSlot::load`]).
    pub fn new(
        sr: u32,
        params: PianoParams,
        fallback: Box<dyn PianoFallback>,
        slot: PianoSlot,
        now: f64,
    ) -> Self {
        let release = clamp(params.release, 0.05, 5.0);
        Self {
            sr,
            params,
            slot,
            sampler: ToneSampler::new(sr, release),
            fallback,
            on_fallback: [false; 128],
            allocator: PolyVoiceAllocator::new(Self::voices(&params)),
            chain: CharacterChain::new(sr, params.character, params.gain, now),
            bus: [[0.0; RENDER_QUANTUM]; 2],
        }
    }

    /// The context rate.
    pub fn sample_rate(&self) -> u32 {
        self.sr
    }

    fn voices(p: &PianoParams) -> usize {
        js_round(p.polyphony).max(8.0) as usize
    }

    /// True once real samples are in use (`isSampled`).
    pub fn is_sampled(&self) -> bool {
        self.slot.get().is_some()
    }

    pub fn params(&self) -> &PianoParams {
        &self.params
    }

    /// `noteOn(midi, velocity, time)`.
    pub fn note_on(&mut self, midi: i32, velocity: f64, time: f64) {
        if !(0..=127).contains(&midi) {
            return;
        }
        let vel = norm_velocity(velocity);
        if !self.is_sampled() {
            self.on_fallback[midi as usize] = true;
            self.fallback.note_on(midi, vel, time);
            return;
        }
        let alloc = self.allocator.note_on(midi, time, vel);
        self.release_for_allocation(alloc, midi, time);
        self.attack(midi, vel, time);
    }

    /// `noteOff(midi, time)`.
    pub fn note_off(&mut self, midi: i32, time: f64) {
        if let Some(f) = usize::try_from(midi)
            .ok()
            .and_then(|m| self.on_fallback.get_mut(m))
            && std::mem::take(f)
        {
            self.fallback.note_off(midi, time);
        }
        let released = self.allocator.note_off(midi, time, self.release_seconds());
        if released > 0 && self.is_sampled() {
            self.sampler.trigger_release(midi, time);
        }
    }

    /// `triggerAttackRelease(midi, duration, time, velocity)`; `duration ≥ 2·MIN_GATE`.
    pub fn trigger_attack_release(&mut self, midi: i32, duration: f64, time: f64, velocity: f64) {
        if !(0..=127).contains(&midi) {
            return;
        }
        let dur = (MIN_GATE * 2.0).max(duration);
        let vel = norm_velocity(velocity);
        if !self.is_sampled() {
            self.fallback.attack_release(midi, dur, time, vel);
            return;
        }
        let alloc = self
            .allocator
            .one_shot(midi, time, dur, self.release_seconds(), vel);
        self.release_for_allocation(alloc, midi, time);
        self.attack(midi, vel, time);
        self.sampler.trigger_release(midi, time + dur);
    }

    /// `releaseAll(time)`.
    pub fn release_all(&mut self, time: f64) {
        self.fallback.release_all(time);
        self.on_fallback = [false; 128];
        self.allocator.release_all(time);
        if self.is_sampled() {
            self.sampler.release_all(time);
        }
    }

    /// `set(params)` with the merged preset, at context time `now`: release of new sources,
    /// allocator size, character chain and gain.
    pub fn set_params(&mut self, params: PianoParams, now: f64) {
        self.params = params;
        self.sampler.release = self.release_seconds();
        self.allocator.set_max_voices(Self::voices(&params));
        self.chain.update(params.character, params.gain, now);
    }

    /// `getActiveVoiceCount(time)`.
    pub fn active_voice_count(&self, time: f64) -> usize {
        self.allocator.active_voice_count(time) + self.fallback.active_voice_count(time)
    }

    /// Whether `midi` is held (sampled or on the fallback), without allocating.
    pub fn is_held(&self, midi: i32) -> bool {
        self.allocator.is_held(midi) || self.fallback.is_held(midi)
    }

    /// `getHeldNotes()` (control side: allocates).
    pub fn held_notes(&self) -> Vec<i32> {
        let mut v = self.allocator.held_notes();
        v.extend((0..128).filter(|&m| self.fallback.is_held(m)));
        v.sort_unstable();
        v.dedup();
        v
    }

    fn attack(&mut self, midi: i32, vel: f64, time: f64) {
        let gain = self.velocity_to_gain(vel);
        if let Some(samples) = self.slot.get() {
            self.sampler.trigger_attack(samples, midi, time, gain);
        }
    }

    /// Re-strike: release the key's old source; steal: release the stolen key.
    fn release_for_allocation(&mut self, a: allocator::Allocation, midi: i32, time: f64) {
        let note = if a.retrigger {
            Some(midi)
        } else if a.stolen {
            a.previous_note
        } else {
            None
        };
        if let Some(n) = note {
            self.sampler.trigger_release(n, time);
        }
    }

    fn velocity_to_gain(&self, vel: f64) -> f64 {
        let curve = clamp(self.params.velocity_curve, 0.5, 3.0);
        0.06 + 0.94 * clamp(vel, 0.0, 1.0).powf(curve)
    }

    fn release_seconds(&self) -> f64 {
        clamp(self.params.release, 0.05, 5.0)
    }
}

impl Instrument for SampledPiano {
    fn process(&mut self, frame: u64, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len()).min(RENDER_QUANTUM);
        let [bl, br] = &mut self.bus;
        bl[..n].fill(0.0);
        br[..n].fill(0.0);
        self.fallback.process(frame, &mut bl[..n], &mut br[..n]);
        if self.slot.get().is_some() {
            self.sampler.process(frame, &mut bl[..n], &mut br[..n]);
        }
        for i in 0..n {
            bl[i] *= SAMPLER_MAKEUP;
            br[i] *= SAMPLER_MAKEUP;
        }
        self.chain.process(frame, &mut bl[..n], &mut br[..n]);
        for i in 0..n {
            left[i] += bl[i];
            right[i] += br[i];
        }
    }
}

impl SynthInstrument for SampledPiano {
    fn note_on(&mut self, note: f64, velocity: f64, time: f64) {
        SampledPiano::note_on(self, js_round(note) as i32, velocity, time);
    }

    fn note_off(&mut self, note: f64, time: f64) {
        SampledPiano::note_off(self, js_round(note) as i32, time);
    }

    fn release_all(&mut self, time: f64) {
        SampledPiano::release_all(self, time);
    }
}
