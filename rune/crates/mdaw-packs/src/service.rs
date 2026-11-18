//! The UI-thread side of the pack store (`src/services/PackService.ts`): catalog, installed
//! packs, per-pack install state (driven by the installer's progress events), uninstall errors,
//! sound previews and the "Get & load" intent.
//!
//! Every store call runs on a worker thread and reports back through a channel; the UI calls
//! [`PackService::poll`] every frame (and after engine edits) to apply the results in order.
//! Nothing here touches the audio thread: previews play through the engine's preview slot
//! ([`PackHost::play_preview`]) and decode on workers.
//!
//! Get & load (M2, N2): [`PackService::get_and_load`] installs the pack when needed (missing or
//! older than the catalog) and loads its kit on the drum track that was active at request time.
//! The request is dropped by a newer request, [`PackService::cancel_pending_load`] (every
//! explicit kit choice), a kit selection with source `user`
//! ([`PackService::on_kit_selected`]), or when the project session, the active drum track or
//! that track's kit changed ([`PackService::check_target`], also run by `poll`). While a take
//! records, the kit waits and loads after it stops. The download keeps running after a cancel.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crossbeam_channel::{Receiver, Sender};
use mdaw_dsp::buffer::SharedBuffer;
use mdaw_engine::buffers::PadBufferCache;
use mdaw_engine::engine::ArrangementEngine;
use mdaw_model::doc::{KitSelectedEvent, KitSelectionSource};
use mdaw_model::packs::{
    InstalledPack, PackCatalog, PackInstallState, PackManifest, PackRefStatus, PackStoreState,
    RememberedNames, ipc_error_message, kit_from_installed, pack_name, parse_remembered_names,
    remember_names, serialize_remembered_names,
};
use mdaw_model::types::{KitState, PadSampleSource};

use crate::store::{PackStore, PackStoreOptions};

/// What the service needs from the app: the engine's drum-track / kit / session / recording
/// state (Get & load), kit application, buffer invalidation and the preview player.
pub trait PackHost {
    fn session_id(&self) -> u64;
    fn active_drum_track_id(&self) -> Option<String>;
    /// The kit id of a drum track.
    fn kit_id(&self, track_id: &str) -> String;
    /// A take is running (Beat loop take or Song recording).
    fn is_recording_now(&self) -> bool;
    /// Puts `kit` on `track_id` (None: the active drum track, created when there is none) with
    /// source `pack-load`. Returns the track.
    fn apply_kit(&mut self, kit: &KitState, track_id: Option<&str>) -> Option<String>;
    /// `invalidatePackBuffers`.
    fn invalidate_pack_buffers(&mut self, pack_id: &str);
    /// The pad buffer cache (decoding for previews, at the device rate).
    fn buffer_cache(&self) -> PadBufferCache;
    fn play_preview(&mut self, buffer: SharedBuffer);
    fn stop_preview(&mut self);
    fn is_preview_playing(&self) -> bool;
}

impl PackHost for ArrangementEngine {
    fn session_id(&self) -> u64 {
        self.doc().get_session_id()
    }
    fn active_drum_track_id(&self) -> Option<String> {
        self.doc().get_active_drum_track_id()
    }
    fn kit_id(&self, track_id: &str) -> String {
        self.doc().get_kit(track_id).id
    }
    fn is_recording_now(&self) -> bool {
        self.doc().is_loop_recording()
            || self.get_state() == mdaw_engine::engine::TransportState::Recording
    }
    fn apply_kit(&mut self, kit: &KitState, track_id: Option<&str>) -> Option<String> {
        let track_id = track_id.map(str::to_owned);
        Some(self.edit(|d| {
            let id = track_id.unwrap_or_else(|| d.ensure_drum_track());
            d.set_kit(&id, kit, KitSelectionSource::PackLoad);
            id
        }))
    }
    fn invalidate_pack_buffers(&mut self, pack_id: &str) {
        self.cache().invalidate_pack_buffers(pack_id);
    }
    fn buffer_cache(&self) -> PadBufferCache {
        self.cache().clone()
    }
    fn play_preview(&mut self, buffer: SharedBuffer) {
        ArrangementEngine::play_preview(self, buffer);
    }
    fn stop_preview(&mut self) {
        ArrangementEngine::stop_preview(self);
    }
    fn is_preview_playing(&self) -> bool {
        ArrangementEngine::is_preview_playing(self)
    }
}

