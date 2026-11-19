//! Golden: rune/parity/fixtures/packs_io.json, written by rune/parity/ts/packs_io.golden.ts from
//! the real `download.ts` / `catalog.ts` / `installer.ts`. Every scenario is replayed here on a
//! local HTTP server and real folders; outcomes, error messages, retry classes, requested URLs,
//! state events and the folder trees must match.

mod common;

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use common::*;
use mdaw_model::packs::{InstalledPack, PACK_CATALOG_URL, PackInstallState, validate_manifest};
use mdaw_packs::catalog::{FetchCatalogOptions, bundled_catalog, fetch_catalog};
use mdaw_packs::download::{
    DownloadError, DownloadOptions, RetryOptions, download_to_buffer, download_to_file_with_retry,
};
use mdaw_packs::installer::pack_json_text;
use mdaw_packs::{InstallerOptions, PackInstaller};
use parking_lot::Mutex;
use serde_json::{Value, json};

fn fixture() -> Value {
    let raw = include_str!("../../../parity/fixtures/packs_io.json");
    serde_json::from_str(raw).unwrap()
}

/// Serves a `RouteSpec`.
fn serve(server: &Server, url: &str, spec: &Value) {
    let spec = spec.clone();
    let hits = Arc::new(AtomicUsize::new(0));
    server.route(url, move |_, s| {
        let mut sp = &spec;
        if let Some(seq) = spec.get("seq").and_then(Value::as_array) {
            let i = hits.fetch_add(1, Ordering::SeqCst).min(seq.len() - 1);
            sp = &seq[i];
        }
        let status = sp.get("status").and_then(Value::as_u64).unwrap_or(200) as u16;
        let body = sp
            .get("body")
            .and_then(Value::as_str)
            .unwrap_or("")
            .as_bytes()
            .to_vec();
        let mut headers: Vec<(String, String)> = sp
            .get("headers")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .map(|h| {
                        (
                            h[0].as_str().unwrap().to_owned(),
                            h[1].as_str().unwrap().to_owned(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let null_body = matches!(status, 204 | 205 | 304);
        if sp.get("cl").and_then(Value::as_str).unwrap_or("auto") == "auto" {
            headers.push((
                "Content-Length".into(),
                if null_body { 0 } else { body.len() }.to_string(),
            ));
        }
        let hs: Vec<(&str, &str)> = headers
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        if sp.get("stall").and_then(Value::as_bool).unwrap_or(false) {
            respond_raw(s, status, &hs, &body);
            use std::io::Write;
            let _ = s.flush();
            wait_close(s, Duration::from_secs(30));
        } else {
            respond_raw(s, status, &hs, if null_body { b"" } else { &body });
        }
    });
}

fn serve_all(server: &Server, routes: &Value) {
    server.clear();
    if let Some(o) = routes.as_object() {
        for (url, spec) in o {
            serve(server, url, spec);
        }
    }
}

fn error_json(e: &DownloadError) -> Value {
    let name = match e {
        DownloadError::Failed { .. } => "DownloadError",
        DownloadError::Timeout => "DownloadTimeoutError",
        DownloadError::Aborted => "AbortError",
    };
    json!({ "error": e.message(), "retryable": e.retryable(), "name": name })
}

#[test]
fn pack_json_is_byte_identical() {
    let f = fixture();
    let cases = f["pack_json"].as_array().unwrap();
    for c in cases {
        let m = validate_manifest(&c["manifest"]).unwrap();
        let inst = InstalledPack {
            id: m.id.clone(),
            version: m.version,
            dir: format!("/packs/{}", m.id),
            installed_at: "2026-11-18T12:34:56.789Z".into(),
            manifest: m,
        };
        assert_eq!(
            pack_json_text(&inst),
            c["text"].as_str().unwrap(),
            "{}",
            inst.id
        );
    }
    eprintln!("pack.json cases: {}", cases.len());
    assert!(cases.len() >= 6);
}

#[test]
fn downloads_match_the_ts_downloader() {
    let f = fixture();
    let server = Server::new();
    let conn = server.connector();
    let tmp = TempDir::new("golden-dl");
    let mut n = 0;
    for c in f["download"].as_array().unwrap() {
        let name = c["name"].as_str().unwrap();
        serve_all(&server, &c["routes"]);
        let url = c["url"].as_str().unwrap();
        let mut o = DownloadOptions::new(c["maxBytes"].as_u64().unwrap(), &*conn);
        o.expected_bytes = c.get("expectedBytes").and_then(Value::as_u64);
        o.sha256 = c.get("sha256").and_then(Value::as_str);
        o.idle_timeout = c
            .get("idleTimeoutMs")
            .and_then(Value::as_u64)
            .map(Duration::from_millis);
        let outcome = if c["mode"] == "buffer" {
            match download_to_buffer(url, &o) {
                Ok(b) => json!({ "ok": { "bytes": b.len(), "sha256": sha256_of(&b) } }),
                Err(e) => error_json(&e),
            }
        } else {
            let dest = tmp.path().join(format!("f-{n}.bin"));
            let r = download_to_file_with_retry(
                url,
                &dest,
                &o,
                RetryOptions {
                    attempts: 3,
                    base_delay: Duration::from_millis(1),
                },
                None,
            );
            match r {
                Ok((bytes, sha)) => {
                    let file: String = std::fs::read(&dest)
                        .unwrap()
                        .iter()
                        .map(|b| *b as char)
                        .collect();
                    json!({ "ok": { "bytes": bytes, "sha256": sha, "file": file } })
                }
                Err(e) => {
                    assert!(!dest.exists(), "{name}: partial file removed");
                    error_json(&e)
                }
            }
        };
        assert_eq!(outcome, c["outcome"], "{name}");
        assert_eq!(json!(server.calls()), c["calls"], "{name}: requested URLs");
        n += 1;
    }
    eprintln!("download scenarios: {n}");
}

#[test]
fn catalog_fetches_match_the_ts_fetch_catalog() {
    let f = fixture();
    let server = Server::new();
    let conn = server.connector();
    let mut n = 0;
    for c in f["catalog"].as_array().unwrap() {
        let name = c["name"].as_str().unwrap();
        server.clear();
        match &c["body"] {
            Value::Null => {}
            Value::String(s) => {
                server.body(PACK_CATALOG_URL, s);
            }
            v => {
                let big = v["repeat"]
                    .as_str()
                    .unwrap()
                    .repeat(v["count"].as_u64().unwrap() as usize);
                server.body(PACK_CATALOG_URL, &big);
            }
        }
        let mut o = FetchCatalogOptions::new(&*conn);
        o.bundled = bundled_catalog();
        let r = fetch_catalog(o);
        let got = json!({
            "source": r.source,
            "updatedAt": r.catalog.updated_at,
            "packs": r.catalog.packs.iter().map(|p| format!("{}@{}", p.id, p.version)).collect::<Vec<_>>(),
        });
        let want =
            json!({ "source": c["source"], "updatedAt": c["updatedAt"], "packs": c["packs"] });
        assert_eq!(got, want, "{name}");
        n += 1;
    }
    eprintln!("catalog scenarios: {n}");
}

fn seed(root: &Path, seeds: &Value) {
    let now = SystemTime::now();
    for s in seeds.as_array().unwrap() {
        let full = root.join(s["path"].as_str().unwrap());
        match s.get("content").and_then(Value::as_str) {
            None => std::fs::create_dir_all(&full).unwrap(),
            Some(c) => {
                std::fs::create_dir_all(full.parent().unwrap()).unwrap();
                std::fs::write(&full, c).unwrap();
            }
        }
    }
    for s in seeds.as_array().unwrap() {
        let Some(age) = s.get("ageMs").and_then(Value::as_u64) else {
            continue;
        };
        let t = now - Duration::from_millis(age);
        let rel = s["path"].as_str().unwrap();
        for p in [root.join(rel), root.join(rel.split('/').next().unwrap())] {
            let f = std::fs::File::open(&p).unwrap();
            f.set_times(std::fs::FileTimes::new().set_modified(t).set_accessed(t))
                .unwrap();
        }
    }
}

const SEEDED_AT: [&str; 5] = [
    "2026-11-18T00:00:00.000Z",
    "2026-11-19T00:00:00.000Z",
    "x",
    "not a date",
    "1970-01-01T00:00:00.000Z",
];

fn mask_at(v: &str) -> String {
    if SEEDED_AT.contains(&v) {
        v.to_owned()
    } else {
        "<NOW>".into()
    }
}

fn mask_name(rel: &str) -> String {
    let (first, rest) = match rel.split_once('/') {
        Some((a, b)) => (a, Some(b)),
        None => (rel, None),
    };
    let mut first = first.to_owned();
    if (first.starts_with(".old-") || first.starts_with(".del-")) && first.len() > 14 {
        let (head, tail) = first.split_at(first.len() - 9);
        if tail.starts_with('-')
            && tail[1..]
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            first = format!("{head}-XXXXXXXX");
        }
    }
    match rest {
        Some(r) => format!("{first}/{r}"),
        None => first,
    }
}

/// UTF-16 order like `Array.prototype.sort`.
fn js_sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    v
}

fn tree(root: &Path, rel: &str) -> Vec<Value> {
    let mut out = Vec::new();
    let names = js_sorted(list_dir(&root.join(rel)));
    for n in names {
        let r = if rel.is_empty() {
            n.clone()
        } else {
            format!("{rel}/{n}")
        };
        let full = root.join(&r);
        let m = std::fs::symlink_metadata(&full).unwrap();
        if m.is_dir() {
            out.push(json!({ "dir": mask_name(&r) }));
            out.extend(tree(root, &r));
        } else {
            let text = String::from_utf8_lossy(&std::fs::read(&full).unwrap()).into_owned();
            let mut content = Value::String(text.clone());
            if n == "pack.json"
                && let Ok(Value::Object(mut j)) = serde_json::from_str::<Value>(&text)
            {
                if j.contains_key("dir") {
                    j.insert("dir".into(), json!("<DIR>"));
                }
                if let Some(at) = j.get("installedAt") {
                    let masked = match at.as_str() {
                        Some(s) => mask_at(s),
                        None => "<NOW>".into(),
                    };
                    j.insert("installedAt".into(), json!(masked));
                }
                content = Value::Object(j);
            }
            out.push(json!({ "file": mask_name(&r), "content": content }));
        }
    }
    out
}

/// Runs of `downloading` events with one fileIndex collapse to the last one.
fn collapse(states: &[PackInstallState]) -> Vec<Value> {
    let mut out: Vec<PackInstallState> = Vec::new();
    for s in states {
        if let (
            PackInstallState::Downloading { file_index: a, .. },
            Some(PackInstallState::Downloading { file_index: b, .. }),
        ) = (s, out.last())
            && a == b
        {
            *out.last_mut().unwrap() = s.clone();
            continue;
        }
        out.push(s.clone());
    }
    out.iter()
        .map(|s| serde_json::to_value(s).unwrap())
        .collect()
}

#[test]
fn installer_scenarios_match_the_ts_installer() {
    let f = fixture();
    let server = Server::new();
    let mut steps_run = 0;
    for c in f["installer"].as_array().unwrap() {
        let name = c["name"].as_str().unwrap();
        let root = TempDir::new("golden-inst");
        seed(root.path(), &c["seeds"]);
        for (i, (step, want)) in c["steps"]
            .as_array()
            .unwrap()
            .iter()
            .zip(c["results"].as_array().unwrap())
            .enumerate()
        {
            let op = step["op"].as_str().unwrap();
            serve_all(&server, step.get("routes").unwrap_or(&Value::Null));
            let states: Arc<Mutex<Vec<PackInstallState>>> = Arc::default();
            let st = states.clone();
            let mut o = InstallerOptions::new(root.path().to_path_buf(), server.connector());
            o.on_state = Some(Arc::new(move |_, s| st.lock().push(s.clone())));
            o.progress_interval = Duration::ZERO;
            o.concurrency = step.get("concurrency").and_then(Value::as_u64).unwrap_or(1) as usize;
            o.retry = RetryOptions {
                attempts: 3,
                base_delay: Duration::from_millis(1),
            };
            let concurrency = o.concurrency;
            let inst = PackInstaller::new(o);
            let rel = |d: &str| {
                Path::new(d)
                    .strip_prefix(root.path())
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            };
            let outcome = match op {
                "install" => match inst.install_json(&step["manifest"]).wait() {
                    Ok(p) => {
                        json!({ "ok": { "id": p.id, "version": p.version, "dir": rel(&p.dir) } })
                    }
                    Err(e) => json!({ "error": e.message() }),
                },
                "cleanup" => {
                    inst.cleanup_stale(SystemTime::now());
                    json!({ "ok": null })
                }
                "uninstall" => match inst.uninstall(step["id"].as_str().unwrap()) {
                    Ok(()) => json!({ "ok": null }),
                    Err(e) => json!({ "error": e }),
                },
                "paused" => json!({ "ok": inst.paused_ids() }),
                "list" => json!({ "ok": inst.list_installed().iter().map(|p| json!({
                    "id": p.id, "version": p.version, "name": p.manifest.name,
                    "installedAt": mask_at(&p.installed_at), "dir": rel(&p.dir),
                })).collect::<Vec<_>>() }),
                other => panic!("unknown op {other}"),
            };
            let ctx = format!("{name} / step {i} ({op})");
            assert_eq!(outcome, want["outcome"], "{ctx}: outcome");
            let (mut got_states, mut want_states) = (
                collapse(&states.lock()),
                want["states"].as_array().unwrap().clone(),
            );
            if concurrency > 1 {
                // Parallel downloads interleave their progress by timing (in both apps): compare
                // the non-downloading events and the last downloading one.
                let keep = |v: Vec<Value>| -> Vec<Value> {
                    let last_dl = v.iter().rposition(|s| s["state"] == "downloading");
                    v.into_iter()
                        .enumerate()
                        .filter(|(i, s)| s["state"] != "downloading" || Some(*i) == last_dl)
                        .map(|(_, s)| s)
                        .collect()
                };
                got_states = keep(got_states);
                want_states = keep(want_states);
            }
            assert_eq!(got_states, want_states, "{ctx}: states");
            let mut calls = server.calls();
            let mut want_calls: Vec<String> =
                serde_json::from_value(want["calls"].clone()).unwrap();
            if concurrency > 1 {
                calls.sort();
                want_calls.sort();
            }
            assert_eq!(calls, want_calls, "{ctx}: requested URLs");
            assert_eq!(
                json!(tree(root.path(), "")),
                want["tree"],
                "{ctx}: folder tree"
            );
            steps_run += 1;
        }
    }
    eprintln!("installer steps: {steps_run}");
}
