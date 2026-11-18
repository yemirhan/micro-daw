//! Pitched track instruments: port of `src/audio/presets.ts`, `presetParams.ts`,
//! `voiceAllocator.ts`, `PolyInstrument.ts` and `voices/{Subtractive,FM,Pluck}Voice.ts`.
//!
//! - [`presets`] / [`bank`]: the preset model and the 41-preset bank + `FALLBACK_PIANO`, verbatim;
//! - [`params`]: override merging (type checks, enums, legacy keys, locked identity fields);
//! - [`alloc`]: voice allocation (reuse / grow / steal) and the mono note stack;
//! - [`SynthEngine`]: the instrument a preset builds (`createSynthFromPreset`), implementing
//!   [`crate::core::SynthInstrument`] (the raw voice bus; `crate::player::SynthPlayer` adds the
//!   `CharacterChain` and assembles the sampled piano around the FM fallback for sampler presets).
//!
//! **Sound.** The voices reproduce the Tone.js graphs as Chromium renders them (see `wa`):
//! oscillators with Chromium's band-limited wavetables (Tone's periodic waves for fat/custom
//! oscillators), a-rate biquads, Tone's envelopes on Chromium's automation timeline, Tone's noise
//! buffers, the Karplus-Strong worklet. Output is the raw voice bus, BEFORE the instrument's
//! `CharacterChain`.
//!
//! Engine / Tone behaviours the Chromium captures exposed and this port reproduces:
//! - an oscillator starting mid-quantum reads its phase increments from the START of the
//!   quantum (`wa::osc`): a fresh voice's first partial quantum plays Tone's default 440 Hz;
//! - a Tone signal (ConstantSource) created on a quantum whose `(frame/sr)·sr` rounds above the
//!   frame skips its first frame ([`common::signals_skip_first_frame`]);
//! - `HandleAllEventsInThePast` keeps the last event, so `param.value = x` / an event at exactly
//!   "now" lands on `ceil(now·sr)` (`wa::native_tl`);
//! - the LFO only advances once a voice connects it to the output (Chromium renders pulled nodes
//!   only);
//! - a new Tone oscillator node per start from "stopped", old nodes playing out their own gate
//!   (`sources`); `Noise._stop` forgets its source even for a future stop, so a re-attack during
//!   a release layers a new noise over the old one;
//! - biquads stop processing (state frozen) once their input is silent past their tail time.
//!
//! Electron bug fixed in both apps (user decision 2026-11-18): `SubtractiveVoice` applies
//! `oscA/oscB` `semi`/`fine` at construction (Tone's `omniOptions` carries no detune, so the TS
//! voice used to get them only from a live `set()`, on the voices that existed at that moment).
//!
//! `parity_tests` replays the Chromium capture of the app's real `PolyInstrument`
//! (`rune/parity/chromium/capture-synth.steps.mjs`) for every non-sampler preset, the fallback
//! piano and four live `set()` cases: whole-render error −103.6 dB (pluck-bass) to −146.6 dB
//! (harp), every block's energy within 0.001 dB.
//!
//! **Time.** Every scheduling call takes context seconds; "now" is the start of the next quantum
//! to render (the frame after the last `process`), which plays the role of both
//! `context.currentTime` and `Tone.now()` (no look-ahead).

pub mod alloc;
pub mod bank;
mod bank_builders;
pub mod common;
mod fm;
mod instrument;
pub mod params;
mod pluck;
mod poly;
pub mod presets;
mod sources;
mod subtractive;
pub(crate) mod wa;

#[cfg(test)]
mod alloc_tests;
#[cfg(test)]
mod instrument_tests;
#[cfg(test)]
mod parity_tests;
#[cfg(test)]
mod preset_tests;

pub use instrument::{DEFAULT_NOISE_SEED, EnginePatch, SynthEngine};
pub use poly::STEAL_GAP;
pub use wa::noise::NoiseSeeds;