/// `catalogStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogStatus {
    Idle,
    Loading,
    Ready,
    Error,
}

/// `PendingLoadInfo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingLoadInfo {
    pub key: String,
    pub pack_id: String,
    pub kit_id: String,
    pub pack_name: String,
    pub waiting_for_recording: bool,
}

/// `KitLoadedEvent`.
#[derive(Debug, Clone, PartialEq)]
pub struct KitLoadedEvent {
    pub pack_id: String,
    pub kit_id: String,
    pub kit: KitState,
    pub track_id: Option<String>,
}

/// `PackServiceSnapshot` (the pure queries live on [`PackStoreState`]).
#[derive(Debug, Clone, PartialEq)]
pub struct PackSnapshot {
    pub available: bool,
    pub store: PackStoreState,
    pub catalog_status: CatalogStatus,
    pub catalog_error: Option<String>,
    pub packs_dir: Option<String>,
    pub previewing: Option<String>,
    /// The last failed preview (key, message): the TS rejects the preview promise.
    pub preview_error: Option<(String, String)>,
    pub uninstall_errors: BTreeMap<String, String>,
    pub pending_load: Option<String>,
    pub pending_load_info: Option<PendingLoadInfo>,
}

/// N2: what a Get & load was aimed at.
#[derive(Debug, Clone, PartialEq)]
struct LoadTarget {
    track_id: Option<String>,
    kit_id: Option<String>,
    session: u64,
}

#[derive(Debug, Clone, PartialEq)]
enum Phase {
    /// Waiting for the install of the pack.
    Installing,
    /// The kit is ready and waits for the take to stop.
    WaitingRecording(Box<KitState>),
}

#[derive(Debug, Clone)]
struct Pending {
    token: u64,
    pack_id: String,
    kit_id: String,
    target: LoadTarget,
    phase: Phase,
}

enum Msg {
    Progress(String, PackInstallState),
    Init {
        dir: String,
        states: BTreeMap<String, PackInstallState>,
    },
    Catalog(PackCatalog),
    Installed {
        seq: u64,
        list: Vec<InstalledPack>,
    },
    InstallDone {
        id: String,
        result: Box<Result<InstalledPack, String>>,
        list: Option<(u64, Vec<InstalledPack>)>,
        load_token: Option<u64>,
    },
    UninstallDone {
        id: String,
        result: Result<(), String>,
        list: Option<(u64, Vec<InstalledPack>)>,
    },
    PreviewReady {
        token: u64,
        key: String,
        url: Option<String>,
        result: Result<SharedBuffer, String>,
    },
}

/// Writes the remembered pack names (`localStorage.setItem`).
type NamePersister = Box<dyn FnMut(&str)>;

/// Decoded remote previews kept (`LruCache<string, AudioBuffer>(8)`).
const DECODED_PREVIEWS: usize = 8;

/// See the module docs.
pub struct PackService {
    store: Arc<PackStore>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    snap: PackSnapshot,
    version: u64,
    initialized: bool,
    catalog_loading: bool,
    list_seq: Arc<AtomicU64>,
    applied_list_seq: u64,
    /// Worker threads whose result has not been sent yet.
    workers: Arc<std::sync::atomic::AtomicUsize>,
    remembered: RememberedNames,
    persist_names: Option<NamePersister>,
    load_token: u64,
    pending: Option<Pending>,
    kit_loaded: Vec<KitLoadedEvent>,
    preview_token: u64,
    preview_playing: bool,
    decoded: VecDeque<(String, SharedBuffer)>,
}

