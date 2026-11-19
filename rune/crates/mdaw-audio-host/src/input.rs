//! cpal audio **input**: a device's channels, converted to interleaved f32 at the context rate,
//! handed to a sink on the device's input callback.
//!
//! Like the output ([`crate::OutputHost`]) the stream lives on a supervisor thread: it opens the
//! named device (or the system default input, followed when it changes), prefers a config at
//! the context rate with as many channels as the device has, and otherwise opens the default
//! config and resamples ([`InputResampler`]: 4-point cubic interpolation; an audio interface
//! that is also the output device runs at the context rate and never needs it). A lost device
//! or a failed open is retried every poll interval; nothing here ever fails for good.
//!
//! The sink is made per stream by the opener the host is started with (the channel count is
//! only known once a device is open): the app builds the engine's input queue there.
//!
//! On macOS the first open asks the user for microphone access; until it is granted the
//! stream delivers silence.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{
    FromSample, Sample, SampleFormat, SampleRate, SizedSample, StreamConfig, SupportedStreamConfig,
};
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use parking_lot::Mutex;

use crate::device::{device_name, format_supported};
use crate::{BufferRequest, HostError, choose_buffer_size};

/// Receives each input block: interleaved frames ([`InputInfo::channels`] per frame) at the
/// context rate. Runs on the device's input callback: must not block or allocate.
pub type InputSink = Box<dyn FnMut(&[f32]) + Send + 'static>;

/// Makes the sink of a newly opened stream (on the supervisor thread).
pub type InputOpener = Box<dyn FnMut(&InputInfo) -> InputSink + Send + 'static>;

/// How to open the input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputOptions {
    /// Exact device name; None = the system default input (followed when it changes).
    pub device_name: Option<String>,
    /// Frames per callback to ask for; None = the backend default.
    pub buffer_size_hint: Option<u32>,
    /// The rate the sink receives (the engine's context rate).
    pub context_rate: u32,
    pub poll_interval: Duration,
}

impl InputOptions {
    pub fn new(context_rate: u32) -> Self {
        Self {
            device_name: None,
            buffer_size_hint: Some(256),
            context_rate,
            poll_interval: Duration::from_millis(500),
        }
    }
}

/// The input stream that is running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputInfo {
    pub device_name: String,
    pub device_rate: u32,
    pub context_rate: u32,
    /// The device runs at another rate: its input is resampled.
    pub resampling: bool,
    pub channels: u16,
    pub buffer: BufferRequest,
    /// Opened as "system default" (follows default-device changes).
    pub follows_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputEvent {
    Opened(InputInfo),
    /// The device went away; the host reopens (the default input, or the named one when it
    /// comes back).
    DeviceLost {
        device: String,
    },
    /// Opening failed (retried every poll interval); sent once per failure streak.
    OpenFailed(String),
    /// A non-fatal backend error.
    StreamError(String),
}

/// An input device as listed for the settings UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputDeviceInfo {
    pub name: String,
    pub is_default: bool,
    pub default_sample_rate: u32,
    /// The most channels any of its configs offers.
    pub channels: u16,
}

/// Every input device with a usable config.
pub fn list_input_devices() -> Result<Vec<InputDeviceInfo>, HostError> {
    let host = cpal::default_host();
    let default_name = host.default_input_device().map(|d| device_name(&d));
    let devices = host
        .input_devices()
        .map_err(|e| HostError::Devices(e.to_string()))?;
    Ok(devices
        .filter_map(|d| {
            let config = d.default_input_config().ok()?;
            let name = device_name(&d);
            let max = d
                .supported_input_configs()
                .ok()
                .and_then(|c| c.map(|r| r.channels()).max())
                .unwrap_or(0);
            Some(InputDeviceInfo {
                is_default: default_name.as_deref() == Some(name.as_str()),
                name,
                default_sample_rate: config.sample_rate().0,
                channels: max.max(config.channels()),
            })
        })
        .collect())
}

fn find_input_device(host: &cpal::Host, name: &str) -> Option<cpal::Device> {
    host.input_devices().ok()?.find(|d| device_name(d) == name)
}

