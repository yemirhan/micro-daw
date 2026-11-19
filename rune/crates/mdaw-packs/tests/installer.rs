//! Port of `src/main/packs/installer.test.ts` (PackInstaller, downloads / previews / catalog,
//! "installer fault injection (L-f)") against a real local HTTP server instead of a fake fetch.

mod common;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, SystemTime};

use common::*;
use mdaw_model::packs::{CatalogSource, PackCatalog, PackInstallState};
use mdaw_packs::catalog::{FetchCatalogOptions, fetch_catalog};
use mdaw_packs::download::{
    DownloadError, DownloadOptions, RetryOptions, download_to_buffer, download_to_file_with_retry,
};
use mdaw_packs::fault::{FsFaults, FsOp};
use mdaw_packs::installer::{PARTIAL_MAX_AGE, resolve_inside};
use mdaw_packs::preview::{PreviewCacheOptions, PreviewFetcher};
use mdaw_packs::{InstallError, InstallerOptions, PackInstaller, Signal};
use parking_lot::Mutex;
use serde_json::{Value, json};

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
            root: TempDir::new("inst"),
            server: Server::new(),
            states: Arc::default(),
        }
    }
    fn root(&self) -> &Path {
        self.root.path()
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
    /// Waits (at most 10 s) until the kick is downloaded and verified (`fileIndex` 1) and the
    /// snare's request has arrived: no fixed sleep, so a loaded machine cannot pause too early.
    fn wait_kick_done_snare_requested(&self) {
        let end = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            let kick_done = self.states.lock().iter().any(|(_, s)| {
                matches!(s, PackInstallState::Downloading { file_index, .. } if *file_index >= 1)
            });
            if kick_done && self.server.count(&sd_url()) >= 1 {
                return;
            }
            assert!(
                std::time::Instant::now() < end,
                "timed out waiting for the kick"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    fn last_state(&self) -> PackInstallState {
        self.states.lock().last().unwrap().1.clone()
    }
    fn last_downloading(&self) -> PackInstallState {
        self.states
            .lock()
            .iter()
            .rev()
            .find(|s| matches!(s.1, PackInstallState::Downloading { .. }))
            .unwrap()
            .1
            .clone()
    }
    fn list(&self) -> Vec<String> {
        list_dir(self.root())
    }
    fn p(&self, rel: &str) -> PathBuf {
        self.root().join(rel)
    }
}

fn install(
    i: &Arc<PackInstaller>,
    m: &Value,
) -> Result<mdaw_model::packs::InstalledPack, InstallError> {
    i.install_json(m).wait()
}

fn err_msg(r: Result<mdaw_model::packs::InstalledPack, InstallError>) -> String {
    r.unwrap_err().message()
}

fn downloading(
    received: u64,
    file_index: u64,
    file_count: u64,
) -> impl Fn(&PackInstallState) -> bool {
    move |s| {
        matches!(s, PackInstallState::Downloading { received_bytes, file_index: fi, file_count: fc, .. }
            if *received_bytes == received && *fi == file_index && *fc == file_count)
    }
}

#[test]
fn installs_files_and_pack_json_atomically_and_lists_the_pack() {
    let c = Ctx::new();
    c.server.body(&bd_url(), "kick").body(&sd_url(), "snare");
    let inst = c.installer();
    let pack = install(&inst, &manifest()).unwrap();
    assert_eq!(pack.dir, c.p("test-pack").to_string_lossy());
    assert_eq!(read(c.p("test-pack/kicks/bd.wav")), "kick");
    assert_eq!(read(c.p("test-pack/sd.wav")), "snare");
    assert_eq!(c.list(), ["test-pack"]);
    let listed = inst.list_installed();
    assert_eq!(
        listed.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
        ["test-pack"]
    );
    assert_eq!(listed[0].dir, c.p("test-pack").to_string_lossy());
    assert_eq!(c.last_state(), PackInstallState::Installed);
    assert!(downloading(9, 2, 2)(&c.last_downloading()));
    // Requests carry the TS headers.
    let req = &c.server.requests()[0];
    let h = |k: &str| {
        req.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(k))
            .map(|(_, v)| v.clone())
    };
    assert_eq!(h("user-agent").as_deref(), Some("micro-daw-packs"));
    assert_eq!(h("accept").as_deref(), Some("*/*"));
    assert_eq!(h("host").as_deref(), Some("raw.githubusercontent.com"));
}

#[test]
fn follows_redirects_to_allowed_hosts_only() {
    let c = Ctx::new();
    let target = raw_test_url("moved/bd.wav");
    let t2 = target.clone();
    c.server
        .route(&bd_url(), move |_, s| {
            respond(s, 302, &[("Location", &t2)], b"")
        })
        .body(&target, "kick")
        .body(&sd_url(), "snare");
    let ok = c.installer();
    install(&ok, &manifest()).unwrap();
    assert_eq!(read(c.p("test-pack/kicks/bd.wav")), "kick");
    ok.uninstall("test-pack").unwrap();

    // Relative locations resolve against the current URL.
    c.server.clear();
    c.server
        .route(&bd_url(), |_, s| {
            respond(s, 307, &[("Location", "moved/bd.wav")], b"")
        })
        .body(&target, "kick")
        .body(&sd_url(), "snare");
    install(&c.installer(), &manifest()).unwrap();
    assert!(c.server.calls().contains(&target));
    c.installer().uninstall("test-pack").unwrap();

    c.server.clear();
    c.server
        .route(&bd_url(), |_, s| {
            respond(s, 301, &[("Location", "https://evil.example/bd")], b"")
        })
        .body(&sd_url(), "snare");
    let e = err_msg(install(&c.installer(), &manifest()));
    assert!(e.contains("Host not allowed: evil.example"), "{e}");
    assert!(!c.server.calls().iter().any(|u| u.contains("evil")));
    assert!(c.list().iter().all(|n| n == STAGE));
    assert!(matches!(c.last_state(), PackInstallState::Error { .. }));

    // github.com's object hosts are not allowed redirect targets (S2).
    c.server.clear();
    c.server
        .route(&bd_url(), |_, s| {
            respond(
                s,
                302,
                &[("Location", "https://objects.githubusercontent.com/blob/bd")],
                b"",
            )
        })
        .body(&sd_url(), "snare");
    assert!(err_msg(install(&c.installer(), &manifest())).contains("Host not allowed"));

    // More than 5 hops; a 3xx without location.
    c.server.clear();
    let loop_url = bd_url();
    c.server
        .route(&bd_url(), move |_, s| {
            respond(s, 302, &[("Location", &loop_url)], b"")
        })
        .body(&sd_url(), "snare");
    assert_eq!(
        err_msg(install(&c.installer(), &manifest())),
        "Too many redirects"
    );
    assert_eq!(
        c.server.count(&bd_url()),
        6,
        "the URL + 5 redirects, never retried"
    );
    c.server.clear();
    c.server
        .route(&bd_url(), |_, s| respond(s, 302, &[], b""))
        .body(&sd_url(), "snare");
    assert_eq!(
        err_msg(install(&c.installer(), &manifest())),
        "Redirect without location (302)"
    );
}

#[test]
fn rejects_oversized_files_size_and_checksum_mismatches_nothing_unverified_is_kept() {
    let c = Ctx::new();
    c.server
        .route(&bd_url(), |_, s| {
            respond_raw(
                s,
                200,
                &[("Content-Length", &(60 * 1024 * 1024).to_string())],
                b"x",
            )
        })
        .body(&sd_url(), "snare");
    let e = err_msg(install(&c.installer(), &manifest()));
    assert!(e.contains("too large"), "{e}");
    assert!(!c.list().contains(&"test-pack".to_owned()));

    c.server.clear();
    c.server.body(&bd_url(), "kick").body(&sd_url(), "snare");
    let mut m = manifest();
    m["files"][1]["sha256"] = json!(sha256_of(b"kick")); // wrong for "snare"
    assert!(err_msg(install(&c.installer(), &m)).contains("Checksum"));
    assert_eq!(c.server.count(&sd_url()), 3, "retried, then failed");
    assert!(!c.list().contains(&"test-pack".to_owned()));
    assert_eq!(read(c.p(&format!("{STAGE}/kicks/bd.wav"))), "kick");
    assert!(!c.p(&format!("{STAGE}/sd.wav")).exists());

    // S2: the exact byte count is checked without a content-length header too.
    c.server.clear();
    c.server
        .body(&bd_url(), "kick")
        .route(&sd_url(), |_, s| respond_raw(s, 200, &[], b"snare"));
    let mut short = manifest_for("kick", "snare", 1);
    short["files"][1]["size"] = json!(6);
    let e = err_msg(install(&c.installer(), &short));
    assert_eq!(e, "Size mismatch (5 bytes, expected 6)");
}

#[test]
fn s7_retries_transient_failures_with_backoff_but_not_4xx() {
    let c = Ctx::new();
    let hits = Arc::new(AtomicU32::new(0));
    let h = hits.clone();
    c.server
        .body(&bd_url(), "kick")
        .route(&sd_url(), move |_, s| {
            if h.fetch_add(1, Ordering::SeqCst) + 1 < 3 {
                respond(s, 503, &[], b"busy")
            } else {
                respond(s, 200, &[], b"snare")
            }
        });
    install(&c.installer(), &manifest()).unwrap();
    assert_eq!(hits.load(Ordering::SeqCst), 3);
    assert_eq!(read(c.p("test-pack/sd.wav")), "snare");

    c.server.clear();
    c.server.body(&bd_url(), "kick");
    let mut m2 = manifest();
    m2["version"] = json!(2);
    let e = err_msg(install(&c.installer(), &m2));
    assert_eq!(e, "HTTP 404 for raw.githubusercontent.com");
    assert_eq!(c.server.count(&sd_url()), 1);
}

#[test]
fn s7_retry_resumes_keeping_verified_files_of_the_failed_attempt() {
    let c = Ctx::new();
    c.server.body(&bd_url(), "kick").route(&sd_url(), |_, s| {
        // After the kick is in and fsynced (a failure aborts the files still in flight).
        std::thread::sleep(Duration::from_millis(150));
        respond(s, 404, &[], b"gone")
    });
    assert!(err_msg(install(&c.installer(), &manifest())).contains("404"));
    assert_eq!(c.list(), [STAGE]);

    c.server.clear();
    c.server.body(&bd_url(), "kick").body(&sd_url(), "snare");
    let pack = install(&c.installer(), &manifest()).unwrap();
    assert_eq!(
        c.server.calls(),
        [sd_url()],
        "kick was not downloaded again"
    );
    assert_eq!(read(Path::new(&pack.dir).join("kicks/bd.wav")), "kick");
    assert_eq!(c.list(), ["test-pack"]);
    assert!(downloading(9, 2, 2)(&c.last_downloading()));
}

#[test]
fn s7_a_stale_or_tampered_staging_file_is_re_downloaded_unknown_files_are_pruned() {
    let c = Ctx::new();
    std::fs::create_dir_all(c.p(&format!("{STAGE}/kicks"))).unwrap();
    std::fs::write(c.p(&format!("{STAGE}/kicks/bd.wav")), "kicK").unwrap();
    std::fs::write(c.p(&format!("{STAGE}/extra.wav")), "junk").unwrap();
    std::fs::write(c.p(&format!("{STAGE}/pack.json")), "{}").unwrap();
    c.server.body(&bd_url(), "kick").body(&sd_url(), "snare");
    let pack = install(&c.installer(), &manifest()).unwrap();
    let mut calls = c.server.calls();
    calls.sort();
    let mut want = vec![bd_url(), sd_url()];
    want.sort();
    assert_eq!(calls, want);
    assert_eq!(
        list_dir(Path::new(&pack.dir)),
        ["kicks", "pack.json", "sd.wav"]
    );
    assert_eq!(read(Path::new(&pack.dir).join("kicks/bd.wav")), "kick");
}

type Injected = (
    FsOp,
    Box<dyn Fn(&Path, Option<&Path>) -> bool + Send + Sync>,
    &'static str,
);

/// Slows down the close of the first `sd.wav` download (B2) and fails injected operations.
#[derive(Default)]
struct Faults {
    delay_sd_close: AtomicBool,
    delayed: AtomicBool,
    fail: Mutex<Option<Injected>>,
}

impl Faults {
    fn inject(
        &self,
        op: FsOp,
        when: impl Fn(&Path, Option<&Path>) -> bool + Send + Sync + 'static,
        code: &'static str,
    ) {
        *self.fail.lock() = Some((op, Box::new(when), code));
    }
    fn restore(&self) {
        *self.fail.lock() = None;
    }
}

impl FsFaults for Faults {
    fn before(&self, op: FsOp, a: &Path, b: Option<&Path>) -> std::io::Result<()> {
        if let Some((o, when, code)) = &*self.fail.lock()
            && *o == op
            && when(a, b)
        {
            return Err(std::io::Error::other(format!("injected {code}")));
        }
        Ok(())
    }
    fn before_close(&self, path: &Path) {
        if self.delay_sd_close.load(Ordering::SeqCst)
            && path.ends_with("sd.wav")
            && !self.delayed.swap(true, Ordering::SeqCst)
        {
            std::thread::sleep(Duration::from_millis(150));
        }
    }
}

#[test]
fn b2_an_immediate_retry_is_not_corrupted_by_the_failed_attempts_still_settling_workers() {
    let c = Ctx::new();
    let attempt = Arc::new(AtomicU32::new(1));
    let (a1, a2) = (attempt.clone(), attempt.clone());
    c.server
        .route(&bd_url(), move |_, s| {
            if a1.load(Ordering::SeqCst) == 1 {
                std::thread::sleep(Duration::from_millis(30));
                respond(s, 404, &[], b"gone");
            } else {
                respond(s, 200, &[], b"kick");
            }
        })
        .route(&sd_url(), move |_, s| {
            if a2.load(Ordering::SeqCst) == 1 {
                hang(s, b"", None);
            } else {
                respond_raw(s, 200, &[], b"");
                let _ = s.write_all(b"sn");
                let _ = s.flush();
                std::thread::sleep(Duration::from_millis(250));
                let _ = s.write_all(b"are");
            }
        });
    let faults = Arc::new(Faults::default());
    faults.delay_sd_close.store(true, Ordering::SeqCst);
    let mut o = c.opts();
    o.concurrency = 2;
    o.retry.attempts = 1;
    o.faults = faults.clone();
    let inst = PackInstaller::new(o);
    assert!(err_msg(install(&inst, &manifest())).contains("404"));
    attempt.store(2, Ordering::SeqCst);
    let pack = install(&inst, &manifest()).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(read(Path::new(&pack.dir).join("sd.wav")), "snare");
    assert_eq!(read(Path::new(&pack.dir).join("kicks/bd.wav")), "kick");
    assert!(
        faults.delayed.load(Ordering::SeqCst),
        "the slow close happened"
    );
}

#[test]
fn rejects_manifests_with_unsafe_paths_before_touching_the_disk() {
    let c = Ctx::new();
    c.server.body(&bd_url(), "x");
    let mut m = manifest();
    m["files"] =
        json!([{ "path": "../escape.wav", "url": bd_url(), "size": 1, "sha256": sha256_of(b"x") }]);
    m["kits"] = json!([]);
    let e = err_msg(install(&c.installer(), &m));
    assert!(e.contains("Unsafe"), "{e}");
    assert!(install(&c.installer(), &json!({ "id": "../x" })).is_err());
    assert!(c.server.calls().is_empty());
    assert!(c.list().is_empty());
    assert!(resolve_inside(c.root(), "a/../../x.wav").is_err());
    assert_eq!(
        resolve_inside(c.root(), "a/b.wav").unwrap(),
        c.root().join("a").join("b.wav")
    );
}

#[test]
fn cancels_a_running_install_and_removes_its_temp_folder() {
    let c = Ctx::new();
    c.server
        .route(&bd_url(), |_, s| hang(s, &[1, 2, 3], None))
        .route(&sd_url(), |_, s| hang(s, &[1, 2, 3], None));
    let inst = c.installer();
    let h = inst.install_json(&manifest());
    std::thread::sleep(Duration::from_millis(50));
    assert!(c.list().contains(&STAGE.to_owned()));
    assert!(inst.cancel("test-pack"));
    let t0 = std::time::Instant::now();
    assert_eq!(h.wait(), Err(InstallError::Cancelled));
    assert!(
        t0.elapsed() < Duration::from_secs(1),
        "a cancel interrupts stalled reads"
    );
    assert_eq!(InstallError::Cancelled.message(), "Install cancelled");
    assert!(c.list().is_empty());
    assert_eq!(c.last_state(), PackInstallState::Idle);
    assert!(!inst.cancel("test-pack"));
}

#[test]
fn concurrent_installs_of_one_pack_share_one_install() {
    let c = Ctx::new();
    c.server
        .route(&bd_url(), |_, s| {
            std::thread::sleep(Duration::from_millis(50));
            respond(s, 200, &[], b"kick")
        })
        .body(&sd_url(), "snare");
    let inst = c.installer();
    let a = inst.install_json(&manifest());
    let b = inst.install_json(&manifest());
    assert!(inst.busy() && inst.is_installing("test-pack"));
    assert_eq!(a.wait().unwrap(), b.wait().unwrap());
    assert_eq!(c.server.count(&bd_url()), 1);
    assert!(!inst.busy());
}

#[test]
fn re_install_replaces_the_old_version_a_failed_update_keeps_it() {
    let c = Ctx::new();
    c.server.body(&bd_url(), "kick").body(&sd_url(), "snare");
    install(&c.installer(), &manifest()).unwrap();
    c.server.clear();
    c.server.body(&bd_url(), "kick2").body(&sd_url(), "snare2");
    let v2 = c.installer();
    install(&v2, &manifest_for("kick2", "snare2", 2)).unwrap();
    assert_eq!(read(c.p("test-pack/sd.wav")), "snare2");
    assert_eq!(v2.list_installed()[0].version, 2);
    assert_eq!(c.list(), ["test-pack"], "the .old- backup is gone");

    c.server.clear();
    c.server.body(&bd_url(), "kick3");
    assert!(err_msg(install(&c.installer(), &manifest_for("kick3", "snare3", 3))).contains("404"));
    assert_eq!(read(c.p("test-pack/sd.wav")), "snare2");
    assert_eq!(c.list(), [STAGE, "test-pack"]);
}

#[test]
fn s5_restores_a_pack_whose_update_swap_was_interrupted() {
    let c = Ctx::new();
    c.server.body(&bd_url(), "kick").body(&sd_url(), "snare");
    install(&c.installer(), &manifest()).unwrap();
    std::fs::rename(c.p("test-pack"), c.p(".old-test-pack-0badf00d")).unwrap();
    std::fs::create_dir(c.p(".old-test-pack-12345678")).unwrap();
    std::fs::create_dir(c.p(".del-other-0000aaaa")).unwrap();
    std::fs::create_dir(c.p(".tmp-test-pack-abc123")).unwrap();
    let fresh = c.installer();
    fresh.cleanup_stale(SystemTime::now());
    assert_eq!(c.list(), ["test-pack"]);
    assert_eq!(fresh.list_installed().len(), 1);
    assert_eq!(read(c.p("test-pack/kicks/bd.wav")), "kick");
}

fn set_mtime(p: &Path, t: SystemTime) {
    let f = std::fs::File::open(p).unwrap();
    f.set_times(std::fs::FileTimes::new().set_modified(t).set_accessed(t))
        .unwrap();
}

#[test]
fn s5_an_old_folder_is_dropped_when_the_pack_is_present_stale_staging_folders_expire() {
    let c = Ctx::new();
    c.server.body(&bd_url(), "kick").body(&sd_url(), "snare");
    let inst = c.installer();
    install(&inst, &manifest()).unwrap();
    std::fs::create_dir(c.p(".old-test-pack-0badf00d")).unwrap();
    std::fs::copy(
        c.p("test-pack/pack.json"),
        c.p(".old-test-pack-0badf00d/pack.json"),
    )
    .unwrap();
    std::fs::create_dir(c.p(".partial-old-pack")).unwrap();
    std::fs::create_dir(c.p(".partial-new-pack")).unwrap();
    set_mtime(
        &c.p(".partial-old-pack"),
        SystemTime::now() - PARTIAL_MAX_AGE - Duration::from_secs(60),
    );
    inst.cleanup_stale(SystemTime::now());
    assert_eq!(c.list(), [".partial-new-pack", "test-pack"]);
}

#[test]
fn uninstalls_cleans_stale_temp_folders_and_ignores_invalid_installs() {
    let c = Ctx::new();
    c.server.body(&bd_url(), "kick").body(&sd_url(), "snare");
    let inst = c.installer();
    install(&inst, &manifest()).unwrap();
    std::fs::create_dir(c.p(".tmp-test-pack-abc123")).unwrap();
    std::fs::create_dir(c.p(".old-x-1")).unwrap();
    std::fs::create_dir(c.p(STAGE)).unwrap();
    std::fs::create_dir(c.p("junk")).unwrap();
    std::fs::write(c.p("junk/pack.json"), r#"{"id":"junk"}"#).unwrap();
    inst.cleanup_stale(SystemTime::now());
    assert_eq!(c.list(), [STAGE, "junk", "test-pack"]);
    assert_eq!(inst.list_installed().len(), 1);
    inst.uninstall("test-pack").unwrap();
    assert_eq!(c.list(), ["junk"]);
    assert_eq!(inst.uninstall("../etc"), Err("Invalid pack id".to_owned()));
}

// --- downloads, previews and catalog ------------------------------------------------------

#[test]
fn caps_in_memory_downloads_while_streaming() {
    let s = Server::new();
    s.route(&bd_url(), |_, st| respond_raw(st, 200, &[], &[0u8; 12]));
    let conn = s.connector();
    let o = DownloadOptions::new(10, &*conn);
    let e = download_to_buffer(&bd_url(), &o).unwrap_err();
    assert_eq!(e.message(), "File too large (over 10 bytes)");
    // Chunked bodies are capped and decoded too.
    s.route(&sd_url(), |_, st| {
        respond_raw(
            st,
            200,
            &[("Transfer-Encoding", "chunked")],
            b"3\r\nabc\r\n2;x=1\r\nde\r\n0\r\n\r\n",
        )
    });
    assert_eq!(download_to_buffer(&sd_url(), &o).unwrap(), b"abcde");
}

#[test]
fn times_out_a_stalled_response() {
    let s = Server::new();
    s.route(&bd_url(), |_, st| hang(st, &[1, 2, 3], None));
    let conn = s.connector();
    let mut o = DownloadOptions::new(100, &*conn);
    o.idle_timeout = Some(Duration::from_millis(30));
    let e = download_to_buffer(&bd_url(), &o).unwrap_err();
    assert_eq!(e, DownloadError::Timeout);
    assert!(e.message().contains("timed out"));
    // A server that never answers the request also times out (the idle timer covers the head).
    s.route(&sd_url(), |_, st| wait_close(st, Duration::from_secs(30)));
    assert_eq!(
        download_to_buffer(&sd_url(), &o).unwrap_err(),
        DownloadError::Timeout
    );
}

#[test]
fn s6_a_total_timeout_is_a_timeout_a_real_abort_is_a_cancel() {
    let s = Server::new();
    s.route(&bd_url(), |_, st| hang(st, &[1, 2, 3], None));
    let conn = s.connector();
    let sig = Signal::with_timeout(Duration::from_millis(30));
    let mut o = DownloadOptions::new(100, &*conn);
    o.signal = Some(&sig);
    let e = download_to_buffer(&bd_url(), &o).unwrap_err();
    assert_eq!(e, DownloadError::Timeout);
    assert!(!e.message().to_lowercase().contains("cancel"));

    let sig = Signal::new();
    let s2 = sig.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(10));
        s2.abort(mdaw_packs::AbortReason::Cancel);
    });
    let mut o = DownloadOptions::new(100, &*conn);
    o.signal = Some(&sig);
    let e = download_to_buffer(&bd_url(), &o).unwrap_err();
    assert_eq!(e.message(), "Download cancelled");
}

