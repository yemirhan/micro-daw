//! `services/SampleManager.ts` + `hooks/useSampleLibrary.ts`: decoded samples by id (the
//! Song view's audio regions and the Samples view's library), their peaks, the library list
//! (persisted under `micro-daw-sample-library`), audition, and the SampleProcessor jobs.
//!
//! - **Decoding** runs on worker threads (`sampleReadFile` + `decodeAudioData` at the context
//!   rate, through the engine's `PadBufferCache::decode`); headless runs decode synchronously so a
//!   snapshot or a test sees the result at once.
//! - The engine reads the buffers through [`SampleStore`] ([`mdaw_engine::engine::AudioSamples`]):
//!   audio regions schedule them at the next play, as Tone players do.
//! - **Audition** plays the trimmed range at unity gain (`Tone.Player(...).toDestination()`)
//!   through the engine's preview slot (its 0.9 × −6 dB gain is compensated), progress follows
//!   the UI clock.
//! - **Process / Generate** run [`mdaw_engine::sample_processor`] on a worker thread and register
//!   the result as a new library entry with an empty path (not persisted, like the TS).

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, RwLock};

use mdaw_dsp::buffer::{AudioBuffer, SharedBuffer};
use mdaw_engine::buffers::PadBufferCache;
use mdaw_engine::engine::{AudioSamples, PREVIEW_GAIN, PREVIEW_VOLUME_DB, db_gain};
use mdaw_engine::sample_processor::{
    MathRandom, NoiseBank, ProcessParams, SignalParams, generate_signal, process_audio, signal_name,
};
use mdaw_model::settings::{
    SerializedSampleEntry, SettingsStore, read_sample_library, write_sample_library,
};
use mdaw_model::types::{SampleRef, SampleTrim};

/// The engine's view of the decoded samples (shared with the audio-region players).
#[derive(Default)]
pub struct SampleStore {
    buffers: RwLock<HashMap<String, SharedBuffer>>,
    unavailable: RwLock<HashSet<String>>,
}

impl AudioSamples for SampleStore {
    fn buffer(&self, sample_id: &str) -> Option<SharedBuffer> {
        self.buffers
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(sample_id)
            .cloned()
    }
    fn is_unavailable(&self, sample_id: &str) -> bool {
        self.unavailable
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .contains(sample_id)
    }
}

impl SampleStore {
    fn insert(&self, id: &str, b: SharedBuffer) {
        self.buffers
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id.to_owned(), b);
    }
    fn remove(&self, id: &str) {
        self.buffers
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id);
    }
}

/// One library entry (`SampleLibraryEntry`).
#[derive(Debug, Clone, PartialEq)]
pub struct LibEntry {
    pub sample: SampleRef,
    pub trim: Option<SampleTrim>,
    pub added_at: f64,
}

impl LibEntry {
    pub fn trim_start(&self) -> f64 {
        self.trim.map_or(0.0, |t| t.start_seconds)
    }
    pub fn trim_end(&self) -> f64 {
        self.trim
            .map_or(self.sample.duration_seconds, |t| t.end_seconds)
    }
}

/// What a finished load is for.
#[derive(Debug, Clone, PartialEq)]
enum LoadFor {
    /// Library import (select the first new one: `select` when true).
    Import { select: bool, batch: u64 },
    /// Library restore at start (saved name + trim kept).
    Restore {
        name: String,
        trim: Option<SampleTrim>,
        added_at: f64,
        order: usize,
    },
    /// The Song view's import (⌘I / Audio): a new audio track per file.
    SongImport,
    /// An audio region of the open project references it.
    Region,
}

enum Msg {
    Loaded {
        id: String,
        path: String,
        purpose: LoadFor,
        result: Result<SharedBuffer, String>,
    },
    Processed {
        name: String,
        result: AudioBuffer,
    },
}

/// A running audition (`playerRef` + the rAF progress).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Audition {
    pub started: f64,
    pub duration: f64,
}

