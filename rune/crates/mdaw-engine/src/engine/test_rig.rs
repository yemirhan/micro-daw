//! Offline rig for the engine tests: the real engine + core (no mock), rendered quantum by
//! quantum, with the UI frame (`tick`) after every render, and test sample sources.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use mdaw_dsp::buffer::{AudioBuffer, SharedBuffer};
use mdaw_model::doc::ArrangementDoc;
use mdaw_model::types::{KitState, PadSample, PadSampleSource, PadSettings};
use mdaw_model::{PAD_COUNT, PadIndex};

use super::{ArrangementEngine, AudioSamples, EngineEvent};
use crate::buffers::{FileLoader, FsReader, LoadResult, PadBufferCache};
use crate::core::{Core, LiveInput};

pub const SR: u32 = 48_000;
pub const BPM: f64 = 120.0;
pub const SPB: f64 = 60.0 / BPM;

/// Synthetic pad samples for `{ kind: 'file', path: '/t/<name>' }`: `click` = 20 ms decaying
/// burst; `long` = 2 s of DC 0.3 (gate tests); `tone` = 1 s 440 Hz.
#[derive(Default)]
pub struct TestFiles {
    pub loads: Mutex<Vec<String>>,
}

pub fn synth_buffer(name: &str) -> AudioBuffer {
    let n = |s: f64| (s * SR as f64) as usize;
    let data: Vec<f32> = match name {
        "long" => vec![0.3; n(2.0)],
        "tone" => (0..n(1.0))
            .map(|i| {
                (0.4 * (2.0 * std::f64::consts::PI * 440.0 * i as f64 / SR as f64).sin()) as f32
            })
            .collect(),
        _ => (0..n(0.02))
            .map(|i| (0.6 * (-(i as f64) / 200.0).exp()) as f32)
            .collect(),
    };
    AudioBuffer::mono(SR, data)
}

impl FileLoader for TestFiles {
    fn load_file(&self, _cache: &PadBufferCache, path: &str) -> LoadResult {
        self.loads.lock().unwrap().push(path.to_owned());
        let name = path.rsplit('/').next().unwrap_or("click");
        Ok(Arc::new(synth_buffer(name)))
    }
    fn reload_by_path_prefix(&self, _prefix: &str) {}
}

/// A SampleManager stand-in for Song audio regions (buffers by sample id; N5 flags).
#[derive(Default)]
pub struct TestSamples {
    pub buffers: Mutex<HashMap<String, SharedBuffer>>,
    pub unavailable: Mutex<Vec<String>>,
}

impl AudioSamples for TestSamples {
    fn buffer(&self, id: &str) -> Option<SharedBuffer> {
        self.buffers.lock().unwrap().get(id).cloned()
    }
    fn is_unavailable(&self, id: &str) -> bool {
        self.unavailable.lock().unwrap().iter().any(|x| x == id)
    }
}

pub fn file_pad(name: &str) -> PadSettings {
    PadSettings {
        sample: Some(PadSample {
            name: name.to_owned(),
            source: PadSampleSource::File {
                path: format!("/t/{name}"),
                sample_id: None,
            },
        }),
        ..PadSettings::default()
    }
}

/// A kit of `click` pads (pad `i` → `/t/click`), `long` on the pads in `long`.
pub fn test_kit(long: &[PadIndex]) -> KitState {
    KitState {
        id: "test".into(),
        name: "Test".into(),
        pads: (0..PAD_COUNT)
            .map(|i| file_pad(if long.contains(&i) { "long" } else { "click" }))
            .collect(),
        extra: Default::default(),
    }
}

pub struct Rig {
    pub e: ArrangementEngine,
    pub core: Core,
    pub live: LiveInput,
    pub left: Vec<f32>,
    pub right: Vec<f32>,
    pub events: Vec<EngineEvent>,
    pub captures: Vec<crate::core::PadCapture>,
    pub samples: Arc<TestSamples>,
}

impl Rig {
    pub fn new() -> Self {
        let mut n = 0u64;
        let doc = ArrangementDoc::with_id_generator(Box::new(move || {
            n += 1;
            format!("id-{n}")
        }));
        Self::with_doc(doc)
    }

    pub fn with_doc(doc: ArrangementDoc) -> Self {
        let files = Arc::new(TestFiles::default());
        let cache = PadBufferCache::new(SR, None, Arc::new(FsReader), files.clone());
        let samples = Arc::new(TestSamples::default());
        let (e, core, live) = ArrangementEngine::with_doc(doc, SR, cache, samples.clone());
        Self {
            e,
            core,
            live,
            left: Vec::new(),
            right: Vec::new(),
            events: Vec::new(),
            captures: Vec::new(),
            samples,
        }
    }

