//! The app's audio I/O service: the device **input** (opened while an audio track is armed) and
//! what follows from the devices in use (the recording latency compensation).
//!
//! The input is not kept open all the time: macOS shows the microphone indicator (and, the
//! first time, asks for permission) for as long as an app listens, so the app listens only
//! while a track wants its input. Arming an audio track opens the configured input device
//! (Settings → Audio; the system default input unless one is chosen) through
//! `mdaw_audio_host::InputHost`; each stream it opens hands the engine a fresh input queue
//! ([`mdaw_engine::io::input_pair`]) sized for the device's channels. Disarming the last one
//! closes it. With no input device chosen, the inputs of the device the output plays on are
//! used when it has any (an audio interface is usually both), else the system default input.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use mdaw_audio_host::{
    BufferRequest, InputEvent, InputHost, InputInfo, InputOpener, InputOptions, OutputHost,
};
use mdaw_engine::engine::ArrangementEngine;
use mdaw_engine::io::{InputSource, input_pair};

/// See the module docs.
pub struct AudioIo {
    rate: u32,
    host: Option<InputHost>,
    tx: Sender<InputSource>,
    rx: Receiver<InputSource>,
    waker: Arc<Mutex<Option<rune::Waker>>>,
    /// Settings: the input device (None: the system default) and the buffer size hint.
    device: Option<String>,
    buffer_hint: Option<u32>,
    /// The stream that is open (None: closed, or still opening).
    pub info: Option<InputInfo>,
    /// Why the input could not be opened (shown next to the armed tracks).
    pub error: Option<String>,
    /// Bumped when `info` / `error` change.
    pub version: u64,
}

impl AudioIo {
    pub fn new(rate: u32) -> Self {
        let (tx, rx) = channel();
        Self {
            rate,
            host: None,
            tx,
            rx,
            waker: Arc::default(),
            device: None,
            buffer_hint: Some(256),
            info: None,
            error: None,
            version: 0,
        }
    }

    pub fn set_waker(&mut self, w: rune::Waker) {
        *self.waker.lock().unwrap_or_else(|e| e.into_inner()) = Some(w);
    }

    /// Whether the input stream is (being) opened.
    pub fn is_listening(&self) -> bool {
        self.host.is_some()
    }

    /// The settings' input device and buffer size (applied to a running stream at once).
    pub fn configure(&mut self, device: Option<String>, buffer_hint: Option<u32>) {
        if let Some(h) = &self.host {
            if self.device != device {
                h.set_device(device.clone());
            }
            if self.buffer_hint != buffer_hint {
                h.set_buffer_size_hint(buffer_hint);
            }
        }
        self.device = device;
        self.buffer_hint = buffer_hint;
    }

