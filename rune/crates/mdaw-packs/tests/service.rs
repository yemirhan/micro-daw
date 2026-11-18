//! Port of the `packService` part of `src/services/PackService.test.ts` (and `quit.test.ts`),
//! with the real store, a local HTTP server and the real `ArrangementEngine` as the host
//! instead of a mocked Electron API and a mocked engine.

mod common;

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::*;
use mdaw_dsp::buffer::SharedBuffer;
use mdaw_engine::buffers::PadBufferCache;
use mdaw_engine::core::Core;
use mdaw_engine::engine::{ArrangementEngine, NoSamples};
use mdaw_model::doc::KitSelectionSource;
use mdaw_model::packs::{PackCatalog, PackInstallState, PackRefStatus, kit_from_installed};
use mdaw_model::types::{InstrumentType, KitState, PadSampleSource};
use mdaw_packs::fault::{FsFaults, FsOp};
use mdaw_packs::service::CatalogStatus;
use mdaw_packs::{PackHost, PackService, PackStoreOptions};
use parking_lot::{Condvar, Mutex};
use serde_json::{Value, json};

const SR: u32 = 48_000;
const CATALOG_URL: &str = "https://raw.githubusercontent.com/t/t/main/catalog.json";

/// A 16-bit mono WAV of `frames` frames at `value`.
fn wav(frames: usize, value: i16) -> Vec<u8> {
    let data = frames * 2;
    let mut b = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&((36 + data) as u32).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&SR.to_le_bytes());
    b.extend_from_slice(&(SR * 2).to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&(data as u32).to_le_bytes());
    for _ in 0..frames {
        b.extend_from_slice(&value.to_le_bytes());
    }
    b
}

/// The engine as the service's host; recording / session can be forced like the TS spies.
struct Host {
    e: ArrangementEngine,
    core: Core,
    recording: Option<bool>,
    session: Option<u64>,
    invalidated: Vec<String>,
    previews: usize,
}

impl Host {
    fn new(packs_dir: &Path) -> Self {
        let cache = PadBufferCache::with_fs(SR, Some(packs_dir.to_string_lossy().into_owned()));
        let (e, core, _live) = ArrangementEngine::new(SR, cache, Arc::new(NoSamples));
        Self {
            e,
            core,
            recording: None,
            session: None,
            invalidated: vec![],
            previews: 0,
        }
    }
    fn run(&mut self, seconds: f64) {
        for _ in 0..((seconds * SR as f64) / 128.0).ceil() as usize {
            let (mut l, mut r) = ([0f32; 128], [0f32; 128]);
            self.core.process(&mut l, &mut r);
            self.e.tick();
        }
    }
    /// The active drum track's kit id ("" without a drum track).
    fn kit_id(&self) -> String {
        match self.e.doc().get_active_drum_track_id() {
            Some(t) => self.e.doc().get_kit(&t).id,
            None => String::new(),
        }
    }
}

impl PackHost for Host {
    fn session_id(&self) -> u64 {
        self.session
            .unwrap_or_else(|| PackHost::session_id(&self.e))
    }
    fn active_drum_track_id(&self) -> Option<String> {
        PackHost::active_drum_track_id(&self.e)
    }
    fn kit_id(&self, track_id: &str) -> String {
        PackHost::kit_id(&self.e, track_id)
    }
    fn is_recording_now(&self) -> bool {
        self.recording
            .unwrap_or_else(|| PackHost::is_recording_now(&self.e))
    }
    fn apply_kit(&mut self, kit: &KitState, track_id: Option<&str>) -> Option<String> {
        PackHost::apply_kit(&mut self.e, kit, track_id)
    }
    fn invalidate_pack_buffers(&mut self, pack_id: &str) {
        self.invalidated.push(pack_id.to_owned());
        PackHost::invalidate_pack_buffers(&mut self.e, pack_id);
    }
    fn buffer_cache(&self) -> PadBufferCache {
        PackHost::buffer_cache(&self.e)
    }
    fn play_preview(&mut self, buffer: SharedBuffer) {
        self.previews += 1;
        PackHost::play_preview(&mut self.e, buffer);
    }
    fn stop_preview(&mut self) {
        PackHost::stop_preview(&mut self.e);
    }
    fn is_preview_playing(&self) -> bool {
        PackHost::is_preview_playing(&self.e)
    }
}

/// Blocks a route until opened (`deferredInstalls`); counts the requests that reached it.
#[derive(Default, Clone)]
struct Gate(Arc<(Mutex<(bool, u32)>, Condvar)>);

impl Gate {
    fn open(&self) {
        self.0.0.lock().0 = true;
        self.0.1.notify_all();
    }
    fn wait(&self) {
        let mut g = self.0.0.lock();
        g.1 += 1;
        self.0.1.notify_all();
        while !g.0 {
            self.0.1.wait(&mut g);
        }
    }
    /// Blocks until `n` requests reached the gate (a request-specific barrier, no timing).
    fn wait_arrived(&self, n: u32) {
        let end = Instant::now() + Duration::from_secs(10);
        let mut g = self.0.0.lock();
        while g.1 < n {
            assert!(Instant::now() < end, "the request never reached the gate");
            self.0.1.wait_for(&mut g, Duration::from_millis(20));
        }
    }
}

fn pack_manifest(id: &str, name: &str, kick: &[u8], snare: &[u8]) -> Value {
    let mut m = manifest();
    m["id"] = json!(id);
    m["name"] = json!(name);
    let bd = raw_test_url(&format!("{id}/bd.wav"));
    let sd = raw_test_url(&format!("{id}/sd.wav"));
    m["files"][0]["url"] = json!(bd);
    m["files"][0]["size"] = json!(kick.len());
    m["files"][0]["sha256"] = json!(sha256_of(kick));
    m["files"][1]["url"] = json!(sd);
    m["files"][1]["size"] = json!(snare.len());
    m["files"][1]["sha256"] = json!(sha256_of(snare));
    m
}

