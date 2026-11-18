//! Installs packs into `<packsDir>/<id>` (`src/main/packs/installer.ts`), same folders, same
//! `pack.json`, same rules:
//!
//! Files download into a staging folder `<packsDir>/.partial-<id>`, each retried with backoff
//! on transient errors, its exact size + sha256 verified. Once every file is in and `pack.json`
//! is written, the folder is renamed into place in one step (an existing install is moved aside
//! to `.old-<id>-<hex>` first and put back if the swap fails).
//!
//! Resume: a failed or paused install (app quit, [`PackInstaller::pause_all`]) keeps its staging
//! folder, so the next install only downloads files that are not already there with the right
//! size and sha256; a cancel removes it. An install settles only after every download worker
//! settled (partial-file cleanup included), so a Retry never races the previous attempt.
//! Startup ([`PackInstaller::cleanup_stale`]): an update interrupted between the two renames is
//! restored from `.old-<id>-*`; `.del-*`, legacy `.tmp-*` and obsolete `.old-*` are removed (a
//! backup only once a valid install of its pack is confirmed); staging folders older than a week
//! are removed.
//!
//! Threads: [`PackInstaller::install`] runs each install on its own thread with up to
//! `concurrency` download threads; state changes go to `on_state` (from those threads). A panic
//! on an install thread settles its handle with `Install failed: internal error`.
//!
//! The packs folder contract (`docs/rune-port/packs-dir-protocol.md`, shared with the Electron
//! app):
//! - **Lock** ([`crate::lock`]): installs, uninstalls and the startup cleanup hold
//!   `<packs>/.lock/`, so two app instances (this app and the Electron one included) never
//!   change one packs folder at the same time. An install waits for it (a cancel or pause
//!   still ends it), an uninstall waits at most [`InstallerOptions::uninstall_lock_wait`], the
//!   cleanup is skipped while another owner holds it.
//! - **Per pack**: an install of a pack being uninstalled waits for the uninstall.
//! - **Durability**: every downloaded file and `pack.json` is fsynced, then every staging
//!   folder (deepest first), then the two renames, then `<packs>` itself; only then is the
//!   replaced version's `.old-*` backup removed. Guarantees: after a process kill at any point
//!   the next start has either the old version or the complete new one; a pack reported
//!   `installed` survives a power loss (macOS: `F_FULLFSYNC`, via `sync_all`). Recovery counts
//!   `<id>` as installed only when its `pack.json` is valid and every manifest file is a
//!   regular file of the declared size.
//! - **Symlinks** are never followed: a symlinked (or file) staging folder is unlinked and
//!   replaced by a real folder; only real folders count as installs or backups; removal
//!   unlinks links. Not race-proof against a concurrent local attacker.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime};

use mdaw_model::fs_entries::locale_compare;
use mdaw_model::js;
use mdaw_model::packs::{
    InstalledPack, MAX_FILE_BYTES, MAX_PACK_BYTES, PACK_META_FILE, PackInstallState, PackManifest,
    declared_pack_bytes, is_valid_pack_id, safe_pack_path_segments, validate_installed,
    validate_manifest,
};
use parking_lot::{Condvar, Mutex};
use sha2::{Digest, Sha256};

use crate::download::{DownloadOptions, RetryOptions, download_to_file_with_retry, hex};
use crate::fault::{FsFaults, FsOp, NoFaults};
use crate::http::Connector;
use crate::json::{iso_now, stringify_pretty};
use crate::lock::{DirLock, LockError, STALE_PREFIX};
use crate::signal::{AbortReason, Signal};

/// Legacy temp folders (older app versions); removed at startup.
pub const TMP_PREFIX: &str = ".tmp-";
/// Previous install moved aside during an update swap.
pub const OLD_PREFIX: &str = ".old-";
/// Folder being deleted by uninstall.
pub const DEL_PREFIX: &str = ".del-";
/// Staging folder of an install (kept after a failure so Retry resumes).
pub const PARTIAL_PREFIX: &str = ".partial-";
/// Staging folders untouched for this long are removed at startup.
pub const PARTIAL_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const MAX_META_BYTES: u64 = 10 * 1024 * 1024;
/// How long an uninstall waits for another app instance's hold on the packs folder.
pub const UNINSTALL_LOCK_WAIT: Duration = Duration::from_secs(10);
/// An uninstall gave up waiting for the packs folder lock.
pub const PACKS_BUSY_MESSAGE: &str =
    "The packs folder is busy (another micro-daw window is changing packs)";
/// An install thread panicked.
pub const INTERNAL_ERROR_MESSAGE: &str = "Install failed: internal error";

/// How an install ended without a pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallError {
    /// `InstallPausedError` ('Install paused'): app quit; staging kept.
    Paused,
    /// `Error('Install cancelled')`: staging removed.
    Cancelled,
    /// Anything else, with the TS message.
    Failed(String),
}

