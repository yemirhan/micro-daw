//! cpal output host with a supervisor thread that owns the stream.

use std::fmt;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{
    FromSample, OutputCallbackInfo, SampleFormat, SampleRate, SizedSample, StreamConfig,
    SupportedStreamConfig,
};
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use parking_lot::Mutex;

use crate::device::{device_name, find_output_device};
use crate::pacer::Pacer;
use crate::{
    BufferRequest, ConfigRange, HostStats, RenderPipe, SharedRender, StatsSnapshot,
    choose_buffer_size, choose_output_config,
};

/// The context rate when none is configured and no device could be opened at start.
pub const DEFAULT_CONTEXT_RATE: u32 = 48_000;
/// Block size of the null fallback pacer (context frames).
const FALLBACK_BLOCK_FRAMES: usize = 256;

/// How to open the output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputOptions {
    /// Exact device name; None = the system default (followed when it changes).
    pub device_name: Option<String>,
    /// Settings `bufferSizeHint` (frames); None = the backend default.
    pub buffer_size_hint: Option<u32>,
    /// How often the supervisor checks for default-device changes / retries a failed open /
    /// looks for render panics.
    pub poll_interval: Duration,
    /// The render rate, fixed for the host's whole life. None = adopt the first opened device's
    /// default rate ([`DEFAULT_CONTEXT_RATE`] when the first open fails and the null fallback
    /// runs). Devices that cannot run at it are resampled.
    pub context_rate: Option<u32>,
    /// Keep calling the render callback (real-time paced, output discarded) whenever no device
    /// stream runs. With it, [`OutputHost::start`] succeeds even when no device can be opened.
    pub null_fallback: bool,
    /// The device channel the left side plays on (the right one on the next): 0 = outputs 1/2,
    /// 2 = outputs 3/4. Clamped to the device's channel count.
    pub channel_offset: u16,
}

impl Default for OutputOptions {
    fn default() -> Self {
        Self {
            device_name: None,
            buffer_size_hint: Some(256),
            poll_interval: Duration::from_millis(500),
            context_rate: None,
            null_fallback: true,
            channel_offset: 0,
        }
    }
}

/// The stream that is running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamInfo {
    pub device_name: String,
    /// The rate the device runs at.
    pub device_rate: u32,
    /// The host's fixed render rate ([`OutputHost::context_rate`]).
    pub context_rate: u32,
    /// `device_rate != context_rate`: the output is resampled (the device offers no config at
    /// the context rate).
    pub resampling: bool,
    pub channels: u16,
    pub sample_format: String,
    /// What was requested (the actual per-callback size is in [`StatsSnapshot::last_frames`]).
    pub buffer: BufferRequest,
    pub buffer_size_range: Option<(u32, u32)>,
    /// Opened as "system default" (follows default-device changes).
    pub follows_default: bool,
}

impl StreamInfo {
    /// Buffer latency of the requested fixed size (s); None for a backend-default buffer.
    pub fn nominal_buffer_latency_secs(&self) -> Option<f64> {
        match self.buffer {
            BufferRequest::Fixed(n) => Some(n as f64 / self.device_rate as f64),
            BufferRequest::Default => None,
        }
    }
}

/// Notifications for the control side ([`OutputHost::events`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostEvent {
    /// A stream (re)opened; also sent for the initial open.
    Opened(StreamInfo),
    /// Sent right after [`Self::Opened`] when that stream resamples (the device has no config
    /// at the context rate). A later `Opened` with `resampling: false` means native again.
    Resampling { context_rate: u32, device_rate: u32 },
    /// The null fallback started (`active: true`: no device stream; the render callback is
    /// paced by a thread, output discarded) or stopped (a device stream took over). Sent on
    /// transitions only.
    NullFallback { active: bool },
    /// The render callback panicked (caught on the audio thread). The output stays silent until
    /// [`OutputHost::clear_render_panic`]; sent once per new panic (noticed within a poll
    /// interval). [`StatsSnapshot::render_panics`] counts them.
    RenderPanicked,
    /// A non-fatal backend error (the stream keeps running).
    StreamError(String),
    /// The device went away; the host reopens on the default device.
    DeviceLost { device: String },
    /// The system default output changed while following it; the host reopens on it.
    DefaultDeviceChanged { from: Option<String>, to: String },
    /// The requested device is not present; the default device is used until it reappears.
    DeviceNotFound { requested: String },
    /// Opening failed (retried every poll interval until it works).
    OpenFailed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostError {
    NoDevice,
    Devices(String),
    Config(String),
    Build(String),
    Play(String),
    Thread(String),
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDevice => f.write_str("no audio output device"),
            Self::Devices(e) => write!(f, "cannot list audio devices: {e}"),
            Self::Config(e) => write!(f, "cannot read the device configuration: {e}"),
            Self::Build(e) => write!(f, "cannot open the audio stream: {e}"),
            Self::Play(e) => write!(f, "cannot start the audio stream: {e}"),
            Self::Thread(e) => write!(f, "audio host thread: {e}"),
        }
    }
}