impl PackService {
    /// A service over a new [`PackStore`] (its progress events feed this service).
    /// `remembered_names` is the stored `micro-daw:pack-names` value.
    pub fn new(opts: PackStoreOptions, remembered_names: Option<&str>) -> Self {
        let (tx, rx) = crossbeam_channel::unbounded();
        let ptx = tx.clone();
        let store = PackStore::new(
            opts,
            Some(Arc::new(move |id: &str, st: &PackInstallState| {
                let _ = ptx.send(Msg::Progress(id.to_owned(), st.clone()));
            })),
        );
        Self {
            store,
            tx,
            rx,
            snap: PackSnapshot {
                available: false,
                store: PackStoreState::default(),
                catalog_status: CatalogStatus::Idle,
                catalog_error: None,
                packs_dir: None,
                previewing: None,
                preview_error: None,
                uninstall_errors: BTreeMap::new(),
                pending_load: None,
                pending_load_info: None,
            },
            version: 0,
            initialized: false,
            catalog_loading: false,
            list_seq: Arc::new(AtomicU64::new(0)),
            applied_list_seq: 0,
            workers: Arc::default(),
            remembered: parse_remembered_names(remembered_names),
            persist_names: None,
            load_token: 0,
            pending: None,
            kit_loaded: Vec::new(),
            preview_token: 0,
            preview_playing: false,
            decoded: VecDeque::new(),
        }
    }

    /// Where remembered pack names are written (`localStorage.setItem(PACK_NAMES_STORAGE_KEY)`).
    pub fn set_name_persister(&mut self, f: impl FnMut(&str) + 'static) {
        self.persist_names = Some(Box::new(f));
    }

    pub fn store(&self) -> &Arc<PackStore> {
        &self.store
    }

    pub fn snapshot(&self) -> &PackSnapshot {
        &self.snap
    }

    /// Bumped on every snapshot change.
    pub fn version(&self) -> u64 {
        self.version
    }

    fn changed(&mut self) {
        self.version += 1;
    }

    fn spawn(&self, f: impl FnOnce(&PackStore, &Sender<Msg>, &AtomicU64) + Send + 'static) {
        let store = self.store.clone();
        let tx = self.tx.clone();
        let seq = self.list_seq.clone();
        let workers = self.workers.clone();
        workers.fetch_add(1, Ordering::SeqCst);
        std::thread::spawn(move || {
            f(&store, &tx, &seq);
            workers.fetch_sub(1, Ordering::SeqCst);
        });
    }

    /// Worker threads are still running (their results are not all queued yet).
    pub fn busy(&self) -> bool {
        self.workers.load(Ordering::SeqCst) > 0
    }

    /// Polls until every worker finished and its result was applied (tests / headless), at
    /// most `timeout`. True when settled.
    pub fn settle(&mut self, host: &mut dyn PackHost, timeout: std::time::Duration) -> bool {
        let end = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < end {
            let busy = self.busy();
            if !self.poll_wait(host, std::time::Duration::from_millis(10)) && !busy && !self.busy()
            {
                return true;
            }
        }
        false
    }

    fn list(store: &PackStore, seq: &AtomicU64) -> (u64, Vec<InstalledPack>) {
        let s = seq.fetch_add(1, Ordering::SeqCst) + 1;
        (s, store.list_installed())
    }

    /// Loads the packs dir, the active states and the installed packs. Idempotent.
    pub fn init(&mut self) {
        if self.initialized {
            return;
        }
        self.initialized = true;
        self.snap.available = true;
        self.changed();
        self.spawn(|store, tx, seq| {
            let _ = tx.send(Msg::Init {
                dir: store.dir().to_string_lossy().into_owned(),
                states: store.states(),
            });
            let (seq, list) = Self::list(store, seq);
            let _ = tx.send(Msg::Installed { seq, list });
        });
    }

    /// Applies worker results, wakes a deferred load and notices a finished preview.
    pub fn poll(&mut self, host: &mut dyn PackHost) {
        while let Ok(m) = self.rx.try_recv() {
            self.handle(m, host);
        }
        self.check_target(host);
        if self.preview_playing && !host.is_preview_playing() {
            self.preview_playing = false;
            if self.snap.previewing.is_some() {
                self.snap.previewing = None;
                self.changed();
            }
        }
    }