impl InstallError {
    pub fn message(&self) -> String {
        match self {
            InstallError::Paused => "Install paused".into(),
            InstallError::Cancelled => "Install cancelled".into(),
            InstallError::Failed(m) => m.clone(),
        }
    }
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for InstallError {}

/// Receives every install state change (and throttled download progress).
pub type StateListener = Arc<dyn Fn(&str, &PackInstallState) + Send + Sync>;

/// `PackInstallerOptions`.
#[derive(Clone)]
pub struct InstallerOptions {
    pub packs_dir: PathBuf,
    pub on_state: Option<StateListener>,
    pub connector: Arc<dyn Connector>,
    /// Parallel file downloads per pack (default 4).
    pub concurrency: usize,
    /// Min time between progress events (default 100 ms).
    pub progress_interval: Duration,
    pub idle_timeout: Option<Duration>,
    /// Per-file retry policy (default 3 attempts, 500 ms / 1.5 s backoff).
    pub retry: RetryOptions,
    /// Tests: file-system fault injection.
    pub faults: Arc<dyn FsFaults>,
    /// How long an uninstall waits for the packs folder lock (default
    /// [`UNINSTALL_LOCK_WAIT`]).
    pub uninstall_lock_wait: Duration,
}

impl InstallerOptions {
    pub fn new(packs_dir: impl Into<PathBuf>, connector: Arc<dyn Connector>) -> Self {
        Self {
            packs_dir: packs_dir.into(),
            on_state: None,
            connector,
            concurrency: 4,
            progress_interval: Duration::from_millis(100),
            idle_timeout: None,
            retry: RetryOptions::default(),
            faults: Arc::new(NoFaults),
            uninstall_lock_wait: UNINSTALL_LOCK_WAIT,
        }
    }
}

type Outcome = Result<InstalledPack, InstallError>;

struct ActiveInstall {
    signal: Signal,
    tmp_dir: Mutex<Option<PathBuf>>,
    done: Mutex<Option<Outcome>>,
    cv: Condvar,
}

/// A running (or finished) install; clones share it (`install` of the same id while it runs).
#[derive(Clone)]
pub struct InstallHandle(Arc<ActiveInstall>);

impl InstallHandle {
    fn settled(outcome: Outcome) -> Self {
        Self(Arc::new(ActiveInstall {
            signal: Signal::new(),
            tmp_dir: Mutex::new(None),
            done: Mutex::new(Some(outcome)),
            cv: Condvar::new(),
        }))
    }

    /// Blocks until the install settled.
    pub fn wait(&self) -> Outcome {
        let mut d = self.0.done.lock();
        while d.is_none() {
            self.0.cv.wait(&mut d);
        }
        d.clone().expect("settled")
    }

    /// Waits at most `d`; None when it has not settled.
    pub fn wait_timeout(&self, d: Duration) -> Option<Outcome> {
        let end = Instant::now() + d;
        let mut g = self.0.done.lock();
        while g.is_none() {
            if self.0.cv.wait_until(&mut g, end).timed_out() {
                return g.clone();
            }
        }
        g.clone()
    }

    pub fn is_settled(&self) -> bool {
        self.0.done.lock().is_some()
    }
}

/// `resolveInside`: the absolute destination of pack file `rel` inside `root`.
pub fn resolve_inside(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let segments = safe_pack_path_segments(rel).map_err(|e| e.0)?;
    let base = absolute(root);
    let mut dest = base.clone();
    for s in &segments {
        dest.push(s);
    }
    if !dest.starts_with(&base) || dest == base {
        return Err(format!("Unsafe file path: {rel}"));
    }
    Ok(dest)
}

fn absolute(p: &Path) -> PathBuf {
    std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf())
}

fn path_str(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn random_hex8() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..8].to_owned()
}

fn exists(p: &Path) -> bool {
    fs::symlink_metadata(p).is_ok()
}

/// `fs.rm(p, { recursive: true, force: true })`.
fn rm_rf(p: &Path) -> io::Result<()> {
    match fs::symlink_metadata(p) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
        Ok(m) if m.is_dir() => fs::remove_dir_all(p),
        Ok(_) => fs::remove_file(p),
    }
}

/// `fs.rm(p, { force: true })` (a folder fails, like Node's EISDIR / ERR_FS_EISDIR).
fn rm_file(p: &Path) -> io::Result<()> {
    match fs::symlink_metadata(p) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
        Ok(m) if m.is_dir() => Err(io::Error::other(format!(
            "Path is a directory: {}",
            p.display()
        ))),
        Ok(_) => fs::remove_file(p),
    }
}

fn io_msg(e: io::Error) -> String {
    e.to_string()
}

/// `isVerifiedFile`: a regular file (not a symlink) of exactly `size` bytes with this sha256.
pub fn is_verified_file(file: &Path, size: i64, sha256: &str) -> bool {
    let Ok(m) = fs::symlink_metadata(file) else {
        return false;
    };
    if !m.is_file() || m.len() as i64 != size {
        return false;
    }
    let Ok(mut f) = fs::File::open(file) else {
        return false;
    };
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match f.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => h.update(&buf[..n]),
            Err(_) => return false,
        }
    }
    hex(&h.finalize()) == sha256.to_lowercase()
}