/// `SampleManager` + the library hook's state.
pub struct Samples {
    pub store: Arc<SampleStore>,
    samples: HashMap<String, SampleRef>,
    peaks: std::cell::RefCell<HashMap<(String, usize), Vec<f32>>>,
    pub entries: Vec<LibEntry>,
    pub selected: Option<String>,
    pub processing: bool,
    pub audition: Option<Audition>,
    /// Song import results waiting to become tracks (the app applies them to the doc).
    pub song_imports: Vec<SampleRef>,
    /// Ids of samples loaded since the last poll (audio regions refresh their players).
    pub loaded_ids: Vec<String>,
    /// Paths of library imports that failed (shown nowhere in the TS: the promise rejects).
    pub errors: Vec<String>,
    restoring: Vec<(usize, LibEntry)>,
    restore_pending: usize,
    import_batch: u64,
    requested: HashSet<String>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    cache: PadBufferCache,
    pub sync: bool,
    noise: Arc<Mutex<NoiseBank>>,
    pub version: u64,
    pub waker: Option<rune::Waker>,
}

/// `Date.now()`.
fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_millis() as f64)
}

/// Basename without its extension (fallback "Sample").
pub fn sample_name_of(path: &str) -> String {
    let base = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let name = match base.rfind('.') {
        Some(i)
            if i > 0
                && base[i + 1..]
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_') =>
        {
            &base[..i]
        }
        _ => base,
    };
    if name.is_empty() {
        "Sample".to_owned()
    } else {
        name.to_owned()
    }
}

fn sample_ref(id: &str, name: &str, path: &str, b: &AudioBuffer) -> SampleRef {
    SampleRef {
        id: id.to_owned(),
        name: name.to_owned(),
        path: path.to_owned(),
        duration_seconds: b.frames() as f64 / b.sample_rate.max(1) as f64,
        sample_rate: b.sample_rate as f64,
        channels: b.channel_count() as u32,
    }
}

impl Samples {
    pub fn new(store: Arc<SampleStore>, cache: PadBufferCache, sync: bool) -> Self {
        let (tx, rx) = channel();
        Self {
            store,
            samples: HashMap::new(),
            peaks: Default::default(),
            entries: Vec::new(),
            selected: None,
            processing: false,
            audition: None,
            song_imports: Vec::new(),
            loaded_ids: Vec::new(),
            errors: Vec::new(),
            restoring: Vec::new(),
            restore_pending: 0,
            import_batch: 0,
            requested: HashSet::new(),
            tx,
            rx,
            cache,
            sync,
            noise: Arc::new(Mutex::new(NoiseBank::new())),
            version: 0,
            waker: None,
        }
    }

    /// The decoder's context rate.
    pub fn rate(&self) -> u32 {
        self.cache.device_rate()
    }

    pub fn get(&self, id: &str) -> Option<&SampleRef> {
        self.samples.get(id)
    }

    pub fn buffer(&self, id: &str) -> Option<SharedBuffer> {
        self.store.buffer(id)
    }

    pub fn selected_entry(&self) -> Option<&LibEntry> {
        let id = self.selected.as_deref()?;
        self.entries.iter().find(|e| e.sample.id == id)
    }

    // ---------------------------------------------------------------- loading

    fn spawn_load(&mut self, id: String, path: String, purpose: LoadFor) {
        let cache = self.cache.clone();
        let tx = self.tx.clone();
        let waker = self.waker.clone();
        let job = move || {
            let result = cache.decode_file_direct(&path).map_err(|e| e.to_string());
            let _ = tx.send(Msg::Loaded {
                id,
                path,
                purpose,
                result,
            });
            if let Some(w) = waker {
                w();
            }
        };
        if self.sync {
            job();
        } else {
            std::thread::spawn(job);
        }
    }

    /// Already loaded by path (`loadFromPath`'s dedupe).
    fn by_path(&self, path: &str) -> Option<&SampleRef> {
        self.samples.values().find(|s| s.path == path)
    }

    /// `importSamples()` with the chosen files: paths already in the library are skipped; the
    /// first new sample gets selected.
    pub fn import_files(&mut self, paths: &[String]) {
        self.import_batch += 1;
        let mut first = true;
        for p in paths {
            if self.entries.iter().any(|e| &e.sample.path == p) {
                continue;
            }
            if let Some(s) = self.by_path(p).cloned() {
                self.entries.push(LibEntry {
                    sample: s.clone(),
                    trim: None,
                    added_at: now_ms(),
                });
                if first {
                    self.selected = Some(s.id.clone());
                    first = false;
                }
                continue;
            }
            let id = uuid::Uuid::new_v4().to_string();
            self.spawn_load(
                id,
                p.clone(),
                LoadFor::Import {
                    select: first,
                    batch: self.import_batch,
                },
            );
            first = false;
        }
        self.version += 1;
    }

