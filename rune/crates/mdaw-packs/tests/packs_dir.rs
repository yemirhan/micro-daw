//! The packs folder contract (`docs/rune-port/packs-dir-protocol.md`, phase-6 review findings
//! 2, 3, 4, 13, 14): the cross-process lock, durability order and recovery, symlinks, per-pack
//! serialization and install-thread panics.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant, SystemTime};

use common::*;
use mdaw_model::packs::PackInstallState;
use mdaw_packs::download::RetryOptions;
use mdaw_packs::fault::{FsFaults, FsOp};
use mdaw_packs::installer::{INTERNAL_ERROR_MESSAGE, PACKS_BUSY_MESSAGE};
use mdaw_packs::lock::{DirLock, STALE_AFTER};
use mdaw_packs::{InstallError, InstallerOptions, PackInstaller};
use parking_lot::{Condvar, Mutex};
use serde_json::Value;

const STAGE: &str = ".partial-test-pack";

type States = Arc<Mutex<Vec<(String, PackInstallState)>>>;

struct Ctx {
    root: TempDir,
    server: Server,
    states: States,
}

impl Ctx {
    fn new() -> Self {
        Self {
            root: TempDir::new("pdir"),
            server: Server::new(),
            states: Arc::default(),
        }
    }
    fn root(&self) -> &Path {
        self.root.path()
    }
    fn p(&self, rel: &str) -> PathBuf {
        self.root().join(rel)
    }
    fn list(&self) -> Vec<String> {
        list_dir(self.root())
    }
    fn opts(&self) -> InstallerOptions {
        let mut o = InstallerOptions::new(self.root().to_path_buf(), self.server.connector());
        let st = self.states.clone();
        o.on_state = Some(Arc::new(move |id: &str, s: &PackInstallState| {
            st.lock().push((id.to_owned(), s.clone()))
        }));
        o.progress_interval = Duration::ZERO;
        o.retry = RetryOptions {
            attempts: 3,
            base_delay: Duration::from_millis(1),
        };
        o
    }
    fn installer(&self) -> Arc<PackInstaller> {
        PackInstaller::new(self.opts())
    }
    fn with_faults(&self, f: Arc<dyn FsFaults>) -> Arc<PackInstaller> {
        let mut o = self.opts();
        o.faults = f;
        PackInstaller::new(o)
    }
    fn serve(&self, kick: &str, snare: &str) {
        self.server.clear();
        self.server.body(&bd_url(), kick).body(&sd_url(), snare);
    }
}

fn install(
    i: &Arc<PackInstaller>,
    m: &Value,
) -> Result<mdaw_model::packs::InstalledPack, InstallError> {
    i.install_json(m).wait()
}

/// Blocks a route until opened; `arrived` counts requests that reached it.
#[derive(Default, Clone)]
struct Gate(Arc<(Mutex<(bool, u32)>, Condvar)>);

impl Gate {
    fn open(&self) {
        self.0.0.lock().0 = true;
        self.0.1.notify_all();
    }
    fn pass(&self) {
        let mut g = self.0.0.lock();
        g.1 += 1;
        self.0.1.notify_all();
        while !g.0 {
            self.0.1.wait(&mut g);
        }
    }
    fn wait_arrived(&self, n: u32) {
        let end = Instant::now() + Duration::from_secs(10);
        let mut g = self.0.0.lock();
        while g.1 < n {
            assert!(Instant::now() < end, "request never arrived");
            self.0.1.wait_for(&mut g, Duration::from_millis(20));
        }
    }
}

fn write_foreign_lock(root: &Path, pid: i64, token: &str, age: Duration) {
    let dir = root.join(".lock");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("owner.json"),
        format!(r#"{{"pid":{pid},"app":"electron","token":"{token}","acquiredAt":"2026-11-19T00:00:00.000Z"}}"#),
    )
    .unwrap();
    let t = SystemTime::now() - age;
    std::fs::File::open(&dir)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(t).set_accessed(t))
        .unwrap();
}

/// A pid that is certainly dead (a reaped child).
fn dead_pid() -> i64 {
    let mut c = std::process::Command::new("true").spawn().unwrap();
    let pid = c.id() as i64;
    c.wait().unwrap();
    pid
}

// --- Finding 4: cross-process lock --------------------------------------------------------