/// `pruneUnexpected`: removes every file / symlink under `root` not in `keep`; folders stay.
fn prune_unexpected(root: &Path, keep: &HashSet<PathBuf>) {
    let Ok(rd) = fs::read_dir(root) else { return };
    for e in rd.flatten() {
        let full = root.join(e.file_name());
        let is_dir = e.file_type().is_ok_and(|t| t.is_dir());
        if is_dir {
            prune_unexpected(&full, keep);
        } else if !keep.contains(&full) {
            let _ = fs::remove_file(&full);
        }
    }
}

/// `readInstalledMeta`: the validated pack.json of a folder; Ok(None) when there is none, Err
/// when it is invalid.
pub fn read_installed_meta(dir: &Path) -> Result<Option<InstalledPack>, String> {
    // Only a real folder (not a symlink) can be an install or a backup.
    if !fs::symlink_metadata(dir).is_ok_and(|m| m.is_dir()) {
        return Ok(None);
    }
    let meta = dir.join(PACK_META_FILE);
    let Ok(m) = fs::symlink_metadata(&meta) else {
        return Ok(None);
    };
    if !m.is_file() || m.len() > MAX_META_BYTES {
        return Ok(None);
    }
    let bytes = fs::read(&meta).map_err(io_msg)?;
    let text = String::from_utf8_lossy(&bytes);
    let raw: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    validate_installed(&raw, &path_str(dir))
        .map(Some)
        .map_err(|e| e.0)
}

/// `dir` holds a valid install of `id`: its `pack.json` validates, and every manifest file is a
/// regular file of the declared size (a cheap `lstat`, no hashing: what a power loss could
/// leave short).
pub fn is_complete_install(dir: &Path, id: &str) -> bool {
    let Ok(Some(p)) = read_installed_meta(dir) else {
        return false;
    };
    p.id == id
        && p.manifest.files.iter().all(|f| {
            resolve_inside(dir, &f.path)
                .ok()
                .and_then(|d| fs::symlink_metadata(d).ok())
                .is_some_and(|m| m.is_file() && m.len() as i64 == f.size)
        })
}

/// fsync a folder (its entries' names). Skipped where folders cannot be opened (Windows).
fn sync_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        fs::File::open(dir)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        Ok(())
    }
}

/// The `pack.json` text the installer writes (`JSON.stringify(installed, null, 2)`).
pub fn pack_json_text(installed: &InstalledPack) -> String {
    stringify_pretty(&serde_json::to_value(installed).expect("serializable"))
}

/// `.old-<id>-<8 hex>` → id (the TS `OLD_NAME_RE`).
fn old_name_id(name: &str) -> Option<&str> {
    let rest = name.strip_prefix(OLD_PREFIX)?;
    if rest.len() < 10 {
        return None;
    }
    let (id, suffix) = rest.split_at(rest.len() - 9);
    let hex = suffix.strip_prefix('-')?;
    if !hex
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    // `[a-z0-9]+(?:-[a-z0-9]+)*` (no length limit here).
    let ok = !id.is_empty()
        && id.split('-').all(|s| {
            !s.is_empty()
                && s.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        });
    ok.then_some(id)
}

/// See the module docs.
pub struct PackInstaller {
    packs_dir: PathBuf,
    opts: InstallerOptions,
    active: Mutex<HashMap<String, InstallHandle>>,
    states: Mutex<HashMap<String, PackInstallState>>,
    /// The cross-process lock on `packs_dir`.
    lock: Arc<DirLock>,
    /// Packs with an install or uninstall in progress (per-pack serialization).
    pack_ops: Mutex<HashSet<String>>,
    pack_ops_cv: Condvar,
}

/// A pack's operation slot (see [`PackInstaller::begin_pack_op`]).
struct PackOp<'a> {
    inst: &'a PackInstaller,
    id: String,
}

impl Drop for PackOp<'_> {
    fn drop(&mut self) {
        self.inst.pack_ops.lock().remove(&self.id);
        self.inst.pack_ops_cv.notify_all();
    }
}

impl PackInstaller {
    pub fn new(opts: InstallerOptions) -> Arc<Self> {
        let packs_dir = absolute(&opts.packs_dir);
        Arc::new(Self {
            lock: DirLock::new(packs_dir.clone()),
            packs_dir,
            opts,
            active: Mutex::new(HashMap::new()),
            states: Mutex::new(HashMap::new()),
            pack_ops: Mutex::new(HashSet::new()),
            pack_ops_cv: Condvar::new(),
        })
    }

    pub fn packs_dir(&self) -> &Path {
        &self.packs_dir
    }

    pub fn get_state(&self, id: &str) -> PackInstallState {
        self.states
            .lock()
            .get(id)
            .cloned()
            .unwrap_or(PackInstallState::Idle)
    }

    pub fn is_installing(&self, id: &str) -> bool {
        self.active.lock().contains_key(id)
    }

    /// True while any install runs.
    pub fn busy(&self) -> bool {
        !self.active.lock().is_empty()
    }

    /// The cross-process lock on the packs folder.
    pub fn dir_lock(&self) -> &Arc<DirLock> {
        &self.lock
    }

