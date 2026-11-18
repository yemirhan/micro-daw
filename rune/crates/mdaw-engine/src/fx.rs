//! Effect chains (phase 4): the per-track strip of `createTrackAudioNodes`, the live synth bus of
//! `AudioEngine`, track automation, and the Tone.js / Chromium nodes they are built from.
//!
//! Everything here runs in 128-frame render quanta ([`Q`]), like Chromium's audio graph, and
//! follows Chromium's node semantics (k-rate vs a-rate params, feedback cycles one quantum late,
//! the `DynamicsCompressor`, the `ConvolverNode`'s partitioned convolution).

pub mod automation;
pub mod biquad;
pub mod cc;
pub mod compressor;
pub mod convolver;
pub mod delay;
pub mod effects;
pub mod filter;
pub mod lfo;
pub mod live;
pub mod meter;
pub mod osc;
pub mod state;
pub mod tone;
pub mod track;
pub mod waveshaper;
pub mod xfade;

/// Chromium's render quantum.
pub const Q: usize = crate::contract::RENDER_QUANTUM;

/// The context clock a param change is scheduled with (`currentTime` and the sample rate).
pub use crate::sampler::param::Clock;

#[cfg(test)]
mod parity_tests;