/// The review probe: two installers sharing one packs folder install the same pack (v1, v2) at
/// once. Before the lock they shared `.partial-test-pack` and published each other's files
/// (wrong bytes, reported success). Now the second waits for the first, then installs v2.
#[test]
fn two_installers_on_one_packs_folder_take_turns() {
    let c = Ctx::new();
    let gate = Gate::default();
    let n_bd = Arc::new(AtomicU32::new(0));
    let n_sd = Arc::new(AtomicU32::new(0));
    let (g2, nb, ns) = (gate.clone(), n_bd.clone(), n_sd.clone());
    c.server
        .route(&bd_url(), move |_, s| {
            if nb.fetch_add(1, Ordering::SeqCst) == 0 {
                g2.pass();
                respond(s, 200, &[], b"kick");
            } else {
                respond(s, 200, &[], b"kick2");
            }
        })
        .route(&sd_url(), move |_, s| {
            if ns.fetch_add(1, Ordering::SeqCst) == 0 {
                respond(s, 200, &[], b"snare");
            } else {
                respond(s, 200, &[], b"snare2");
            }
        });
    let a = c.installer();
    let b = c.installer(); // another owner (≙ the other app)
    let ha = a.install_json(&manifest());
    gate.wait_arrived(1);
    let hb = b.install_json(&manifest_for("kick2", "snare2", 2));
    std::thread::sleep(Duration::from_millis(300));
    assert!(!hb.is_settled(), "the second installer waits");
    assert_eq!(n_bd.load(Ordering::SeqCst), 1, "and requests nothing");
    assert!(c.list().contains(&".lock".to_owned()));
    gate.open();
    let pa = ha.wait().unwrap();
    assert_eq!(pa.version, 1);
    let pb = hb.wait().unwrap();
    assert_eq!(pb.version, 2);
    assert_eq!(read(c.p("test-pack/kicks/bd.wav")), "kick2");
    assert_eq!(read(c.p("test-pack/sd.wav")), "snare2");
    assert_eq!(c.list(), ["test-pack"], "lock released, no leftovers");
}

#[test]
fn a_stale_lock_of_a_dead_process_is_stolen_at_once() {
    let c = Ctx::new();
    c.serve("kick", "snare");
    write_foreign_lock(c.root(), dead_pid(), &"d".repeat(32), Duration::ZERO);
    let t0 = Instant::now();
    install(&c.installer(), &manifest()).unwrap();
    assert!(t0.elapsed() < Duration::from_secs(2), "{:?}", t0.elapsed());
    assert_eq!(c.list(), ["test-pack"]);
}

#[test]
fn a_lock_not_refreshed_for_30_s_is_stolen() {
    let c = Ctx::new();
    c.serve("kick", "snare");
    // pid 1 is alive (EPERM): only the mtime makes it stale.
    write_foreign_lock(
        c.root(),
        1,
        &"e".repeat(32),
        STALE_AFTER + Duration::from_secs(5),
    );
    install(&c.installer(), &manifest()).unwrap();
    assert_eq!(c.list(), ["test-pack"]);
}

#[test]
fn a_live_foreign_lock_blocks_installs_uninstalls_and_cleanup_and_is_never_removed() {
    let c = Ctx::new();
    c.serve("kick", "snare");
    install(&c.installer(), &manifest()).unwrap();
    let token = "f".repeat(32);
    write_foreign_lock(c.root(), 1, &token, Duration::ZERO);
    let owner_before = read(c.p(".lock/owner.json"));
    std::fs::create_dir(c.p(".tmp-leftover")).unwrap();

    let mut o = c.opts();
    o.uninstall_lock_wait = Duration::from_millis(300);
    let inst = PackInstaller::new(o);
    let t0 = Instant::now();
    assert_eq!(inst.uninstall("test-pack"), Err(PACKS_BUSY_MESSAGE.into()));
    assert!(t0.elapsed() >= Duration::from_millis(300));
    assert!(c.p("test-pack/pack.json").exists(), "nothing uninstalled");

    inst.cleanup_stale(SystemTime::now());
    assert!(c.p(".tmp-leftover").exists(), "cleanup skipped");

    c.server.clear();
    let h = inst.install_json(&manifest_for("kick2", "snare2", 2));
    std::thread::sleep(Duration::from_millis(200));
    assert!(!h.is_settled(), "the install waits for the lock");
    assert!(c.server.calls().is_empty());
    assert!(inst.cancel("test-pack"));
    assert_eq!(h.wait(), Err(InstallError::Cancelled));
    assert!(!c.p(STAGE).exists(), "nothing was touched");
    assert_eq!(
        c.states.lock().last().unwrap().1,
        PackInstallState::Installed
    );
    assert_eq!(read(c.p(".lock/owner.json")), owner_before);

    // The other owner releases: everything works again.
    std::fs::remove_dir_all(c.p(".lock")).unwrap();
    inst.cleanup_stale(SystemTime::now());
    assert!(!c.p(".tmp-leftover").exists());
    inst.uninstall("test-pack").unwrap();
    assert!(c.list().is_empty());
}

