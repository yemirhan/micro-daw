//! The cross-process lock on a packs folder: `<packs>/.lock/` (see
//! `docs/rune-port/packs-dir-protocol.md` §1; the Electron app's `src/main/packs/lock.ts` follows
//! the same protocol, so the two apps never change one packs folder at the same time).
//!
//! - A directory created with `mkdir` (atomic), holding `owner.json`
//!   `{"pid", "app": "rune", "token", "acquiredAt"}`.
//! - Held for every mutating operation (install, uninstall, startup cleanup) and refcounted per
//!   [`DirLock`]: concurrent operations of one installer share the hold.
//! - Heartbeat: the holder sets the folder's mtime every [`HEARTBEAT`].
//! - Stale (stolen): its owner's pid is not alive (unix), or its mtime is older than
//!   [`STALE_AFTER`]. A steal renames the folder aside, checks the token it judged stale and
//!   renames it back when a fresh lock was caught by mistake.
//! - Released only when `owner.json` still carries our token.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use parking_lot::{Condvar, Mutex};

/// The lock folder's name inside the packs folder.
pub const LOCK_DIR: &str = ".lock";
/// Prefix of a lock folder renamed aside by a steal.
pub const STALE_PREFIX: &str = ".lock-stale-";
const OWNER_FILE: &str = "owner.json";
/// How often the holder refreshes the lock folder's mtime.
pub const HEARTBEAT: Duration = Duration::from_secs(5);
/// A lock folder not refreshed for this long is stale.
pub const STALE_AFTER: Duration = Duration::from_secs(30);
/// Retry interval while another owner holds the lock.
pub const RETRY: Duration = Duration::from_millis(50);

/// Why [`DirLock::acquire`] gave up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockError {
    /// The caller's `give_up` said so (cancel / pause).
    Aborted,
    /// Another owner still holds it after the deadline (or on a single try).
    Busy,
    /// The folder could not be created.
    Io(String),
}

struct Held {
    count: usize,
    token: String,
    stop: Arc<(Mutex<bool>, Condvar)>,
    heartbeat: Option<JoinHandle<()>>,
}

struct State {
    held: Option<Held>,
    /// A thread of this process is creating / stealing the folder.
    acquiring: bool,
}

/// One owner of the lock of one packs folder (see the module docs).
pub struct DirLock {
    packs_dir: PathBuf,
    state: Mutex<State>,
    cv: Condvar,
}

/// A share of the hold; the folder is released when the last guard drops.
pub struct LockGuard {
    lock: Arc<DirLock>,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        self.lock.release();
    }
}

#[derive(serde::Deserialize)]
struct Owner {
    pid: Option<i64>,
    token: Option<String>,
}

fn random_token() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