struct T {
    root: TempDir,
    server: Server,
    svc: PackService,
    host: Host,
    kick: Vec<u8>,
    snare: Vec<u8>,
    catalog: String,
}

impl T {
    fn new() -> Self {
        Self::with(|_| {})
    }
    fn with(tweak: impl FnOnce(&mut PackStoreOptions)) -> Self {
        Self::with_server(|o, _| tweak(o))
    }
    fn with_server(tweak: impl FnOnce(&mut PackStoreOptions, &Server)) -> Self {
        let root = TempDir::new("svc");
        let server = Server::new();
        let kick = wav(4800, 8000);
        let snare = wav(2400, -8000);
        let catalog = json!({
            "version": 1, "updatedAt": "2026-11-18",
            "packs": [pack_manifest("test-pack", "Test Pack", &kick, &snare), pack_manifest("other-pack", "Other", &kick, &snare)]
        });
        server.body(CATALOG_URL, &catalog.to_string());
        for id in ["test-pack", "other-pack"] {
            let (k, s) = (kick.clone(), snare.clone());
            server.route(&raw_test_url(&format!("{id}/bd.wav")), move |_, st| {
                respond(st, 200, &[], &k)
            });
            server.route(&raw_test_url(&format!("{id}/sd.wav")), move |_, st| {
                respond(st, 200, &[], &s)
            });
        }
        let mut o = PackStoreOptions::with_connector(root.path().to_path_buf(), server.connector());
        o.catalog_url = CATALOG_URL.into();
        o.bundled = PackCatalog {
            version: 1,
            updated_at: "1970-01-01T00:00:00.000Z".into(),
            packs: vec![],
        };
        o.progress_interval = Duration::ZERO;
        o.retry.base_delay = Duration::from_millis(1);
        tweak(&mut o, &server);
        let host = Host::new(root.path());
        let svc = PackService::new(o, None);
        Self {
            root,
            server,
            svc,
            host,
            kick,
            snare,
            catalog: catalog.to_string(),
        }
    }
    fn settle(&mut self) {
        assert!(
            self.svc.settle(&mut self.host, Duration::from_secs(10)),
            "workers did not settle"
        );
    }
    fn until(&mut self, what: &str, cond: impl Fn(&PackService, &Host) -> bool) {
        let end = Instant::now() + Duration::from_secs(10);
        while Instant::now() < end {
            self.svc
                .poll_wait(&mut self.host, Duration::from_millis(10));
            if cond(&self.svc, &self.host) {
                return;
            }
        }
        panic!("timed out waiting for {what}");
    }
    fn init(&mut self) {
        self.svc.init();
        self.svc.load_catalog(false);
        self.settle();
    }
    /// Installs `id` through the service and waits.
    fn installed(&mut self, id: &str) {
        self.svc.install(id);
        self.settle();
        assert!(self.svc.snapshot().store.is_installed(id));
    }
    /// Gates `id`'s snare file.
    fn gate_snare(&self, id: &str) -> Gate {
        let g = Gate::default();
        let (g2, sn) = (g.clone(), self.snare.clone());
        self.server
            .route(&raw_test_url(&format!("{id}/sd.wav")), move |_, st| {
                g2.wait();
                respond(st, 200, &[], &sn)
            });
        g
    }
    /// Gates the catalog.
    fn gate_catalog(&self) -> Gate {
        let g = Gate::default();
        let (g2, body) = (g.clone(), self.catalog.clone());
        self.server.route(CATALOG_URL, move |_, st| {
            g2.wait();
            respond(st, 200, &[], body.as_bytes())
        });
        g
    }
    fn gate(&self, id: &str) -> Gate {
        let g = Gate::default();
        let (g2, k) = (g.clone(), self.kick.clone());
        self.server
            .route(&raw_test_url(&format!("{id}/bd.wav")), move |_, st| {
                g2.wait();
                respond(st, 200, &[], &k)
            });
        g
    }
    /// The app's engine → service wiring: kit-selection events and the target check.
    fn sync(&mut self) {
        for e in self.host.e.take_kit_selected() {
            self.svc.on_kit_selected(&e);
        }
        self.svc.check_target(&mut self.host);
    }
    fn pending(&self) -> Option<String> {
        self.svc.snapshot().pending_load.clone()
    }
}

#[test]
fn loads_dir_installed_packs_and_the_catalog() {
    let mut t = T::new();
    t.init();
    t.installed("test-pack");
    let s = t.svc.snapshot();
    assert!(s.available);
    assert_eq!(
        s.packs_dir.as_deref(),
        Some(&*t.root.path().to_string_lossy())
    );
    assert_eq!(s.catalog_status, CatalogStatus::Ready);
    assert_eq!(s.store.catalog.as_ref().unwrap().packs.len(), 2);
    assert_eq!(
        t.svc.install_state("test-pack"),
        PackInstallState::Installed
    );
    // A fresh service over the same folder (next app start) lists it at init.
    let mut o = PackStoreOptions::with_connector(t.root.path().to_path_buf(), t.server.connector());
    o.catalog_url = CATALOG_URL.into();
    let mut svc2 = PackService::new(o, None);
    svc2.init();
    assert!(svc2.settle(&mut t.host, Duration::from_secs(5)));
    assert!(svc2.snapshot().store.is_installed("test-pack"));
}

#[test]
fn tracks_install_progress_and_completion() {
    let mut t = T::new();
    t.init();
    let g = t.gate("test-pack");
    let v0 = t.svc.version();
    t.svc.install("test-pack");
    assert!(matches!(
        t.svc.install_state("test-pack"),
        PackInstallState::Downloading { .. }
    ));
    t.until("progress", |s, _| {
        matches!(
            s.install_state("test-pack"),
            PackInstallState::Downloading { file_count: 2, .. }
        )
    });
    g.open();
    t.settle();
    assert_eq!(
        t.svc.install_state("test-pack"),
        PackInstallState::Installed
    );
    assert!(t.svc.version() > v0);
}