/// Streaming resampler for interleaved input blocks: device rate → context rate, any channel
/// count, 4-point cubic (Catmull-Rom) interpolation. Allocation-free after `new`.
pub struct InputResampler {
    channels: usize,
    /// Input frames per output frame.
    step: f64,
    /// Position of the next output frame, in input frames relative to `hist[1]` (the output
    /// runs two input frames behind the newest input: the interpolator's look-ahead).
    pos: f64,
    /// The last three input frames (interleaved), oldest first.
    hist: Box<[f32]>,
    out: Box<[f32]>,
    max_block: usize,
}

impl InputResampler {
    /// `max_block`: the largest input block (frames) one `process` call receives.
    pub fn new(device_rate: u32, context_rate: u32, channels: usize, max_block: usize) -> Self {
        let channels = channels.max(1);
        let step = device_rate.max(1) as f64 / context_rate.max(1) as f64;
        let max_block = max_block.max(1);
        let max_out = (max_block as f64 / step).ceil() as usize + 8;
        Self {
            channels,
            step,
            pos: 0.0,
            hist: vec![0.0; 3 * channels].into_boxed_slice(),
            out: vec![0.0; max_out * channels].into_boxed_slice(),
            max_block,
        }
    }

    /// Resample one interleaved block; returns the interleaved output (valid until the next
    /// call). A block larger than `max_block` is truncated to it.
    pub fn process(&mut self, input: &[f32]) -> &[f32] {
        let ch = self.channels;
        let n = (input.len() / ch).min(self.max_block);
        // Frame `i` of the virtual signal: i < 3 from the history, else from the input.
        let at = |hist: &[f32], i: usize, c: usize| -> f32 {
            if i < 3 {
                hist[i * ch + c]
            } else {
                input[(i - 3) * ch + c]
            }
        };
        let mut produced = 0;
        // Output at `pos` (relative to virtual frame 1) needs virtual frames floor(pos)..+3.
        while self.pos + 2.0 < (n + 2) as f64 && (produced + 1) * ch <= self.out.len() {
            let i = self.pos.floor() as usize;
            let t = (self.pos - i as f64) as f32;
            for c in 0..ch {
                let (p0, p1, p2, p3) = (
                    at(&self.hist, i, c),
                    at(&self.hist, i + 1, c),
                    at(&self.hist, i + 2, c),
                    at(&self.hist, i + 3, c),
                );
                let a = -0.5 * p0 + 1.5 * p1 - 1.5 * p2 + 0.5 * p3;
                let b = p0 - 2.5 * p1 + 2.0 * p2 - 0.5 * p3;
                let cc = -0.5 * p0 + 0.5 * p2;
                self.out[produced * ch + c] = ((a * t + b) * t + cc) * t + p1;
            }
            produced += 1;
            self.pos += self.step;
        }
        self.pos -= n as f64;
        // The last three virtual frames become the history.
        let mut next = [0f32; 3 * crate::input::MAX_HISTORY_CHANNELS];
        let keep = 3 * ch;
        if keep <= next.len() {
            for (k, slot) in next[..keep].iter_mut().enumerate() {
                let (i, c) = (n + k / ch, k % ch);
                *slot = at(&self.hist, i, c);
            }
            self.hist.copy_from_slice(&next[..keep]);
        }
        &self.out[..produced * ch]
    }
}

/// Most channels [`InputResampler`] keeps history for (more are opened without resampling
/// support: such a device is opened with fewer channels).
pub const MAX_HISTORY_CHANNELS: usize = 64;

enum Cmd {
    Stop,
    SetDevice(Option<String>),
    SetBufferHint(Option<u32>),
    StreamFailed(cpal::StreamError),
}

/// A running device input. Dropping it (or [`InputHost::stop`]) closes the stream.
pub struct InputHost {
    cmd: Sender<Cmd>,
    events: Receiver<InputEvent>,
    info: Arc<Mutex<Option<InputInfo>>>,
    latency_ns: Arc<AtomicU64>,
    thread: Option<JoinHandle<()>>,
}

