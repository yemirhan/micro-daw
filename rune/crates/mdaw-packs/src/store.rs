//! The pack store's "main process" side (`src/main/packs/ipc.ts`, `quit.ts`): the packs folder,
//! the installer, the loaded catalog and previews behind blocking calls that take only a pack id
//! or a preview URL (every download URL comes from the validated catalog held here).
//!
//! There is no IPC boundary in the Rust app, but the calls keep the IPC guards (`assertId`,
//! catalog-only preview URLs) so the contract stays the same. Startup housekeeping
//! ([`PackInstaller::cleanup_stale`]) runs on a thread at construction; listing, installing,
//! uninstalling and `states` wait for it, like the TS `await ready`.
//!
//! An install request is registered (with its own [`Signal`]) before those waits, so a cancel
//! or an app quit that arrives while it waits for the startup cleanup or the catalog ends it
//! there (`Install cancelled` / `Install paused`) and it never starts (review finding 10; the
//! TS `cancel` returned false and the install ran anyway). Worker panics never leave a waiter
//! hanging: the cleanup always opens the gate, a catalog fetch always settles (bundled copy).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};
use std::time::{Duration, SystemTime};

use mdaw_model::packs::{
    INSTALL_PAUSED_MESSAGE, InstalledPack, PACK_CATALOG_URL, PackCatalog, PackInstallState,
    find_catalog_file, is_valid_pack_id,
};
use parking_lot::{Condvar, Mutex};

use crate::catalog::{CATALOG_TIMEOUT, FetchCatalogOptions, bundled_catalog, fetch_catalog};
use crate::fault::{FsFaults, NoFaults};
use crate::http::{Connector, TlsConnector};
use crate::installer::{InstallerOptions, PackInstaller, StateListener};
use crate::preview::{PreviewCacheOptions, PreviewFetcher};
use crate::signal::{AbortReason, Signal};

/// `QUIT_PAUSE_TIMEOUT_MS`.
pub const QUIT_PAUSE_TIMEOUT: Duration = Duration::from_millis(3000);

/// How [`PackStore`] is built.
#[derive(Clone)]
pub struct PackStoreOptions {
    /// `<userData>/packs`.
    pub packs_dir: PathBuf,
    pub connector: Arc<dyn Connector>,
    pub catalog_url: String,
    pub catalog_timeout: Duration,
    pub bundled: PackCatalog,
    /// Installer tuning (tests): concurrency, progress interval, retry, idle timeout, faults.
    pub concurrency: usize,
    pub progress_interval: Duration,
    pub retry: crate::download::RetryOptions,
    pub idle_timeout: Option<Duration>,
    pub faults: Arc<dyn FsFaults>,
    pub previews: PreviewCacheOptions,
}

impl PackStoreOptions {
    /// The app's settings: TLS, the real catalog URL, the bundled catalog.
    pub fn new(packs_dir: impl Into<PathBuf>) -> Self {
        Self::with_connector(packs_dir, Arc::new(TlsConnector::new()))
    }

    pub fn with_connector(packs_dir: impl Into<PathBuf>, connector: Arc<dyn Connector>) -> Self {
        Self {
            packs_dir: packs_dir.into(),
            connector,
            catalog_url: PACK_CATALOG_URL.into(),
            catalog_timeout: CATALOG_TIMEOUT,
            bundled: bundled_catalog(),
            concurrency: 4,
            progress_interval: Duration::from_millis(100),
            retry: Default::default(),
            idle_timeout: None,
            faults: Arc::new(NoFaults),
            previews: PreviewCacheOptions::default(),
        }
    }
}

type Gate = Arc<(Mutex<bool>, Condvar)>;
type CatalogSlot = Arc<(Mutex<Option<PackCatalog>>, Condvar)>;