    /// Blocks until the next worker result arrives (or `timeout`), then polls. Tests / headless.
    pub fn poll_wait(&mut self, host: &mut dyn PackHost, timeout: std::time::Duration) -> bool {
        match self.rx.recv_timeout(timeout) {
            Ok(m) => {
                self.handle(m, host);
                self.poll(host);
                true
            }
            Err(_) => {
                self.poll(host);
                false
            }
        }
    }

    fn handle(&mut self, m: Msg, host: &mut dyn PackHost) {
        match m {
            Msg::Progress(id, state) => {
                if self.snap.store.apply_progress(&id, state) {
                    self.refresh_installed();
                }
                self.changed();
            }
            Msg::Init { dir, states } => {
                let mut merged = states;
                for (k, v) in std::mem::take(&mut self.snap.store.install_states) {
                    merged.insert(k, v);
                }
                self.snap.store.install_states = merged;
                self.snap.packs_dir = Some(dir);
                self.changed();
            }
            Msg::Catalog(cat) => {
                self.catalog_loading = false;
                let names: Vec<(String, String)> = cat
                    .packs
                    .iter()
                    .map(|p| (p.id.clone(), p.name.clone()))
                    .collect();
                self.remember(&names);
                self.snap.store.catalog = Some(cat);
                self.snap.catalog_status = CatalogStatus::Ready;
                self.changed();
            }
            Msg::Installed { seq, list } => self.apply_list(seq, list, host),
            Msg::InstallDone {
                id,
                result,
                list,
                load_token,
            } => match *result {
                Ok(inst) => {
                    self.snap.store.install_states.remove(&id);
                    self.changed();
                    if let Some((seq, l)) = list {
                        self.apply_list(seq, l, host);
                    }
                    if let Some(t) = load_token {
                        self.continue_load(t, inst, host);
                    }
                }
                Err(msg) => {
                    self.snap
                        .store
                        .install_failed(&id, &ipc_error_message(&msg));
                    self.changed();
                    if let Some(t) = load_token
                        && t == self.load_token
                    {
                        self.drop_pending_load();
                    }
                }
            },
            Msg::UninstallDone { id, result, list } => match result {
                Ok(()) => {
                    self.snap.store.install_states.remove(&id);
                    self.changed();
                    if let Some((seq, l)) = list {
                        self.apply_list(seq, l, host);
                    }
                }
                Err(msg) => {
                    self.snap
                        .uninstall_errors
                        .insert(id, ipc_error_message(&msg));
                    self.changed();
                }
            },
            Msg::PreviewReady {
                token,
                key,
                url,
                result,
            } => match result {
                Ok(buffer) => {
                    if let Some(url) = url {
                        self.decoded.retain(|(k, _)| *k != url);
                        self.decoded.push_back((url, buffer.clone()));
                        while self.decoded.len() > DECODED_PREVIEWS {
                            self.decoded.pop_front();
                        }
                    }
                    if token == self.preview_token {
                        self.play(buffer, host);
                    }
                }
                Err(msg) => {
                    eprintln!("[packs] preview failed {msg}");
                    if token == self.preview_token {
                        self.snap.previewing = None;
                        self.snap.preview_error = Some((key, msg));
                        self.changed();
                    }
                }
            },
        }
    }

    /// `refreshInstalled` + `invalidateChanged`; a list older than the one applied is ignored.
    fn apply_list(&mut self, seq: u64, list: Vec<InstalledPack>, host: &mut dyn PackHost) {
        if seq <= self.applied_list_seq {
            return;
        }
        self.applied_list_seq = seq;
        for id in self.snap.store.invalidate_changed(&list) {
            host.invalidate_pack_buffers(&id);
        }
        let names: Vec<(String, String)> = list
            .iter()
            .map(|p| (p.manifest.id.clone(), p.manifest.name.clone()))
            .collect();
        self.remember(&names);
        self.snap.store.installed = list;
        self.changed();
    }