#[test]
fn s6_a_stalled_body_with_a_declared_size_times_out_not_truncated() {
    let s = Server::new();
    s.route(&bd_url(), |_, st| hang(st, &[1, 2], Some(4)));
    let conn = s.connector();
    let mut o = DownloadOptions::new(100, &*conn);
    o.idle_timeout = Some(Duration::from_millis(30));
    assert_eq!(
        download_to_buffer(&bd_url(), &o).unwrap_err(),
        DownloadError::Timeout
    );
    // A body cut off before its declared length is a retryable network error (`terminated`).
    s.route(&sd_url(), |_, st| {
        respond_raw(st, 200, &[("Content-Length", "4")], &[1, 2])
    });
    let e = download_to_buffer(&sd_url(), &o).unwrap_err();
    assert_eq!(e.message(), "terminated");
    assert!(e.retryable());
}

#[test]
fn s7_download_to_file_with_retry_gives_up_after_n_attempts_and_stops_on_cancel() {
    let s = Server::new();
    s.route(&bd_url(), |_, st| respond(st, 500, &[], b"x"));
    let root = TempDir::new("retry");
    let dest = root.path().join("a.wav");
    let conn = s.connector();
    let o = DownloadOptions::new(10, &*conn);
    let retries = Mutex::new(Vec::new());
    let r = download_to_file_with_retry(
        &bd_url(),
        &dest,
        &o,
        RetryOptions {
            attempts: 3,
            base_delay: Duration::from_millis(1),
        },
        Some(&|n, e: &DownloadError| retries.lock().push((n, e.message()))),
    );
    assert_eq!(
        r.unwrap_err().message(),
        "HTTP 500 for raw.githubusercontent.com"
    );
    assert_eq!(s.count(&bd_url()), 3);
    assert_eq!(
        retries.lock().iter().map(|r| r.0).collect::<Vec<_>>(),
        [2, 3]
    );
    assert!(!dest.exists());
    // A cancel during the backoff ends at once.
    let sig = Signal::new();
    let s2 = sig.clone();
    let mut o = DownloadOptions::new(10, &*conn);
    o.signal = Some(&sig);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        s2.abort(mdaw_packs::AbortReason::Cancel);
    });
    let t0 = std::time::Instant::now();
    let r = download_to_file_with_retry(
        &bd_url(),
        &dest,
        &o,
        RetryOptions {
            attempts: 3,
            base_delay: Duration::from_secs(10),
        },
        None,
    );
    assert_eq!(r.unwrap_err(), DownloadError::Aborted);
    assert!(t0.elapsed() < Duration::from_secs(2));
}

