//! Live: one real pack from the real catalog URL over real TLS, installed by the Rust installer
//! and by the Electron installer (`rune/parity/ts/live_install.golden.ts` through vitest), then
//! compared on disk (byte-identical files, equivalent pack.json) and used across apps in both
//! directions (list + verify, resume of the other app's staging, decode + kit).
//!
//!     cargo test -p mdaw-packs --test live -- --ignored --nocapture
//!
//! Needs the network and `npx` (run from the repo with node_modules installed).

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use common::*;
use mdaw_engine::buffers::PadBufferCache;
use mdaw_model::packs::{CatalogSource, kit_from_installed};
use mdaw_packs::http::{Check, Conn, Connector, HttpError, TlsConnector};
use mdaw_packs::installer::{is_verified_file, read_installed_meta};
use mdaw_packs::{InstallerOptions, PackInstaller, PackStore, PackStoreOptions};
use serde_json::Value;
use url::Url;

const PACK: &str = "uzu-drumkit";

struct Counting {
    inner: TlsConnector,
    n: AtomicUsize,
}

impl Connector for Counting {
    fn connect(
        &self,
        url: &Url,
        timeout: Duration,
        check: Check,
    ) -> Result<Box<dyn Conn>, HttpError> {
        self.n.fetch_add(1, Ordering::SeqCst);
        self.inner.connect(url, timeout, check)
    }
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap()
}

