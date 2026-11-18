//! micro-daw audio output host.
//!
//! The engine renders planar stereo f32 (`Core::process(left, right)`); this crate delivers it to
//! a device:
//!
//! - [`OutputHost`] (cpal): opens the default output device (or one by name) at its default
//!   sample rate and channel count, asks for the preferred buffer size (settings
//!   `bufferSizeHint`, clamped to 128..=2048, as `BufferSize::Fixed` when the device reports a
//!   range, else the device default), converts to the device's channel layout and sample format,
//!   reports the actual buffer size / latency ([`HostStats`]), and recovers from device loss and
//!   default-device changes by reopening ([`HostEvent`]). The stream lives on a dedicated
//!   supervisor thread, so the host is `Send` on every platform.
//! - [`NullHost`]: no device; a thread drives the same callback at real-time pace (headless runs,
//!   tests, CI machines without audio hardware).
//!
//! The render callback runs on the audio thread and must not block. The host side of the callback
//! ([`RenderPipe`]) never allocates: the planar scratch buffers are allocated up front
//! ([`MAX_BLOCK_FRAMES`]) and larger device buffers are rendered in chunks.

mod convert;
mod device;
mod host;
mod null;
mod stats;

pub use convert::{MAX_BLOCK_FRAMES, RenderPipe, interleave};
pub use device::{
    BUFFER_SIZE_MAX, BUFFER_SIZE_MIN, BufferRequest, DeviceInfo, choose_buffer_size,
    clamp_buffer_size_hint, default_output_device_info, list_output_devices,
};
pub use host::{HostError, HostEvent, OutputHost, OutputOptions, StreamInfo};
pub use null::NullHost;
pub use stats::{HostStats, StatsSnapshot};

/// The engine render callback: fill `left` / `right` (same length, zeroed on entry) with the next
/// block of audio.
pub type RenderFn = Box<dyn FnMut(&mut [f32], &mut [f32]) + Send + 'static>;

/// The render callback shared between successive streams (reopen after a device change hands
/// the same callback to the new stream). The audio thread only `try_lock`s it: while a control
/// thread holds the lock (or two streams briefly overlap) the block is silent instead of blocking.
pub type SharedRender = std::sync::Arc<parking_lot::Mutex<RenderFn>>;

/// Wrap a render closure for [`OutputHost::start`] / [`NullHost::start`].
pub fn shared_render(f: impl FnMut(&mut [f32], &mut [f32]) + Send + 'static) -> SharedRender {
    std::sync::Arc::new(parking_lot::Mutex::new(Box::new(f)))
}
