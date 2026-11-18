//! The pack store's "main process" side (`src/main/packs/ipc.ts`, `quit.ts`): the packs folder,
//! the installer, the loaded catalog and previews behind blocking calls that take only a pack id
//! or a preview URL (every download URL comes from the validated catalog held here).
//!
//! There is no IPC boundary in the Rust app, but the calls keep the IPC guards (`assertId`,
//! catalog-only preview URLs) so the contract stays the same. Startup housekeeping
//! ([`PackInstaller::cleanup_stale`]) runs on a thread at construction; listing, installing,
//! uninstalling and `states` wait for it, like the TS `await ready`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
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
    opts: PackStoreOptions,
    installer: Arc<PackInstaller>,
    previews: PreviewFetcher,
    catalog: Mutex<Option<PackCatalog>>,
    loading: Mutex<Option<CatalogSlot>>,
    ready: Gate,
    quit_paused: Mutex<bool>,
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
        let store = Arc::new(Self {
            opts,
            installer: installer.clone(),
            previews,
            catalog: Mutex::new(None),
            loading: Mutex::new(None),
            ready: ready.clone(),
            quit_paused: Mutex::new(false),
        });
        std::thread::Builder::new()
            .name("packs-cleanup".into())
            .spawn(move || {
                installer.cleanup_stale(SystemTime::now());
                *ready.0.lock() = true;
                ready.1.notify_all();
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
        let (slot, leader) = {
            let mut l = self.loading.lock();
            match &*l {
                Some(s) => (s.clone(), false),
                None => {
                    let s: CatalogSlot = Arc::new((Mutex::new(None), Condvar::new()));
                    *l = Some(s.clone());
                    (s, true)
                }
            }
        };
        if !leader {
            let mut g = slot.0.lock();
            while g.is_none() {
                slot.1.wait(&mut g);
            }
            return g.clone().expect("settled");
        }
        let mut fo = FetchCatalogOptions::new(&*self.opts.connector);
        fo.url = &self.opts.catalog_url;
        fo.timeout = self.opts.catalog_timeout;
        fo.bundled = self.opts.bundled.clone();
        let cat = fetch_catalog(fo).catalog;
        *self.catalog.lock() = Some(cat.clone());
        *self.loading.lock() = None;
        *slot.0.lock() = Some(cat.clone());
        slot.1.notify_all();
        cat
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
        self.await_ready();
        let cat = self.catalog_or_load();
        let Some(manifest) = cat.packs.iter().find(|p| p.id == id) else {
            return Err(format!("Unknown pack: {id}"));
        };
        self.installer
            .install(manifest)
            .wait()
            .map_err(|e| e.message())
    }

    /// `packs:cancel`: false if nothing was running.
    pub fn cancel(&self, id: &str) -> Result<bool, String> {
        Ok(self.installer.cancel(assert_id(id)?))
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
            if *q || !self.installer.busy() {
                return true;
            }
            *q = true;
        }
        let (tx, rx) = crossbeam_channel::bounded(1);
        let inst = self.installer.clone();
        std::thread::spawn(move || {
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