/// See the module docs. Every method blocks: call them from worker threads.
pub struct PackStore {
    me: Weak<PackStore>,
    opts: PackStoreOptions,
    installer: Arc<PackInstaller>,
    previews: PreviewFetcher,
    catalog: Mutex<Option<PackCatalog>>,
    loading: Mutex<Option<CatalogSlot>>,
    ready: Gate,
    quit_paused: Mutex<bool>,
    /// Install requests waiting for the startup cleanup / the catalog (not yet handed to the
    /// installer), each with its signal.
    queued: Mutex<Vec<(String, Signal)>>,
}

/// Sets a flag + notifies on drop (also when the owner panics).
struct OpenOnDrop(Gate);

impl Drop for OpenOnDrop {
    fn drop(&mut self) {
        *self.0.0.lock() = true;
        self.0.1.notify_all();
    }
}

/// Settles a catalog fetch on drop: the leader's result, or the bundled copy when it panicked.
struct SettleCatalog {
    store: Weak<PackStore>,
    slot: CatalogSlot,
    bundled: PackCatalog,
    result: Option<PackCatalog>,
}

impl Drop for SettleCatalog {
    fn drop(&mut self) {
        let cat = match self.result.take() {
            Some(c) => c,
            None => {
                eprintln!("[packs] catalog fetch failed internally, using bundled copy");
                self.bundled.clone()
            }
        };
        if let Some(store) = self.store.upgrade() {
            *store.catalog.lock() = Some(cat.clone());
            let mut l = store.loading.lock();
            if l.as_ref().is_some_and(|s| Arc::ptr_eq(s, &self.slot)) {
                *l = None;
            }
        }
        *self.slot.0.lock() = Some(cat);
        self.slot.1.notify_all();
    }
}

/// Removes a queued request on drop.
struct Dequeue<'a>(&'a PackStore, Signal);

impl Drop for Dequeue<'_> {
    fn drop(&mut self) {
        let sig = &self.1;
        self.0.queued.lock().retain(|(_, s)| !s.same(sig));
    }
}

fn assert_id(id: &str) -> Result<&str, String> {
    if is_valid_pack_id(id) {
        Ok(id)
    } else {
        Err("Invalid pack id".into())
    }
}

impl PackStore {
    /// Builds the store and starts the startup cleanup. `on_state` receives every install state
    /// change (`packs:progress`).
    pub fn new(opts: PackStoreOptions, on_state: Option<StateListener>) -> Arc<Self> {
        let mut iopts = InstallerOptions::new(opts.packs_dir.clone(), opts.connector.clone());
        iopts.on_state = on_state;
        iopts.concurrency = opts.concurrency;
        iopts.progress_interval = opts.progress_interval;
        iopts.retry = opts.retry;
        iopts.idle_timeout = opts.idle_timeout;
        iopts.faults = opts.faults.clone();
        let installer = PackInstaller::new(iopts);
        let previews = PreviewFetcher::new(opts.connector.clone(), opts.previews);
        let ready: Gate = Arc::new((Mutex::new(false), Condvar::new()));
        let store = Arc::new_cyclic(|me| Self {
            me: me.clone(),
            opts,
            installer: installer.clone(),
            previews,
            catalog: Mutex::new(None),
            loading: Mutex::new(None),
            ready: ready.clone(),
            quit_paused: Mutex::new(false),
            queued: Mutex::new(Vec::new()),
        });
        std::thread::Builder::new()
            .name("packs-cleanup".into())
            .spawn(move || {
                // The gate opens even if the cleanup panics.
                let _open = OpenOnDrop(ready);
                installer.cleanup_stale(SystemTime::now());
            })
            .expect("spawn cleanup thread");
        store
    }

    fn await_ready(&self) {
        let mut r = self.ready.0.lock();
        while !*r {
            self.ready.1.wait(&mut r);
        }
    }