#[test]
fn reports_install_errors_and_treats_cancel_as_idle() {
    let mut t = T::new();
    t.init();
    t.server.route(&raw_test_url("test-pack/sd.wav"), |_, st| {
        respond(st, 200, &[], b"corrupt")
    });
    t.svc.install("test-pack");
    t.settle();
    assert_eq!(
        t.svc.install_state("test-pack"),
        PackInstallState::Error {
            message: "Size mismatch (7 bytes, expected 4844)".into()
        }
    );
    t.svc.dismiss_error("test-pack");
    assert_eq!(t.svc.install_state("test-pack"), PackInstallState::Idle);

    // The failed attempt kept the verified kick in staging, so the retry only downloads the
    // snare: gate that request and wait until it is in flight (it was flaky when it gated the
    // kick, which the resume never requests, and polled `is_installing` against the clock).
    let g = t.gate_snare("test-pack");
    t.svc.install("test-pack");
    g.wait_arrived(1);
    assert!(t.svc.store().installer().is_installing("test-pack"));
    t.svc.cancel("test-pack");
    g.open();
    t.settle();
    assert_eq!(t.svc.install_state("test-pack"), PackInstallState::Idle);
    assert!(
        t.svc.store().installer().paused_ids().is_empty(),
        "a cancel discards staging"
    );
}

#[test]
fn unknown_and_invalid_ids_are_errors() {
    let mut t = T::new();
    t.init();
    t.svc.install("no-such-pack");
    t.settle();
    assert_eq!(
        t.svc.install_state("no-such-pack"),
        PackInstallState::Error {
            message: "Unknown pack: no-such-pack".into()
        }
    );
    assert_eq!(t.svc.store().install("../x"), Err("Invalid pack id".into()));
    assert_eq!(
        t.svc.store().uninstall("../x"),
        Err("Invalid pack id".into())
    );
    assert_eq!(t.svc.store().cancel("../x"), Err("Invalid pack id".into()));
}

#[test]
fn previews_remote_files_through_the_store_and_local_files_through_the_cache() {
    let mut t = T::new();
    t.init();
    let url = raw_test_url("test-pack/bd.wav");
    t.svc.preview_remote(&url, &url, &mut t.host);
    assert_eq!(t.svc.snapshot().previewing.as_deref(), Some(url.as_str()));
    t.settle();
    assert_eq!(t.host.previews, 1);
    assert!(t.host.e.is_preview_playing());
    assert_eq!(t.svc.snapshot().previewing.as_deref(), Some(url.as_str()));
    // Cached decode: no second fetch.
    t.svc.preview_remote(&url, &url, &mut t.host);
    assert_eq!(t.host.previews, 2);
    assert_eq!(t.server.count(&url), 1);
    t.svc.stop_preview(&mut t.host);
    assert_eq!(t.svc.snapshot().previewing, None);
    // Only exact catalog URLs.
    let other = raw_test_url("not-in-catalog.wav");
    t.svc.preview_remote(&other, "k", &mut t.host);
    t.settle();
    assert_eq!(t.svc.snapshot().previewing, None);
    assert_eq!(
        t.svc.snapshot().preview_error,
        Some(("k".into(), "Preview URL not allowed".into()))
    );
    assert_eq!(t.server.count(&other), 0);
    // Installed files: by path and by pack id + path.
    t.installed("test-pack");
    let path = t
        .root
        .path()
        .join("test-pack/sd.wav")
        .to_string_lossy()
        .into_owned();
    t.svc.preview_file(&path, &path, &mut t.host);
    t.settle();
    assert_eq!(t.svc.snapshot().previewing.as_deref(), Some(path.as_str()));
    assert_eq!(t.host.previews, 3);
    t.svc
        .preview_pack_file("test-pack", "sd.wav", "test-pack/sd.wav", &mut t.host);
    t.settle();
    assert_eq!(
        t.svc.snapshot().previewing.as_deref(),
        Some("test-pack/sd.wav")
    );
    assert_eq!(t.host.previews, 4);
}

#[test]
fn now_playing_follows_the_real_playback_cleared_when_the_sound_ends() {
    let mut t = T::new();
    t.init();
    t.svc.preview_sound(
        PadSampleSource::Classic {
            machine: mdaw_model::types::ClassicMachineId::Tr808,
            voice: "snare".into(),
        },
        "builtin:tr808::snare",
        &mut t.host,
    );
    t.settle();
    assert_eq!(
        t.svc.snapshot().previewing.as_deref(),
        Some("builtin:tr808::snare")
    );
    t.host.run(2.0);
    t.svc.poll(&mut t.host);
    assert_eq!(t.svc.snapshot().previewing, None);
    // A superseded preview never plays or clears the newer one.
    t.installed("test-pack");
    let url = raw_test_url("other-pack/bd.wav");
    t.svc.preview_remote(&url, "old", &mut t.host);
    t.svc
        .preview_pack_file("test-pack", "sd.wav", "k2", &mut t.host);
    t.settle();
    assert_eq!(t.svc.snapshot().previewing.as_deref(), Some("k2"));
}

#[test]
fn p2_drops_cached_audio_of_a_pack_when_it_is_installed_updated_or_removed() {
    let mut t = T::new();
    t.init();
    t.installed("test-pack");
    assert_eq!(t.host.invalidated, ["test-pack"], "a newly installed pack");
    t.host.invalidated.clear();
    t.svc.refresh_installed();
    t.settle();
    assert!(t.host.invalidated.is_empty(), "unchanged: no invalidation");
    t.svc.install("test-pack"); // re-install: new installedAt
    t.settle();
    assert_eq!(t.host.invalidated, ["test-pack"]);
    t.host.invalidated.clear();
    t.svc.uninstall("test-pack");
    t.settle();
    assert_eq!(t.host.invalidated, ["test-pack"]);
    assert!(!t.svc.snapshot().store.is_installed("test-pack"));
}