    fn remember(&mut self, names: &[(String, String)]) {
        if let Some(next) = remember_names(
            &self.remembered,
            names.iter().map(|(a, b)| (a.as_str(), b.as_str())),
        ) {
            self.remembered = next;
            let text = serialize_remembered_names(&self.remembered);
            if let Some(p) = &mut self.persist_names {
                p(&text);
            }
        }
    }

    /// Re-lists installed packs.
    pub fn refresh_installed(&self) {
        self.spawn(|store, tx, seq| {
            let (seq, list) = Self::list(store, seq);
            let _ = tx.send(Msg::Installed { seq, list });
        });
    }

    /// Loads the catalog once (remote with bundled fallback); `force` refetches.
    pub fn load_catalog(&mut self, force: bool) {
        if self.catalog_loading && !force {
            return;
        }
        if !force && self.snap.store.catalog.is_some() {
            return;
        }
        self.catalog_loading = true;
        self.snap.catalog_status = CatalogStatus::Loading;
        self.snap.catalog_error = None;
        self.changed();
        self.spawn(|store, tx, _| {
            let _ = tx.send(Msg::Catalog(store.fetch_catalog()));
        });
    }

    // --- Queries --------------------------------------------------------------------------

    /// `packName(id)`.
    pub fn pack_name(&self, id: &str) -> String {
        pack_name(
            id,
            self.snap.store.catalog.as_ref(),
            &self.snap.store.installed,
            &self.remembered,
        )
    }

    pub fn pack_status(&self, id: &str) -> PackRefStatus {
        self.snap.store.pack_status(id)
    }

    pub fn install_state(&self, id: &str) -> PackInstallState {
        self.snap.store.install_state(id)
    }

    pub fn has_update(&self, manifest: &PackManifest) -> bool {
        self.snap.store.has_update(manifest)
    }

    // --- Install / cancel / uninstall -----------------------------------------------------

    /// Downloads + installs a catalog pack (result in `install_state`).
    pub fn install(&mut self, id: &str) {
        self.start_install(id, None);
    }

    fn start_install(&mut self, id: &str, load_token: Option<u64>) {
        self.snap.store.begin_install(id);
        self.changed();
        let id = id.to_owned();
        self.spawn(move |store, tx, seq| {
            let result = store.install(&id);
            let list = result.is_ok().then(|| Self::list(store, seq));
            let _ = tx.send(Msg::InstallDone {
                id,
                result: Box::new(result),
                list,
                load_token,
            });
        });
    }

    /// Cancels a running install (and a pending Get & load of that pack).
    pub fn cancel(&mut self, id: &str) {
        if self
            .snap
            .pending_load
            .as_deref()
            .is_some_and(|k| k.starts_with(&format!("{id}/")))
        {
            self.cancel_pending_load();
        }
        let _ = self.store.cancel(id);
    }

    /// Removes an installed pack; a failure lands in `uninstall_errors[id]`.
    pub fn uninstall(&mut self, id: &str) {
        if self.snap.uninstall_errors.remove(id).is_some() {
            self.changed();
        }
        let id = id.to_owned();
        self.spawn(move |store, tx, seq| {
            let result = store.uninstall(&id);
            let list = result.is_ok().then(|| Self::list(store, seq));
            let _ = tx.send(Msg::UninstallDone { id, result, list });
        });
    }

    /// Clears a failed install's (or uninstall's) error.
    pub fn dismiss_error(&mut self, id: &str) {
        if matches!(
            self.snap.store.install_states.get(id),
            Some(PackInstallState::Error { .. })
        ) {
            self.snap.store.install_states.remove(id);
            self.changed();
        }
        if self.snap.uninstall_errors.remove(id).is_some() {
            self.changed();
        }
    }

    // --- Get & load -----------------------------------------------------------------------

