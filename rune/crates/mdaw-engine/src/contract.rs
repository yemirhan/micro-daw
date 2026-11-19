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
    /// Hand over buffers this instrument no longer uses (called after every render quantum and
    /// after every command or pattern tick that touched the instrument) so that a last reference
    /// is never dropped on the audio thread.
    fn take_garbage(&mut self, _sink: &mut dyn FnMut(mdaw_dsp::buffer::SharedBuffer)) {}
    /// Buffers this instrument had to leak (`mem::forget`) since the last call because its
    /// garbage list was full: never freed on the audio thread, counted in
    /// `Shared::leaked_garbage`.
    fn take_leaks(&mut self) -> u64 {
        0
    }
    /// The core's output guard found a non-finite sample in this instrument's output (and
    /// replaced the quantum with silence): clear whatever state produced it.
    fn clear_non_finite(&mut self) {}
    /// The transport during the next [`Self::process`] (hosted plugin instruments pass it on to
    /// the plugin; the built-in instruments ignore it).
    fn set_transport(&mut self, _transport: &crate::plugin::PluginTransport) {}
}