#[test]
fn previews_allowlist_size_and_sha256_check_cache_and_lru_eviction() {
    let s = Server::new();
    s.body(&bd_url(), "kick").body(&sd_url(), "snare");
    let pf = PreviewFetcher::new(
        s.connector(),
        PreviewCacheOptions {
            max_entries: 1,
            ..Default::default()
        },
    );
    assert!(
        pf.fetch("https://evil.example/x.wav", None, None)
            .unwrap_err()
            .message()
            .contains("not allowed")
    );
    let a = pf
        .fetch(&bd_url(), Some(4), Some(&sha256_of(b"kick")))
        .unwrap();
    assert_eq!(a, b"kick");
    pf.fetch(&bd_url(), None, None).unwrap();
    assert_eq!(s.calls(), [bd_url()]);
    pf.fetch(&sd_url(), None, None).unwrap();
    assert_eq!(pf.keys(), [sd_url()]);
    let bad = PreviewFetcher::new(s.connector(), Default::default());
    let e = bad
        .fetch(&bd_url(), Some(4), Some(&sha256_of(b"nope")))
        .unwrap_err();
    assert_eq!(e.message(), "Checksum mismatch");
    // In-flight requests for one URL share one download.
    s.route(&raw_test_url("slow.wav"), |_, st| {
        std::thread::sleep(Duration::from_millis(80));
        respond(st, 200, &[], b"slow")
    });
    let pf = Arc::new(PreviewFetcher::new(s.connector(), Default::default()));
    let threads: Vec<_> = (0..3)
        .map(|_| {
            let pf = pf.clone();
            std::thread::spawn(move || pf.fetch(&raw_test_url("slow.wav"), None, None).unwrap())
        })
        .collect();
    for t in threads {
        assert_eq!(t.join().unwrap(), b"slow");
    }
    assert_eq!(s.count(&raw_test_url("slow.wav")), 1);
}

