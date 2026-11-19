//! Pad sample buffers shared by every SamplerKit, export and the browser preview: port of the
//! contract of `src/audio/sampler/bufferCache.ts`. Control side only (never the audio thread):
//! every call may block on file IO, decoding or synthesis, and is safe from any thread.
//!
//! - Factory voices: `mdaw_dsp::factory::cached_factory_voice` at the device rate.
//! - Classic voices: `mdaw_dsp::classic::cached_classic_voice` (mono, 44.1 kHz).
//! - Files: through the [`FileLoader`] hook (the SampleManager port registers and dedups them;
//!   the default reads the file and decodes it at the device rate like `decodeAudioData`).
//! - Pack sounds (`{ kind: 'pack' }`): resolved against the packs folder, read with the
//!   [`SampleReader`] and decoded directly (not registered with the file loader). A pack that
//!   cannot be resolved or read fails with [`BufferLoadError::MissingPackSample`].
//! - Decoded audio is resampled to the device rate at decode (as Chromium does) and NaN / ±∞
//!   samples are zeroed (`sanitize_non_finite`, the engine boundary).
//!
//! Keys are exactly the TS ones (`factory:<id>`, `classic:<m>:<v>`, `pack:<id>@<gen>:<file>`,
//! `file:<path>` or `file@<gen>:<path>` for files inside the packs folder): see
//! [`PadBufferCache::key`]. Concurrent requests for one key share one load (in-flight dedup); a
//! failed load is not cached, so the next request retries; a load that finishes after its pack
//! was invalidated is returned to its callers but not cached under the new key.
//!
//! [`PadBufferCache::invalidate_pack_buffers`] bumps the pack's generation, drops its cached pad
//! and preview buffers and in-flight entries, asks the file loader to reload the SampleManager
//! samples under the pack folder in place, then notifies the listeners
//! ([`PadBufferCache::on_pad_buffers_invalidated`]) so live samplers reload.
//!
//! Previews (sample-browser audition) live in their own LRU of [`PREVIEW_CACHE_SIZE`] entries.

mod lru;

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, Weak};

use mdaw_dsp::audio_io::{decode_vec, resample, sanitize_non_finite};
use mdaw_dsp::buffer::{AudioBuffer, SharedBuffer};
use mdaw_dsp::classic::cached_classic_voice;
use mdaw_dsp::factory::cached_factory_voice;
use mdaw_model::kit::{pack_id_of_source, pad_source_key};
use mdaw_model::pack_source::{pack_dir_prefix, pack_ref_from_path, resolve_pack_file_path};
use mdaw_model::types::{KitState, PadSampleSource};

pub use lru::LruCache;

/// Decoded previews kept for quick re-audition.
pub const PREVIEW_CACHE_SIZE: usize = 8;

/// Why a pad buffer could not be loaded. Cloneable: every caller sharing an in-flight load gets
/// the same error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BufferLoadError {
    /// `MissingPackSampleError`: the pack is not installed (or the file is gone).
    MissingPackSample {
        pack_id: String,
        file: String,
        cause: Option<String>,
    },
    UnknownClassicVoice {
        machine: String,
        voice: String,
    },
    /// The file could not be read.
    Read {
        path: String,
        message: String,
    },
    /// The file could not be decoded.
    Decode {
        path: String,
        message: String,
    },
    /// A designed sound that has not been rendered (or no longer exists).
    MissingSound {
        sound_id: String,
    },
}

impl std::fmt::Display for BufferLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingPackSample { pack_id, file, .. } => {
                write!(f, "Pack not installed: {pack_id} ({file})")
            }
            Self::UnknownClassicVoice { machine, voice } => {
                write!(f, "Unknown classic voice: {machine}/{voice}")
            }
            Self::Read { path, message } => write!(f, "could not read {path}: {message}"),
            Self::Decode { path, message } => write!(f, "could not decode {path}: {message}"),
            Self::MissingSound { sound_id } => write!(f, "Sound not rendered: {sound_id}"),
        }
    }
}

impl std::error::Error for BufferLoadError {}

pub type LoadResult = Result<SharedBuffer, BufferLoadError>;

/// Reads sample files (`electronAPI.sampleReadFile`). Injected for tests.
pub trait SampleReader: Send + Sync {
    fn read(&self, path: &str) -> std::io::Result<Vec<u8>>;
}

/// The real file system.
#[derive(Debug, Clone, Copy, Default)]
pub struct FsReader;

