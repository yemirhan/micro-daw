//! Types shared by the control side (UI thread), the MIDI thread and the audio core.
//!
//! **Time.** All engine times are *context seconds* (`f64`), the equivalent of Web Audio
//! `AudioContext.currentTime`: frames rendered since the core started ÷ sample rate. Tone's
//! `Tone.immediate()` is "now"; the Electron app's `Tone.now()` adds a 0.1 s lookahead, which the
//! Rust engine does not need (events are applied at the exact frame); see the phase-3 parity doc
//! for every place where the Electron code adds a lookahead and what the Rust engine does instead.
//!
//! **Render quantum.** The core renders in blocks of [`RENDER_QUANTUM`] frames whose start frame
//! is a multiple of 128, like Chromium; per-frame work inside a block follows Web Audio's rules
//! (an event at time t takes effect at frame ceil(t·sr) within the block), which is what makes the
//! offline renders comparable with Chromium/Tone captures.

/// Web Audio render quantum (frames).
pub const RENDER_QUANTUM: usize = 128;

/// Transport ticks per quarter note (Tone's default PPQ, which the Electron app never changes).
pub const PPQ: u32 = 192;

/// Something that renders audio into a stereo bus, block by block (a SamplerKit, a synth, the
/// metronome click). Implementations must not allocate or lock in `process`.
pub trait Instrument: Send {
    /// Render `left.len()` (== `right.len()` ≤ RENDER_QUANTUM) frames starting at absolute frame
    /// `frame`, ADDING into the buffers.
    fn process(&mut self, frame: u64, left: &mut [f32], right: &mut [f32]);
}