impl InputHost {
    /// Start opening the input. Returns at once; watch [`Self::events`] / [`Self::info`].
    pub fn start(options: InputOptions, opener: InputOpener) -> Result<Self, HostError> {
        let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
        let (ev_tx, ev_rx) = crossbeam_channel::unbounded();
        let info = Arc::new(Mutex::new(None));
        let latency_ns = Arc::new(AtomicU64::new(0));
        let (inf, lat, tx2) = (info.clone(), latency_ns.clone(), cmd_tx.clone());
        let thread = std::thread::Builder::new()
            .name("mdaw-audio-input".into())
            .spawn(move || {
                Supervisor {
                    opts: options,
                    opener,
                    info: inf,
                    latency_ns: lat,
                    events: ev_tx,
                    cmd_tx: tx2,
                    stream: None,
                    current: None,
                    open_failed: false,
                }
                .run(cmd_rx)
            })
            .map_err(|e| HostError::Thread(e.to_string()))?;
        Ok(Self {
            cmd: cmd_tx,
            events: ev_rx,
            info,
            latency_ns,
            thread: Some(thread),
        })
    }

    /// The current stream (None while none is open).
    pub fn info(&self) -> Option<InputInfo> {
        self.info.lock().clone()
    }

    pub fn events(&self) -> &Receiver<InputEvent> {
        &self.events
    }

    /// The backend's capture → callback time of the last block (0 when it gives none).
    pub fn latency(&self) -> Duration {
        Duration::from_nanos(self.latency_ns.load(Ordering::Relaxed))
    }

    /// Switch device (None = follow the system default). Asynchronous.
    pub fn set_device(&self, name: Option<String>) {
        let _ = self.cmd.send(Cmd::SetDevice(name));
    }