impl std::error::Error for HostError {}

enum Cmd {
    Stop,
    SetDevice(Option<String>),
    SetBufferHint(Option<u32>),
    SetChannelOffset(u16),
    StreamFailed(cpal::StreamError),
}

/// A running device output. Dropping it (or [`OutputHost::stop`]) closes the stream (and stops
/// the null fallback).
pub struct OutputHost {
    cmd: Sender<Cmd>,
    events: Receiver<HostEvent>,
    info: Arc<Mutex<Option<StreamInfo>>>,
    stats: Arc<HostStats>,
    render: SharedRender,
    context_rate: u32,
    thread: Option<JoinHandle<()>>,
}

impl OutputHost {
    /// Open the output and start calling `render`. Returns once the first open succeeded, or
    /// failed: then, with [`OutputOptions::null_fallback`], the null fallback drives `render`
    /// and this still returns Ok (`info()` = None, [`HostEvent::OpenFailed`] +
    /// [`HostEvent::NullFallback`] queued, reopen retried every poll interval); without it, the
    /// error is returned and no thread is left running.
    pub fn start(options: OutputOptions, render: SharedRender) -> Result<Self, HostError> {
        Self::start_inner(options, render, false)
    }

    /// [`Self::start`] with every device open failing (tests the no-device paths without
    /// touching hardware).
    #[cfg(test)]
    pub(crate) fn start_failing(
        options: OutputOptions,
        render: SharedRender,
    ) -> Result<Self, HostError> {
        Self::start_inner(options, render, true)
    }

    fn start_inner(
        options: OutputOptions,
        render: SharedRender,
        force_open_failure: bool,
    ) -> Result<Self, HostError> {
        let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
        let (ev_tx, ev_rx) = crossbeam_channel::unbounded();
        let (ready_tx, ready_rx) = crossbeam_channel::bounded(1);
        let info = Arc::new(Mutex::new(None));
        let stats = Arc::new(HostStats::default());
        let (opts, r, st, inf) = (options, render.clone(), stats.clone(), info.clone());
        let cmd_tx2 = cmd_tx.clone();
        let thread = std::thread::Builder::new()
            .name("mdaw-audio-host".into())
            // The supervisor (and the !Send cpal stream) is created on its own thread.
            .spawn(move || {
                let sup = Supervisor {
                    ctx_rate: opts.context_rate.filter(|&r| r > 0),
                    opts,
                    render: r,
                    stats: st,
                    info: inf,
                    events: ev_tx,
                    cmd_tx: cmd_tx2,
                    stream: None,
                    current: None,
                    fell_back: false,
                    open_failed: false,
                    fallback: None,
                    fallback_reported: false,
                    panics_seen: 0,
                    force_open_failure,
                };
                sup.run(cmd_rx, ready_tx)
            })
            .map_err(|e| HostError::Thread(e.to_string()))?;
        match ready_rx.recv() {
            Ok(Ok(context_rate)) => Ok(Self {
                cmd: cmd_tx,
                events: ev_rx,
                info,
                stats,
                render,
                context_rate,
                thread: Some(thread),
            }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                let _ = thread.join();
                Err(HostError::Thread("supervisor exited".into()))
            }
        }
    }

    /// The render rate, fixed for the host's life (every stream runs at it or resamples to the
    /// device; the null fallback runs at it).
    pub fn context_rate(&self) -> u32 {
        self.context_rate
    }

    /// The current stream (None while no device stream runs: a reopen is failing and the null
    /// fallback, if enabled, drives the render callback).
    pub fn info(&self) -> Option<StreamInfo> {
        self.info.lock().clone()
    }

    pub fn stats(&self) -> StatsSnapshot {
        self.stats.snapshot()
    }

    pub fn stats_handle(&self) -> &Arc<HostStats> {
        &self.stats
    }

    pub fn events(&self) -> &Receiver<HostEvent> {
        &self.events
    }

    /// The shared render callback (lock it to swap state between blocks).
    pub fn render(&self) -> &SharedRender {
        &self.render
    }

