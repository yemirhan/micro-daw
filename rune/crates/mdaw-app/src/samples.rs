//! `services/SampleManager.ts`: decoded samples by id (the Song view's audio regions and the
//! Sound Designer's rendered sounds) and their peaks.
//!
//! - **Decoding** runs on a bounded worker pool ([`Pool`], a few threads fed by one queue;
//!   `sampleReadFile` + `decodeAudioData` at the context rate, through the engine's
//!   `PadBufferCache::decode`); headless runs decode synchronously so a snapshot or a test sees
//!   the result at once.
//! - The engine reads the buffers through [`SampleStore`] ([`mdaw_engine::engine::AudioSamples`]):
//!   audio regions schedule them at the next play, as Tone players do. The store is also the pad
//!   cache's [`FileLoader`]: `invalidate_pack_buffers` (a pack installed / updated / removed)
//!   reaches [`FileLoader::reload_by_path_prefix`], which re-reads every sample under that pack
//!   folder in place (`reloadByPathPrefix`: latest request wins per sample; a file that can no
//!   longer be read keeps its audio and is marked unavailable).
//! - **Peaks** are computed off the UI thread in tiles of [`PEAK_TILE`] columns, cached by
//!   sample + range + width + tile ([`PeakKey`]); the views draw the tiles they have.
//! - **Audition** plays the trimmed range at unity gain (`Tone.Player(...).toDestination()`)
//!   on the engine's audition slot (`play_audition`: offset + duration, no copy); progress follows
//!   the UI clock.
//! - **Designed sounds** (the Sound Designer, `crate::designer`) are rendered on the UI thread
//!   and registered here under their sound id ([`Samples::put_sound`]): pads load them through
//!   [`FileLoader::load_sound`], audio regions play them like any sample.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, RwLock};

use mdaw_dsp::buffer::{AudioBuffer, SharedBuffer};
use mdaw_engine::buffers::BufferLoadError;
use mdaw_engine::buffers::{FileLoader, LoadResult, PadBufferCache};
use mdaw_engine::engine::AudioSamples;
use mdaw_model::types::SampleRef;

type Job = Box<dyn FnOnce() + Send>;

/// A small worker pool (one queue, at most [`Pool::THREADS`] threads, started on first use);
/// headless runs run each job inline.
#[derive(Clone)]
pub struct Pool {
    inner: Arc<PoolInner>,
}

struct PoolInner {
    sync: AtomicBool,
    tx: Mutex<Option<Sender<Job>>>,
}

impl Pool {
    /// Most decoder / DSP threads (`decodeAudioData` runs on a few audio threads in Chromium).
    pub const THREADS: usize = 3;

    pub fn new(sync: bool) -> Self {
        Self {
            inner: Arc::new(PoolInner {
                sync: AtomicBool::new(sync),
                tx: Mutex::new(None),
            }),
        }
    }

    pub fn is_sync(&self) -> bool {
        self.inner.sync.load(Ordering::Relaxed)
    }

    pub fn set_sync(&self, sync: bool) {
        self.inner.sync.store(sync, Ordering::Relaxed);
    }

    /// Runs `job` on a worker (inline when synchronous).
    pub fn run(&self, job: impl FnOnce() + Send + 'static) {
        if self.is_sync() {
            job();
            return;
        }
        let mut tx = self.inner.tx.lock().unwrap_or_else(|e| e.into_inner());
        let sender = tx.get_or_insert_with(|| {
            let (tx, rx) = channel::<Job>();
            let rx = Arc::new(Mutex::new(rx));
            for i in 0..Self::THREADS {
                let rx = rx.clone();
                let _ = std::thread::Builder::new()
                    .name(format!("mdaw-samples-{i}"))
                    .spawn(move || {
                        loop {
                            let job = {
                                let r = rx.lock().unwrap_or_else(|e| e.into_inner());
                                r.recv()
                            };
                            match job {
                                Ok(j) => j(),
                                Err(_) => break, // every sender dropped: the app is gone
                            }
                        }
                    });
            }
            tx
        });
        let _ = sender.send(Box::new(job));
    }
}

/// What a reload changed (read by [`Samples::poll`]).
#[derive(Default)]
struct StoreEvents {
    /// New audio swapped in (rebuild the region players, redraw).
    reloaded: Vec<String>,
    /// Availability changed (redraw the badges).
    changed: Vec<String>,
}