    /// Core time.
    pub fn now(&self) -> f64 {
        self.e.now()
    }

    /// Render whole quanta for at least `seconds`, one UI frame (`tick`) per quantum.
    pub fn run(&mut self, seconds: f64) {
        let quanta = ((seconds * SR as f64) / 128.0).ceil() as usize;
        for _ in 0..quanta {
            let (mut l, mut r) = ([0f32; 128], [0f32; 128]);
            self.core.process(&mut l, &mut r);
            self.left.extend_from_slice(&l);
            self.right.extend_from_slice(&r);
            self.e.tick();
            self.events.extend(self.e.take_ui_events());
            self.captures.append(&mut self.e.capture_log);
        }
    }

    /// Render until core time ≥ `t`.
    pub fn run_until(&mut self, t: f64) {
        if t > self.now() {
            self.run(t - self.now());
        }
    }

    /// A live pad hit (MIDI thread) at context time `time` (None: next quantum).
    pub fn pad_down(&mut self, pad: PadIndex, velocity: f64, time: Option<f64>) {
        self.live.pad_down(pad, velocity, time);
    }

    pub fn pad_up(&mut self, pad: PadIndex, time: Option<f64>) {
        self.live.pad_up(pad, time);
    }

    /// Hit `pad` when the transport is at `beat` (exactly, by time), then render past it.
    pub fn hit_at_beat(&mut self, beat: f64, pad: PadIndex, velocity: f64) {
        let t = self.time_of_beat(beat);
        self.pad_down(pad, velocity, Some(t));
        self.pad_up(pad, Some(t + 0.001));
        self.run_until(t + 0.01);
    }

    /// The context time the running transport reaches `beat` (no loop folding: the next time
    /// the playhead passes it, `beat` given in the transport's own numbers).
    pub fn time_of_beat(&self, beat: f64) -> f64 {
        let now = self.now();
        let pos = self.e.get_position();
        let bpm = self.e.doc().get_bpm();
        now + (beat - pos) * 60.0 / bpm
    }

    /// Pattern hits (track, pad, time) seen so far.
    pub fn pattern_hits(&self) -> Vec<(String, PadIndex, f64)> {
        self.events
            .iter()
            .filter_map(|e| match e {
                EngineEvent::PatternHit {
                    track_id,
                    pad,
                    time,
                } => Some((track_id.clone(), *pad, *time)),
                _ => None,
            })
            .collect()
    }

    pub fn clicks(&self) -> Vec<(f64, bool)> {
        self.events
            .iter()
            .filter_map(|e| match e {
                EngineEvent::MetronomeClick { time, accent, .. } => Some((*time, *accent)),
                _ => None,
            })
            .collect()
    }

    /// RMS of the output (both channels) over context seconds [a, b).
    pub fn rms(&self, a: f64, b: f64) -> f64 {
        let (fa, fb) = (
            (a * SR as f64) as usize,
            ((b * SR as f64) as usize).min(self.left.len()),
        );
        if fb <= fa {
            return 0.0;
        }
        let s: f64 = (fa..fb)
            .map(|i| (self.left[i] as f64).powi(2) + (self.right[i] as f64).powi(2))
            .sum();
        (s / (2 * (fb - fa)) as f64).sqrt()
    }

    /// First frame ≥ `from` whose |sample| exceeds `thr`.
    pub fn onset_after(&self, from: f64, thr: f32) -> Option<f64> {
        let f = (from * SR as f64) as usize;
        (f..self.left.len())
            .find(|&i| self.left[i].abs() > thr || self.right[i].abs() > thr)
            .map(|i| i as f64 / SR as f64)
    }

    /// The active drum track's pattern notes as (pad, start, velocity).
    pub fn pattern(&self, track: &str) -> Vec<(PadIndex, f64, f64)> {
        self.e
            .doc()
            .get_pattern_region(track)
            .map(|r| {
                r.notes
                    .iter()
                    .map(|n| ((n.note - 36.0) as PadIndex, n.start_beat, n.velocity))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// A drum track with the test kit, made active; returns its id.
    pub fn drum_track(&mut self) -> String {
        let id = self.e.edit(|d| d.ensure_drum_track());
        self.e.edit(|d| {
            d.set_kit(&id, &test_kit(&[]), Default::default());
            d.clear_history();
        });
        self.e.wait_kits_ready();
        id
    }
}