#[test]
fn p1_missing_pack_status() {
    let mut t = T::new();
    assert_eq!(t.svc.pack_status("test-pack"), PackRefStatus::Unknown);
    t.svc.init();
    t.settle();
    assert_eq!(t.svc.pack_status("test-pack"), PackRefStatus::Missing);
    t.svc.load_catalog(false);
    t.svc.install("test-pack");
    t.settle();
    assert_eq!(t.svc.pack_status("test-pack"), PackRefStatus::Installed);
}

/// Fails the uninstall's move-aside rename once.
struct FailUninstall(Mutex<bool>);

impl FsFaults for FailUninstall {
    fn before(&self, op: FsOp, _a: &Path, b: Option<&Path>) -> std::io::Result<()> {
        let mut armed = self.0.lock();
        if *armed && op == FsOp::Rename && b.is_some_and(|b| b.to_string_lossy().contains("/.del-"))
        {
            *armed = false;
            return Err(std::io::Error::other("EBUSY: resource busy"));
        }
        Ok(())
    }
}

#[test]
fn keeps_an_uninstall_failure_for_the_ui_and_clears_it_on_the_next_attempt() {
    let faults = Arc::new(FailUninstall(Mutex::new(false)));
    let f2 = faults.clone();
    let mut t = T::with(move |o| o.faults = f2);
    t.init();
    t.installed("test-pack");
    *faults.0.lock() = true;
    t.svc.uninstall("test-pack");
    t.settle();
    assert_eq!(
        t.svc
            .snapshot()
            .uninstall_errors
            .get("test-pack")
            .map(String::as_str),
        Some("EBUSY: resource busy")
    );
    assert!(t.svc.snapshot().store.is_installed("test-pack"));
    t.svc.uninstall("test-pack");
    assert!(
        t.svc.snapshot().uninstall_errors.is_empty(),
        "cleared when the next attempt starts"
    );
    t.settle();
    assert!(t.svc.snapshot().uninstall_errors.is_empty());
    assert!(!t.svc.snapshot().store.is_installed("test-pack"));
    t.installed("test-pack");
    *faults.0.lock() = true;
    t.svc.uninstall("test-pack");
    t.settle();
    t.svc.dismiss_error("test-pack");
    assert!(t.svc.snapshot().uninstall_errors.is_empty());
}

// --- Get & load (M2) ----------------------------------------------------------------------

#[test]
fn the_latest_request_wins_even_when_an_older_download_finishes_first() {
    let mut t = T::new();
    t.init();
    let (ga, gb) = (t.gate("test-pack"), t.gate("other-pack"));
    t.svc.get_and_load("test-pack", "main", &mut t.host);
    assert_eq!(t.pending().as_deref(), Some("test-pack/main"));
    t.svc.get_and_load("other-pack", "main", &mut t.host);
    assert_eq!(t.pending().as_deref(), Some("other-pack/main"));
    ga.open();
    t.until("test-pack installed", |s, _| {
        s.snapshot().store.is_installed("test-pack")
    });
    assert!(t.svc.take_kit_loaded().is_empty());
    assert_eq!(t.pending().as_deref(), Some("other-pack/main"));
    gb.open();
    t.settle();
    let loaded = t.svc.take_kit_loaded();
    assert_eq!(loaded.len(), 1);
    assert_eq!(
        (loaded[0].pack_id.as_str(), loaded[0].kit_id.as_str()),
        ("other-pack", "main")
    );
    assert_eq!(loaded[0].kit.id, "pack:other-pack:main");
    assert_eq!(t.host.kit_id(), "pack:other-pack:main");
    assert_eq!(t.pending(), None);
}

#[test]
fn an_older_request_finishing_after_the_latest_one_never_overrides_it() {
    let mut t = T::new();
    t.init();
    let (ga, gb) = (t.gate("test-pack"), t.gate("other-pack"));
    t.svc.get_and_load("test-pack", "main", &mut t.host);
    t.svc.get_and_load("other-pack", "main", &mut t.host);
    gb.open();
    t.until("other loaded", |_, h| h.kit_id() == "pack:other-pack:main");
    ga.open();
    t.settle();
    assert_eq!(t.host.kit_id(), "pack:other-pack:main");
}

#[test]
fn loads_an_installed_pack_at_once_without_downloading() {
    let mut t = T::new();
    t.init();
    t.installed("test-pack");
    t.server.clear();
    t.svc.get_and_load("test-pack", "main", &mut t.host);
    assert_eq!(
        t.host.kit_id(),
        "pack:test-pack:main",
        "synchronously, no download"
    );
    assert!(t.server.calls().is_empty());
    assert_eq!(t.pending(), None);
}

#[test]
fn an_outdated_install_is_updated_first() {
    let mut t = T::new();
    t.init();
    t.installed("test-pack");
    // The installed copy claims version 0: the catalog's version 1 is newer.
    let meta = t.root.path().join("test-pack/pack.json");
    let mut v: Value = serde_json::from_str(&read(&meta)).unwrap();
    v["version"] = json!(0);
    std::fs::write(&meta, v.to_string()).unwrap();
    t.svc.refresh_installed();
    t.settle();
    let g = t.gate("test-pack");
    t.svc.get_and_load("test-pack", "main", &mut t.host);
    assert_eq!(t.pending().as_deref(), Some("test-pack/main"));
    g.open();
    t.settle();
    assert_eq!(t.host.kit_id(), "pack:test-pack:main");
    assert_eq!(
        t.svc
            .snapshot()
            .store
            .get_installed("test-pack")
            .unwrap()
            .version,
        1
    );
}