    /// Resume calling the render callback after a caught render panic
    /// ([`HostEvent::RenderPanicked`]); do it once the render state is consistent again.
    pub fn clear_render_panic(&self) {
        self.stats.clear_render_panic();
    }

    /// Switch device (None = follow the system default). Asynchronous: watch for
    /// [`HostEvent::Opened`].
    pub fn set_device(&self, name: Option<String>) {
        let _ = self.cmd.send(Cmd::SetDevice(name));
    }

    /// Reopen with another buffer size hint. Asynchronous like [`Self::set_device`].
    pub fn set_buffer_size_hint(&self, hint: Option<u32>) {
        let _ = self.cmd.send(Cmd::SetBufferHint(hint));
    }

    /// Play on another channel pair of the device ([`OutputOptions::channel_offset`]).
    /// Asynchronous like [`Self::set_device`].
    pub fn set_channel_offset(&self, offset: u16) {
        let _ = self.cmd.send(Cmd::SetChannelOffset(offset));
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

impl Drop for OutputHost {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct Supervisor {
    opts: OutputOptions,
    /// Fixed once known (options, first device default, or [`DEFAULT_CONTEXT_RATE`]).
    ctx_rate: Option<u32>,
    render: SharedRender,
    stats: Arc<HostStats>,
    info: Arc<Mutex<Option<StreamInfo>>>,
    events: Sender<HostEvent>,
    cmd_tx: Sender<Cmd>,
    stream: Option<cpal::Stream>,
    /// Name of the device the stream runs on.
    current: Option<String>,
    /// The requested device was missing; running on the default instead.
    fell_back: bool,
    /// Last open failed (OpenFailed is reported once per failure streak).
    open_failed: bool,
    /// The null fallback pacer (only while `stream` is None).
    fallback: Option<Pacer>,
    /// The fallback state last reported with [`HostEvent::NullFallback`].
    fallback_reported: bool,
    /// `render_panics` already reported with [`HostEvent::RenderPanicked`].
    panics_seen: u64,
    force_open_failure: bool,
}

impl Supervisor {
    fn run(mut self, cmds: Receiver<Cmd>, ready: Sender<Result<u32, HostError>>) {
        let host = cpal::default_host();
        match self.open(&host) {
            Ok(info) => {
                self.publish(info);
                let _ = ready.send(Ok(self.ctx()));
            }
            Err(e) if self.opts.null_fallback => {
                self.open_failed = true;
                let _ = self.events.send(HostEvent::OpenFailed(e.to_string()));
                self.start_fallback();
                if self.fallback.is_none() {
                    let _ = ready.send(Err(e));
                    return;
                }
                self.report_fallback();
                let _ = ready.send(Ok(self.ctx()));
            }
            Err(e) => {
                let _ = ready.send(Err(e));
                return;
            }
        }
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
                Ok(Cmd::SetChannelOffset(offset)) => {
                    if self.opts.channel_offset != offset {
                        self.opts.channel_offset = offset;
                        self.reopen(&host);
                    }
                }
                Ok(Cmd::StreamFailed(cpal::StreamError::DeviceNotAvailable)) => {
                    let device = self.current.clone().unwrap_or_default();
                    let _ = self.events.send(HostEvent::DeviceLost { device });
                    self.reopen(&host);
                }
                Ok(Cmd::StreamFailed(cpal::StreamError::BackendSpecific { err })) => {
                    let _ = self.events.send(HostEvent::StreamError(err.description));
                }
                Err(RecvTimeoutError::Timeout) => self.poll(&host),
            }
            self.check_render_panics();
        }
        self.stream = None;
        if let Some(p) = self.fallback.take() {
            p.stop();
        }
    }

    fn ctx(&mut self) -> u32 {
        *self.ctx_rate.get_or_insert(DEFAULT_CONTEXT_RATE)
    }

    /// Start the null fallback if enabled and no driver runs (no event: see `report_fallback`).
    fn start_fallback(&mut self) {
        if !self.opts.null_fallback || self.fallback.is_some() || self.stream.is_some() {
            return;
        }
        let rate = self.ctx();
        match Pacer::spawn(
            "mdaw-audio-fallback",
            rate,
            rate,
            FALLBACK_BLOCK_FRAMES,
            self.render.clone(),
            self.stats.clone(),
            None,
        ) {
            Ok(p) => {
                self.stats.reset_range();
                self.fallback = Some(p);
            }
            Err(e) => {
                let _ = self.events.send(HostEvent::StreamError(e.to_string()));
            }
        }
    }

    /// Stop (join) the null fallback: no render call from it happens after this returns.
    fn stop_fallback(&mut self) {
        if let Some(p) = self.fallback.take() {
            p.stop();
        }
    }

    /// Send [`HostEvent::NullFallback`] when the state differs from the last report, so a
    /// failing reopen (stop → build fails → restart) does not flap.
    fn report_fallback(&mut self) {
        let active = self.fallback.is_some();
        if active != self.fallback_reported {
            self.fallback_reported = active;
            let _ = self.events.send(HostEvent::NullFallback { active });
        }
    }

    fn check_render_panics(&mut self) {
        let n = self.stats.snapshot().render_panics;
        if n > self.panics_seen {
            self.panics_seen = n;
            let _ = self.events.send(HostEvent::RenderPanicked);
        }
    }

    fn poll(&mut self, host: &cpal::Host) {
        if self.stream.is_none() {
            self.reopen(host);
            return;
        }
        match &self.opts.device_name {
            None => {
                let now = host.default_output_device().map(|d| device_name(&d));
                if let Some(to) = now
                    && Some(&to) != self.current.as_ref()
                {
                    let from = self.current.clone();
                    let _ = self
                        .events
                        .send(HostEvent::DefaultDeviceChanged { from, to });
                    self.reopen(host);
                }
            }
            Some(name) => {
                if self.fell_back && find_output_device(host, name).is_some() {
                    self.reopen(host);
                }
            }
        }
    }

    fn reopen(&mut self, host: &cpal::Host) {
        self.stream = None;
        // Keep the core rendering while the devices are enumerated (open stops it before
        // building the new stream).
        self.start_fallback();
        match self.open(host) {
            Ok(info) => {
                self.open_failed = false;
                self.report_fallback();
                self.publish(info);
            }
            Err(e) => {
                *self.info.lock() = None;
                self.current = None;
                if !self.open_failed {
                    let _ = self.events.send(HostEvent::OpenFailed(e.to_string()));
                }
                self.open_failed = true;
                self.start_fallback();
                self.report_fallback();
            }
        }
    }

    fn publish(&mut self, info: StreamInfo) {
        self.current = Some(info.device_name.clone());
        *self.info.lock() = Some(info.clone());
        let resampling = info.resampling.then_some(HostEvent::Resampling {
            context_rate: info.context_rate,
            device_rate: info.device_rate,
        });
        let _ = self.events.send(HostEvent::Opened(info));
        if let Some(ev) = resampling {
            let _ = self.events.send(ev);
        }
    }

    /// The configs to try, best first: one at the context rate when the device has it (its
    /// default config if that already runs at it), then the default config (resampled).
    fn candidate_configs(
        device: &cpal::Device,
        default: SupportedStreamConfig,
        ctx: u32,
    ) -> Vec<SupportedStreamConfig> {
        if default.sample_rate().0 == ctx {
            return vec![default];
        }
        let mut out = Vec::with_capacity(2);
        if let Ok(ranges) = device.supported_output_configs() {
            let ranges: Vec<cpal::SupportedStreamConfigRange> = ranges.collect();
            let reduced: Vec<ConfigRange> = ranges.iter().map(ConfigRange::from).collect();
            let pick =
                choose_output_config(&reduced, ctx, default.channels(), default.sample_format());
            if let Some(cfg) = pick.and_then(|i| ranges[i].try_with_sample_rate(SampleRate(ctx))) {
                out.push(cfg);
            }
        }
        out.push(default);
        out
    }

    fn open(&mut self, host: &cpal::Host) -> Result<StreamInfo, HostError> {
        if self.force_open_failure {
            return Err(HostError::NoDevice);
        }
        let requested = self.opts.device_name.clone();
        let device = match &requested {
            Some(name) => match find_output_device(host, name) {
                Some(d) => {
                    self.fell_back = false;
                    d
                }
                None => {
                    if !self.fell_back {
                        let _ = self.events.send(HostEvent::DeviceNotFound {
                            requested: name.clone(),
                        });
                    }
                    self.fell_back = true;
                    host.default_output_device().ok_or(HostError::NoDevice)?
                }
            },
            None => {
                self.fell_back = false;
                host.default_output_device().ok_or(HostError::NoDevice)?
            }
        };
        let default = device
            .default_output_config()
            .map_err(|e| HostError::Config(e.to_string()))?;
        let ctx = *self.ctx_rate.get_or_insert(default.sample_rate().0);
        let candidates = Self::candidate_configs(&device, default, ctx);
        // Two drivers never run at once: the fallback is joined before a stream exists.
        self.stop_fallback();
        let mut last_err = HostError::NoDevice;
        for supported in candidates {
            match self.start_stream(&device, &supported, ctx) {
                Ok((stream, buffer)) => {
                    self.stream = Some(stream);
                    let device_rate = supported.sample_rate().0;
                    return Ok(StreamInfo {
                        device_name: device_name(&device),
                        device_rate,
                        context_rate: ctx,
                        resampling: device_rate != ctx,
                        channels: supported.channels(),
                        sample_format: supported.sample_format().to_string(),
                        buffer,
                        buffer_size_range: match supported.buffer_size() {
                            cpal::SupportedBufferSize::Range { min, max } => Some((*min, *max)),
                            cpal::SupportedBufferSize::Unknown => None,
                        },
                        follows_default: requested.is_none(),
                    });
                }
                Err(e) => last_err = e,
            }
        }
        Err(last_err)
    }

    /// Build and play a stream with `supported` (the hinted fixed buffer, then the backend
    /// default buffer).
    fn start_stream(
        &self,
        device: &cpal::Device,
        supported: &SupportedStreamConfig,
        ctx: u32,
    ) -> Result<(cpal::Stream, BufferRequest), HostError> {
        let format = supported.sample_format();
        let mut buffer = choose_buffer_size(self.opts.buffer_size_hint, supported.buffer_size());
        let mut config = supported.config();
        config.buffer_size = buffer.to_cpal();
        let stream = match self.build(device, &config, format, ctx) {
            Ok(s) => s,
            Err(_) if buffer != BufferRequest::Default => {
                buffer = BufferRequest::Default;
                config.buffer_size = buffer.to_cpal();
                self.build(device, &config, format, ctx)?
            }
            Err(e) => return Err(e),
        };
        self.stats.reset_range();
        stream.play().map_err(|e| HostError::Play(e.to_string()))?;
        Ok((stream, buffer))
    }

    fn build(
        &self,
        device: &cpal::Device,
        config: &StreamConfig,
        format: SampleFormat,
        ctx: u32,
    ) -> Result<cpal::Stream, HostError> {
        match format {
            SampleFormat::F32 => self.build_typed::<f32>(device, config, ctx),
            SampleFormat::F64 => self.build_typed::<f64>(device, config, ctx),
            SampleFormat::I16 => self.build_typed::<i16>(device, config, ctx),
            SampleFormat::I32 => self.build_typed::<i32>(device, config, ctx),
            SampleFormat::I64 => self.build_typed::<i64>(device, config, ctx),
            SampleFormat::I8 => self.build_typed::<i8>(device, config, ctx),
            SampleFormat::U8 => self.build_typed::<u8>(device, config, ctx),
            SampleFormat::U16 => self.build_typed::<u16>(device, config, ctx),
            SampleFormat::U32 => self.build_typed::<u32>(device, config, ctx),
            SampleFormat::U64 => self.build_typed::<u64>(device, config, ctx),
            other => Err(HostError::Build(format!(
                "unsupported sample format {other}"
            ))),
        }
    }

    fn build_typed<T>(
        &self,
        device: &cpal::Device,
        config: &StreamConfig,
        ctx: u32,
    ) -> Result<cpal::Stream, HostError>
    where
        T: SizedSample + FromSample<f32> + Send + 'static,
    {
        let mut pipe = RenderPipe::new(
            self.render.clone(),
            ctx,
            config.sample_rate.0,
            self.stats.clone(),
        )
        .with_channel_offset(self.opts.channel_offset as usize);
        let channels = config.channels as usize;
        let stats = self.stats.clone();
        let tx = self.cmd_tx.clone();
        device
            .build_output_stream::<T, _, _>(
                config,
                move |data: &mut [T], info: &OutputCallbackInfo| {
                    let now = Instant::now();
                    let ts = info.timestamp();
                    let latency = ts.playback.duration_since(&ts.callback);
                    if let Some(d) = latency {
                        stats.record_latency_ns(d.as_nanos().min(u64::MAX as u128) as u64);
                    }
                    pipe.fill_at(data, channels, now, latency.unwrap_or(Duration::ZERO));
                },
                move |err| {
                    let _ = tx.send(Cmd::StreamFailed(err));
                },
                None,
            )
            .map_err(|e| HostError::Build(e.to_string()))
    }
}

#[cfg(test)]
#[path = "host_tests.rs"]
mod tests;