    /// Takes `id`'s operation slot, waiting while another operation on that pack runs, until
    /// `give_up()`.
    fn begin_pack_op(&self, id: &str, give_up: &dyn Fn() -> bool) -> Option<PackOp<'_>> {
        let mut ops = self.pack_ops.lock();
        loop {
            if ops.insert(id.to_owned()) {
                return Some(PackOp {
                    inst: self,
                    id: id.to_owned(),
                });
            }
            self.pack_ops_cv.wait_for(&mut ops, crate::lock::RETRY);
            if give_up() {
                return None;
            }
        }
    }

    /// Emits what a cancelled install leaves: `installed` when the pack is there, else `idle`
    /// (a cancelled or paused request that never started, [`crate::store::PackStore`]).
    pub(crate) fn emit_settled(&self, id: &str) {
        let st = if self.is_installed(id) {
            PackInstallState::Installed
        } else {
            PackInstallState::Idle
        };
        self.emit(id, st);
    }

    fn emit(&self, id: &str, state: PackInstallState) {
        self.states.lock().insert(id.to_owned(), state.clone());
        if let Some(cb) = &self.opts.on_state {
            cb(id, &state);
        }
    }

    fn pack_dir(&self, id: &str) -> Result<PathBuf, String> {
        if !is_valid_pack_id(id) {
            return Err("Invalid pack id".into());
        }
        Ok(self.packs_dir.join(id))
    }

    /// Staging folder of an install of `id`.
    pub fn staging_dir(&self, id: &str) -> Result<PathBuf, String> {
        if !is_valid_pack_id(id) {
            return Err("Invalid pack id".into());
        }
        Ok(self.packs_dir.join(format!("{PARTIAL_PREFIX}{id}")))
    }

    /// Installs (or re-installs / updates) a pack on a new thread. Concurrent calls for the same
    /// id share one install. An invalid manifest gives a handle that already failed.
    pub fn install(self: &Arc<Self>, raw: &PackManifest) -> InstallHandle {
        let manifest = match serde_json::to_value(raw)
            .map_err(|e| e.to_string())
            .and_then(|v| validate_manifest(&v).map_err(|e| e.0))
        {
            Ok(m) => m,
            Err(e) => return InstallHandle::settled(Err(InstallError::Failed(e))),
        };
        self.install_value(manifest)
    }

    /// `install` of a manifest given as raw JSON (validated first).
    pub fn install_json(self: &Arc<Self>, raw: &serde_json::Value) -> InstallHandle {
        match validate_manifest(raw) {
            Ok(m) => self.install_value(m),
            Err(e) => InstallHandle::settled(Err(InstallError::Failed(e.0))),
        }
    }

    fn install_value(self: &Arc<Self>, manifest: PackManifest) -> InstallHandle {
        let mut active = self.active.lock();
        if let Some(h) = active.get(&manifest.id) {
            return h.clone();
        }
        let handle = InstallHandle(Arc::new(ActiveInstall {
            signal: Signal::new(),
            tmp_dir: Mutex::new(None),
            done: Mutex::new(None),
            cv: Condvar::new(),
        }));
        active.insert(manifest.id.clone(), handle.clone());
        drop(active);
        let me = self.clone();
        let h = handle.clone();
        std::thread::Builder::new()
            .name(format!("pack-install-{}", manifest.id))
            .spawn(move || {
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    me.run_locked(&manifest, &h)
                }))
                .unwrap_or_else(|_| {
                    eprintln!("[packs] install of {} panicked", manifest.id);
                    let message = INTERNAL_ERROR_MESSAGE.to_owned();
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        me.emit(
                            &manifest.id,
                            PackInstallState::Error {
                                message: message.clone(),
                            },
                        )
                    }));
                    Err(InstallError::Failed(message))
                });
                // `finally`: the active entry goes before awaiters see the result.
                {
                    let mut active = me.active.lock();
                    if active
                        .get(&manifest.id)
                        .is_some_and(|x| Arc::ptr_eq(&x.0, &h.0))
                    {
                        active.remove(&manifest.id);
                    }
                }
                *h.0.done.lock() = Some(outcome);
                h.0.cv.notify_all();
            })
            .expect("spawn install thread");
        handle
    }

    /// Cancels a running install (its staging folder is removed). False if none was running.
    pub fn cancel(&self, id: &str) -> bool {
        match self.active.lock().get(id) {
            Some(h) => {
                h.0.signal.abort(AbortReason::Cancel);
                true
            }
            None => false,
        }
    }

    /// `run` under the pack's operation slot and the packs folder lock (both released before
    /// the handle settles).
    fn run_locked(&self, manifest: &PackManifest, entry: &InstallHandle) -> Outcome {
        let id = manifest.id.as_str();
        let signal = &entry.0.signal;
        let give_up = || signal.aborted();
        let Some(_op) = self.begin_pack_op(id, &give_up) else {
            return self.ended_before_start(id, signal);
        };
        match self.lock.acquire(&give_up, None) {
            Ok(_guard) => self.run(manifest, entry),
            Err(LockError::Aborted) => self.ended_before_start(id, signal),
            Err(LockError::Busy) => unreachable!("no deadline"),
            Err(LockError::Io(m)) => {
                self.emit(id, PackInstallState::Error { message: m.clone() });
                Err(InstallError::Failed(m))
            }
        }
    }

    /// An install aborted while waiting for its turn: nothing was touched.
    fn ended_before_start(&self, id: &str, signal: &Signal) -> Outcome {
        self.emit_settled(id);
        match signal.reason() {
            Some(AbortReason::Paused) => Err(InstallError::Paused),
            Some(AbortReason::Error(m)) => Err(InstallError::Failed(m)),
            _ => Err(InstallError::Cancelled),
        }
    }

    fn run(&self, manifest: &PackManifest, entry: &InstallHandle) -> Outcome {
        let id = manifest.id.as_str();
        let signal = &entry.0.signal;
        let result = self.run_inner(manifest, entry);
        match result {
            Ok(p) => Ok(p),
            Err(raw) => {
                let reason = if signal.aborted() {
                    signal.reason()
                } else {
                    None
                };
                let paused = reason == Some(AbortReason::Paused);
                let cancelled = !paused && reason == Some(AbortReason::Cancel);
                // A cancel discards the staging folder; a failure or pause keeps it (Retry resumes).
                let tmp = entry.0.tmp_dir.lock().take();
                if let Some(tmp) = tmp {
                    if cancelled {
                        let _ = rm_rf(&tmp);
                    } else {
                        let _ = rm_file(&tmp.join(PACK_META_FILE));
                    }
                }
                if cancelled || paused {
                    let installed = self.is_installed(id);
                    self.emit(
                        id,
                        if installed {
                            PackInstallState::Installed
                        } else {
                            PackInstallState::Idle
                        },
                    );
                    return Err(if paused {
                        InstallError::Paused
                    } else {
                        InstallError::Cancelled
                    });
                }
                self.emit(
                    id,
                    PackInstallState::Error {
                        message: raw.clone(),
                    },
                );
                Err(InstallError::Failed(raw))
            }
        }
    }

    /// The `try` block of `run`; Err carries the thrown error's message.
    fn run_inner(
        &self,
        manifest: &PackManifest,
        entry: &InstallHandle,
    ) -> Result<InstalledPack, String> {
        let id = manifest.id.as_str();
        let signal = &entry.0.signal;
        let files = &manifest.files;
        let total = declared_pack_bytes(manifest).max(0) as u64;
        struct Prog {
            received: Vec<u64>,
            done: usize,
            last: Option<Instant>,
        }
        let prog = Mutex::new(Prog {
            received: vec![0; files.len()],
            done: 0,
            last: None,
        });
        let interval = self.opts.progress_interval;
        let progress = |force: bool| {
            let mut p = prog.lock();
            let now = Instant::now();
            if !force && p.last.is_some_and(|t| now.duration_since(t) < interval) {
                return;
            }
            p.last = Some(now);
            let st = PackInstallState::Downloading {
                received_bytes: p.received.iter().sum(),
                total_bytes: Some(total),
                file_index: p.done as u64,
                file_count: files.len() as u64,
            };
            self.emit(id, st);
        };
        let abort_err = || -> String {
            match signal.reason() {
                Some(AbortReason::Error(m)) => m,
                Some(AbortReason::Paused) => "Install paused".into(),
                _ => "aborted".into(),
            }
        };

        progress(true);
        fs::create_dir_all(&self.packs_dir).map_err(io_msg)?;
        let stage = self.staging_dir(id)?;
        *entry.0.tmp_dir.lock() = Some(stage.clone());
        // Never follow a planted link: a symlink (or a file) in the staging folder's place is
        // unlinked (the link itself, not its target) and a real folder created.
        if fs::symlink_metadata(&stage).is_ok_and(|m| !m.is_dir()) {
            fs::remove_file(&stage).map_err(io_msg)?;
        }
        fs::create_dir_all(&stage).map_err(io_msg)?;

        // Resolve every destination before downloading anything.
        let dests: Vec<PathBuf> = files
            .iter()
            .map(|f| resolve_inside(&stage, &f.path))
            .collect::<Result<_, _>>()?;
        rm_file(&stage.join(PACK_META_FILE)).map_err(io_msg)?;
        prune_unexpected(&stage, &dests.iter().cloned().collect());
        let mut dirs: Vec<&Path> = Vec::new();
        for d in &dests {
            let p = d.parent().unwrap_or(&stage);
            if !dirs.contains(&p) {
                dirs.push(p);
            }
        }
        for d in dirs {
            fs::create_dir_all(d).map_err(io_msg)?;
        }

        // Resume: keep files of an earlier attempt that are complete and verified.
        let mut todo: Vec<usize> = Vec::new();
        for (i, f) in files.iter().enumerate() {
            if signal.aborted() {
                return Err(abort_err());
            }
            if is_verified_file(&dests[i], f.size, &f.sha256) {
                let mut p = prog.lock();
                p.received[i] = f.size as u64;
                p.done += 1;
            } else {
                rm_rf(&dests[i]).map_err(io_msg)?;
                todo.push(i);
            }
        }
        progress(true);

        let next = AtomicUsize::new(0);
        let workers = self.opts.concurrency.min(todo.len()).max(1);
        let connector = self.opts.connector.clone();
        let faults = self.opts.faults.clone();
        let any_failed = std::thread::scope(|s| {
            let handles: Vec<_> = (0..workers)
                .map(|_| {
                    s.spawn(|| -> Result<(), String> {
                        while !signal.aborted() {
                            let k = next.fetch_add(1, Ordering::SeqCst);
                            let Some(&i) = todo.get(k) else { return Ok(()) };
                            let file = &files[i];
                            let on_progress = |bytes: u64| {
                                let sum: u64 = {
                                    let mut p = prog.lock();
                                    p.received[i] = bytes;
                                    p.received.iter().sum()
                                };
                                if sum > MAX_PACK_BYTES as u64 {
                                    signal.abort(AbortReason::Error("Pack too large".into()));
                                    return;
                                }
                                progress(false);
                            };
                            let on_retry = |_attempt: u32, _e: &crate::download::DownloadError| {
                                prog.lock().received[i] = 0;
                                progress(true);
                            };
                            let mut opts = DownloadOptions::new(
                                (MAX_FILE_BYTES.min(file.size)).max(0) as u64,
                                &*connector,
                            );
                            opts.expected_bytes = Some(file.size.max(0) as u64);
                            opts.sha256 = Some(&file.sha256);
                            opts.signal = Some(signal);
                            opts.idle_timeout = self.opts.idle_timeout;
                            opts.on_progress = Some(&on_progress);
                            opts.faults = Some(&*faults);
                            let r = download_to_file_with_retry(
                                &file.url,
                                &dests[i],
                                &opts,
                                self.opts.retry,
                                Some(&on_retry),
                            );
                            if let Err(e) = r {
                                if !signal.aborted() {
                                    signal.abort(AbortReason::Error(e.message()));
                                }
                                return Err(e.message());
                            }
                            prog.lock().done += 1;
                            progress(true);
                        }
                        Ok(())
                    })
                })
                .collect();
            // Wait for EVERY worker (B2): an aborted worker still removes its partial file.
            let mut failed = false;
            for h in handles {
                if !matches!(h.join(), Ok(Ok(()))) {
                    failed = true;
                }
            }
            failed
        });
        if any_failed {
            return Err(match signal.reason() {
                Some(AbortReason::Error(m)) => m,
                _ => "aborted".into(),
            });
        }
        if signal.aborted() {
            return Err(abort_err());
        }

        let final_dir = self.pack_dir(id)?;
        let installed = InstalledPack {
            id: id.to_owned(),
            version: manifest.version,
            dir: path_str(&final_dir),
            installed_at: iso_now(),
            manifest: manifest.clone(),
        };
        let meta = stage.join(PACK_META_FILE);
        let faults = &self.opts.faults;
        faults
            .before(FsOp::WriteFile, &meta, None)
            .and_then(|_| {
                let mut f = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&meta)?;
                io::Write::write_all(&mut f, pack_json_text(&installed).as_bytes())?;
                faults.before(FsOp::Sync, &meta, None)?;
                f.sync_all()
            })
            .map_err(io_msg)?;
        // Every staging folder, deepest first, so the files' names are on disk too.
        let mut folders: Vec<PathBuf> = Vec::new();
        for d in &dests {
            let mut p = d.parent();
            while let Some(dir) = p {
                if !dir.starts_with(&stage) {
                    break;
                }
                if !folders.iter().any(|f| f == dir) {
                    folders.push(dir.to_path_buf());
                }
                p = dir.parent();
            }
        }
        folders.sort_by_key(|f| std::cmp::Reverse(f.components().count()));
        for f in &folders {
            faults
                .before(FsOp::Sync, f, None)
                .and_then(|_| sync_dir(f))
                .map_err(io_msg)?;
        }
        if signal.aborted() {
            return Err(abort_err());
        }
        self.swap_into_place(&stage, &final_dir, id)
            .map_err(io_msg)?;
        *entry.0.tmp_dir.lock() = None;
        self.emit(id, PackInstallState::Installed);
        Ok(installed)
    }

    fn rename(&self, a: &Path, b: &Path) -> io::Result<()> {
        self.opts.faults.before(FsOp::Rename, a, Some(b))?;
        fs::rename(a, b)
    }

    /// `swapIntoPlace`: atomically replaces `<packsDir>/<id>` with `tmp`.
    fn swap_into_place(&self, tmp: &Path, final_dir: &Path, id: &str) -> io::Result<()> {
        let mut old = None;
        if exists(final_dir) {
            let o = self
                .packs_dir
                .join(format!("{OLD_PREFIX}{id}-{}", random_hex8()));
            self.rename(final_dir, &o)?;
            old = Some(o);
        }
        if let Err(e) = self.rename(tmp, final_dir) {
            if let Some(o) = &old {
                let _ = self.rename(o, final_dir);
            }
            return Err(e);
        }
        // Both renames on disk before the backup goes. Should that fail, the new version is
        // in place anyway: keep the backup (the next start drops it once the install checks
        // out complete).
        let faults = &self.opts.faults;
        if let Err(e) = faults
            .before(FsOp::Sync, &self.packs_dir, None)
            .and_then(|_| sync_dir(&self.packs_dir))
        {
            eprintln!("[packs] could not sync {}: {e}", self.packs_dir.display());
            return Ok(());
        }
        if let Some(o) = old
            && faults.before(FsOp::Remove, &o, None).is_ok()
        {
            let _ = rm_rf(&o);
        }
        Ok(())
    }

    pub fn is_installed(&self, id: &str) -> bool {
        self.pack_dir(id).is_ok_and(|d| {
            fs::symlink_metadata(&d).is_ok_and(|m| m.is_dir()) && exists(&d.join(PACK_META_FILE))
        })
    }

    /// Ids of packs with a resumable staging folder (holding at least one file) and no install
    /// running: paused by an app quit or failed in an earlier session (N7). Sorted.
    pub fn paused_ids(&self) -> Vec<String> {
        let Ok(rd) = fs::read_dir(&self.packs_dir) else {
            return vec![];
        };
        let mut out = Vec::new();
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Some(id) = name.strip_prefix(PARTIAL_PREFIX) else {
                continue;
            };
            if !is_valid_pack_id(id) || self.is_installing(id) {
                continue;
            }
            let full = self.packs_dir.join(&name);
            if fs::symlink_metadata(&full).is_ok_and(|m| m.is_dir()) && has_file(&full) {
                out.push(id.to_owned());
            }
        }
        out.sort();
        out
    }

    /// Removes an installed pack (cancelling a running install first).
    pub fn uninstall(&self, id: &str) -> Result<(), String> {
        let dir = self.pack_dir(id)?;
        // Cancel a running install, then hold the pack's slot: an install started from now on
        // waits for this uninstall.
        let _op = loop {
            let running = self.active.lock().get(id).cloned();
            if let Some(r) = running {
                r.0.signal.abort(AbortReason::Cancel);
                let _ = r.wait();
                continue;
            }
            if let Some(op) = self.begin_pack_op(id, &|| true) {
                break op;
            }
        };
        let _lock = self
            .lock
            .acquire(
                &|| false,
                Some(Instant::now() + self.opts.uninstall_lock_wait),
            )
            .map_err(|e| match e {
                LockError::Io(m) => m,
                _ => PACKS_BUSY_MESSAGE.to_owned(),
            })?;
        if exists(&dir) {
            // Moved aside first so a half-deleted folder never looks installed; a different
            // prefix from the update swap, so startup recovery never brings it back.
            let doomed = self
                .packs_dir
                .join(format!("{DEL_PREFIX}{id}-{}", random_hex8()));
            self.rename(&dir, &doomed).map_err(io_msg)?;
            rm_rf(&doomed).map_err(io_msg)?;
        }
        if let Ok(stage) = self.staging_dir(id) {
            let _ = rm_rf(&stage);
        }
        self.emit(id, PackInstallState::Idle);
        Ok(())
    }

    /// Installed packs with a valid pack.json (folder name = pack id), by `manifest.name`
    /// (`localeCompare`).
    pub fn list_installed(&self) -> Vec<InstalledPack> {
        let Ok(rd) = fs::read_dir(&self.packs_dir) else {
            return vec![];
        };
        let mut out = Vec::new();
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !e.file_type().is_ok_and(|t| t.is_dir()) || !is_valid_pack_id(&name) {
                continue;
            }
            match read_installed_meta(&self.packs_dir.join(&name)) {
                Ok(Some(p)) if p.id == name => out.push(p),
                Ok(_) => {}
                Err(err) => eprintln!("[packs] skipping invalid installed pack {name} {err}"),
            }
        }
        out.sort_by(|a, b| locale_compare(&a.manifest.name, &b.manifest.name));
        out
    }

    fn is_valid_install(&self, id: &str) -> bool {
        self.pack_dir(id).is_ok_and(|d| is_complete_install(&d, id))
    }

    /// Startup housekeeping (skips folders of running installs), see the module docs. `now`
    /// dates the staging-folder expiry.
    pub fn cleanup_stale(&self, now: SystemTime) {
        let Ok(rd) = fs::read_dir(&self.packs_dir) else {
            return;
        };
        let names: Vec<String> = rd
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        // One try: another owner (an other app instance) is changing the folder right now.
        let _lock = match self.lock.acquire(&|| false, Some(Instant::now())) {
            Ok(g) => g,
            Err(e) => {
                eprintln!("[packs] startup cleanup skipped: {e:?}");
                return;
            }
        };
        let busy: HashSet<PathBuf> = self
            .active
            .lock()
            .values()
            .filter_map(|a| a.0.tmp_dir.lock().clone())
            .collect();
        let present: HashSet<&str> = names.iter().map(String::as_str).collect();

        // 1. Recovery of interrupted swaps (M1: backups survive until a valid install is
        // confirmed).
        let mut candidates: Vec<(String, Vec<(PathBuf, f64)>)> = Vec::new();
        for n in &names {
            let Some(id) = old_name_id(n) else { continue };
            if self.is_installing(id) {
                continue;
            }
            let dir = self.packs_dir.join(n);
            let Ok(Some(pack)) = read_installed_meta(&dir) else {
                continue;
            };
            if pack.id != id {
                continue;
            }
            let at = js::date_parse(&pack.installed_at);
            let at = if at.is_nan() || at == 0.0 { 0.0 } else { at };
            match candidates.iter_mut().find(|(k, _)| k == id) {
                Some((_, l)) => l.push((dir, at)),
                None => candidates.push((id.to_owned(), vec![(dir, at)])),
            }
        }
        let mut keep: HashSet<PathBuf> = HashSet::new();
        for (id, mut list) in candidates {
            let Ok(final_dir) = self.pack_dir(&id) else {
                continue;
            };
            if present.contains(id.as_str()) && self.is_valid_install(&id) {
                continue; // the backups are obsolete
            }
            list.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            // The newest backup that is complete (a power loss can leave one short).
            let Some(best) = list
                .iter()
                .map(|c| c.0.clone())
                .find(|d| is_complete_install(d, &id))
            else {
                keep.extend(list.into_iter().map(|c| c.0));
                continue;
            };
            if self.restore_backup(&id, &final_dir, &best, present.contains(id.as_str())) {
                keep.insert(best);
                eprintln!("[packs] restored {id} after an interrupted update");
            } else {
                keep.extend(list.into_iter().map(|c| c.0));
            }
        }

        // 2 + 3. Removal.
        for n in &names {
            let full = self.packs_dir.join(n);
            if busy.contains(&full) || keep.contains(&full) {
                continue;
            }
            if n.starts_with(TMP_PREFIX)
                || n.starts_with(OLD_PREFIX)
                || n.starts_with(DEL_PREFIX)
                || n.starts_with(STALE_PREFIX)
            {
                let _ = rm_rf(&full);
            } else if n.starts_with(PARTIAL_PREFIX) {
                if fs::symlink_metadata(&full).is_ok_and(|m| !m.is_dir()) {
                    // Not ours: unlink the link, never follow it.
                    let _ = fs::remove_file(&full);
                    continue;
                }
                let old = fs::symlink_metadata(&full)
                    .and_then(|m| m.modified())
                    .ok()
                    .is_some_and(|m| now.duration_since(m).is_ok_and(|age| age > PARTIAL_MAX_AGE));
                if old {
                    let _ = rm_rf(&full);
                }
            }
        }
    }

    /// `restoreBackup`: moves `backup` to `final_dir` (a present but invalid `final_dir` is moved
    /// aside first and put back if the restore fails). True once the restored folder is a valid
    /// install.
    fn restore_backup(&self, id: &str, final_dir: &Path, backup: &Path, present: bool) -> bool {
        let mut aside = None;
        let moved = (|| -> io::Result<()> {
            if present {
                let a = self
                    .packs_dir
                    .join(format!("{DEL_PREFIX}{id}-{}", random_hex8()));
                self.rename(final_dir, &a)?;
                aside = Some(a);
            }
            self.rename(backup, final_dir)
        })();
        if let Err(err) = moved {
            eprintln!("[packs] could not restore {id} {err}");
            if let Some(a) = &aside {
                let _ = self.rename(a, final_dir);
            }
            return false;
        }
        if self.is_valid_install(id) {
            if let Some(a) = &aside {
                let _ = rm_rf(a);
            }
            return true;
        }
        let _ = self.rename(final_dir, backup);
        if let Some(a) = &aside {
            let _ = self.rename(a, final_dir);
        }
        false
    }

    /// Cancels everything; the cancelled installs remove their staging folders.
    pub fn cancel_all(&self) {
        let running: Vec<InstallHandle> = self.active.lock().values().cloned().collect();
        for r in &running {
            r.0.signal.abort(AbortReason::Cancel);
        }
        for r in &running {
            let _ = r.wait();
        }
    }

    /// App quit: stops every running install but KEEPS its staging folder (the next start
    /// resumes). Returns once every install settled.
    pub fn pause_all(&self) {
        let running: Vec<InstallHandle> = self.active.lock().values().cloned().collect();
        for r in &running {
            r.0.signal.abort(AbortReason::Paused);
        }
        for r in &running {
            let _ = r.wait();
        }
    }
}