    fn capture_target(host: &dyn PackHost) -> LoadTarget {
        let track_id = host.active_drum_track_id();
        let kit_id = track_id.as_deref().map(|t| host.kit_id(t));
        LoadTarget {
            track_id,
            kit_id,
            session: host.session_id(),
        }
    }

    fn target_holds(t: &LoadTarget, host: &dyn PackHost) -> bool {
        if host.session_id() != t.session {
            return false;
        }
        if host.active_drum_track_id() != t.track_id {
            return false;
        }
        match &t.track_id {
            None => true,
            Some(track) => Some(host.kit_id(track)) == t.kit_id,
        }
    }

    /// Get & load (see the module docs).
    pub fn get_and_load(&mut self, pack_id: &str, kit_id: &str, host: &mut dyn PackHost) {
        self.drop_pending_load();
        self.load_token += 1;
        let token = self.load_token;
        let key = format!("{pack_id}/{kit_id}");
        self.pending = Some(Pending {
            token,
            pack_id: pack_id.to_owned(),
            kit_id: kit_id.to_owned(),
            target: Self::capture_target(host),
            phase: Phase::Installing,
        });
        self.snap.pending_load = Some(key.clone());
        self.snap.pending_load_info = Some(PendingLoadInfo {
            key,
            pack_id: pack_id.to_owned(),
            kit_id: kit_id.to_owned(),
            pack_name: self.pack_name(pack_id),
            waiting_for_recording: false,
        });
        self.changed();
        let inst = self.snap.store.get_installed(pack_id).cloned();
        let newer = self
            .snap
            .store
            .catalog
            .as_ref()
            .and_then(|c| c.packs.iter().find(|p| p.id == pack_id))
            .map(|m| m.version);
        match inst {
            Some(i) if newer.is_none_or(|v| i.version >= v) => self.continue_load(token, i, host),
            _ => self.start_install(pack_id, Some(token)),
        }
    }

    fn continue_load(&mut self, token: u64, inst: InstalledPack, host: &mut dyn PackHost) {
        let Some(p) = self
            .pending
            .clone()
            .filter(|p| p.token == token && token == self.load_token)
        else {
            return; // cancelled or superseded
        };
        let Some(kit) = kit_from_installed(&inst, &p.kit_id) else {
            eprintln!("[packs] {} has no kit {}", p.pack_id, p.kit_id);
            self.drop_pending_load();
            return;
        };
        if !Self::target_holds(&p.target, host) {
            self.drop_pending_load();
            return;
        }
        if host.is_recording_now() {
            self.wait_for_recording(kit);
            return;
        }
        self.apply(p, kit, host);
    }

    fn wait_for_recording(&mut self, kit: KitState) {
        if let Some(p) = &mut self.pending {
            p.phase = Phase::WaitingRecording(Box::new(kit));
        }
        if let Some(info) = &mut self.snap.pending_load_info {
            info.waiting_for_recording = true;
        }
        self.changed();
    }

    fn apply(&mut self, p: Pending, kit: KitState, host: &mut dyn PackHost) {
        let track_id = host.apply_kit(&kit, p.target.track_id.as_deref());
        self.kit_loaded.push(KitLoadedEvent {
            pack_id: p.pack_id,
            kit_id: p.kit_id,
            kit,
            track_id,
        });
        self.drop_pending_load();
    }

    /// The watch on engine state / kit changes: drops the request when its target changed, and
    /// loads a kit that waited for a take once the take stopped. Call after engine edits (poll
    /// calls it too).
    pub fn check_target(&mut self, host: &mut dyn PackHost) {
        let Some(p) = self.pending.clone() else {
            return;
        };
        if !Self::target_holds(&p.target, host) {
            self.cancel_pending_load();
            return;
        }
        if let Phase::WaitingRecording(kit) = &p.phase
            && !host.is_recording_now()
        {
            let kit = (**kit).clone();
            self.apply(p, kit, host);
        }
    }

    /// Forward the engine's `onKitSelected` events: an explicit (`user`) choice, even of the
    /// kit already loaded, drops a pending request.
    pub fn on_kit_selected(&mut self, e: &KitSelectedEvent) {
        if e.source == KitSelectionSource::User && self.pending.is_some() {
            self.cancel_pending_load();
        }
    }