#[test]
fn cancel_or_failure_clear_the_pending_load_and_load_nothing() {
    let mut t = T::new();
    t.init();
    let before = t.host.kit_id();
    let g = t.gate("test-pack");
    t.svc.get_and_load("test-pack", "main", &mut t.host);
    t.until("running", |s, _| {
        s.store().installer().is_installing("test-pack")
    });
    t.svc.cancel("test-pack");
    assert_eq!(t.pending(), None);
    g.open();
    t.settle();
    assert_eq!(t.host.kit_id(), before);

    t.server.route(&raw_test_url("test-pack/bd.wav"), |_, st| {
        respond(st, 404, &[], b"")
    });
    t.svc.get_and_load("test-pack", "main", &mut t.host);
    t.settle();
    assert_eq!(t.pending(), None);
    assert_eq!(t.host.kit_id(), before);
    assert!(matches!(
        t.svc.install_state("test-pack"),
        PackInstallState::Error { .. }
    ));
}

#[test]
fn the_download_keeps_running_after_the_load_is_cancelled() {
    let mut t = T::new();
    t.init();
    let g = t.gate("test-pack");
    t.svc.get_and_load("test-pack", "main", &mut t.host);
    t.svc.cancel_pending_load();
    g.open();
    t.settle();
    assert!(t.svc.snapshot().store.is_installed("test-pack"));
    assert!(t.svc.take_kit_loaded().is_empty());
}

// --- Get & load intent (N2) ---------------------------------------------------------------

/// The TS beforeEach: a starter project with a drum track, catalog loaded.
fn n2() -> (T, String, Gate) {
    let mut t = T::new();
    t.host.e.edit(|d| d.reset_to_starter());
    let drums = t.host.e.edit(|d| d.ensure_drum_track());
    t.init();
    let g = t.gate("test-pack");
    (t, drums, g)
}

fn finish(t: &mut T, g: &Gate) {
    g.open();
    t.settle();
}

#[test]
fn loads_on_the_drum_track_active_at_request_time_and_shows_the_pack_name() {
    let (mut t, drums, g) = n2();
    t.svc.get_and_load("test-pack", "main", &mut t.host);
    let info = t.svc.snapshot().pending_load_info.clone().unwrap();
    assert_eq!(
        (
            info.pack_id.as_str(),
            info.kit_id.as_str(),
            info.pack_name.as_str(),
            info.waiting_for_recording
        ),
        ("test-pack", "main", "Test Pack", false)
    );
    finish(&mut t, &g);
    assert_eq!(t.host.kit_id(), "pack:test-pack:main");
    let loaded = t.svc.take_kit_loaded();
    assert_eq!(loaded[0].track_id.as_deref(), Some(drums.as_str()));
    assert_eq!(t.svc.snapshot().pending_load_info, None);
    // The kit was applied with source "pack-load" (it does not cancel anything).
    let sel = t.host.e.take_kit_selected();
    assert_eq!(sel.last().unwrap().source, KitSelectionSource::PackLoad);
}

#[test]
fn a_kit_chosen_on_the_target_track_meanwhile_drops_the_request_at_once() {
    let (mut t, drums, g) = n2();
    t.svc.get_and_load("test-pack", "main", &mut t.host);
    let mut user = t.host.e.doc().get_kit(&drums);
    user.id = "user-kit".into();
    user.name = "Mine".into();
    t.host
        .e
        .edit(|d| d.set_kit(&drums, &user, KitSelectionSource::User));
    t.sync();
    assert_eq!(t.pending(), None);
    finish(&mut t, &g);
    assert_eq!(t.host.kit_id(), "user-kit");
}

#[test]
fn re_choosing_the_kit_already_loaded_also_drops_it() {
    let (mut t, drums, g) = n2();
    t.svc.get_and_load("test-pack", "main", &mut t.host);
    let same = t.host.e.doc().get_kit(&drums);
    t.host
        .e
        .edit(|d| d.set_kit(&drums, &same, KitSelectionSource::User));
    t.sync();
    assert_eq!(t.pending(), None);
    finish(&mut t, &g);
    assert_eq!(t.host.kit_id(), same.id);
}

#[test]
fn a_real_project_switch_drops_it() {
    let (mut t, _drums, g) = n2();
    t.svc.get_and_load("test-pack", "main", &mut t.host);
    t.host.e.edit(|d| d.reset_to_starter());
    t.sync();
    assert_eq!(t.pending(), None);
    finish(&mut t, &g);
    assert!(!t.host.e.doc().get_tracks().iter().any(|tr| {
        tr.instrument
            .kit
            .as_ref()
            .is_some_and(|k| k.id == "pack:test-pack:main")
    }));
}

#[test]
fn pad_edits_on_the_target_kit_keep_the_request() {
    let (mut t, drums, g) = n2();
    t.svc.get_and_load("test-pack", "main", &mut t.host);
    let patch = serde_json::Map::from_iter([("gainDb".to_owned(), json!(-3))]);
    t.host.e.edit(|d| d.update_pad(&drums, 0, &patch));
    t.sync();
    assert_eq!(t.pending().as_deref(), Some("test-pack/main"));
    finish(&mut t, &g);
    assert_eq!(t.host.kit_id(), "pack:test-pack:main");
}

#[test]
fn switching_the_active_drum_track_drops_the_request() {
    let (mut t, drums, g) = n2();
    t.svc.get_and_load("test-pack", "main", &mut t.host);
    let other = t.host.e.edit(|d| d.add_track(InstrumentType::Drums, 0)).id;
    t.host.e.edit(|d| d.set_active_drum_track(&other));
    t.sync();
    assert_eq!(t.pending(), None);
    finish(&mut t, &g);
    assert_ne!(t.host.e.doc().get_kit(&drums).id, "pack:test-pack:main");
    assert_ne!(t.host.e.doc().get_kit(&other).id, "pack:test-pack:main");
}