/// The engine's view of the decoded samples (shared with the audio-region players), and the
/// pad cache's [`FileLoader`] (in-place reloads of pack samples).
pub struct SampleStore {
    buffers: RwLock<HashMap<String, SharedBuffer>>,
    unavailable: RwLock<HashSet<String>>,
    /// id → file path of every sample loaded from a file (`reloadByPathPrefix`'s targets).
    paths: RwLock<HashMap<String, String>>,
    /// `reloadSeq` + `reloadGen`: the latest requested reload of a sample applies its result.
    reload_gen: Mutex<(u64, HashMap<String, u64>)>,
    events: Mutex<StoreEvents>,
    /// Reads + decodes at the context rate (a plain cache: no loader of its own, no cycle).
    decoder: PadBufferCache,
    pub pool: Pool,
    waker: Mutex<Option<rune::Waker>>,
    /// Itself (reload jobs hold a strong reference while they run).
    weak: Mutex<std::sync::Weak<SampleStore>>,
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

impl FileLoader for SampleStore {
    /// `{ kind: 'file' }` pad sources: read + decode (pads keep their own cache entry, which
    /// `invalidate_pack_buffers` drops; nothing is registered here).
    fn load_file(&self, cache: &PadBufferCache, path: &str) -> LoadResult {
        cache.decode_file_direct(path)
    }

    /// `{ kind: 'sound' }` pad sources: the sound's latest render (registered by
    /// [`Samples::put_sound`] under the sound id).
    fn load_sound(&self, sound_id: &str) -> LoadResult {
        self.buffer(sound_id)
            .ok_or_else(|| BufferLoadError::MissingSound {
                sound_id: sound_id.to_owned(),
            })
    }

    /// `reloadByPathPrefix(prefix)`: re-reads, in place, every sample loaded from a path under
    /// `prefix` (a pack folder after an install, update or removal), keeping its id. Each claims
    /// a new generation; only the latest request applies its audio, or marks the sample
    /// unavailable when its file can no longer be read (the loaded audio is kept, so the Song
    /// keeps playing it).
    fn reload_by_path_prefix(&self, prefix: &str) {
        if prefix.is_empty() {
            return;
        }
        let targets: Vec<(String, String)> = self
            .paths
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|(_, p)| p.starts_with(prefix))
            .map(|(id, p)| (id.clone(), p.clone()))
            .collect();
        for (id, path) in targets {
            let generation = {
                let mut g = self.reload_gen.lock().unwrap_or_else(|e| e.into_inner());
                g.0 += 1;
                let n = g.0;
                g.1.insert(id.clone(), n);
                n
            };
            let Some(me) = self
                .weak
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .upgrade()
            else {
                return;
            };
            self.pool.run(move || me.reload_one(&id, &path, generation));
        }
    }
}

impl SampleStore {
    /// A store decoding at `rate` (the context rate); `sync` runs its jobs inline (headless).
    pub fn new(rate: u32, sync: bool) -> Arc<Self> {
        let store = Arc::new(Self {
            buffers: RwLock::default(),
            unavailable: RwLock::default(),
            paths: RwLock::default(),
            reload_gen: Mutex::default(),
            events: Mutex::default(),
            decoder: PadBufferCache::with_fs(rate, None),
            pool: Pool::new(sync),
            waker: Mutex::new(None),
            weak: Mutex::new(std::sync::Weak::new()),
        });
        *store.weak.lock().unwrap_or_else(|e| e.into_inner()) = Arc::downgrade(&store);
        store
    }

    pub fn set_waker(&self, w: Option<rune::Waker>) {
        *self.waker.lock().unwrap_or_else(|e| e.into_inner()) = w;
    }

