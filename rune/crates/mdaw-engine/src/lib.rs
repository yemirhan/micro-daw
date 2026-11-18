//! micro-daw audio engine.
//!
//! Architecture (see docs/rune-port-plan.md §3 "Threading and realtime ownership"):
//! - [`core`]: `Core`, the only thing the audio thread runs. It owns the sample clock, the tick
//!   transport (PPQ 192), per-track schedules in beats, instruments and the metronome, and fires
//!   events at exact frames inside 128-frame render quanta (Chromium's quantum). The same `Core`
//!   runs inside the cpal callback and offline (export, tests), so timing tests run on the real
//!   engine with no mock.
//! - [`sampler`]: `SamplerKit`, the 16-pad sampler voice engine (port of src/audio/sampler/SamplerKit.ts),
//!   playing buffers through `mdaw_dsp::abs_node::BufferPlayer` (Chromium-exact rate conversion).
//! - [`contract`]: the types shared between the control side and the core (commands, events,
//!   times, the `Instrument` trait).

pub mod buffers;
pub mod contract;
pub mod core;
pub mod sampler;

#[cfg(test)]
pub(crate) mod test_alloc;