#[test]
fn a_lock_stolen_from_us_is_not_removed_on_release_and_shares_are_refcounted() {
    let root = TempDir::new("lockrel");
    let l = DirLock::new(root.path());
    let a = l.acquire(&|| false, None).unwrap();
    let b = l.acquire(&|| false, None).unwrap();
    let owner = read(root.path().join(".lock/owner.json"));
    assert!(owner.contains("\"app\":\"rune\""), "{owner}");
    assert!(owner.contains(&format!("\"pid\":{}", std::process::id())));
    drop(a);
    assert!(root.path().join(".lock").exists(), "still shared");
    // Someone judged us stale and took it over.
    std::fs::write(
        root.path().join(".lock/owner.json"),
        r#"{"pid":1,"token":"0000"}"#,
    )
    .unwrap();
    drop(b);
    assert!(
        root.path().join(".lock/owner.json").exists(),
        "not ours: left alone"
    );
    // A second owner in this process cannot take it while it is live.
    let other = DirLock::new(root.path());
    assert!(
        other
            .acquire(&|| false, Some(Instant::now() + Duration::from_millis(100)))
            .is_err()
    );
    std::fs::remove_dir_all(root.path().join(".lock")).unwrap();
    let g = other.acquire(&|| false, None).unwrap();
    drop(g);
    assert!(list_dir(root.path()).is_empty());
}

// --- Finding 3: durability ----------------------------------------------------------------

type Event = (FsOp, PathBuf, Option<PathBuf>);

#[derive(Default)]
struct Recorder {
    events: Mutex<Vec<Event>>,
    fail_sync: Mutex<Option<&'static str>>,
}

impl FsFaults for Recorder {
    fn before(&self, op: FsOp, a: &Path, b: Option<&Path>) -> std::io::Result<()> {
        self.events
            .lock()
            .push((op, a.to_path_buf(), b.map(Path::to_path_buf)));
        if op == FsOp::Sync
            && let Some(f) = *self.fail_sync.lock()
            && a.ends_with(f)
        {
            return Err(std::io::Error::other(
                "injected ENOSPC: no space left on device",
            ));
        }
        Ok(())
    }
}

#[test]
fn durability_files_then_pack_json_then_folders_then_renames_then_packs_dir_then_backup_removal() {
    let c = Ctx::new();
    c.serve("kick", "snare");
    install(&c.installer(), &manifest()).unwrap();
    c.serve("kick2", "snare2");
    let rec = Arc::new(Recorder::default());
    install(
        &c.with_faults(rec.clone()),
        &manifest_for("kick2", "snare2", 2),
    )
    .unwrap();
    let ev = rec.events.lock().clone();
    let at = |pred: &dyn Fn(&Event) -> bool| {
        ev.iter()
            .position(pred)
            .unwrap_or_else(|| panic!("missing event in {ev:#?}"))
    };
    let stage = c.p(STAGE);
    let sync = |p: PathBuf| move |e: &Event| e.0 == FsOp::Sync && e.1 == p;
    let bd = at(&sync(stage.join("kicks/bd.wav")));
    let sd = at(&sync(stage.join("sd.wav")));
    let meta = at(&sync(stage.join("pack.json")));
    let kicks = at(&sync(stage.join("kicks")));
    let st = at(&sync(stage.clone()));
    let to_old = at(&|e| e.0 == FsOp::Rename && e.1 == c.p("test-pack"));
    let swap = at(&|e| e.0 == FsOp::Rename && e.1 == stage);
    let root = at(&sync(c.root().to_path_buf()));
    let remove = at(&|e| e.0 == FsOp::Remove);
    assert!(bd < meta && sd < meta, "files before pack.json");
    assert!(meta < kicks && kicks < st, "then folders, deepest first");
    assert!(
        st < to_old && to_old < swap && swap < root,
        "renames, then <packs>"
    );
    assert!(root < remove, "the backup goes last");
    assert!(
        ev[remove]
            .1
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with(".old-test-pack-")
    );
    assert_eq!(c.list(), ["test-pack"]);
}

