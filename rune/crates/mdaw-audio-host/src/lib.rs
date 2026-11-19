//! micro-daw audio output host.
//!
//! The engine renders planar stereo f32 at a fixed **context rate** (`Core::process(left,
//! right)`); this crate delivers it to a device:
//!
//! - [`OutputHost`] (cpal): opens the default output device (or one by name). The context rate
//!   is fixed for the host's whole life ([`OutputOptions::context_rate`], or the first opened
//!   device's default rate), like a Web Audio `AudioContext`: every (re)open (device change,
//!   device loss, buffer-hint change) picks a device config that runs at the context rate
//!   ([`choose_output_config`]) and, when the device has none, opens its default config and
//!   resamples ([`Resampler`], exact rational step, so tempo and pitch never change). It asks
//!   for the preferred buffer size (settings `bufferSizeHint`, clamped to 128..=2048, as
//!   `BufferSize::Fixed` when the device reports a range, else the device default), converts to
//!   the device's channel layout and sample format, reports the actual buffer size / latency
//!   ([`HostStats`]), and recovers from device loss and default-device changes by reopening
//!   ([`HostEvent`]). The stream lives on a dedicated supervisor thread, so the host is `Send`
//!   on every platform.
//! - **Null fallback** ([`OutputOptions::null_fallback`], on by default): whenever no device
//!   stream is running (the initial open failed, a reopen keeps failing, or between losing a
//!   device and reopening) the supervisor drives the same render callback from a real-time
//!   pacer thread at the context rate and discards the output, so the engine clock keeps
//!   running and its command queue keeps draining. The pacer is stopped (joined) before a device
//!   stream is built, so two drivers never call the render callback concurrently or
//!   interleaved. [`HostEvent::NullFallback`] reports each transition.
//! - [`NullHost`]: no device; a thread drives the same callback at real-time pace (headless runs,
//!   tests, CI machines without audio hardware), optionally at a device rate different from the
//!   context rate (resampled like a real device) and with an output tap.
//!
//! The render callback runs on the audio thread and must not block. The host side of the callback
//! ([`RenderPipe`]) never allocates or blocks: the planar scratch buffers and the resampler are
//! allocated up front ([`MAX_BLOCK_FRAMES`]) and larger device buffers are rendered in chunks.
//! Each chunk comes with a [`BlockTiming`] so the engine can publish a wall-clock anchor for its
//! clock. A panic in the render callback is caught: the output goes silent and stays silent
//! (the render state may be inconsistent) until the control side calls
//! [`OutputHost::clear_render_panic`] / [`NullHost::clear_render_panic`]
//! ([`StatsSnapshot::render_panics`], [`HostEvent::RenderPanicked`]).

use std::time::{Duration, Instant};

mod convert;
mod device;
mod host;
mod null;
mod pacer;
mod resample;
mod stats;
#[cfg(test)]
mod test_util;

pub use convert::{MAX_BLOCK_FRAMES, RenderPipe, interleave};
pub use device::{
    BUFFER_SIZE_MAX, BUFFER_SIZE_MIN, BufferRequest, ConfigRange, DeviceInfo, choose_buffer_size,
    choose_output_config, clamp_buffer_size_hint, default_output_device_info, list_output_devices,
};
pub use host::{DEFAULT_CONTEXT_RATE, HostError, HostEvent, OutputHost, OutputOptions, StreamInfo};
pub use null::{NullHost, NullOptions, OutputTap};
pub use pacer::block_deadline;
pub use resample::Resampler;
pub use stats::{HostStats, StatsSnapshot};

/// When the chunk handed to the render callback plays, for the engine clock (plan §3: frames
/// rendered + the wall-clock instant of that callback).
#[derive(Debug, Clone, Copy)]
pub struct BlockTiming {
    /// `Instant::now()` taken at entry of the device callback (or the null host's tick).
    pub callback: Instant,
    /// Context-rate frames already rendered earlier in this same callback (chunking /
    /// resampling), so this chunk's first frame is rendered at `callback + frame_offset /
    /// context_rate` in callback-relative terms.
    pub frame_offset: u64,
    pub context_rate: u32,
    /// Estimated callback → speaker time of this chunk's first frame: the backend's
    /// playback-callback timestamp difference (cpal `OutputCallbackInfo::timestamp()`,
    /// `playback.duration_since(&callback)`) plus the resampler's group delay (how far the
    /// resampler's input runs ahead of its output); `Duration::ZERO` when unknown. The null
    /// host and the null fallback report `ZERO` (no device).
    pub output_latency: Duration,
}

impl BlockTiming {
    /// `callback + frame_offset / context_rate`: when this chunk starts in callback time.
    pub fn chunk_instant(&self) -> Instant {
        let ns = self.frame_offset as u128 * 1_000_000_000 / self.context_rate.max(1) as u128;
        self.callback + Duration::from_nanos(ns.min(u64::MAX as u128) as u64)
    }

    /// Estimated speaker time of this chunk's first frame.
    pub fn speaker_instant(&self) -> Instant {
        self.chunk_instant() + self.output_latency
    }
}

/// The engine render callback: fill `left` / `right` (same length, zeroed on entry) with the next
/// block of audio at the context rate ([`BlockTiming::context_rate`]).
pub type RenderFn = Box<dyn FnMut(&mut [f32], &mut [f32], &BlockTiming) + Send + 'static>;

/// The render callback shared between successive streams (reopen after a device change hands
/// the same callback to the new stream). The audio thread only `try_lock`s it: while a control
/// thread holds the lock (or two streams briefly overlap) the block is silent instead of blocking.
pub type SharedRender = std::sync::Arc<parking_lot::Mutex<RenderFn>>;

/// Wrap a render closure for [`OutputHost::start`] / [`NullHost::start`].
pub fn shared_render(
    f: impl FnMut(&mut [f32], &mut [f32], &BlockTiming) + Send + 'static,
) -> SharedRender {
    std::sync::Arc::new(parking_lot::Mutex::new(Box::new(f)))
}