/// The folder (recursively, not following links) holds at least one regular file.
fn has_file(dir: &Path) -> bool {
    let Ok(rd) = fs::read_dir(dir) else {
        return false;
    };
    for e in rd.flatten() {
        let Ok(t) = e.file_type() else { continue };
        if t.is_file() {
            return true;
        }
        if t.is_dir() && has_file(&e.path()) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_folder_names_parse_like_the_ts_regex() {
        assert_eq!(old_name_id(".old-test-pack-0badf00d"), Some("test-pack"));
        assert_eq!(old_name_id(".old-a-12345678"), Some("a"));
        assert_eq!(old_name_id(".old-x-1"), None);
        assert_eq!(old_name_id(".old-test-pack-0BADF00D"), None);
        assert_eq!(old_name_id(".old--12345678"), None);
        assert_eq!(old_name_id(".old-a--b-12345678"), None);
        assert_eq!(old_name_id(".del-a-12345678"), None);
    }

    #[test]
    fn resolve_inside_rejects_escapes() {
        let root = Path::new("/tmp/packs-root");
        assert!(resolve_inside(root, "a/../../x.wav").is_err());
        assert!(resolve_inside(root, "../x.wav").is_err());
        assert_eq!(
            resolve_inside(root, "a/b.wav").unwrap(),
            Path::new("/tmp/packs-root/a/b.wav")
        );
    }
}