    /// The Song view's `importAudioFile()` for the chosen files (a track each once decoded).
    pub fn import_for_song(&mut self, paths: &[String]) {
        for p in paths {
            if let Some(s) = self.by_path(p).cloned() {
                self.song_imports.push(s);
                continue;
            }
            self.spawn_load(
                uuid::Uuid::new_v4().to_string(),
                p.clone(),
                LoadFor::SongImport,
            );
        }
    }

    /// Restore at mount (`loadFromPathWithId` per stored entry, in order).
    pub fn restore_library(&mut self, settings: &SettingsStore) {
        let raw = read_sample_library(settings);
        for (order, v) in raw.into_iter().enumerate() {
            let Ok(e) = serde_json::from_value::<SerializedSampleEntry>(v) else {
                continue;
            };
            self.restore_pending += 1;
            self.requested.insert(e.sample_id.clone());
            self.spawn_load(
                e.sample_id,
                e.sample_path,
                LoadFor::Restore {
                    name: e.sample_name,
                    trim: e.trim,
                    added_at: e.added_at,
                    order,
                },
            );
        }
    }

    /// Audio regions of the project whose samples are not loaded: load them by id from their
    /// path (project restore), once per id.
    pub fn ensure_region_samples(&mut self, regions: &[(String, String)]) {
        for (id, path) in regions {
            if id.is_empty()
                || path.is_empty()
                || self.samples.contains_key(id)
                || !self.requested.insert(id.clone())
            {
                continue;
            }
            self.spawn_load(id.clone(), path.clone(), LoadFor::Region);
        }
    }

    /// Applies finished jobs. True when something changed.
    pub fn poll(&mut self, settings: &mut SettingsStore) -> bool {
        let mut changed = false;
        let mut persist = false;
        while let Ok(m) = self.rx.try_recv() {
            changed = true;
            match m {
                Msg::Loaded {
                    id,
                    path,
                    purpose,
                    result,
                } => match result {
                    Ok(buf) => {
                        let name = match &purpose {
                            LoadFor::Restore { name, .. } => name.clone(),
                            _ => sample_name_of(&path),
                        };
                        let s = sample_ref(&id, &name, &path, &buf);
                        self.store.insert(&id, buf);
                        self.samples.insert(id.clone(), s.clone());
                        self.loaded_ids.push(id.clone());
                        match purpose {
                            LoadFor::Import { select, batch } => {
                                if !self.entries.iter().any(|e| e.sample.path == path) {
                                    self.entries.push(LibEntry {
                                        sample: s,
                                        trim: None,
                                        added_at: now_ms(),
                                    });
                                    persist = true;
                                    if select && batch == self.import_batch {
                                        self.selected = Some(id);
                                    }
                                }
                            }
                            LoadFor::Restore {
                                trim,
                                added_at,
                                order,
                                ..
                            } => {
                                self.restoring.push((
                                    order,
                                    LibEntry {
                                        sample: s,
                                        trim,
                                        added_at,
                                    },
                                ));
                                self.finish_restore_one();
                            }
                            LoadFor::SongImport => self.song_imports.push(s),
                            LoadFor::Region => {}
                        }
                    }
                    Err(e) => {
                        if matches!(purpose, LoadFor::Restore { .. }) {
                            self.finish_restore_one();
                        } else {
                            self.errors.push(format!("{path}: {e}"));
                        }
                    }
                },
                Msg::Processed { name, result } => {
                    self.processing = false;
                    let s = self.register_buffer(&name, result);
                    self.entries.push(LibEntry {
                        sample: s.clone(),
                        trim: None,
                        added_at: now_ms(),
                    });
                    self.selected = Some(s.id);
                    persist = true;
                }
            }
        }
        if persist {
            self.persist(settings);
        }
        if changed {
            self.version += 1;
        }
        changed
    }