    /// `await_ready`, giving up when `sig` is aborted. False when aborted.
    fn await_ready_or(&self, sig: &Signal) -> bool {
        let mut r = self.ready.0.lock();
        while !*r {
            if sig.aborted() {
                return false;
            }
            self.ready.1.wait_for(&mut r, crate::http::POLL);
        }
        !sig.aborted()
    }

    pub fn installer(&self) -> &Arc<PackInstaller> {
        &self.installer
    }

    /// `packs:dir`.
    pub fn dir(&self) -> &Path {
        self.installer.packs_dir()
    }

    /// `packs:fetch-catalog`: remote merged over bundled (never fails: falls back to the bundled
    /// copy); concurrent calls share one fetch.
    pub fn fetch_catalog(&self) -> PackCatalog {
        let slot = self.catalog_slot();
        let mut g = slot.0.lock();
        while g.is_none() {
            slot.1.wait(&mut g);
        }
        g.clone().expect("settled")
    }

    /// The running catalog fetch, or a new one on its own thread.
    fn catalog_slot(&self) -> CatalogSlot {
        let mut l = self.loading.lock();
        if let Some(s) = &*l {
            return s.clone();
        }
        let slot: CatalogSlot = Arc::new((Mutex::new(None), Condvar::new()));
        *l = Some(slot.clone());
        drop(l);
        let settle = SettleCatalog {
            store: self.me.clone(),
            slot: slot.clone(),
            bundled: self.opts.bundled.clone(),
            result: None,
        };
        let opts = self.opts.clone();
        let spawned = std::thread::Builder::new()
            .name("packs-catalog".into())
            .spawn(move || {
                // The whole guard moves here (not just the field), so it settles when this
                // thread ends, panics included.
                let mut settle = settle;
                let mut fo = FetchCatalogOptions::new(&*opts.connector);
                fo.url = &opts.catalog_url;
                fo.timeout = opts.catalog_timeout;
                fo.bundled = opts.bundled.clone();
                settle.result = Some(fetch_catalog(fo).catalog);
            });
        if spawned.is_err() {
            // `settle` was dropped with the closure: the slot holds the bundled copy.
            eprintln!("[packs] could not start the catalog fetch");
        }
        slot
    }

    /// The loaded catalog, or the next fetch's result; None when `sig` was aborted first.
    fn catalog_or_load_until(&self, sig: &Signal) -> Option<PackCatalog> {
        if let Some(c) = self.loaded_catalog() {
            return Some(c);
        }
        let slot = self.catalog_slot();
        let mut g = slot.0.lock();
        loop {
            if let Some(c) = &*g {
                return Some(c.clone());
            }
            if sig.aborted() {
                return None;
            }
            slot.1.wait_for(&mut g, crate::http::POLL);
        }
    }

    /// The catalog loaded last (None before the first fetch).
    pub fn loaded_catalog(&self) -> Option<PackCatalog> {
        self.catalog.lock().clone()
    }

    fn catalog_or_load(&self) -> PackCatalog {
        match self.loaded_catalog() {
            Some(c) => c,
            None => self.fetch_catalog(),
        }
    }

    /// `packs:list-installed`.
    pub fn list_installed(&self) -> Vec<InstalledPack> {
        self.await_ready();
        self.installer.list_installed()
    }

    /// `packs:install`.
    pub fn install(&self, id: &str) -> Result<InstalledPack, String> {
        let id = assert_id(id)?;
        let sig = Signal::new();
        self.queued.lock().push((id.to_owned(), sig.clone()));
        let dequeue = Dequeue(self, sig.clone());
        let cat = if self.await_ready_or(&sig) {
            self.catalog_or_load_until(&sig)
        } else {
            None
        };
        let handle = {
            // Under the queue lock, so a cancel either finds the request queued or finds the
            // install running.
            let mut q = self.queued.lock();
            if sig.aborted() {
                drop(q);
                drop(dequeue);
                self.installer.emit_settled(id);
                return Err(match sig.reason() {
                    Some(AbortReason::Paused) => "Install paused".into(),
                    _ => "Install cancelled".into(),
                });
            }
            let Some(manifest) = cat
                .as_ref()
                .and_then(|c| c.packs.iter().find(|p| p.id == id))
            else {
                q.retain(|(_, s)| !s.same(&sig));
                return Err(format!("Unknown pack: {id}"));
            };
            let h = self.installer.install(manifest);
            q.retain(|(_, s)| !s.same(&sig));
            h
        };
        drop(dequeue);
        handle.wait().map_err(|e| e.message())
    }