    fn wake(&self) {
        if let Some(w) = self.waker.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            w();
        }
    }

    fn latest(&self, id: &str, generation: u64) -> bool {
        self.reload_gen
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .1
            .get(id)
            == Some(&generation)
            && self
                .paths
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .contains_key(id)
    }

    /// `reloadOne`: 'stale' (a newer reload or a removal meanwhile) applies nothing.
    fn reload_one(&self, id: &str, path: &str, generation: u64) {
        let result = self.decoder.decode_file_direct(path);
        if !self.latest(id, generation) {
            return;
        }
        let mut ev = self.events.lock().unwrap_or_else(|e| e.into_inner());
        match result {
            Ok(buf) => {
                self.insert(id, buf);
                self.unavailable
                    .write()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(id);
                ev.reloaded.push(id.to_owned());
                ev.changed.push(id.to_owned());
            }
            Err(_) => {
                let newly = self
                    .unavailable
                    .write()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(id.to_owned());
                if newly {
                    ev.changed.push(id.to_owned());
                }
            }
        }
        drop(ev);
        self.wake();
    }

    fn insert(&self, id: &str, b: SharedBuffer) {
        self.buffers
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id.to_owned(), b);
    }

    fn register_path(&self, id: &str, path: &str) {
        if !path.is_empty() {
            self.paths
                .write()
                .unwrap_or_else(|e| e.into_inner())
                .insert(id.to_owned(), path.to_owned());
        }
    }

    fn remove(&self, id: &str) {
        self.buffers
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id);
        self.paths
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id);
        self.unavailable
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id);
        self.reload_gen
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .1
            .remove(id);
    }

    fn take_events(&self) -> StoreEvents {
        std::mem::take(&mut *self.events.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

/// What a finished load is for.
#[derive(Debug, Clone, PartialEq)]
enum LoadFor {
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
    Peaks {
        key: PeakKey,
        peaks: Arc<Vec<f32>>,
    },
}

/// Columns per peak tile.
pub const PEAK_TILE: usize = 256;

/// One tile of peaks: `cols` columns over frames `[start, end)` of sample `id` (buffer
/// revision `rev`), columns `[tile·PEAK_TILE, (tile+1)·PEAK_TILE)`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PeakKey {
    pub id: String,
    pub rev: u64,
    pub start: usize,
    pub end: usize,
    pub cols: usize,
    pub tile: usize,
}

/// Peak caches stop growing past this many tiles (dropped wholesale: they are rebuilt from
/// the buffers on demand).
const PEAK_CACHE_MAX: usize = 4096;

/// How long a failed region load waits before the next try (seconds of UI time).
pub const REGION_RETRY: f64 = 5.0;

/// `SampleManager` + the library hook's state.
pub struct Samples {
    pub store: Arc<SampleStore>,
    samples: HashMap<String, SampleRef>,
    peaks: RefCell<HashMap<PeakKey, Arc<Vec<f32>>>>,
    peaks_pending: RefCell<HashSet<PeakKey>>,
    /// Buffer revision per id (reloads bump it: peaks of the old audio are not reused).
    revs: HashMap<String, u64>,
    /// Song import results waiting to become tracks (the app applies them to the doc).
    pub song_imports: Vec<SampleRef>,
    /// Ids of samples loaded (or reloaded in place) since the last poll (audio regions refresh
    /// their players).
    pub loaded_ids: Vec<String>,
    /// Region samples asked for (loading or loaded).
    requested: HashSet<String>,
    /// Region samples whose load failed: retried after [`REGION_RETRY`] (UI time).
    retry_at: HashMap<String, f64>,
    /// Region sample loads that failed since the last poll: (id, error) (the project's load
    /// report takes them).
    pub region_failures: Vec<(String, String)>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    cache: PadBufferCache,
    pub sync: bool,
    pub version: u64,
    pub waker: Option<rune::Waker>,
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
        store.pool.set_sync(sync);
        Self {
            store,
            samples: HashMap::new(),
            peaks: Default::default(),
            peaks_pending: Default::default(),
            revs: HashMap::new(),
            song_imports: Vec::new(),
            loaded_ids: Vec::new(),
            requested: HashSet::new(),
            retry_at: HashMap::new(),
            region_failures: Vec::new(),
            tx,
            rx,
            cache,
            sync,
            version: 0,
            waker: None,
        }
    }

    /// The UI waker (worker results and reloads wake the window).
    pub fn set_waker(&mut self, w: rune::Waker) {
        self.store.set_waker(Some(w.clone()));
        self.waker = Some(w);
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

    /// The buffer revision of `id` (bumped by in-place reloads).
    pub fn rev(&self, id: &str) -> u64 {
        self.revs.get(id).copied().unwrap_or(0)
    }

    // ---------------------------------------------------------------- loading

    fn spawn_load(&mut self, id: String, path: String, purpose: LoadFor) {
        let cache = self.cache.clone();
        let tx = self.tx.clone();
        let waker = self.waker.clone();
        self.store.pool.run(move || {
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
        });
    }

    /// Already loaded by path (`loadFromPath`'s dedupe).
    fn by_path(&self, path: &str) -> Option<&SampleRef> {
        self.samples.values().find(|s| s.path == path)
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

    /// Audio regions of the project whose samples are not loaded: load them by id from their
    /// path (project restore), once per id; a failed load is tried again after
    /// [`REGION_RETRY`] seconds (the file may come back: a pack reinstalled, a drive mounted).
    pub fn ensure_region_samples(&mut self, regions: &[(String, String)], now: f64) {
        // A failed region load is stamped with this poll's clock.
        for at in self.retry_at.values_mut() {
            if at.is_nan() {
                *at = now + REGION_RETRY;
            }
        }
        for (id, path) in regions {
            if id.is_empty() || path.is_empty() || self.samples.contains_key(id) {
                continue;
            }
            if let Some(at) = self.retry_at.get(id) {
                if now < *at {
                    continue;
                }
                self.retry_at.remove(id);
                self.requested.remove(id);
            }
            if !self.requested.insert(id.clone()) {
                continue;
            }
            self.spawn_load(id.clone(), path.clone(), LoadFor::Region);
        }
    }

    /// Applies finished jobs and in-place reloads. True when something changed.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        let ev = self.store.take_events();
        for id in ev.reloaded {
            // `reloadOne`: the sample keeps its id and name; duration / rate / channels follow
            // the new audio; its peaks are redrawn.
            if let Some(b) = self.store.buffer(&id)
                && let Some(s) = self.samples.get_mut(&id)
            {
                let fresh = sample_ref(&id, &s.name, &s.path, &b);
                *s = fresh;
            }
            *self.revs.entry(id.clone()).or_insert(0) += 1;
            self.clear_peaks(&id);
            self.loaded_ids.push(id);
            changed = true;
        }
        if !ev.changed.is_empty() {
            changed = true;
        }
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
                        let name = sample_name_of(&path);
                        let s = sample_ref(&id, &name, &path, &buf);
                        self.store.insert(&id, buf);
                        self.store.register_path(&id, &path);
                        self.samples.insert(id.clone(), s.clone());
                        self.retry_at.remove(&id);
                        self.loaded_ids.push(id.clone());
                        match purpose {
                            LoadFor::SongImport => self.song_imports.push(s),
                            LoadFor::Region => {}
                        }
                    }
                    Err(e) => match purpose {
                        LoadFor::Region => {
                            // Stamped with the clock by the next `ensure_region_samples`.
                            self.retry_at.insert(id.clone(), f64::NAN);
                            self.region_failures.push((id, e));
                        }
                        LoadFor::SongImport => {
                            eprintln!("[samples] import failed: {path}: {e}");
                        }
                    },
                },
                Msg::Peaks { key, peaks } => {
                    self.peaks_pending.borrow_mut().remove(&key);
                    if key.rev == self.rev(&key.id) {
                        let mut cache = self.peaks.borrow_mut();
                        if cache.len() >= PEAK_CACHE_MAX {
                            cache.clear();
                        }
                        cache.insert(key, peaks);
                    }
                }
            }
        }
        if changed {
            self.version += 1;
        }
        changed
    }

    /// `registerBuffer(name, buffer)`: an in-memory sample (empty path).
    pub fn register_buffer(&mut self, name: &str, b: AudioBuffer) -> SampleRef {
        let id = uuid::Uuid::new_v4().to_string();
        let s = sample_ref(&id, name, "", &b);
        self.store.insert(&id, Arc::new(b));
        self.samples.insert(id, s.clone());
        s
    }

    // ---------------------------------------------------------------- peaks

    fn clear_peaks(&self, id: &str) {
        self.peaks.borrow_mut().retain(|k, _| k.id != id);
        self.peaks_pending.borrow_mut().retain(|k| k.id != id);
    }

    /// One tile of peaks (see [`PeakKey`]); computed on the pool when missing (inline in
    /// headless runs), None until it is ready or when the sample is not loaded.
    pub fn peak_tile(&self, key: &PeakKey) -> Option<Arc<Vec<f32>>> {
        if let Some(p) = self.peaks.borrow().get(key) {
            return Some(p.clone());
        }
        let b = self.store.buffer(&key.id)?;
        if self.store.pool.is_sync() {
            let p = Arc::new(range_peaks(&b, key.start, key.end, key.cols, key.tile));
            let mut cache = self.peaks.borrow_mut();
            if cache.len() >= PEAK_CACHE_MAX {
                cache.clear();
            }
            cache.insert(key.clone(), p.clone());
            return Some(p);
        }
        if self.peaks_pending.borrow_mut().insert(key.clone()) {
            let tx = self.tx.clone();
            let waker = self.waker.clone();
            let key = key.clone();
            self.store.pool.run(move || {
                let peaks = Arc::new(range_peaks(&b, key.start, key.end, key.cols, key.tile));
                let _ = tx.send(Msg::Peaks { key, peaks });
                if let Some(w) = waker {
                    w();
                }
            });
        }
        None
    }

    /// The keys of the tiles covering columns `[c0, c1)` of `cols` columns over frames
    /// `[start, end)` of `id`.
    pub fn peak_keys(
        &self,
        id: &str,
        start: usize,
        end: usize,
        cols: usize,
        c0: usize,
        c1: usize,
    ) -> Vec<PeakKey> {
        let rev = self.rev(id);
        let c1 = c1.min(cols);
        if c0 >= c1 {
            return Vec::new();
        }
        (c0 / PEAK_TILE..c1.div_ceil(PEAK_TILE))
            .map(|tile| PeakKey {
                id: id.to_owned(),
                rev,
                start,
                end,
                cols,
                tile,
            })
            .collect()
    }

    /// `computePeaks(id, width)`: max |x| per column over the whole sample (all tiles, or
    /// None while one is still being computed).
    pub fn peaks(&self, id: &str, width: usize) -> Option<Vec<f32>> {
        let frames = self.store.buffer(id)?.frames();
        let mut out = Vec::with_capacity(width);
        let mut ready = true;
        for k in self.peak_keys(id, 0, frames, width, 0, width) {
            match self.peak_tile(&k) {
                Some(p) => out.extend_from_slice(&p),
                None => ready = false,
            }
        }
        ready.then_some(out)
    }

    // ---------------------------------------------------------------- designed sounds

    /// Registers (or replaces) the rendered audio of designed sound `id`: the store serves it to
    /// pads (`load_sound`) and audio regions, its peaks are redrawn, the regions playing it
    /// refresh (through [`Self::loaded_ids`]).
    pub fn put_sound(&mut self, id: &str, name: &str, b: AudioBuffer) {
        let s = sample_ref(id, name, "", &b);
        self.store.insert(id, Arc::new(b));
        self.samples.insert(id.to_owned(), s);
        *self.revs.entry(id.to_owned()).or_insert(0) += 1;
        self.clear_peaks(id);
        self.loaded_ids.push(id.to_owned());
        self.version += 1;
    }

    /// Forgets a designed sound's audio.
    pub fn remove_sound(&mut self, id: &str) {
        self.samples.remove(id);
        self.store.remove(id);
        self.clear_peaks(id);
        self.version += 1;
    }
}

/// Peaks of tile `tile` of `cols` columns over frames `[start, end)` (the buffer clamped):
/// column `i` covers `[start + floor(i·n/cols), start + min(floor((i+1)·n/cols), n))`, max |x|
/// over the channels (`computePeaksFromBuffer` / the waveform canvases' loop).
pub fn range_peaks(
    b: &AudioBuffer,
    start: usize,
    end: usize,
    cols: usize,
    tile: usize,
) -> Vec<f32> {
    let end = end.min(b.frames());
    let start = start.min(end);
    let n = end - start;
    let per = n as f64 / cols.max(1) as f64;
    let c0 = (tile * PEAK_TILE).min(cols);
    let c1 = ((tile + 1) * PEAK_TILE).min(cols);
    (c0..c1)
        .map(|i| {
            let a = start + (i as f64 * per).floor() as usize;
            let z = start + (((i + 1) as f64 * per).floor() as usize).min(n);
            let mut m = 0f32;
            for c in &b.channels {
                for &x in &c[a.min(z)..z] {
                    m = m.max(x.abs());
                }
            }
            m
        })
        .collect()
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