impl SampleReader for FsReader {
    fn read(&self, path: &str) -> std::io::Result<Vec<u8>> {
        std::fs::read(path)
    }
}

/// How `{ kind: 'file' }` sources are loaded and how pack invalidation reaches them: the hook
/// the SampleManager port implements (`sampleManager.loadFromPath` + `getBuffer`,
/// `reloadByPathPrefix`).
pub trait FileLoader: Send + Sync {
    /// Load (and register) the file at the device rate.
    fn load_file(&self, cache: &PadBufferCache, path: &str) -> LoadResult;
    /// Reload, in place, every registered sample under `prefix` (pack folder, trailing
    /// separator). Called by [`PadBufferCache::invalidate_pack_buffers`].
    fn reload_by_path_prefix(&self, prefix: &str);
    /// The rendered audio of a designed sound (`{ kind: 'sound' }` sources). The app renders
    /// sounds from their recipes and calls [`PadBufferCache::invalidate_sound`] after each
    /// re-render.
    fn load_sound(&self, sound_id: &str) -> LoadResult {
        Err(BufferLoadError::MissingSound {
            sound_id: sound_id.to_owned(),
        })
    }
}

/// Default [`FileLoader`]: read + decode, nothing registered, nothing to reload.
#[derive(Debug, Clone, Copy, Default)]
pub struct DirectFileLoader;

impl FileLoader for DirectFileLoader {
    fn load_file(&self, cache: &PadBufferCache, path: &str) -> LoadResult {
        cache.decode_file_direct(path)
    }
    fn reload_by_path_prefix(&self, _prefix: &str) {}
}

type Listener = Arc<dyn Fn(&str) + Send + Sync>;

/// One shared in-flight load.
#[derive(Default)]
struct Flight {
    result: Mutex<Option<LoadResult>>,
    done: Condvar,
}

impl Flight {
    fn wait(&self) -> LoadResult {
        let mut r = lock(&self.result);
        loop {
            if let Some(r) = r.as_ref() {
                return r.clone();
            }
            r = self.done.wait(r).unwrap_or_else(PoisonError::into_inner);
        }
    }
    fn finish(&self, result: LoadResult) {
        *lock(&self.result) = Some(result);
        self.done.notify_all();
    }
}

struct State {
    packs_dir: Option<String>,
    cache: HashMap<String, SharedBuffer>,
    pending: HashMap<String, Arc<Flight>>,
    previews: LruCache<String, SharedBuffer>,
    preview_pending: HashMap<String, Arc<Flight>>,
    generations: HashMap<String, u64>,
    listeners: Vec<(u64, Listener)>,
    next_listener: u64,
    /// Reversed copies keyed by the forward buffer's address (weakly held, like a WeakMap).
    reversed: HashMap<usize, (Weak<AudioBuffer>, SharedBuffer)>,
}

struct Inner {
    device_rate: u32,
    reader: Arc<dyn SampleReader>,
    files: Arc<dyn FileLoader>,
    state: Mutex<State>,
}