#[test]
fn a_new_project_session_drops_the_request_even_with_the_same_track_and_kit_ids() {
    let (mut t, _drums, g) = n2();
    t.host.session = Some(1);
    let before = t.host.kit_id();
    t.svc.get_and_load("test-pack", "main", &mut t.host);
    t.host.session = Some(2);
    t.sync();
    assert_eq!(t.pending(), None);
    finish(&mut t, &g);
    assert_eq!(t.host.kit_id(), before);
}

fn remove_drum_tracks(t: &mut T) {
    let ids: Vec<String> = t
        .host
        .e
        .doc()
        .get_tracks()
        .iter()
        .filter(|tr| tr.instrument.kind == InstrumentType::Drums)
        .map(|tr| tr.id.clone())
        .collect();
    for id in ids {
        t.host.e.edit(|d| d.remove_track(&id));
    }
    assert_eq!(t.host.e.doc().get_active_drum_track_id(), None);
}

#[test]
fn null_target_still_loads_creating_a_drum_track_when_nothing_changes() {
    let (mut t, _drums, g) = n2();
    remove_drum_tracks(&mut t);
    t.svc.get_and_load("test-pack", "main", &mut t.host);
    finish(&mut t, &g);
    assert_eq!(t.host.kit_id(), "pack:test-pack:main");
}

#[test]
fn null_target_a_drum_track_created_meanwhile_drops_the_request() {
    let (mut t, _drums, g) = n2();
    remove_drum_tracks(&mut t);
    t.svc.get_and_load("test-pack", "main", &mut t.host);
    let a = t.host.e.edit(|d| d.add_track(InstrumentType::Drums, 0)).id;
    t.host.e.edit(|d| d.set_active_drum_track(&a));
    t.sync();
    finish(&mut t, &g);
    assert_ne!(t.host.e.doc().get_kit(&a).id, "pack:test-pack:main");
}

#[test]
fn null_target_two_drum_tracks_and_switching_drops_the_request() {
    let (mut t, _drums, g) = n2();
    remove_drum_tracks(&mut t);
    t.svc.get_and_load("test-pack", "main", &mut t.host);
    let a = t.host.e.edit(|d| d.add_track(InstrumentType::Drums, 0)).id;
    let b = t.host.e.edit(|d| d.add_track(InstrumentType::Drums, 0)).id;
    t.host.e.edit(|d| d.set_active_drum_track(&a));
    t.host.e.edit(|d| d.set_active_drum_track(&b));
    t.sync();
    finish(&mut t, &g);
    assert_ne!(t.host.e.doc().get_kit(&a).id, "pack:test-pack:main");
    assert_ne!(t.host.e.doc().get_kit(&b).id, "pack:test-pack:main");
}

#[test]
fn waits_while_recording_and_loads_after_the_take_stops() {
    let (mut t, _drums, g) = n2();
    t.host.recording = Some(true);
    let before = t.host.kit_id();
    t.svc.get_and_load("test-pack", "main", &mut t.host);
    finish(&mut t, &g);
    assert_eq!(t.host.kit_id(), before, "never swaps kit mid-take");
    assert!(
        t.svc
            .snapshot()
            .pending_load_info
            .as_ref()
            .unwrap()
            .waiting_for_recording
    );
    t.host.recording = Some(false);
    t.sync();
    assert_eq!(t.host.kit_id(), "pack:test-pack:main");
    assert_eq!(t.pending(), None);
}

#[test]
fn r3_a_take_restarted_before_the_check_keeps_the_request_waiting() {
    let (mut t, _drums, g) = n2();
    t.host.recording = Some(true);
    let before = t.host.kit_id();
    t.svc.get_and_load("test-pack", "main", &mut t.host);
    finish(&mut t, &g);
    // Stop → Record again within one frame: the next check still sees a take.
    t.host.recording = Some(false);
    t.host.recording = Some(true);
    t.sync();
    assert_eq!(t.host.kit_id(), before);
    assert!(
        t.svc
            .snapshot()
            .pending_load_info
            .as_ref()
            .unwrap()
            .waiting_for_recording
    );
    t.host.recording = Some(false);
    t.sync();
    assert_eq!(t.host.kit_id(), "pack:test-pack:main");
}

#[test]
fn r4_waits_during_a_real_loop_take_too() {
    let (mut t, _drums, g) = n2();
    // A real Beat loop take on the engine (no override).
    t.host.e.start_loop_recording();
    t.host.run(0.05);
    assert!(PackHost::is_recording_now(&t.host));
    let before = t.host.kit_id();
    t.svc.get_and_load("test-pack", "main", &mut t.host);
    finish(&mut t, &g);
    assert_eq!(t.host.kit_id(), before);
    t.host.e.stop();
    t.host.run(0.05);
    t.sync();
    assert_eq!(t.host.kit_id(), "pack:test-pack:main");
}

#[test]
fn cancelling_while_it_waits_for_the_take_to_stop_loads_nothing() {
    let (mut t, _drums, g) = n2();
    t.host.recording = Some(true);
    let before = t.host.kit_id();
    t.svc.get_and_load("test-pack", "main", &mut t.host);
    finish(&mut t, &g);
    t.svc.cancel_pending_load();
    t.host.recording = Some(false);
    t.sync();
    assert_eq!(t.host.kit_id(), before);
}

#[test]
fn the_loaded_kit_plays_the_installed_files() {
    let (mut t, drums, g) = n2();
    t.svc.get_and_load("test-pack", "main", &mut t.host);
    finish(&mut t, &g);
    let kit = t.host.e.doc().get_kit(&drums);
    let inst = t
        .svc
        .snapshot()
        .store
        .get_installed("test-pack")
        .unwrap()
        .clone();
    assert_eq!(Some(kit.clone()), kit_from_installed(&inst, "main"));
    let src = kit.pads[0].sample.as_ref().unwrap().source.clone();
    let buf = t.host.e.cache().load_pad_buffer(&src).unwrap();
    assert_eq!(buf.frames(), 4800);
}