    fn finish_restore_one(&mut self) {
        self.restore_pending = self.restore_pending.saturating_sub(1);
        if self.restore_pending == 0 && !self.restoring.is_empty() {
            let mut got = std::mem::take(&mut self.restoring);
            got.sort_by_key(|(o, _)| *o);
            // `setEntries(loaded)` (the stored order).
            let mut loaded: Vec<LibEntry> = got.into_iter().map(|(_, e)| e).collect();
            loaded.extend(std::mem::take(&mut self.entries));
            self.entries = loaded;
        }
    }

    /// `registerBuffer(name, buffer)`: an in-memory sample (empty path).
    pub fn register_buffer(&mut self, name: &str, b: AudioBuffer) -> SampleRef {
        let id = uuid::Uuid::new_v4().to_string();
        let s = sample_ref(&id, name, "", &b);
        self.store.insert(&id, Arc::new(b));
        self.samples.insert(id, s.clone());
        s
    }

    fn persist(&self, settings: &mut SettingsStore) {
        let list: Vec<SerializedSampleEntry> = self
            .entries
            .iter()
            .map(|e| SerializedSampleEntry {
                sample_id: e.sample.id.clone(),
                sample_path: e.sample.path.clone(),
                sample_name: e.sample.name.clone(),
                trim: e.trim,
                added_at: e.added_at,
            })
            .collect();
        let _ = write_sample_library(settings, &list);
    }

    // ---------------------------------------------------------------- peaks

    /// `computePeaks(id, width)`: max |x| per bucket, max across channels, cached by id + width.
    pub fn peaks(&self, id: &str, width: usize) -> Option<Vec<f32>> {
        let key = (id.to_owned(), width);
        if let Some(p) = self.peaks.borrow().get(&key) {
            return Some(p.clone());
        }
        let b = self.store.buffer(id)?;
        let p = column_peaks(&b, width);
        self.peaks.borrow_mut().insert(key, p.clone());
        Some(p)
    }

    // ---------------------------------------------------------------- library edits

    pub fn select(&mut self, id: Option<String>) {
        self.selected = id;
    }

    pub fn rename(&mut self, id: &str, name: &str, settings: &mut SettingsStore) {
        for e in &mut self.entries {
            if e.sample.id == id {
                e.sample.name = name.to_owned();
            }
        }
        self.persist(settings);
        self.version += 1;
    }

    pub fn remove(&mut self, id: &str, settings: &mut SettingsStore) {
        self.samples.remove(id);
        self.store.remove(id);
        self.peaks.borrow_mut().retain(|(k, _), _| k != id);
        self.entries.retain(|e| e.sample.id != id);
        self.persist(settings);
        if self.selected.as_deref() == Some(id) {
            self.selected = None;
        }
        self.version += 1;
    }

    /// `setTrimStart`: start ∈ [0, end − 0.01].
    pub fn set_trim_start(&mut self, id: &str, seconds: f64, settings: &mut SettingsStore) {
        for e in &mut self.entries {
            if e.sample.id == id {
                let end = e.trim_end();
                e.trim = Some(SampleTrim {
                    start_seconds: 0f64.max(seconds.min(end - 0.01)),
                    end_seconds: end,
                });
            }
        }
        self.persist(settings);
        self.version += 1;
    }

    /// `setTrimEnd`: end ∈ [start + 0.01, duration].
    pub fn set_trim_end(&mut self, id: &str, seconds: f64, settings: &mut SettingsStore) {
        for e in &mut self.entries {
            if e.sample.id == id {
                let start = e.trim_start();
                e.trim = Some(SampleTrim {
                    start_seconds: start,
                    end_seconds: (start + 0.01).max(seconds.min(e.sample.duration_seconds)),
                });
            }
        }
        self.persist(settings);
        self.version += 1;
    }

    pub fn reset_trim(&mut self, id: &str, settings: &mut SettingsStore) {
        for e in &mut self.entries {
            if e.sample.id == id {
                e.trim = None;
            }
        }
        self.persist(settings);
        self.version += 1;
    }

    // ---------------------------------------------------------------- audition