    pub fn set_buffer_size_hint(&self, hint: Option<u32>) {
        let _ = self.cmd.send(Cmd::SetBufferHint(hint));
    }

    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        let _ = self.cmd.send(Cmd::Stop);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for InputHost {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct Supervisor {
    opts: InputOptions,
    opener: InputOpener,
    info: Arc<Mutex<Option<InputInfo>>>,
    latency_ns: Arc<AtomicU64>,
    events: Sender<InputEvent>,
    cmd_tx: Sender<Cmd>,
    stream: Option<cpal::Stream>,
    current: Option<String>,
    open_failed: bool,
}

impl Supervisor {
    fn run(mut self, cmds: Receiver<Cmd>) {
        let host = cpal::default_host();
        self.reopen(&host);
        loop {
            match cmds.recv_timeout(self.opts.poll_interval) {
                Ok(Cmd::Stop) | Err(RecvTimeoutError::Disconnected) => break,
                Ok(Cmd::SetDevice(name)) => {
                    self.opts.device_name = name;
                    self.reopen(&host);
                }
                Ok(Cmd::SetBufferHint(hint)) => {
                    self.opts.buffer_size_hint = hint;
                    self.reopen(&host);
                }
                Ok(Cmd::StreamFailed(cpal::StreamError::DeviceNotAvailable)) => {
                    let device = self.current.clone().unwrap_or_default();
                    let _ = self.events.send(InputEvent::DeviceLost { device });
                    self.reopen(&host);
                }
                Ok(Cmd::StreamFailed(cpal::StreamError::BackendSpecific { err })) => {
                    let _ = self.events.send(InputEvent::StreamError(err.description));
                }
                Err(RecvTimeoutError::Timeout) => self.poll(&host),
            }
        }
        self.stream = None;
        *self.info.lock() = None;
    }

    fn poll(&mut self, host: &cpal::Host) {
        if self.stream.is_none() {
            self.reopen(host);
            return;
        }
        let want = match &self.opts.device_name {
            // A named device that came back replaces the default it fell back to.
            Some(name) => find_input_device(host, name).map(|_| name.clone()),
            None => host.default_input_device().map(|d| device_name(&d)),
        };
        if let Some(w) = want
            && Some(&w) != self.current.as_ref()
        {
            self.reopen(host);
        }
    }

    fn reopen(&mut self, host: &cpal::Host) {
        self.stream = None;
        match self.open(host) {
            Ok(info) => {
                self.open_failed = false;
                self.current = Some(info.device_name.clone());
                *self.info.lock() = Some(info.clone());
                let _ = self.events.send(InputEvent::Opened(info));
            }
            Err(e) => {
                self.current = None;
                *self.info.lock() = None;
                if !self.open_failed {
                    let _ = self.events.send(InputEvent::OpenFailed(e.to_string()));
                }
                self.open_failed = true;
            }
        }
    }

    /// The configs to try, best first: the most channels at the context rate, then the default
    /// config (resampled when its rate differs).
    fn candidates(device: &cpal::Device, ctx: u32) -> Vec<SupportedStreamConfig> {
        let mut out = Vec::new();
        if let Ok(ranges) = device.supported_input_configs() {
            let best = ranges
                .filter(|r| {
                    format_supported(r.sample_format())
                        && r.min_sample_rate().0 <= ctx
                        && ctx <= r.max_sample_rate().0
                        && r.channels() >= 1
                })
                .max_by_key(|r| (r.channels(), r.sample_format() == SampleFormat::F32));
            if let Some(cfg) = best.and_then(|r| r.try_with_sample_rate(SampleRate(ctx))) {
                out.push(cfg);
            }
        }
        if let Ok(default) = device.default_input_config() {
            out.push(default);
        }
        out
    }

    fn open(&mut self, host: &cpal::Host) -> Result<InputInfo, HostError> {
        let requested = self.opts.device_name.clone();
        let device = requested
            .as_deref()
            .and_then(|name| find_input_device(host, name))
            .or_else(|| host.default_input_device())
            .ok_or(HostError::NoDevice)?;
        let ctx = self.opts.context_rate.max(1);
        let mut last_err = HostError::NoDevice;
        for supported in Self::candidates(&device, ctx) {
            let mut buffer =
                choose_buffer_size(self.opts.buffer_size_hint, supported.buffer_size());
            let mut config = supported.config();
            let rate = config.sample_rate.0;
            let info = |buffer| InputInfo {
                device_name: device_name(&device),
                device_rate: rate,
                context_rate: ctx,
                resampling: rate != ctx,
                channels: supported.channels(),
                buffer,
                follows_default: requested.is_none(),
            };
            if rate != ctx && supported.channels() as usize > MAX_HISTORY_CHANNELS {
                continue;
            }
            config.buffer_size = buffer.to_cpal();
            let mut built = self.build(&device, &config, supported.sample_format(), &info(buffer));
            if built.is_err() && buffer != BufferRequest::Default {
                buffer = BufferRequest::Default;
                config.buffer_size = buffer.to_cpal();
                built = self.build(&device, &config, supported.sample_format(), &info(buffer));
            }
            match built.and_then(|s| {
                s.play().map_err(|e| HostError::Play(e.to_string()))?;
                Ok(s)
            }) {
                Ok(stream) => {
                    self.stream = Some(stream);
                    return Ok(info(buffer));
                }
                Err(e) => last_err = e,
            }
        }
        Err(last_err)
    }

    fn build(
        &mut self,
        device: &cpal::Device,
        config: &StreamConfig,
        format: SampleFormat,
        info: &InputInfo,
    ) -> Result<cpal::Stream, HostError> {
        match format {
            SampleFormat::F32 => self.build_typed::<f32>(device, config, info),
            SampleFormat::F64 => self.build_typed::<f64>(device, config, info),
            SampleFormat::I16 => self.build_typed::<i16>(device, config, info),
            SampleFormat::I32 => self.build_typed::<i32>(device, config, info),
            SampleFormat::I64 => self.build_typed::<i64>(device, config, info),
            SampleFormat::I8 => self.build_typed::<i8>(device, config, info),
            SampleFormat::U8 => self.build_typed::<u8>(device, config, info),
            SampleFormat::U16 => self.build_typed::<u16>(device, config, info),
            SampleFormat::U32 => self.build_typed::<u32>(device, config, info),
            SampleFormat::U64 => self.build_typed::<u64>(device, config, info),
            other => Err(HostError::Build(format!(
                "unsupported sample format {other}"
            ))),
        }
    }

    fn build_typed<T>(
        &mut self,
        device: &cpal::Device,
        config: &StreamConfig,
        info: &InputInfo,
    ) -> Result<cpal::Stream, HostError>
    where
        T: SizedSample + Send + 'static,
        f32: FromSample<T>,
    {
        let channels = config.channels as usize;
        let mut sink = (self.opener)(info);
        let mut resampler = info.resampling.then(|| {
            InputResampler::new(
                info.device_rate,
                info.context_rate,
                channels,
                MAX_INPUT_BLOCK,
            )
        });
        let mut scratch = vec![0f32; MAX_INPUT_BLOCK * channels].into_boxed_slice();
        let latency = self.latency_ns.clone();
        let tx = self.cmd_tx.clone();
        device
            .build_input_stream::<T, _, _>(
                config,
                move |data: &[T], cb: &cpal::InputCallbackInfo| {
                    let ts = cb.timestamp();
                    if let Some(d) = ts.callback.duration_since(&ts.capture) {
                        latency.store(d.as_nanos().min(u64::MAX as u128) as u64, Ordering::Relaxed);
                    }
                    // In pieces of at most MAX_INPUT_BLOCK frames (the scratch size).
                    for piece in data.chunks(scratch.len().max(channels)) {
                        let n = piece.len() - piece.len() % channels;
                        for (o, s) in scratch[..n].iter_mut().zip(piece) {
                            *o = f32::from_sample(*s);
                        }
                        match resampler.as_mut() {
                            Some(r) => sink(r.process(&scratch[..n])),
                            None => sink(&scratch[..n]),
                        }
                    }
                },
                move |err| {
                    let _ = tx.send(Cmd::StreamFailed(err));
                },
                None,
            )
            .map_err(|e| HostError::Build(e.to_string()))
    }
}

/// Frames of scratch per input stream (larger device blocks are converted in pieces).
const MAX_INPUT_BLOCK: usize = 4096;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_resampler_keeps_the_ratio_and_a_sine_across_blocks() {
        // 48 kHz → 44.1 kHz, stereo, 1 kHz on the left, silence on the right.
        let mut r = InputResampler::new(48_000, 44_100, 2, 512);
        let mut out = Vec::new();
        let mut frame = 0usize;
        for block in [256usize, 512, 100, 512, 37, 512, 512] {
            let input: Vec<f32> = (0..block)
                .flat_map(|i| {
                    let t = (frame + i) as f32 / 48_000.0;
                    [(t * 1000.0 * std::f32::consts::TAU).sin(), 0.0]
                })
                .collect();
            frame += block;
            out.extend_from_slice(r.process(&input));
        }
        let frames = out.len() / 2;
        let want = frame as f64 * 44_100.0 / 48_000.0;
        assert!((frames as f64 - want).abs() <= 3.0, "{frames} vs {want}");
        // Output frame k is input time k·step − 2 input frames (the interpolator's delay).
        let mut worst = 0f32;
        for k in 8..frames {
            let t = (k as f64 * 48_000.0 / 44_100.0 - 2.0) / 48_000.0;
            let exact = (t * 1000.0 * std::f64::consts::TAU).sin() as f32;
            worst = worst.max((out[2 * k] - exact).abs());
            assert_eq!(out[2 * k + 1], 0.0);
        }
        assert!(worst < 2e-3, "max error {worst}");
    }

    #[test]
    fn the_resampler_upsamples_too() {
        let mut r = InputResampler::new(22_050, 44_100, 1, 256);
        let input: Vec<f32> = (0..256).map(|i| i as f32).collect();
        let out = r.process(&input).to_vec();
        assert!((out.len() as i64 - 512).abs() <= 4, "{}", out.len());
        // a ramp stays a ramp (cubic interpolation is exact on it), half the slope
        for w in out[8..].windows(2) {
            assert!((w[1] - w[0] - 0.5).abs() < 1e-3, "{w:?}");
        }
    }
}