// --- pack names (N6), L-c ------------------------------------------------------------------

#[test]
fn names_withdrawn_packs_and_remembers_catalog_names_across_starts() {
    let mut t = T::new();
    assert!(t.svc.pack_name("tr8-mckaudio").contains("TR-8 Recordings"));
    assert_eq!(t.svc.pack_name("never-heard-of"), "never-heard-of");
    let stored = Arc::new(Mutex::new(String::new()));
    let s2 = stored.clone();
    t.svc.set_name_persister(move |v| *s2.lock() = v.to_owned());
    t.init();
    assert!(stored.lock().contains("\"test-pack\":\"Test Pack\""));
    // Next start with a catalog that no longer lists test-pack.
    let url = "https://raw.githubusercontent.com/t/t/main/catalog2.json";
    t.server.body(
        url,
        &json!({ "version": 1, "updatedAt": "2026-11-19", "packs": [] }).to_string(),
    );
    let mut o = PackStoreOptions::with_connector(t.root.path().to_path_buf(), t.server.connector());
    o.catalog_url = url.into();
    o.bundled = PackCatalog {
        version: 1,
        updated_at: "1970-01-01T00:00:00.000Z".into(),
        packs: vec![],
    };
    let raw = stored.lock().clone();
    let mut svc = PackService::new(o, Some(&raw));
    svc.init();
    svc.load_catalog(false);
    assert!(svc.settle(&mut t.host, Duration::from_secs(5)));
    assert!(svc.snapshot().store.is_unavailable("test-pack"));
    assert_eq!(svc.pack_name("test-pack"), "Test Pack");
}

#[test]
fn l_c_a_pack_the_loaded_catalog_does_not_list_is_unavailable_not_missing() {
    let mut t = T::new();
    t.svc.init();
    t.settle();
    assert_eq!(t.svc.pack_status("gone-pack"), PackRefStatus::Missing);
    t.svc.load_catalog(false);
    t.settle();
    assert_eq!(t.svc.pack_status("gone-pack"), PackRefStatus::Unavailable);
    assert_eq!(t.svc.pack_status("test-pack"), PackRefStatus::Missing);
}

// --- quit (quit.test.ts) -------------------------------------------------------------------

#[test]
fn quit_lets_through_when_idle_pauses_running_installs_and_does_not_intercept_twice() {
    let mut t = T::new();
    t.init();
    assert!(t.svc.pause_for_quit(), "nothing installs");
    t.server.route(&raw_test_url("test-pack/sd.wav"), |_, st| {
        hang(st, b"x", None)
    });
    t.svc.install("test-pack");
    t.until("running", |s, _| {
        s.store().installer().is_installing("test-pack")
    });
    std::thread::sleep(Duration::from_millis(50));
    let t0 = Instant::now();
    assert!(t.svc.store().pause_for_quit(Duration::from_millis(3000)));
    assert!(t0.elapsed() < Duration::from_secs(1));
    assert!(!t.svc.store().installer().busy());
    assert!(list_dir(t.root.path()).contains(&".partial-test-pack".to_owned()));
    // States at the next start: paused → "Download paused".
    let mut o = PackStoreOptions::with_connector(t.root.path().to_path_buf(), t.server.connector());
    o.catalog_url = CATALOG_URL.into();
    let store = mdaw_packs::PackStore::new(o, None);
    assert_eq!(
        store.states().get("test-pack"),
        Some(&PackInstallState::Error {
            message: "Download paused".into()
        })
    );
}

/// Ignores the pause: blocks the swap rename for a while.
struct SlowSwap;

impl FsFaults for SlowSwap {
    fn before(&self, op: FsOp, a: &Path, _b: Option<&Path>) -> std::io::Result<()> {
        if op == FsOp::Rename && a.ends_with(".partial-test-pack") {
            std::thread::sleep(Duration::from_millis(1500));
        }
        Ok(())
    }
}

#[test]
fn quit_gives_up_waiting_after_the_timeout() {
    let mut t = T::with(|o| o.faults = Arc::new(SlowSwap));
    t.init();
    t.svc.install("test-pack");
    t.until("swap", |s, _| {
        s.store().installer().is_installing("test-pack")
            && matches!(
                s.install_state("test-pack"),
                PackInstallState::Downloading { file_index: 2, .. }
            )
    });
    std::thread::sleep(Duration::from_millis(50));
    let t0 = Instant::now();
    assert!(!t.svc.store().pause_for_quit(Duration::from_millis(100)));
    assert!(t0.elapsed() < Duration::from_millis(1000));
    // A second will-quit is not intercepted again.
    assert!(t.svc.store().pause_for_quit(Duration::from_millis(100)));
    t.settle();
}

// --- Review phase 6: cancel / quit while queued (finding 10), worker panics (finding 14) ----

#[test]
fn a_cancel_while_the_install_waits_for_the_catalog_ends_it_before_it_starts() {
    let mut t = T::new();
    t.svc.init();
    t.settle();
    let cat = t.gate_catalog();
    t.svc.install("test-pack");
    cat.wait_arrived(1);
    t.until("queued", |s, _| s.store().queued_installs() == 1);
    let t0 = Instant::now();
    t.svc.cancel("test-pack");
    t.until("cancelled", |s, _| {
        s.store().queued_installs() == 0 && s.install_state("test-pack") == PackInstallState::Idle
    });
    let latency = t0.elapsed();
    assert!(latency < Duration::from_millis(500), "{latency:?}");
    println!("cancel while waiting for the catalog: {latency:?}");
    cat.open();
    t.settle();
    assert_eq!(t.svc.install_state("test-pack"), PackInstallState::Idle);
    assert!(!t.svc.store().installer().is_installing("test-pack"));
    assert_eq!(
        t.server.count(&raw_test_url("test-pack/bd.wav")),
        0,
        "the install never started"
    );
    assert!(!t.root.path().join(".partial-test-pack").exists());
}