#[test]
fn a_failing_fsync_fails_the_install_and_is_not_retried() {
    let c = Ctx::new();
    c.serve("kick", "snare");
    let rec = Arc::new(Recorder::default());
    *rec.fail_sync.lock() = Some("sd.wav");
    let e = install(&c.with_faults(rec.clone()), &manifest()).unwrap_err();
    assert!(e.message().contains("ENOSPC"), "{e:?}");
    assert_eq!(c.server.count(&sd_url()), 1, "not retried");
    assert!(!c.p("test-pack").exists());
    // pack.json failing to sync fails it too.
    *rec.fail_sync.lock() = Some("pack.json");
    c.serve("kick", "snare");
    let e = install(&c.with_faults(rec.clone()), &manifest()).unwrap_err();
    assert!(e.message().contains("ENOSPC"), "{e:?}");
    assert!(!c.p("test-pack").exists());
}

#[test]
fn recovery_restores_the_backup_when_the_new_version_has_a_short_file() {
    let c = Ctx::new();
    c.serve("kick", "snare");
    install(&c.installer(), &manifest()).unwrap();
    std::fs::rename(c.p("test-pack"), c.p(".old-test-pack-0badf00d")).unwrap();
    c.serve("kick2", "snare2");
    install(&c.installer(), &manifest_for("kick2", "snare2", 2)).unwrap();
    // What a power loss without fsync could leave: valid pack.json, a short file.
    std::fs::write(c.p("test-pack/sd.wav"), "sn").unwrap();
    let inst = c.installer();
    inst.cleanup_stale(SystemTime::now());
    assert_eq!(c.list(), ["test-pack"]);
    assert_eq!(inst.list_installed()[0].version, 1);
    assert_eq!(read(c.p("test-pack/sd.wav")), "snare");
}

#[test]
fn an_incomplete_backup_is_never_restored_and_never_deleted() {
    let c = Ctx::new();
    c.serve("kick", "snare");
    install(&c.installer(), &manifest()).unwrap();
    std::fs::rename(c.p("test-pack"), c.p(".old-test-pack-0badf00d")).unwrap();
    std::fs::remove_file(c.p(".old-test-pack-0badf00d/sd.wav")).unwrap();
    c.installer().cleanup_stale(SystemTime::now());
    assert_eq!(c.list(), [".old-test-pack-0badf00d"]);
}

// --- Finding 2: symlinks ------------------------------------------------------------------

#[cfg(unix)]
fn outside() -> (TempDir, PathBuf) {
    let t = TempDir::new("outside");
    let f = t.path().join("unrelated.txt");
    std::fs::write(&f, "keep me").unwrap();
    (t, f)
}

#[cfg(unix)]
fn untouched(t: &TempDir) {
    assert_eq!(list_dir(t.path()), ["unrelated.txt"]);
    assert_eq!(read(t.path().join("unrelated.txt")), "keep me");
}

/// The review probe: `.partial-test-pack` symlinked to an outside folder. Before, the install
/// pruned (deleted) the unrelated file there, wrote the pack into it and renamed the link into
/// place.
#[cfg(unix)]
#[test]
fn a_symlinked_staging_folder_is_never_followed() {
    use std::os::unix::fs::symlink;
    let c = Ctx::new();
    let (out, _) = outside();
    // install
    symlink(out.path(), c.p(STAGE)).unwrap();
    c.serve("kick", "snare");
    install(&c.installer(), &manifest()).unwrap();
    untouched(&out);
    let m = std::fs::symlink_metadata(c.p("test-pack")).unwrap();
    assert!(m.is_dir() && !m.file_type().is_symlink());
    assert_eq!(read(c.p("test-pack/sd.wav")), "snare");
    // resume after a failure
    symlink(out.path(), c.p(STAGE)).unwrap();
    c.server.clear();
    c.server.body(&bd_url(), "kick2");
    assert!(install(&c.installer(), &manifest_for("kick2", "snare2", 2)).is_err());
    untouched(&out);
    assert!(
        !std::fs::symlink_metadata(c.p(STAGE))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    c.serve("kick2", "snare2");
    install(&c.installer(), &manifest_for("kick2", "snare2", 2)).unwrap();
    untouched(&out);
    // uninstall
    symlink(out.path(), c.p(STAGE)).unwrap();
    c.installer().uninstall("test-pack").unwrap();
    untouched(&out);
    assert!(c.list().is_empty());
    // startup cleanup (an old one: would be removed recursively)
    symlink(out.path(), c.p(STAGE)).unwrap();
    symlink(out.path(), c.p(".tmp-x")).unwrap();
    symlink(out.path(), c.p(".del-test-pack-00000000")).unwrap();
    let inst = c.installer();
    assert!(
        inst.paused_ids().is_empty(),
        "a link is not a paused install"
    );
    inst.cleanup_stale(SystemTime::now() + Duration::from_secs(30 * 24 * 3600));
    untouched(&out);
    assert!(c.list().is_empty());
}

#[cfg(unix)]
#[test]
fn symlinked_pack_folders_and_backups_are_not_installs() {
    use std::os::unix::fs::symlink;
    let c = Ctx::new();
    c.serve("kick", "snare");
    install(&c.installer(), &manifest()).unwrap();
    // A valid pack living outside.
    let out = TempDir::new("outside-pack");
    std::fs::rename(c.p("test-pack"), out.path().join("test-pack")).unwrap();
    let ext = out.path().join("test-pack");
    symlink(&ext, c.p(".old-test-pack-0badf00d")).unwrap();
    let inst = c.installer();
    inst.cleanup_stale(SystemTime::now());
    assert!(
        c.list().is_empty(),
        "a linked backup is not restored: {:?}",
        c.list()
    );
    assert!(
        ext.join("pack.json").exists(),
        "and its target is left alone"
    );

    symlink(&ext, c.p("test-pack")).unwrap();
    assert!(inst.list_installed().is_empty());
    assert!(!inst.is_installed("test-pack"));
    inst.uninstall("test-pack").unwrap();
    assert!(
        ext.join("sd.wav").exists(),
        "uninstall unlinks, never follows"
    );
    assert!(c.list().is_empty());
}

// --- Finding 13: per-pack serialization ---------------------------------------------------

/// Delays the uninstall's move-aside rename.
#[derive(Default)]
struct SlowUninstall {
    entered: AtomicBool,
}

impl FsFaults for SlowUninstall {
    fn before(&self, op: FsOp, a: &Path, b: Option<&Path>) -> std::io::Result<()> {
        let _ = a;
        if op == FsOp::Rename
            && b.is_some_and(|b| {
                b.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".del-")
            })
        {
            self.entered.store(true, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(300));
        }
        Ok(())
    }
}