fn read_owner(dir: &Path) -> Option<Owner> {
    let text = fs::read_to_string(dir.join(OWNER_FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

/// The pid is a live process (unix: `kill(pid, 0)` does not fail with `ESRCH`). Elsewhere
/// always true (the mtime rule only).
pub fn pid_alive(pid: i64) -> bool {
    #[cfg(unix)]
    {
        if pid <= 0 || pid > i32::MAX as i64 {
            return false;
        }
        // SAFETY: signal 0 only checks that the process exists.
        let r = unsafe { libc::kill(pid as libc::pid_t, 0) };
        r == 0 || io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        true
    }
}

fn touch(dir: &Path) -> io::Result<()> {
    let now = SystemTime::now();
    fs::File::open(dir)?.set_times(fs::FileTimes::new().set_modified(now).set_accessed(now))
}

enum Try {
    Acquired(String),
    Busy,
    /// A stale lock was removed: try again at once.
    Stolen,
}

impl DirLock {
    pub fn new(packs_dir: impl Into<PathBuf>) -> Arc<Self> {
        Arc::new(Self {
            packs_dir: packs_dir.into(),
            state: Mutex::new(State {
                held: None,
                acquiring: false,
            }),
            cv: Condvar::new(),
        })
    }

    pub fn path(&self) -> PathBuf {
        self.packs_dir.join(LOCK_DIR)
    }

    /// This owner holds the folder now.
    pub fn is_held(&self) -> bool {
        self.state.lock().held.is_some()
    }

    /// Takes a share of the lock, waiting while another owner holds it until `give_up()` is
    /// true ([`LockError::Aborted`]) or `deadline` passes ([`LockError::Busy`]; `Some(now)`
    /// = a single try).
    pub fn acquire(
        self: &Arc<Self>,
        give_up: &dyn Fn() -> bool,
        deadline: Option<Instant>,
    ) -> Result<LockGuard, LockError> {
        loop {
            {
                let mut st = self.state.lock();
                if let Some(h) = &mut st.held {
                    h.count += 1;
                    return Ok(LockGuard { lock: self.clone() });
                }
                if st.acquiring {
                    self.cv.wait_for(&mut st, RETRY);
                    drop(st);
                    if give_up() {
                        return Err(LockError::Aborted);
                    }
                    if deadline.is_some_and(|d| Instant::now() >= d) {
                        return Err(LockError::Busy);
                    }
                    continue;
                }
                st.acquiring = true;
            }
            if give_up() {
                self.end_acquiring(None);
                return Err(LockError::Aborted);
            }
            match self.try_once() {
                Ok(Try::Acquired(token)) => {
                    self.end_acquiring(Some(token));
                    return Ok(LockGuard { lock: self.clone() });
                }
                Ok(Try::Stolen) => {
                    self.end_acquiring(None);
                    continue;
                }
                Ok(Try::Busy) => {
                    self.end_acquiring(None);
                    if deadline.is_some_and(|d| Instant::now() >= d) {
                        return Err(LockError::Busy);
                    }
                    let mut st = self.state.lock();
                    if st.held.is_none() {
                        self.cv.wait_for(&mut st, RETRY);
                    }
                }
                Err(e) => {
                    self.end_acquiring(None);
                    return Err(LockError::Io(e.to_string()));
                }
            }
        }
    }

    fn end_acquiring(&self, token: Option<String>) {
        let mut st = self.state.lock();
        st.acquiring = false;
        if let Some(token) = token {
            let stop = Arc::new((Mutex::new(false), Condvar::new()));
            let s2 = stop.clone();
            let dir = self.path();
            let heartbeat = std::thread::Builder::new()
                .name("packs-lock-heartbeat".into())
                .spawn(move || {
                    let mut g = s2.0.lock();
                    while !*g {
                        if s2.1.wait_for(&mut g, HEARTBEAT).timed_out() && !*g {
                            let _ = touch(&dir);
                        }
                    }
                })
                .ok();
            st.held = Some(Held {
                count: 1,
                token,
                stop,
                heartbeat,
            });
        }
        self.cv.notify_all();
    }

    fn try_once(&self) -> io::Result<Try> {
        fs::create_dir_all(&self.packs_dir)?;
        let dir = self.path();
        match fs::create_dir(&dir) {
            Ok(()) => {
                let token = random_token();
                let owner = serde_json::json!({
                    "pid": std::process::id(),
                    "app": "rune",
                    "token": token,
                    "acquiredAt": crate::json::iso_now(),
                });
                fs::write(dir.join(OWNER_FILE), owner.to_string())?;
                Ok(Try::Acquired(token))
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(self.steal_if_stale()),
            Err(e) => Err(e),
        }
    }

    /// The stale holder's token (`Some(None)`: stale without a readable token), or None when
    /// the lock is live.
    fn judge_stale(&self, dir: &Path) -> Option<Option<String>> {
        let meta = fs::symlink_metadata(dir).ok()?;
        let owner = read_owner(dir);
        let token = owner.as_ref().and_then(|o| o.token.clone());
        if !meta.is_dir() {
            return Some(token);
        }
        let old = meta
            .modified()
            .ok()
            .and_then(|m| SystemTime::now().duration_since(m).ok())
            .is_some_and(|age| age > STALE_AFTER);
        if old {
            return Some(token);
        }
        let dead = owner
            .and_then(|o| o.pid)
            .is_some_and(|pid| pid != std::process::id() as i64 && !pid_alive(pid));
        dead.then_some(token)
    }

    fn steal_if_stale(&self) -> Try {
        let dir = self.path();
        let Some(judged) = self.judge_stale(&dir) else {
            return Try::Busy;
        };
        let aside = self.packs_dir.join(format!(
            "{STALE_PREFIX}{}",
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        ));
        if fs::rename(&dir, &aside).is_err() {
            // Someone else stole or released it: retry the create.
            return Try::Stolen;
        }
        let moved = read_owner(&aside).and_then(|o| o.token);
        if moved == judged {
            let _ = remove_any(&aside);
            eprintln!(
                "[packs] removed a stale lock of {}",
                self.packs_dir.display()
            );
            Try::Stolen
        } else {
            // A fresh lock was taken between the check and the rename: put it back (fails
            // without harm when a newer `.lock` exists already).
            let _ = fs::rename(&aside, &dir);
            Try::Busy
        }
    }

    fn release(&self) {
        let mut st = self.state.lock();
        let Some(h) = &mut st.held else { return };
        h.count -= 1;
        if h.count > 0 {
            return;
        }
        let h = st.held.take().expect("held");
        *h.stop.0.lock() = true;
        h.stop.1.notify_all();
        if let Some(t) = h.heartbeat {
            let _ = t.join();
        }
        let dir = self.path();
        if read_owner(&dir).and_then(|o| o.token).as_deref() == Some(h.token.as_str()) {
            let _ = fs::remove_file(dir.join(OWNER_FILE));
            let _ = fs::remove_dir(&dir);
        }
        self.cv.notify_all();
    }
}

impl Drop for DirLock {
    fn drop(&mut self) {
        // Guards hold an Arc, so nothing is held here; stop a leftover heartbeat anyway.
        if let Some(h) = self.state.get_mut().held.take() {
            *h.stop.0.lock() = true;
            h.stop.1.notify_all();
        }
    }
}

/// Removes a file, a symlink or a folder tree without following links.
pub(crate) fn remove_any(p: &Path) -> io::Result<()> {
    match fs::symlink_metadata(p) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
        Ok(m) if m.is_dir() => fs::remove_dir_all(p),
        Ok(_) => fs::remove_file(p),
    }
}