/// Every file under `dir` (relative path → bytes), sorted.
fn files(dir: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(root: &Path, rel: &str, out: &mut Vec<(String, Vec<u8>)>) {
        for n in list_dir(&root.join(rel)) {
            let r = if rel.is_empty() {
                n
            } else {
                format!("{rel}/{n}")
            };
            let p = root.join(&r);
            if p.is_dir() {
                walk(root, &r, out);
            } else {
                out.push((r, std::fs::read(&p).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, "", &mut out);
    out
}

fn run_electron(envs: &[(&str, &Path)], report: &Path) -> Value {
    let mut cmd = Command::new("npx");
    cmd.current_dir(repo_root())
        .args([
            "vitest",
            "run",
            "--config",
            "rune/parity/vitest.golden.config.mts",
            "rune/parity/ts/live_install.golden.ts",
        ])
        .env("MDAW_LIVE_PACK", PACK)
        .env("MDAW_LIVE_REPORT", report);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("npx runs");
    assert!(
        out.status.success(),
        "vitest failed:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_str(&read(report)).unwrap()
}

/// Copies a pack folder's files (not pack.json) into `<dir>/.partial-<id>`.
fn stage_copy(pack_dir: &Path, packs_dir: &Path) {
    let stage = packs_dir.join(format!(".partial-{PACK}"));
    for (rel, bytes) in files(pack_dir) {
        if rel == "pack.json" {
            continue;
        }
        let p = stage.join(&rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }
}

#[test]
#[ignore = "network + npx: cargo test -p mdaw-packs --test live -- --ignored"]
fn live_catalog_install_matches_the_electron_installer() {
    let tmp = TempDir::new("live");
    let (rust_dir, electron_dir) = (tmp.path().join("rust"), tmp.path().join("electron"));
    let (rust_resume, electron_resume) = (
        tmp.path().join("rust-resume"),
        tmp.path().join("electron-resume"),
    );
    let report_path = tmp.path().join("report.json");

    // 1. Rust: real catalog URL, real TLS.
    let store = PackStore::new(PackStoreOptions::new(&rust_dir), None);
    let catalog = store.fetch_catalog();
    let remote = mdaw_packs::catalog::fetch_catalog(mdaw_packs::catalog::FetchCatalogOptions::new(
        &TlsConnector::new(),
    ));
    eprintln!(
        "catalog source: {:?}, updatedAt {}",
        remote.source, catalog.updated_at
    );
    // The remote catalog really came over TLS (it merges to "bundled" when it is not newer).
    let tls = TlsConnector::new();
    let bytes = mdaw_packs::download::download_to_buffer(
        mdaw_model::packs::PACK_CATALOG_URL,
        &mdaw_packs::download::DownloadOptions::new(mdaw_model::packs::MAX_CATALOG_BYTES, &tls),
    )
    .expect("the real catalog downloads over TLS");
    let (live, errors) = mdaw_packs::catalog::parse_catalog(&bytes).expect("valid");
    assert!(errors.is_empty());
    eprintln!(
        "remote catalog: {} bytes, updatedAt {}, {} packs",
        bytes.len(),
        live.updated_at,
        live.packs.len()
    );
    assert!(live.packs.iter().any(|p| p.id == PACK));
    let t0 = std::time::Instant::now();
    let installed = store.install(PACK).expect("rust install");
    eprintln!(
        "rust install: {} files in {:?}",
        installed.manifest.files.len(),
        t0.elapsed()
    );

    // 2. Electron: the same pack with the TS installer; it also lists + verifies the Rust folder.
    let report = run_electron(
        &[
            ("MDAW_LIVE_PACKS_DIR", &electron_dir),
            ("MDAW_LIVE_LIST_DIR", &rust_dir),
        ],
        &report_path,
    );
    eprintln!(
        "electron report: {}",
        serde_json::to_string(&report).unwrap()
    );
    assert_eq!(report["catalogUpdatedAt"], catalog.updated_at.as_str());
    assert_eq!(
        report["catalogSource"],
        serde_json::to_value(remote.source).unwrap()
    );
    assert_eq!(report["version"], installed.version);

    // 3. Same folders, byte-identical files.
    assert_eq!(list_dir(&rust_dir), [PACK]);
    assert_eq!(list_dir(&electron_dir), [PACK]);
    let (rf, ef) = (files(&rust_dir.join(PACK)), files(&electron_dir.join(PACK)));
    assert_eq!(
        rf.iter().map(|f| &f.0).collect::<Vec<_>>(),
        ef.iter().map(|f| &f.0).collect::<Vec<_>>(),
        "same file list"
    );
    let mut audio = 0;
    for ((rp, rb), (_, eb)) in rf.iter().zip(&ef) {
        if rp == "pack.json" {
            continue;
        }
        assert_eq!(rb, eb, "{rp} differs");
        audio += 1;
    }
    eprintln!("{audio} files byte-identical");

    // 4. pack.json: identical except `dir` and `installedAt`.
    let (rj, ej) = (
        read(rust_dir.join(PACK).join("pack.json")),
        read(electron_dir.join(PACK).join("pack.json")),
    );
    let (mut rv, mut ev): (Value, Value) = (
        serde_json::from_str(&rj).unwrap(),
        serde_json::from_str(&ej).unwrap(),
    );
    let e_dir = ev["dir"].as_str().unwrap().to_owned();
    let e_at = ev["installedAt"].as_str().unwrap().to_owned();
    assert_eq!(rv["dir"], rust_dir.join(PACK).to_string_lossy().as_ref());
    assert_eq!(e_dir, electron_dir.join(PACK).to_string_lossy());
    for v in [&mut rv, &mut ev] {
        v["dir"] = Value::Null;
        v["installedAt"] = Value::Null;
    }
    assert_eq!(rv, ev, "equivalent pack.json");
    let rewritten = rj
        .replace(
            &serde_json::to_string(rust_dir.join(PACK).to_string_lossy().as_ref()).unwrap(),
            &serde_json::to_string(&e_dir).unwrap(),
        )
        .replace(&installed.installed_at, &e_at);
    assert_eq!(
        rewritten, ej,
        "pack.json bytes identical apart from dir / installedAt"
    );

    // 5. Electron uses the Rust install: listed with the right dir, every file verified.
    let listed = &report["listedRust"];
    assert_eq!(listed["packs"][0]["id"], PACK);
    assert_eq!(
        listed["packs"][0]["dir"],
        rust_dir.join(PACK).to_string_lossy().as_ref()
    );
    assert_eq!(
        listed["packs"][0]["installedAt"],
        installed.installed_at.as_str()
    );
    let verified = listed["verified"].as_object().unwrap();
    assert_eq!(verified.len(), installed.manifest.files.len());
    assert!(verified.values().all(|v| v == true));

    // 6. Rust uses the Electron install: listed, verified, its kit decodes.
    let inst = PackInstaller::new(InstallerOptions::new(
        &electron_dir,
        Arc::new(TlsConnector::new()),
    ));
    let list = inst.list_installed();
    assert_eq!(list.len(), 1);
    let ep = &list[0];
    assert_eq!(ep.dir, electron_dir.join(PACK).to_string_lossy());
    assert_eq!(ep.installed_at, e_at);
    for f in &ep.manifest.files {
        assert!(
            is_verified_file(&electron_dir.join(PACK).join(&f.path), f.size, &f.sha256),
            "{}",
            f.path
        );
    }
    assert!(
        read_installed_meta(&electron_dir.join(PACK))
            .unwrap()
            .is_some()
    );
    let kit = kit_from_installed(ep, &ep.manifest.kits[0].id).unwrap();
    let cache = PadBufferCache::with_fs(48_000, Some(electron_dir.to_string_lossy().into_owned()));
    let mut decoded = 0;
    for pad in kit.pads.iter().filter_map(|p| p.sample.as_ref()) {
        let b = cache
            .load_pad_buffer(&pad.source)
            .expect("pad decodes from the Electron folder");
        assert!(b.frames() > 0);
        decoded += 1;
    }
    eprintln!("{decoded} kit pads decoded from the Electron install");

    // 7. Resume across apps: each app resumes the other's staging without a download.
    stage_copy(&electron_dir.join(PACK), &rust_resume);
    let counting = Arc::new(Counting {
        inner: TlsConnector::new(),
        n: AtomicUsize::new(0),
    });
    let mut o = PackStoreOptions::with_connector(&rust_resume, counting.clone());
    o.bundled = catalog.clone();
    o.catalog_url = "https://raw.githubusercontent.com/invalid/none/main/none.json".into();
    let rs = PackStore::new(o, None);
    let n_before = counting.n.load(Ordering::SeqCst); // the catalog fetch (404 → bundled = the live catalog)
    rs.fetch_catalog();
    let after_catalog = counting.n.load(Ordering::SeqCst);
    rs.install(PACK).expect("rust resume");
    assert_eq!(
        counting.n.load(Ordering::SeqCst),
        after_catalog,
        "Rust resumed Electron's staging without downloading"
    );
    let _ = n_before;
    stage_copy(&rust_dir.join(PACK), &electron_resume);
    let report = run_electron(&[("MDAW_LIVE_RESUME_DIR", &electron_resume)], &report_path);
    assert_eq!(
        report["resumed"]["requests"], 0,
        "Electron resumed Rust's staging without downloading"
    );
    assert_eq!(list_dir(&electron_resume), [PACK]);
    let _ = CatalogSource::Remote;
}