    /// Drops the pending Get & load (its download keeps running).
    pub fn cancel_pending_load(&mut self) {
        self.load_token += 1;
        self.drop_pending_load();
    }

    fn drop_pending_load(&mut self) {
        self.pending = None;
        if self.snap.pending_load.is_some() || self.snap.pending_load_info.is_some() {
            self.snap.pending_load = None;
            self.snap.pending_load_info = None;
            self.changed();
        }
    }

    /// `onKitLoaded` events since the last call.
    pub fn take_kit_loaded(&mut self) -> Vec<KitLoadedEvent> {
        std::mem::take(&mut self.kit_loaded)
    }

    // --- Previews -------------------------------------------------------------------------

    fn begin_preview(&mut self, key: &str, host: &mut dyn PackHost) -> u64 {
        self.stop_preview(host);
        self.snap.previewing = Some(key.to_owned());
        self.snap.preview_error = None;
        self.changed();
        self.preview_token
    }

    /// Downloads (allowlisted, catalog files only, cached), decodes and plays a remote pack
    /// file; `previewing` is `key` until the sound ends.
    pub fn preview_remote(&mut self, url: &str, key: &str, host: &mut dyn PackHost) {
        let token = self.begin_preview(key, host);
        if let Some(i) = self.decoded.iter().position(|(k, _)| k == url) {
            let hit = self.decoded.remove(i).expect("present");
            let buf = hit.1.clone();
            self.decoded.push_back(hit);
            self.play(buf, host);
            return;
        }
        let cache = host.buffer_cache();
        let (url, key) = (url.to_owned(), key.to_owned());
        self.spawn(move |store, tx, _| {
            let result = store
                .fetch_preview(&url)
                .and_then(|bytes| cache.decode(&url, bytes).map_err(|e| e.to_string()));
            let _ = tx.send(Msg::PreviewReady {
                token,
                key,
                url: Some(url),
                result,
            });
        });
    }

    /// Plays an installed (local) file.
    pub fn preview_file(&mut self, path: &str, key: &str, host: &mut dyn PackHost) {
        self.preview_sound(
            PadSampleSource::File {
                path: path.into(),
                sample_id: None,
            },
            key,
            host,
        );
    }

    /// Plays a sound of an installed pack by pack id + pack path.
    pub fn preview_pack_file(
        &mut self,
        pack_id: &str,
        file: &str,
        key: &str,
        host: &mut dyn PackHost,
    ) {
        self.preview_sound(
            PadSampleSource::Pack {
                pack_id: pack_id.into(),
                file: file.into(),
            },
            key,
            host,
        );
    }

    /// Plays any pad source through the preview player (`loadPreviewBuffer`).
    pub fn preview_sound(&mut self, source: PadSampleSource, key: &str, host: &mut dyn PackHost) {
        let token = self.begin_preview(key, host);
        let cache = host.buffer_cache();
        let key = key.to_owned();
        self.spawn(move |_, tx, _| {
            let result = cache
                .load_preview_buffer(&source)
                .map_err(|e| e.to_string());
            let _ = tx.send(Msg::PreviewReady {
                token,
                key,
                url: None,
                result,
            });
        });
    }

    fn play(&mut self, buffer: SharedBuffer, host: &mut dyn PackHost) {
        host.play_preview(buffer);
        self.preview_playing = true;
    }

    pub fn stop_preview(&mut self, host: &mut dyn PackHost) {
        self.preview_token += 1;
        if self.preview_playing {
            host.stop_preview();
            self.preview_playing = false;
        }
        if self.snap.previewing.is_some() {
            self.snap.previewing = None;
            self.changed();
        }
    }

    /// App quit: pause running installs (staging kept), at most 3 s.
    pub fn pause_for_quit(&self) -> bool {
        self.store.pause_for_quit(crate::store::QUIT_PAUSE_TIMEOUT)
    }
}