    fn opener(&self) -> InputOpener {
        let (tx, waker, rate) = (self.tx.clone(), self.waker.clone(), self.rate);
        Box::new(move |info: &InputInfo| {
            // The block size in context frames (a hint: the queue measures the real one).
            let block = match info.buffer {
                BufferRequest::Fixed(n) => {
                    (n as u64 * info.context_rate as u64 / info.device_rate.max(1) as u64) as usize
                }
                BufferRequest::Default => 512,
            };
            let (mut feed, source) = input_pair(info.channels as usize, block, rate);
            let _ = tx.send(source);
            if let Some(w) = waker.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                w();
            }
            Box::new(move |frames: &[f32]| feed.push_interleaved(frames))
        })
    }

    /// Open the input when `wanted` and it is closed; close it when it is not wanted.
    /// `output_device`: the device the output plays on. Without a chosen input device its
    /// inputs are used when it has any (an audio interface is both), else the system default
    /// input.
    pub fn set_wanted(
        &mut self,
        wanted: bool,
        engine: &mut ArrangementEngine,
        output_device: Option<String>,
    ) {
        if wanted == self.host.is_some() {
            return;
        }
        if wanted {
            let mut options = InputOptions::new(self.rate);
            // A named device that has no inputs falls back to the default input.
            options.device_name = self.device.clone().or(output_device);
            options.buffer_size_hint = self.buffer_hint;
            match InputHost::start(options, self.opener()) {
                Ok(h) => self.host = Some(h),
                Err(e) => self.error = Some(e.to_string()),
            }
        } else {
            self.host = None; // joins the stream's thread: no more blocks after this
            while self.rx.try_recv().is_ok() {}
            engine.set_input_source(None);
            self.info = None;
            self.error = None;
        }
        self.version += 1;
    }

    /// One UI frame: hand a newly opened stream's queue to the engine, read the stream's
    /// events, keep the engine's latency compensation current. True when the status changed.
    pub fn poll(&mut self, engine: &mut ArrangementEngine, output: Option<&OutputHost>) -> bool {
        let mut changed = false;
        // Only the newest queue is live (a reopen made the older ones dead).
        let mut newest = None;
        while let Ok(s) = self.rx.try_recv() {
            newest = Some(s);
        }
        if let Some(source) = newest
            && self.host.is_some()
        {
            engine.set_input_source(Some(source));
            changed = true;
        }
        let Some(host) = &self.host else {
            return changed;
        };
        while let Ok(e) = host.events().try_recv() {
            changed = true;
            match e {
                InputEvent::Opened(info) => {
                    self.info = Some(info);
                    self.error = None;
                }
                InputEvent::DeviceLost { device } => {
                    self.info = None;
                    self.error = Some(format!("{device} was disconnected"));
                    engine.set_input_source(None);
                }
                InputEvent::OpenFailed(e) => {
                    self.info = None;
                    self.error = Some(e);
                }
                InputEvent::StreamError(e) => eprintln!("[audio input] {e}"),
            }
        }
        // What is played against the click is heard after the output latency, captured after
        // the input latency, and read by the engine after the queue's cushion (between one and
        // two input blocks: one and a half on average).
        let out = output.map_or(0.0, |o| o.stats().output_latency_ns as f64 / 1e9);
        let cushion = engine.input_stats().map_or(0.0, |s| {
            1.5 * s.block_frames() as f64 / self.rate.max(1) as f64
        });
        let latency = out + host.latency().as_secs_f64() + cushion;
        if (latency - engine.record_latency()).abs() > 1e-4 {
            engine.set_record_latency(latency);
        }
        if changed {
            self.version += 1;
        }
        changed
    }
}

/// The audio devices as the settings and the track input menus list them (read when the
/// Settings page or an input menu opens: enumerating talks to the audio server).
#[derive(Debug, Clone, Default)]
pub struct AudioDevices {
    pub outputs: Vec<mdaw_audio_host::DeviceInfo>,
    pub inputs: Vec<mdaw_audio_host::InputDeviceInfo>,
    pub loaded: bool,
}

impl AudioDevices {
    /// Enumerate the devices now.
    pub fn refresh(&mut self) {
        self.outputs = mdaw_audio_host::list_output_devices().unwrap_or_default();
        self.inputs = mdaw_audio_host::list_input_devices().unwrap_or_default();
        self.loaded = true;
    }

    /// The input device the settings name (else the system default one).
    pub fn input(
        &self,
        audio: &mdaw_model::types::AudioSettings,
    ) -> Option<&mdaw_audio_host::InputDeviceInfo> {
        audio
            .input_device
            .as_deref()
            .and_then(|n| self.inputs.iter().find(|d| d.name == n))
            .or_else(|| self.inputs.iter().find(|d| d.is_default))
    }

    /// The output device the settings name (else the system default one).
    pub fn output(
        &self,
        audio: &mdaw_model::types::AudioSettings,
    ) -> Option<&mdaw_audio_host::DeviceInfo> {
        audio
            .output_device
            .as_deref()
            .and_then(|n| self.outputs.iter().find(|d| d.name == n))
            .or_else(|| self.outputs.iter().find(|d| d.is_default))
    }

    /// Channels of the configured input device (2 when unknown).
    pub fn input_channels(&self, audio: &mdaw_model::types::AudioSettings) -> usize {
        self.input(audio).map_or(2, |d| d.channels.max(1) as usize)
    }

    /// Channels of the configured output device (2 when unknown).
    pub fn output_channels(&self, audio: &mdaw_model::types::AudioSettings) -> usize {
        self.output(audio).map_or(2, |d| d.channels.max(1) as usize)
    }
}