#[test]
fn quitting_while_an_install_waits_for_the_catalog_pauses_it_before_it_starts() {
    let mut t = T::new();
    t.svc.init();
    t.settle();
    let cat = t.gate_catalog();
    t.svc.install("test-pack");
    cat.wait_arrived(1);
    t.until("queued", |s, _| s.store().queued_installs() == 1);
    let t0 = Instant::now();
    assert!(t.svc.pause_for_quit(), "settled within the quit timeout");
    assert!(t0.elapsed() < Duration::from_secs(1), "{:?}", t0.elapsed());
    assert_eq!(t.svc.store().queued_installs(), 0);
    cat.open();
    t.settle();
    assert_eq!(
        t.server.count(&raw_test_url("test-pack/bd.wav")),
        0,
        "the install never started"
    );
    assert!(!t.root.path().join(".partial-test-pack").exists());
}

/// Panics on connections to URLs containing `needle` (after a short delay, so a second
/// request for the same preview becomes a waiter).
struct PanicOn {
    inner: Arc<mdaw_packs::http::FixedAddrConnector>,
    needle: &'static str,
}

impl mdaw_packs::http::Connector for PanicOn {
    fn connect(
        &self,
        url: &url::Url,
        timeout: Duration,
        check: mdaw_packs::http::Check,
    ) -> Result<Box<dyn mdaw_packs::http::Conn>, mdaw_packs::http::HttpError> {
        if url.as_str().contains(self.needle) {
            std::thread::sleep(Duration::from_millis(100));
            panic!("injected connector panic");
        }
        self.inner.connect(url, timeout, check)
    }
}

#[test]
fn a_panicking_preview_fetch_reports_an_error_to_every_waiter_and_frees_the_worker() {
    let mut t = T::with_server(|o, server| {
        o.connector = Arc::new(PanicOn {
            inner: server.connector(),
            needle: "test-pack/bd.wav",
        })
    });
    t.init();
    let url = raw_test_url("test-pack/bd.wav");
    // Two requests at once: the leader panics, the waiter must not hang.
    let store = t.svc.store().clone();
    let (s1, s2) = (store.clone(), store.clone());
    let (u1, u2) = (url.clone(), url.clone());
    let a = std::thread::spawn(move || s1.fetch_preview(&u1));
    std::thread::sleep(Duration::from_millis(20));
    let b = std::thread::spawn(move || s2.fetch_preview(&u2));
    assert!(a.join().is_err(), "the leader panicked");
    assert_eq!(
        b.join().unwrap(),
        Err("Preview failed: internal error".into())
    );
    // Through the service: the worker reports and is counted out.
    t.svc.preview_remote(&url, "k", &mut t.host);
    t.settle();
    assert_eq!(
        t.svc.snapshot().preview_error,
        Some(("k".into(), "Internal error".into()))
    );
    assert_eq!(t.svc.snapshot().previewing, None);
    assert!(!t.svc.busy());
}

#[test]
fn a_panicking_catalog_fetch_settles_with_the_bundled_copy_and_can_be_retried() {
    let base = T::new();
    let conn = Arc::new(PanicOn {
        inner: base.server.connector(),
        needle: "catalog.json",
    });
    let mut o = PackStoreOptions::with_connector(base.root.path().to_path_buf(), conn);
    o.catalog_url = CATALOG_URL.into();
    o.bundled = PackCatalog {
        version: 1,
        updated_at: "1970-01-01T00:00:00.000Z".into(),
        packs: vec![],
    };
    let store = mdaw_packs::PackStore::new(o, None);
    let t0 = Instant::now();
    assert!(store.fetch_catalog().packs.is_empty(), "bundled copy");
    assert!(
        store.fetch_catalog().packs.is_empty(),
        "a second fetch runs again"
    );
    assert!(t0.elapsed() < Duration::from_secs(5));
    assert_eq!(
        store.install("test-pack"),
        Err("Unknown pack: test-pack".into())
    );
}

struct PanicOnRename;

impl FsFaults for PanicOnRename {
    fn before(&self, op: FsOp, _a: &Path, _b: Option<&Path>) -> std::io::Result<()> {
        if op == FsOp::Rename {
            panic!("injected rename panic");
        }
        Ok(())
    }
}

#[test]
fn a_panicking_startup_cleanup_still_opens_the_store() {
    let mut t = T::new();
    t.init();
    t.installed("test-pack");
    std::fs::rename(
        t.root.path().join("test-pack"),
        t.root.path().join(".old-test-pack-0badf00d"),
    )
    .unwrap();
    let mut o = PackStoreOptions::with_connector(t.root.path().to_path_buf(), t.server.connector());
    o.catalog_url = CATALOG_URL.into();
    o.faults = Arc::new(PanicOnRename);
    let store = mdaw_packs::PackStore::new(o, None);
    let (tx, rx) = crossbeam_channel::bounded(1);
    std::thread::spawn(move || {
        let _ = tx.send(store.list_installed().len());
    });
    assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok(0));
}

#[test]
fn a_panicking_install_shows_an_error_and_frees_the_worker() {
    struct PanicOnPackJson;
    impl FsFaults for PanicOnPackJson {
        fn before(&self, op: FsOp, _a: &Path, _b: Option<&Path>) -> std::io::Result<()> {
            if op == FsOp::WriteFile {
                panic!("injected panic");
            }
            Ok(())
        }
    }
    let mut t = T::with(|o| o.faults = Arc::new(PanicOnPackJson));
    t.init();
    t.svc.install("test-pack");
    t.settle();
    assert_eq!(
        t.svc.install_state("test-pack"),
        PackInstallState::Error {
            message: "Install failed: internal error".into()
        }
    );
    assert!(!t.svc.busy());
}
