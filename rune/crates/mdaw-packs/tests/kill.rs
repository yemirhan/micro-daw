//! Crash recovery with a real process kill: the install runs in a child process (this test
//! binary re-run with `MDAW_PACKS_CHILD=...`), which is SIGKILLed mid-download or between the
//! two renames of an update swap; the parent then starts the store like the next app launch.

mod common;

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use common::*;
use mdaw_packs::fault::{FsFaults, FsOp};
use mdaw_packs::{InstallerOptions, PackInstaller};

const STAGE: &str = ".partial-test-pack";

/// Blocks forever before the staging → pack rename (the moment between the two renames).
struct HangBeforeSwap;

impl FsFaults for HangBeforeSwap {
    fn before(&self, op: FsOp, a: &Path, b: Option<&Path>) -> std::io::Result<()> {
        if op == FsOp::Rename && a.ends_with(STAGE) && b.is_some_and(|b| b.ends_with("test-pack")) {
            println!("SWAP-HALFWAY");
            std::thread::sleep(Duration::from_secs(600));
        }
        Ok(())
    }
}

/// The child: installs the manifest given in the environment, then waits to be killed.
#[test]
fn child_install_helper() {
    let Ok(mode) = std::env::var("MDAW_PACKS_CHILD") else {
        return;
    };
    let dir = std::env::var("MDAW_PACKS_DIR").unwrap();
    let addr = std::env::var("MDAW_PACKS_ADDR").unwrap().parse().unwrap();
    let manifest: serde_json::Value =
        serde_json::from_str(&std::env::var("MDAW_PACKS_MANIFEST").unwrap()).unwrap();
    let mut o = InstallerOptions::new(dir, Arc::new(mdaw_packs::http::FixedAddrConnector { addr }));
    o.concurrency = 1;
    o.progress_interval = Duration::ZERO;
    if mode == "swap" {
        o.faults = Arc::new(HangBeforeSwap);
    }
    o.on_state = Some(Arc::new(|_, s| {
        println!("STATE {}", serde_json::to_string(s).unwrap())
    }));
    let inst = PackInstaller::new(o);
    let h = inst.install_json(&manifest);
    println!("STARTED");
    let r = h.wait();
    println!("DONE {r:?}");
    std::thread::sleep(Duration::from_secs(600));
}

fn spawn_child(
    mode: &str,
    root: &Path,
    server: &Server,
    manifest: &serde_json::Value,
) -> (std::process::Child, std::sync::mpsc::Receiver<String>) {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "child_install_helper",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("MDAW_PACKS_CHILD", mode)
        .env("MDAW_PACKS_DIR", root)
        .env("MDAW_PACKS_ADDR", server.addr.to_string())
        .env("MDAW_PACKS_MANIFEST", manifest.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let out = child.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for l in BufReader::new(out).lines().map_while(Result::ok) {
            let _ = tx.send(l);
        }
    });
    (child, rx)
}

fn wait_line(rx: &std::sync::mpsc::Receiver<String>, pred: impl Fn(&str) -> bool) {
    let end = Instant::now() + Duration::from_secs(20);
    while Instant::now() < end {
        if let Ok(l) = rx.recv_timeout(Duration::from_millis(100))
            && pred(&l)
        {
            return;
        }
    }
    panic!("child never printed the expected line");
}

fn installer(root: &Path, server: &Server) -> Arc<PackInstaller> {
    PackInstaller::new(InstallerOptions::new(
        root.to_path_buf(),
        server.connector(),
    ))
}

#[test]
fn killed_mid_download_the_next_start_reports_it_paused_and_resumes_the_missing_files() {
    let root = TempDir::new("kill-dl");
    let server = Server::new();
    server
        .body(&bd_url(), "kick")
        .route(&sd_url(), |_, s| hang(s, b"sn", None));
    let (mut child, rx) = spawn_child("download", root.path(), &server, &manifest());
    // bd.wav done (file 1 of 2 counted), sd.wav streaming.
    wait_line(&rx, |l| {
        l.contains("\"fileIndex\":1") && l.contains("\"receivedBytes\":6")
    });
    child.kill().unwrap();
    child.wait().unwrap();
    assert_eq!(
        list_dir(root.path()),
        [".lock", STAGE],
        "the killed child held the lock"
    );

    // Next launch: the dead owner's lock does not block it.
    let inst = installer(root.path(), &server);
    inst.cleanup_stale(SystemTime::now());
    assert_eq!(list_dir(root.path()), [STAGE]);
    assert!(inst.list_installed().is_empty());
    assert_eq!(inst.paused_ids(), ["test-pack"]);
    server.clear();
    server.body(&bd_url(), "kick").body(&sd_url(), "snare");
    let pack = inst.install_json(&manifest()).wait().unwrap();
    assert_eq!(server.calls(), [sd_url()], "the verified kick was kept");
    assert_eq!(read(Path::new(&pack.dir).join("sd.wav")), "snare");
    assert_eq!(list_dir(root.path()), ["test-pack"]);
}

#[test]
fn killed_between_the_swap_renames_the_next_start_restores_the_old_version() {
    let root = TempDir::new("kill-swap");
    let server = Server::new();
    server.body(&bd_url(), "kick").body(&sd_url(), "snare");
    installer(root.path(), &server)
        .install_json(&manifest())
        .wait()
        .unwrap();
    server.clear();
    server.body(&bd_url(), "kick2").body(&sd_url(), "snare2");
    let v2 = manifest_for("kick2", "snare2", 2);
    let (mut child, rx) = spawn_child("swap", root.path(), &server, &v2);
    wait_line(&rx, |l| l == "SWAP-HALFWAY");
    let pid = child.id();
    child.kill().unwrap();
    child.wait().unwrap();
    let names = list_dir(root.path());
    // The dead child still "holds" the packs folder lock.
    assert_eq!(names.len(), 3, "{names:?}");
    assert!(
        names[0] == ".lock" && names[1].starts_with(".old-test-pack-") && names[2] == STAGE,
        "{names:?}"
    );
    let owner = read(root.path().join(".lock/owner.json"));
    assert!(owner.contains(&format!("\"pid\":{pid}")), "{owner}");

    // Next launch: the dead owner's lock is stolen at once, v1 is back, the staged v2 resumes
    // without downloading.
    let inst = installer(root.path(), &server);
    let t0 = Instant::now();
    inst.cleanup_stale(SystemTime::now());
    assert!(t0.elapsed() < Duration::from_secs(1), "{:?}", t0.elapsed());
    assert_eq!(list_dir(root.path()), [STAGE, "test-pack"]);
    let listed = inst.list_installed();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].version, 1);
    assert_eq!(read(root.path().join("test-pack/sd.wav")), "snare");
    server.clear();
    inst.install_json(&v2).wait().unwrap();
    assert!(server.calls().is_empty());
    assert_eq!(read(root.path().join("test-pack/sd.wav")), "snare2");
    assert_eq!(list_dir(root.path()), ["test-pack"]);
}