    /// `packs:cancel`: false if nothing was running or waiting to run.
    pub fn cancel(&self, id: &str) -> Result<bool, String> {
        let id = assert_id(id)?;
        let q = self.queued.lock();
        let mut found = self.installer.cancel(id);
        for (qid, sig) in q.iter() {
            if qid == id {
                sig.abort(AbortReason::Cancel);
                found = true;
            }
        }
        Ok(found)
    }

    /// Install requests waiting for the startup cleanup or the catalog.
    pub fn queued_installs(&self) -> usize {
        self.queued.lock().len()
    }

    /// `packs:uninstall`.
    pub fn uninstall(&self, id: &str) -> Result<(), String> {
        let id = assert_id(id)?;
        self.await_ready();
        self.installer.uninstall(id)
    }

    /// `packs:fetch-preview`: only exact URLs of files in the loaded catalog.
    pub fn fetch_preview(&self, url: &str) -> Result<Vec<u8>, String> {
        if url.encode_utf16().count() > 4096 {
            return Err("Preview URL not allowed".into());
        }
        let cat = self.catalog_or_load();
        let Some(file) = find_catalog_file(&cat, url) else {
            return Err("Preview URL not allowed".into());
        };
        self.previews
            .fetch(&file.url, Some(file.size.max(0) as u64), Some(&file.sha256))
            .map_err(|e| e.message())
    }

    /// `packs:states`: paused installs (staging kept) as `error: 'Download paused'`, overlaid by
    /// running installs of catalog packs.
    pub fn states(&self) -> BTreeMap<String, PackInstallState> {
        self.await_ready();
        let mut out = BTreeMap::new();
        for id in self.installer.paused_ids() {
            out.insert(
                id,
                PackInstallState::Error {
                    message: INSTALL_PAUSED_MESSAGE.into(),
                },
            );
        }
        if let Some(cat) = self.loaded_catalog() {
            for p in &cat.packs {
                if self.installer.is_installing(&p.id) {
                    out.insert(p.id.clone(), self.installer.get_state(&p.id));
                }
            }
        }
        out
    }

    /// `pauseInstallsOnQuit` (`will-quit`): when installs run, pauses them (staging kept) and
    /// waits for them to settle, at most `timeout`. Returns false when it gave up waiting. A
    /// second call does nothing (the quit that follows is not intercepted again).
    pub fn pause_for_quit(&self, timeout: Duration) -> bool {
        {
            let mut q = self.quit_paused.lock();
            if *q || (!self.installer.busy() && self.queued.lock().is_empty()) {
                return true;
            }
            *q = true;
        }
        // Queued requests end as paused before they start.
        for (_, sig) in self.queued.lock().iter() {
            sig.abort(AbortReason::Paused);
        }
        let (tx, rx) = crossbeam_channel::bounded(1);
        let inst = self.installer.clone();
        let me = self.me.clone();
        std::thread::spawn(move || {
            inst.pause_all();
            // And wait for the queued ones to leave.
            while me.upgrade().is_some_and(|s| !s.queued.lock().is_empty()) {
                std::thread::sleep(crate::http::POLL);
            }
            inst.pause_all();
            let _ = tx.send(());
        });
        let settled = rx.recv_timeout(timeout).is_ok();
        if !settled {
            eprintln!(
                "[packs] installs did not settle within {} ms; quitting anyway",
                timeout.as_millis()
            );
        }
        settled
    }
}