/// The shared pad buffer cache (cheap to clone; all clones share one cache).
#[derive(Clone)]
pub struct PadBufferCache {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for PadBufferCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PadBufferCache")
            .field("device_rate", &self.inner.device_rate)
            .finish_non_exhaustive()
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Unsubscribes its listener when dropped (`onPadBuffersInvalidated`'s return value).
#[must_use = "dropping the subscription unsubscribes"]
pub struct Subscription {
    cache: Weak<Inner>,
    id: u64,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(inner) = self.cache.upgrade() {
            lock(&inner.state)
                .listeners
                .retain(|(id, _)| *id != self.id);
        }
    }
}

impl PadBufferCache {
    /// A cache rendering factory voices and decoding files at `device_rate`, reading through
    /// `reader`, loading `{ kind: 'file' }` sources through `files`.
    pub fn new(
        device_rate: u32,
        packs_dir: Option<String>,
        reader: Arc<dyn SampleReader>,
        files: Arc<dyn FileLoader>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                device_rate,
                reader,
                files,
                state: Mutex::new(State {
                    packs_dir,
                    cache: HashMap::new(),
                    pending: HashMap::new(),
                    previews: LruCache::new(PREVIEW_CACHE_SIZE),
                    preview_pending: HashMap::new(),
                    generations: HashMap::new(),
                    listeners: Vec::new(),
                    next_listener: 0,
                    reversed: HashMap::new(),
                }),
            }),
        }
    }

    /// File system reader, direct file decoding.
    pub fn with_fs(device_rate: u32, packs_dir: Option<String>) -> Self {
        Self::new(
            device_rate,
            packs_dir,
            Arc::new(FsReader),
            Arc::new(DirectFileLoader),
        )
    }

    pub fn device_rate(&self) -> u32 {
        self.inner.device_rate
    }

    pub fn packs_dir(&self) -> Option<String> {
        lock(&self.inner.state).packs_dir.clone()
    }

    /// `setPacksDirForTests` / the packs folder changing.
    pub fn set_packs_dir(&self, dir: Option<String>) {
        lock(&self.inner.state).packs_dir = dir;
    }

    fn key_in(st: &State, source: &PadSampleSource) -> String {
        pad_source_key(source, st.packs_dir.as_deref(), &|id| {
            st.generations.get(id).copied().unwrap_or(0)
        })
    }

    /// `padSourceKey` with this cache's pack generations and packs folder.
    pub fn key(&self, source: &PadSampleSource) -> String {
        Self::key_in(&lock(&self.inner.state), source)
    }

    /// `packIdOfSource`.
    pub fn pack_id_of_source(&self, source: &PadSampleSource) -> Option<String> {
        pack_id_of_source(source, lock(&self.inner.state).packs_dir.as_deref())
    }

    /// `getCachedPadBuffer`.
    pub fn get_cached(&self, source: &PadSampleSource) -> Option<SharedBuffer> {
        let st = lock(&self.inner.state);
        st.cache.get(&Self::key_in(&st, source)).cloned()
    }

    /// `loadPadBuffer`, blocking: the cached buffer, the result of the load already in flight
    /// for the same key, or a new load.
    pub fn load_pad_buffer(&self, source: &PadSampleSource) -> LoadResult {
        let (key, flight) = {
            let mut st = lock(&self.inner.state);
            let key = Self::key_in(&st, source);
            if let Some(hit) = st.cache.get(&key) {
                return Ok(hit.clone());
            }
            if let Some(f) = st.pending.get(&key) {
                let f = f.clone();
                drop(st);
                return f.wait();
            }
            let f = Arc::new(Flight::default());
            st.pending.insert(key.clone(), f.clone());
            (key, f)
        };
        let result =
            catch_unwind(AssertUnwindSafe(|| self.load_source(source))).unwrap_or_else(|_| {
                Err(BufferLoadError::Decode {
                    path: key.clone(),
                    message: "loader panicked".into(),
                })
            });
        {
            let mut st = lock(&self.inner.state);
            if let Ok(buf) = &result
                && Self::key_in(&st, source) == key
            {
                st.cache.insert(key.clone(), buf.clone());
                st.previews.delete(&key);
            }
            if st
                .pending
                .get(&key)
                .is_some_and(|f| Arc::ptr_eq(f, &flight))
            {
                st.pending.remove(&key);
            }
        }
        flight.finish(result.clone());
        result
    }

    /// [`load_pad_buffer`](Self::load_pad_buffer) on a worker thread; `done` runs there.
    pub fn load_pad_buffer_async(
        &self,
        source: PadSampleSource,
        done: impl FnOnce(LoadResult) + Send + 'static,
    ) {
        let cache = self.clone();
        std::thread::spawn(move || done(cache.load_pad_buffer(&source)));
    }

    /// `preloadKitBuffers`: load every pad sample of a kit in parallel; failures are returned
    /// (the TS logs them), never fatal.
    pub fn preload_kit_buffers(&self, kit: &KitState) -> Vec<(usize, BufferLoadError)> {
        std::thread::scope(|s| {
            let handles: Vec<_> = kit
                .pads
                .iter()
                .enumerate()
                .filter_map(|(i, p)| p.sample.as_ref().map(|smp| (i, &smp.source)))
                .map(|(i, src)| (i, s.spawn(move || self.load_pad_buffer(src))))
                .collect();
            handles
                .into_iter()
                .filter_map(|(i, h)| h.join().ok().and_then(|r| r.err()).map(|e| (i, e)))
                .collect()
        })
    }

    /// `invalidatePadBuffer`: drop a source from the pad and preview caches.
    pub fn invalidate_pad_buffer(&self, source: &PadSampleSource) {
        let mut st = lock(&self.inner.state);
        let key = Self::key_in(&st, source);
        st.cache.remove(&key);
        st.previews.delete(&key);
    }

    /// `invalidatePackBuffers` (call after install / update / uninstall).
    pub fn invalidate_pack_buffers(&self, pack_id: &str) {
        let (prefix, listeners) = {
            let mut st = lock(&self.inner.state);
            *st.generations.entry(pack_id.to_owned()).or_insert(0) += 1;
            let dir = st.packs_dir.clone();
            let belongs = |key: &str| {
                key.starts_with(&format!("pack:{pack_id}@"))
                    || (key.starts_with("file@")
                        && key.find(':').is_some_and(|i| {
                            pack_ref_from_path(dir.as_deref(), &key[i + 1..])
                                .is_some_and(|r| r.pack_id == pack_id)
                        }))
            };
            st.cache.retain(|k, _| !belongs(k));
            st.pending.retain(|k, _| !belongs(k));
            for k in st.previews.keys() {
                if belongs(&k) {
                    st.previews.delete(&k);
                }
            }
            st.preview_pending.retain(|k, _| !belongs(k));
            let prefix = dir.as_deref().and_then(|d| pack_dir_prefix(d, pack_id));
            let listeners: Vec<Listener> = st.listeners.iter().map(|(_, l)| l.clone()).collect();
            (prefix, listeners)
        };
        if let Some(prefix) = prefix {
            self.inner.files.reload_by_path_prefix(&prefix);
        }
        for l in listeners {
            // A failing listener must not stop the others (the TS catches and logs).
            let _ = catch_unwind(AssertUnwindSafe(|| l(pack_id)));
        }
    }

    /// A designed sound was (re-)rendered: its old buffers are dropped, its key moves on and
    /// live samplers reload the pads playing it (the listeners run with `sound:<id>`).
    pub fn invalidate_sound(&self, sound_id: &str) {
        let gid = mdaw_model::kit::sound_generation_id(sound_id);
        let listeners: Vec<Listener> = {
            let mut st = lock(&self.inner.state);
            *st.generations.entry(gid.clone()).or_insert(0) += 1;
            let prefix = format!("sound:{sound_id}@");
            st.cache.retain(|k, _| !k.starts_with(&prefix));
            st.pending.retain(|k, _| !k.starts_with(&prefix));
            st.listeners.iter().map(|(_, l)| l.clone()).collect()
        };
        for l in listeners {
            let _ = catch_unwind(AssertUnwindSafe(|| l(&gid)));
        }
    }

    /// `onPadBuffersInvalidated`: `cb(pack_id)` after each invalidation, until the returned
    /// subscription is dropped.
    pub fn on_pad_buffers_invalidated(
        &self,
        cb: impl Fn(&str) + Send + Sync + 'static,
    ) -> Subscription {
        let mut st = lock(&self.inner.state);
        st.next_listener += 1;
        let id = st.next_listener;
        st.listeners.push((id, Arc::new(cb)));
        Subscription {
            cache: Arc::downgrade(&self.inner),
            id,
        }
    }

    /// `loadPreviewBuffer`: a pad buffer when one is loaded (or loading), else a decode into the
    /// preview LRU only. Factory and classic voices go through the pad cache.
    pub fn load_preview_buffer(&self, source: &PadSampleSource) -> LoadResult {
        if !matches!(
            source,
            PadSampleSource::File { .. } | PadSampleSource::Pack { .. }
        ) {
            return self.load_pad_buffer(source);
        }
        let (key, flight) = {
            let mut st = lock(&self.inner.state);
            let key = Self::key_in(&st, source);
            if let Some(hit) = st.cache.get(&key) {
                return Ok(hit.clone());
            }
            if let Some(f) = st.pending.get(&key).cloned() {
                drop(st);
                return f.wait();
            }
            if let Some(hit) = st.previews.get(&key) {
                return Ok(hit);
            }
            if let Some(f) = st.preview_pending.get(&key).cloned() {
                drop(st);
                return f.wait();
            }
            let f = Arc::new(Flight::default());
            st.preview_pending.insert(key.clone(), f.clone());
            (key, f)
        };
        let result = match source {
            PadSampleSource::Pack { pack_id, file } => self.load_pack_sound(pack_id, file),
            PadSampleSource::File { path, .. } => self.decode_file_direct(path),
            _ => unreachable!(),
        };
        {
            let mut st = lock(&self.inner.state);
            if let Ok(buf) = &result
                && Self::key_in(&st, source) == key
            {
                st.previews.set(key.clone(), buf.clone());
            }
            if st
                .preview_pending
                .get(&key)
                .is_some_and(|f| Arc::ptr_eq(f, &flight))
            {
                st.preview_pending.remove(&key);
            }
        }
        flight.finish(result.clone());
        result
    }

    /// Keys held by the preview cache, most recent last.
    pub fn preview_cache_keys(&self) -> Vec<String> {
        lock(&self.inner.state).previews.keys()
    }

    /// `getReversedBuffer`: time-reversed copy, cached per buffer while the buffer lives.
    pub fn reversed_buffer(&self, buffer: &SharedBuffer) -> SharedBuffer {
        let addr = Arc::as_ptr(buffer) as usize;
        {
            let mut st = lock(&self.inner.state);
            st.reversed.retain(|_, (w, _)| w.strong_count() > 0);
            if let Some((w, r)) = st.reversed.get(&addr)
                && w.upgrade().is_some_and(|b| Arc::ptr_eq(&b, buffer))
            {
                return r.clone();
            }
        }
        let rev = Arc::new(reverse(buffer));
        lock(&self.inner.state)
            .reversed
            .entry(addr)
            .or_insert_with(|| (Arc::downgrade(buffer), rev))
            .1
            .clone()
    }

    /// `clearPadBufferCache` (tests): forget everything, generations included.
    pub fn clear(&self) {
        let mut st = lock(&self.inner.state);
        st.cache.clear();
        st.pending.clear();
        st.previews.clear();
        st.preview_pending.clear();
        st.generations.clear();
        st.reversed.clear();
    }

    // ------------------------------------------------------------------------

    fn load_source(&self, source: &PadSampleSource) -> LoadResult {
        match source {
            PadSampleSource::Factory { voice } => {
                Ok(cached_factory_voice(*voice, self.inner.device_rate))
            }
            PadSampleSource::Classic { machine, voice } => cached_classic_voice(*machine, voice)
                .map_err(|e| BufferLoadError::UnknownClassicVoice {
                    machine: e.machine.id().to_owned(),
                    voice: e.voice,
                }),
            PadSampleSource::Pack { pack_id, file } => self.load_pack_sound(pack_id, file),
            PadSampleSource::File { path, .. } => self.inner.files.load_file(self, path),
            PadSampleSource::Sound { sound_id } => self.inner.files.load_sound(sound_id),
        }
    }

    /// `resolvePackSource`.
    pub fn resolve_pack_source(
        &self,
        pack_id: &str,
        file: &str,
    ) -> Result<String, BufferLoadError> {
        let dir = lock(&self.inner.state).packs_dir.clone();
        dir.as_deref()
            .and_then(|d| resolve_pack_file_path(d, pack_id, file))
            .ok_or_else(|| BufferLoadError::MissingPackSample {
                pack_id: pack_id.to_owned(),
                file: file.to_owned(),
                cause: None,
            })
    }

    fn load_pack_sound(&self, pack_id: &str, file: &str) -> LoadResult {
        let path = self.resolve_pack_source(pack_id, file)?;
        let bytes =
            self.inner
                .reader
                .read(&path)
                .map_err(|e| BufferLoadError::MissingPackSample {
                    pack_id: pack_id.to_owned(),
                    file: file.to_owned(),
                    cause: Some(e.to_string()),
                })?;
        self.decode(&path, bytes)
    }

    /// Read and decode a file without registering it anywhere (`decodeFileDirect`).
    pub fn decode_file_direct(&self, path: &str) -> LoadResult {
        let bytes = self
            .inner
            .reader
            .read(path)
            .map_err(|e| BufferLoadError::Read {
                path: path.to_owned(),
                message: e.to_string(),
            })?;
        self.decode(path, bytes)
    }

    /// `decodeAudioData` in the device-rate context: decode, resample to the device rate,
    /// sanitize non-finite samples.
    pub fn decode(&self, path: &str, bytes: Vec<u8>) -> LoadResult {
        let ext = Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let decoded = decode_vec(bytes, ext).map_err(|e| BufferLoadError::Decode {
            path: path.to_owned(),
            message: e.to_string(),
        })?;
        let mut buf = if decoded.sample_rate == self.inner.device_rate {
            decoded
        } else {
            resample(&decoded, self.inner.device_rate)
        };
        sanitize_non_finite(&mut buf);
        Ok(Arc::new(buf))
    }
}

/// Time-reversed copy of a buffer.
pub fn reverse(buffer: &AudioBuffer) -> AudioBuffer {
    AudioBuffer {
        sample_rate: buffer.sample_rate,
        channels: buffer
            .channels
            .iter()
            .map(|c| c.iter().rev().copied().collect())
            .collect(),
    }
}

#[cfg(test)]
mod tests;
