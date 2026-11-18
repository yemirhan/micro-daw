//! cpal output host with a supervisor thread that owns the stream.

use std::fmt;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, OutputCallbackInfo, SampleFormat, SizedSample, StreamConfig};
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use parking_lot::Mutex;

use crate::device::{device_name, find_output_device};
use crate::{
    BufferRequest, HostStats, MAX_BLOCK_FRAMES, RenderPipe, SharedRender, StatsSnapshot,
    choose_buffer_size,
};

/// How to open the output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputOptions {
    /// Exact device name; None = the system default (followed when it changes).
    pub device_name: Option<String>,
    /// Settings `bufferSizeHint` (frames); None = the backend default.
    pub buffer_size_hint: Option<u32>,
    /// How often the supervisor checks for default-device changes / retries a failed open.
    pub poll_interval: Duration,
}

impl Default for OutputOptions {
    fn default() -> Self {
        Self {
            device_name: None,
            buffer_size_hint: Some(256),
            poll_interval: Duration::from_millis(500),
        }
    }
}

/// The stream that is running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamInfo {
    pub device_name: String,
    pub sample_rate: u32,
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
            BufferRequest::Fixed(n) => Some(n as f64 / self.sample_rate as f64),
            BufferRequest::Default => None,
        }
    }
}

/// Notifications for the control side ([`OutputHost::events`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostEvent {
    /// A stream (re)opened; also sent for the initial open.
    Opened(StreamInfo),
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
    StreamFailed(cpal::StreamError),
}

/// A running device output. Dropping it (or [`OutputHost::stop`]) closes the stream.
pub struct OutputHost {
    cmd: Sender<Cmd>,
    events: Receiver<HostEvent>,
    info: Arc<Mutex<Option<StreamInfo>>>,
    stats: Arc<HostStats>,
    render: SharedRender,
    thread: Option<JoinHandle<()>>,
}

impl OutputHost {
    /// Open the output and start calling `render`. Returns once the first open succeeded (or
    /// failed: then no thread is left running).
    pub fn start(options: OutputOptions, render: SharedRender) -> Result<Self, HostError> {
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
                };
                sup.run(cmd_rx, ready_tx)
            })
            .map_err(|e| HostError::Thread(e.to_string()))?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                cmd: cmd_tx,
                events: ev_rx,
                info,
                stats,
                render,
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

    /// The current stream (None while a reopen is failing).
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

    /// Switch device (None = follow the system default). Asynchronous: watch for
    /// [`HostEvent::Opened`].
    pub fn set_device(&self, name: Option<String>) {
        let _ = self.cmd.send(Cmd::SetDevice(name));
    }

    /// Reopen with another buffer size hint. Asynchronous like [`Self::set_device`].
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

impl Drop for OutputHost {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct Supervisor {
    opts: OutputOptions,
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
}

impl Supervisor {
    fn run(mut self, cmds: Receiver<Cmd>, ready: Sender<Result<(), HostError>>) {
        let host = cpal::default_host();
        match self.open(&host) {
            Ok(info) => {
                self.publish(info);
                let _ = ready.send(Ok(()));
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
        }
        self.stream = None;
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
        match self.open(host) {
            Ok(info) => {
                self.open_failed = false;
                self.publish(info);
            }
            Err(e) => {
                *self.info.lock() = None;
                self.current = None;
                if !self.open_failed {
                    let _ = self.events.send(HostEvent::OpenFailed(e.to_string()));
                }
                self.open_failed = true;
            }
        }
    }

    fn publish(&mut self, info: StreamInfo) {
        self.current = Some(info.device_name.clone());
        *self.info.lock() = Some(info.clone());
        let _ = self.events.send(HostEvent::Opened(info));
    }

    fn open(&mut self, host: &cpal::Host) -> Result<StreamInfo, HostError> {
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
        let supported = device
            .default_output_config()
            .map_err(|e| HostError::Config(e.to_string()))?;
        let format = supported.sample_format();
        let mut buffer = choose_buffer_size(self.opts.buffer_size_hint, supported.buffer_size());
        let mut config = supported.config();
        config.buffer_size = buffer.to_cpal();
        let stream = match self.build(&device, &config, format) {
            Ok(s) => s,
            Err(_) if buffer != BufferRequest::Default => {
                buffer = BufferRequest::Default;
                config.buffer_size = buffer.to_cpal();
                self.build(&device, &config, format)?
            }
            Err(e) => return Err(e),
        };
        self.stats.reset_range();
        stream.play().map_err(|e| HostError::Play(e.to_string()))?;
        self.stream = Some(stream);
        Ok(StreamInfo {
            device_name: device_name(&device),
            sample_rate: config.sample_rate.0,
            channels: config.channels,
            sample_format: format.to_string(),
            buffer,
            buffer_size_range: match supported.buffer_size() {
                cpal::SupportedBufferSize::Range { min, max } => Some((*min, *max)),
                cpal::SupportedBufferSize::Unknown => None,
            },
            follows_default: requested.is_none(),
        })
    }

    fn build(
        &self,
        device: &cpal::Device,
        config: &StreamConfig,
        format: SampleFormat,
    ) -> Result<cpal::Stream, HostError> {
        match format {
            SampleFormat::F32 => self.build_typed::<f32>(device, config),
            SampleFormat::F64 => self.build_typed::<f64>(device, config),
            SampleFormat::I16 => self.build_typed::<i16>(device, config),
            SampleFormat::I32 => self.build_typed::<i32>(device, config),
            SampleFormat::I64 => self.build_typed::<i64>(device, config),
            SampleFormat::I8 => self.build_typed::<i8>(device, config),
            SampleFormat::U8 => self.build_typed::<u8>(device, config),
            SampleFormat::U16 => self.build_typed::<u16>(device, config),
            SampleFormat::U32 => self.build_typed::<u32>(device, config),
            SampleFormat::U64 => self.build_typed::<u64>(device, config),
            other => Err(HostError::Build(format!(
                "unsupported sample format {other}"
            ))),
        }
    }

    fn build_typed<T>(
        &self,
        device: &cpal::Device,
        config: &StreamConfig,
    ) -> Result<cpal::Stream, HostError>
    where
        T: SizedSample + FromSample<f32> + Send + 'static,
    {
        let mut pipe = RenderPipe::new(self.render.clone(), MAX_BLOCK_FRAMES, self.stats.clone());
        let channels = config.channels as usize;
        let stats = self.stats.clone();
        let tx = self.cmd_tx.clone();
        device
            .build_output_stream::<T, _, _>(
                config,
                move |data: &mut [T], info: &OutputCallbackInfo| {
                    let ts = info.timestamp();
                    if let Some(d) = ts.playback.duration_since(&ts.callback) {
                        stats.record_latency_ns(d.as_nanos().min(u64::MAX as u128) as u64);
                    }
                    pipe.fill(data, channels);
                },
                move |err| {
                    let _ = tx.send(Cmd::StreamFailed(err));
                },
                None,
            )
            .map_err(|e| HostError::Build(e.to_string()))
    }
}