#[test]
fn an_install_of_a_pack_being_uninstalled_waits_for_the_uninstall() {
    let c = Ctx::new();
    c.serve("kick", "snare");
    install(&c.installer(), &manifest()).unwrap();
    let slow = Arc::new(SlowUninstall::default());
    let inst = c.with_faults(slow.clone());
    let i2 = inst.clone();
    let un = std::thread::spawn(move || i2.uninstall("test-pack"));
    while !slow.entered.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(5));
    }
    c.serve("kick2", "snare2");
    let h = inst.install_json(&manifest_for("kick2", "snare2", 2));
    std::thread::sleep(Duration::from_millis(100));
    assert!(c.server.calls().is_empty(), "waits for the uninstall");
    assert!(!c.p(STAGE).exists());
    un.join().unwrap().unwrap();
    let p = h.wait().unwrap();
    assert_eq!(p.version, 2);
    assert_eq!(read(c.p("test-pack/sd.wav")), "snare2");
    assert_eq!(c.list(), ["test-pack"]);

    // A cancel while waiting ends it without touching anything.
    let i3 = inst.clone();
    slow.entered.store(false, Ordering::SeqCst);
    let un = std::thread::spawn(move || i3.uninstall("test-pack"));
    while !slow.entered.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(5));
    }
    c.server.clear();
    let h = inst.install_json(&manifest());
    std::thread::sleep(Duration::from_millis(50));
    assert!(inst.cancel("test-pack"));
    assert_eq!(h.wait(), Err(InstallError::Cancelled));
    un.join().unwrap().unwrap();
    assert!(c.server.calls().is_empty());
    assert!(c.list().is_empty());
}

// --- Finding 14: install-thread panic -----------------------------------------------------

struct PanicOnPackJson;

impl FsFaults for PanicOnPackJson {
    fn before(&self, op: FsOp, _a: &Path, _b: Option<&Path>) -> std::io::Result<()> {
        if op == FsOp::WriteFile {
            panic!("injected panic");
        }
        Ok(())
    }
}

#[test]
fn a_panicking_install_thread_settles_its_handle_and_releases_everything() {
    let c = Ctx::new();
    c.serve("kick", "snare");
    let inst = c.with_faults(Arc::new(PanicOnPackJson));
    let h = inst.install_json(&manifest());
    assert_eq!(
        h.wait_timeout(Duration::from_secs(10)),
        Some(Err(InstallError::Failed(INTERNAL_ERROR_MESSAGE.into())))
    );
    assert!(!inst.is_installing("test-pack"));
    assert_eq!(
        c.states.lock().last().unwrap().1,
        PackInstallState::Error {
            message: INTERNAL_ERROR_MESSAGE.into()
        }
    );
    assert!(!c.p(".lock").exists(), "lock released");
    // The pack's slot is free again: a normal install works.
    c.serve("kick", "snare");
    install(&c.installer(), &manifest()).unwrap();
}