#[test]
fn falls_back_to_the_bundled_catalog_when_the_remote_one_fails_is_invalid_or_not_newer() {
    let s = Server::new();
    let conn = s.connector();
    let offline = fetch_catalog(FetchCatalogOptions::new(&*conn));
    assert_eq!(offline.source, CatalogSource::Bundled);
    assert_eq!(offline.catalog.version, 1);
    assert!(
        !offline.catalog.packs.is_empty(),
        "the bundled catalog is compiled in"
    );

    let url = "https://raw.githubusercontent.com/x/y/main/catalog.json";
    s.body(url, r#"{"version":9}"#);
    let mut o = FetchCatalogOptions::new(&*conn);
    o.url = url;
    assert_eq!(fetch_catalog(o).source, CatalogSource::Bundled);

    let bundled: PackCatalog = serde_json::from_value(json!({
        "version": 1, "updatedAt": "2026-11-17",
        "packs": [mdaw_model::packs::validate_manifest(&manifest()).unwrap()]
    }))
    .unwrap();
    let with = |remote: Value| {
        s.body(url, &remote.to_string());
        let mut o = FetchCatalogOptions::new(&*conn);
        o.url = url;
        o.bundled = bundled.clone();
        fetch_catalog(o)
    };
    let mut m5 = manifest();
    m5["version"] = json!(5);
    assert_eq!(
        with(json!({ "version": 1, "updatedAt": "2026-11-17", "packs": [m5] })).source,
        CatalogSource::Bundled
    );
    let mut m2 = manifest();
    m2["version"] = json!(2);
    let remote = with(json!({ "version": 1, "updatedAt": "2026-12-01", "packs": [m2] }));
    assert_eq!(remote.source, CatalogSource::Remote);
    assert_eq!(
        remote
            .catalog
            .packs
            .iter()
            .map(|p| format!("{}@{}", p.id, p.version))
            .collect::<Vec<_>>(),
        ["test-pack@2"]
    );
    // Too big for the 5 MiB cap: bundled.
    s.route(url, |_, st| {
        respond_raw(st, 200, &[("Content-Length", "6000000")], b"")
    });
    let mut o = FetchCatalogOptions::new(&*conn);
    o.url = url;
    assert_eq!(fetch_catalog(o).source, CatalogSource::Bundled);
    // A remote catalog that stalls: bundled after the timeout.
    s.route(url, |_, st| hang(st, b"{", None));
    let mut o = FetchCatalogOptions::new(&*conn);
    o.url = url;
    o.timeout = Duration::from_millis(50);
    let t0 = std::time::Instant::now();
    assert_eq!(fetch_catalog(o).source, CatalogSource::Bundled);
    assert!(t0.elapsed() < Duration::from_secs(2));
}

// --- installer fault injection (L-f) ------------------------------------------------------

fn faulty(c: &Ctx) -> (Arc<PackInstaller>, Arc<Faults>) {
    let f = Arc::new(Faults::default());
    let mut o = c.opts();
    o.faults = f.clone();
    (PackInstaller::new(o), f)
}

#[test]
fn m1_a_failed_crash_recovery_keeps_every_backup_for_the_next_start() {
    let c = Ctx::new();
    c.server.body(&bd_url(), "kick").body(&sd_url(), "snare");
    install(&c.installer(), &manifest()).unwrap();
    std::fs::rename(c.p("test-pack"), c.p(".old-test-pack-0123abcd")).unwrap();
    let (inst, f) = faulty(&c);
    f.inject(
        FsOp::Rename,
        |a, _| a.to_string_lossy().contains(".old-test-pack-"),
        "EIO",
    );
    inst.cleanup_stale(SystemTime::now());
    assert_eq!(c.list(), [".old-test-pack-0123abcd"]);
    f.restore();
    let fresh = c.installer();
    fresh.cleanup_stale(SystemTime::now());
    assert_eq!(c.list(), ["test-pack"]);
    assert_eq!(fresh.list_installed().len(), 1);
}

#[test]
fn m1_a_present_but_invalid_pack_folder_is_replaced_by_the_valid_backup() {
    let c = Ctx::new();
    c.server.body(&bd_url(), "kick").body(&sd_url(), "snare");
    let inst = c.installer();
    install(&inst, &manifest()).unwrap();
    std::fs::rename(c.p("test-pack"), c.p(".old-test-pack-0123abcd")).unwrap();
    std::fs::create_dir(c.p("test-pack")).unwrap();
    inst.cleanup_stale(SystemTime::now());
    assert_eq!(c.list(), ["test-pack"]);
    assert_eq!(read(c.p("test-pack/sd.wav")), "snare");
}

#[test]
fn the_newest_backup_wins_when_several_are_valid() {
    let c = Ctx::new();
    c.server.body(&bd_url(), "kick").body(&sd_url(), "snare");
    install(&c.installer(), &manifest()).unwrap();
    std::fs::rename(c.p("test-pack"), c.p(".old-test-pack-00000001")).unwrap();
    c.server.clear();
    c.server.body(&bd_url(), "kick2").body(&sd_url(), "snare2");
    std::thread::sleep(Duration::from_millis(5));
    install(&c.installer(), &manifest_for("kick2", "snare2", 2)).unwrap();
    std::fs::rename(c.p("test-pack"), c.p(".old-test-pack-00000002")).unwrap();
    let inst = c.installer();
    inst.cleanup_stale(SystemTime::now());
    assert_eq!(c.list(), ["test-pack"]);
    assert_eq!(inst.list_installed()[0].version, 2);
}

#[test]
fn a_failed_swap_keeps_the_old_version_and_the_resumable_staging_folder() {
    let c = Ctx::new();
    c.server.body(&bd_url(), "kick").body(&sd_url(), "snare");
    install(&c.installer(), &manifest()).unwrap();
    c.server.clear();
    c.server.body(&bd_url(), "kick2").body(&sd_url(), "snare2");
    let (v2, f) = faulty(&c);
    f.inject(
        FsOp::Rename,
        |a, b| a.ends_with(STAGE) && b.is_some_and(|b| b.ends_with("test-pack")),
        "EIO",
    );
    assert!(err_msg(install(&v2, &manifest_for("kick2", "snare2", 2))).contains("injected"));
    assert_eq!(c.list(), [STAGE, "test-pack"]);
    assert_eq!(v2.list_installed()[0].version, 1);
    assert!(matches!(
        v2.get_state("test-pack"),
        PackInstallState::Error { .. }
    ));
    f.restore();
    c.server.clear();
    c.server.body(&bd_url(), "kick2").body(&sd_url(), "snare2");
    install(&c.installer(), &manifest_for("kick2", "snare2", 2)).unwrap();
    assert!(
        c.server.calls().is_empty(),
        "both files resumed from staging"
    );
    assert_eq!(read(c.p("test-pack/sd.wav")), "snare2");
}

#[test]
fn a_disk_error_while_opening_a_file_fails_the_install_keeps_verified_files_retry_resumes() {
    let c = Ctx::new();
    c.server.body(&bd_url(), "kick").body(&sd_url(), "snare");
    let f = Arc::new(Faults::default());
    f.inject(FsOp::Open, |a, _| a.ends_with("sd.wav"), "ENOSPC");
    let mut o = c.opts();
    o.faults = f.clone();
    o.concurrency = 1;
    let one = PackInstaller::new(o);
    assert!(err_msg(install(&one, &manifest())).contains("ENOSPC"));
    assert_eq!(c.server.count(&sd_url()), 0, "an open error is not retried");
    assert_eq!(c.list(), [STAGE]);
    f.restore();
    c.server.clear();
    c.server.body(&bd_url(), "kick").body(&sd_url(), "snare");
    install(&c.installer(), &manifest()).unwrap();
    assert_eq!(c.server.calls(), [sd_url()]);
}

#[test]
fn a_failed_pack_json_write_leaves_no_pack_json_in_staging_and_nothing_installed() {
    let c = Ctx::new();
    c.server.body(&bd_url(), "kick").body(&sd_url(), "snare");
    let (inst, f) = faulty(&c);
    f.inject(
        FsOp::WriteFile,
        |a, _| a.ends_with(format!("{STAGE}/pack.json")),
        "EIO",
    );
    assert!(err_msg(install(&inst, &manifest())).contains("injected"));
    assert_eq!(c.list(), [STAGE]);
    assert!(inst.list_installed().is_empty());
    assert!(!list_dir(&c.p(STAGE)).contains(&"pack.json".to_owned()));
}

#[test]
fn l_e_pause_all_stops_installs_keeps_the_staging_folder_and_awaits_settlement() {
    let c = Ctx::new();
    c.server
        .body(&bd_url(), "kick")
        .route(&sd_url(), |_, s| hang(s, &[1, 2, 3], None));
    let inst = c.installer();
    let h = inst.install_json(&manifest());
    c.wait_kick_done_snare_requested();
    assert!(inst.busy());
    inst.pause_all();
    assert!(!inst.busy());
    assert_eq!(h.wait(), Err(InstallError::Paused));
    assert_eq!(c.list(), [STAGE]);
    assert_eq!(read(c.p(&format!("{STAGE}/kicks/bd.wav"))), "kick");
    assert!(
        !list_dir(&c.p(STAGE)).contains(&"sd.wav".to_owned()),
        "partial file removed"
    );
    assert_eq!(c.last_state(), PackInstallState::Idle);
    let next = c.installer();
    assert_eq!(next.paused_ids(), ["test-pack"]);
    c.server.clear();
    c.server.body(&bd_url(), "kick").body(&sd_url(), "snare");
    install(&c.installer(), &manifest()).unwrap();
    assert_eq!(c.server.calls(), [sd_url()]);
    assert!(next.paused_ids().is_empty());
}

#[test]
fn n7_paused_ids_ignores_empty_staging_folders_invalid_ids_and_running_installs() {
    let c = Ctx::new();
    std::fs::create_dir_all(c.p(".partial-empty-pack/kicks")).unwrap();
    std::fs::create_dir_all(c.p(".partial-Bad_ID")).unwrap();
    std::fs::write(c.p(".partial-Bad_ID/x.wav"), "x").unwrap();
    std::fs::create_dir_all(c.p(".partial-nested-pack/a/b")).unwrap();
    std::fs::write(c.p(".partial-nested-pack/a/b/c.wav"), "c").unwrap();
    c.server
        .body(&bd_url(), "kick")
        .route(&sd_url(), |_, s| hang(s, &[1, 2, 3], None));
    let inst = c.installer();
    let h = inst.install_json(&manifest());
    c.wait_kick_done_snare_requested();
    assert_eq!(inst.paused_ids(), ["nested-pack"]);
    inst.pause_all();
    let _ = h.wait();
    assert_eq!(inst.paused_ids(), ["nested-pack", "test-pack"]);
}

#[test]
fn cancel_all_discards_staging() {
    let c = Ctx::new();
    c.server
        .route(&bd_url(), |_, s| hang(s, &[1], None))
        .route(&sd_url(), |_, s| hang(s, &[1], None));
    let inst = c.installer();
    let h = inst.install_json(&manifest());
    std::thread::sleep(Duration::from_millis(50));
    inst.cancel_all();
    assert_eq!(h.wait(), Err(InstallError::Cancelled));
    assert!(c.list().is_empty());
}

#[test]
fn uninstall_during_an_install_cancels_it_first() {
    let c = Ctx::new();
    c.server.body(&bd_url(), "kick").body(&sd_url(), "snare");
    install(&c.installer(), &manifest()).unwrap();
    c.server.clear();
    c.server
        .route(&bd_url(), |_, s| hang(s, &[1], None))
        .route(&sd_url(), |_, s| hang(s, &[1], None));
    let inst = c.installer();
    let h = inst.install_json(&manifest_for("k", "s", 2));
    std::thread::sleep(Duration::from_millis(50));
    inst.uninstall("test-pack").unwrap();
    assert_eq!(h.wait(), Err(InstallError::Cancelled));
    assert!(c.list().is_empty());
    assert_eq!(c.last_state(), PackInstallState::Idle);
}

#[test]
fn a_manifest_over_the_pack_cap_is_rejected_before_any_download() {
    // `Pack too large` during the download needs more bytes than declared, which the per-file
    // cap (min(50 MiB, size)) already prevents; the declared total is checked by the validator.
    let c = Ctx::new();
    let big = 50 * 1024 * 1024;
    let files: Vec<Value> = (0..11)
        .map(|i| json!({ "path": format!("big{i}.wav"), "url": raw_test_url(&format!("big{i}.wav")), "size": big, "sha256": "0".repeat(64) }))
        .collect();
    let mut m = manifest();
    m["files"] = json!(files);
    m["kits"] = json!([]);
    let e = err_msg(install(&c.installer(), &m));
    assert!(e.starts_with("Pack test-pack:"), "{e}");
    assert!(c.server.calls().is_empty());
}