    /// `startAudition`: the trimmed range at unity gain (the preview slot's gain compensated).
    /// Returns the buffer to play.
    pub fn audition_buffer(&self) -> Option<(SharedBuffer, f64)> {
        let e = self.selected_entry()?;
        let b = self.store.buffer(&e.sample.id)?;
        let sr = b.sample_rate as f64;
        let start = (e.trim_start() * sr).round() as usize;
        let end = ((e.trim_end() * sr).round() as usize).min(b.frames());
        let comp = 1.0 / (PREVIEW_GAIN as f32 * db_gain(PREVIEW_VOLUME_DB));
        let channels = b
            .channels
            .iter()
            .map(|c| c[start.min(end)..end].iter().map(|x| x * comp).collect())
            .collect();
        let duration = e.trim_end() - e.trim_start();
        Some((
            Arc::new(AudioBuffer {
                sample_rate: b.sample_rate,
                channels,
            }),
            duration,
        ))
    }

    /// Audition progress 0..1 at UI time `now`.
    pub fn audition_progress(&self, now: f64) -> f64 {
        self.audition.map_or(0.0, |a| {
            ((now - a.started) / a.duration.max(1e-9)).clamp(0.0, 1.0)
        })
    }

    // ---------------------------------------------------------------- jobs

    /// `processSample(params)`: a new sample "{name} (processed)".
    pub fn process(&mut self, p: ProcessParams) {
        let Some(e) = self.selected_entry().cloned() else {
            return;
        };
        let Some(src) = self.store.buffer(&e.sample.id) else {
            return;
        };
        self.processing = true;
        let tx = self.tx.clone();
        let noise = self.noise.clone();
        let waker = self.waker.clone();
        let name = format!("{} (processed)", e.sample.name);
        let job = move || {
            let mut r = MathRandom::new();
            let mut rnd = move || r.next_f64();
            let result = process_audio(
                &src,
                &p,
                &mut rnd,
                &mut noise.lock().unwrap_or_else(|e| e.into_inner()),
            );
            let _ = tx.send(Msg::Processed { name, result });
            if let Some(w) = waker {
                w();
            }
        };
        if self.sync {
            job();
        } else {
            std::thread::spawn(job);
        }
        self.version += 1;
    }

    /// `generateSample(params)` at the context rate.
    pub fn generate(&mut self, p: SignalParams) {
        self.processing = true;
        let tx = self.tx.clone();
        let rate = self.rate();
        let waker = self.waker.clone();
        let job = move || {
            let mut r = MathRandom::new();
            let mut rnd = move || r.next_f64();
            let result = generate_signal(&p, rate, &mut rnd);
            let _ = tx.send(Msg::Processed {
                name: signal_name(&p),
                result,
            });
            if let Some(w) = waker {
                w();
            }
        };
        if self.sync {
            job();
        } else {
            std::thread::spawn(job);
        }
        self.version += 1;
    }
}

/// Peak per column over all channels (`computePeaksFromBuffer` / WaveformDetail's loop):
/// bucket `i` covers `[floor(i·n/w), min(floor((i+1)·n/w), n))`.
pub fn column_peaks(b: &AudioBuffer, width: usize) -> Vec<f32> {
    let n = b.frames();
    let per = n as f64 / width.max(1) as f64;
    (0..width)
        .map(|i| {
            let s = (i as f64 * per).floor() as usize;
            let e = (((i + 1) as f64 * per).floor() as usize).min(n);
            let mut m = 0f32;
            for c in &b.channels {
                for &x in &c[s.min(e)..e] {
                    m = m.max(x.abs());
                }
            }
            m
        })
        .collect()
}

/// `formatDuration` of the list rows: ms / x.xs / m:ss.
pub fn list_duration(seconds: f64) -> String {
    if seconds < 1.0 {
        format!("{:.0}ms", seconds * 1000.0)
    } else if seconds < 60.0 {
        format!("{seconds:.1}s")
    } else {
        let mins = (seconds / 60.0).floor();
        let secs = seconds % 60.0;
        format!("{}:{:0>2}", mins as i64, format!("{secs:.0}"))
    }
}

/// `formatDuration` of the Edit tab: ms / x.xxs / m:ss.s.
pub fn info_duration(seconds: f64) -> String {
    if seconds < 1.0 {
        format!("{:.0}ms", seconds * 1000.0)
    } else if seconds < 60.0 {
        format!("{seconds:.2}s")
    } else {
        let mins = (seconds / 60.0).floor();
        let secs = seconds % 60.0;
        format!("{}:{:0>4}", mins as i64, format!("{secs:.1}"))
    }
}
