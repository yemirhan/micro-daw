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
//! - [`synth`]: the pitched track instruments (presets, voice allocation, Subtractive / FM / Pluck
//!   voices rendered like Chromium renders their Tone.js graphs).
//! - [`engine`]: `ArrangementEngine`, the control side (UI thread) wiring the `ArrangementDoc` to
//!   the core: track instruments / schedules / strips, the live drum sink, transport, loop and
//!   Song recording from the core's pad captures, audio regions, WAV export.
//! - [`contract`]: the types shared between the control side and the core (commands, events,
//!   times, the `Instrument` trait).

pub mod buffers;
pub mod character;
pub mod contract;
pub mod core;
pub mod engine;
pub mod export;
pub mod piano;
pub mod sampler;
pub mod synth;

#[cfg(test)]
pub(crate) mod test_alloc;
