//! Test helpers (`src/main/packs/testUtils.ts`): a local HTTP server standing in for
//! raw.githubusercontent.com (the client keeps checking the https URLs against the allowlist and
//! the connector sends every request here), manifests and temp folders.
#![allow(dead_code)]

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use mdaw_packs::http::FixedAddrConnector;
use parking_lot::Mutex;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub const COMMIT: &str = "c0ffee0000000000000000000000000000000000";

pub fn raw_test_url(path: &str) -> String {
    format!("https://raw.githubusercontent.com/o/r/{COMMIT}/{path}")
}

pub fn bd_url() -> String {
    raw_test_url("bd.wav")
}

pub fn sd_url() -> String {
    raw_test_url("sd.wav")
}

pub fn sha256_of(body: &[u8]) -> String {
    mdaw_packs::download::hex(&Sha256::digest(body))
}

pub fn pack_file(path: &str, body: &str, url: &str) -> Value {
    json!({ "path": path, "url": url, "size": body.len(), "sha256": sha256_of(body.as_bytes()) })
}

/// `manifest()`: files `kicks/bd.wav` ("kick" at BD) and `sd.wav` ("snare" at SD).
pub fn manifest() -> Value {
    json!({
        "id": "test-pack",
        "name": "Test Pack",
        "tagline": "808-style",
        "description": "A test pack",
        "tags": ["808"],
        "author": "Someone",
        "source": "https://github.com/example/repo",
        "license": "CC0-1.0",
        "color": "#f80",
        "files": [pack_file("kicks/bd.wav", "kick", &bd_url()), pack_file("sd.wav", "snare", &sd_url())],
        "kits": [{ "id": "main", "name": "Main", "pads": [{ "pad": 0, "file": "kicks/bd.wav", "name": "Kick" }, { "pad": 1, "file": "sd.wav" }] }],
        "version": 1
    })
}

/// `manifestFor(kick, snare, version)`.
pub fn manifest_for(kick: &str, snare: &str, version: i64) -> Value {
    let mut m = manifest();
    m["version"] = json!(version);
    m["files"][0]["size"] = json!(kick.len());
    m["files"][0]["sha256"] = json!(sha256_of(kick.as_bytes()));
    m["files"][1]["size"] = json!(snare.len());
    m["files"][1]["sha256"] = json!(sha256_of(snare.as_bytes()));
    m
}

/// A request as the server saw it.
#[derive(Debug, Clone)]
pub struct Req {
    /// `https://<host><path>`.
    pub url: String,
    pub headers: Vec<(String, String)>,
}

pub type Handler = Arc<dyn Fn(&Req, &mut TcpStream) + Send + Sync>;

/// A local HTTP/1.1 server (one thread per connection).
pub struct Server {
    pub addr: SocketAddr,
    routes: Arc<Mutex<HashMap<String, Handler>>>,
    calls: Arc<Mutex<Vec<Req>>>,
}

impl Server {
    pub fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let routes: Arc<Mutex<HashMap<String, Handler>>> = Arc::default();
        let calls: Arc<Mutex<Vec<Req>>> = Arc::default();
        let (r, c) = (routes.clone(), calls.clone());
        std::thread::spawn(move || {
            for s in listener.incoming() {
                let Ok(mut s) = s else { continue };
                let (r, c) = (r.clone(), c.clone());
                std::thread::spawn(move || {
                    let Some(req) = read_request(&mut s) else {
                        return;
                    };
                    c.lock().push(req.clone());
                    let h = r.lock().get(&req.url).cloned();
                    match h {
                        Some(h) => h(&req, &mut s),
                        None => respond(&mut s, 404, &[], b"nope"),
                    }
                    let _ = s.flush();
                });
            }
        });
        Self {
            addr,
            routes,
            calls,
        }
    }

    pub fn connector(&self) -> Arc<FixedAddrConnector> {
        Arc::new(FixedAddrConnector { addr: self.addr })
    }

    pub fn route(
        &self,
        url: &str,
        h: impl Fn(&Req, &mut TcpStream) + Send + Sync + 'static,
    ) -> &Self {
        self.routes.lock().insert(url.to_owned(), Arc::new(h));
        self
    }

    /// 200 with this body and its content-length.
    pub fn body(&self, url: &str, body: &str) -> &Self {
        let b = body.as_bytes().to_vec();
        self.route(url, move |_, s| respond(s, 200, &[], &b))
    }

    pub fn clear(&self) {
        self.routes.lock().clear();
        self.calls.lock().clear();
    }

    /// Requested URLs, in order.
    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().iter().map(|r| r.url.clone()).collect()
    }

    pub fn requests(&self) -> Vec<Req> {
        self.calls.lock().clone()
    }

    pub fn count(&self, url: &str) -> usize {
        self.calls().iter().filter(|u| *u == url).count()
    }
}

fn read_request(s: &mut TcpStream) -> Option<Req> {
    let mut r = BufReader::new(s.try_clone().ok()?);
    let mut line = String::new();
    r.read_line(&mut line).ok()?;
    let path = line.split_whitespace().nth(1)?.to_owned();
    let mut headers = Vec::new();
    loop {
        let mut l = String::new();
        r.read_line(&mut l).ok()?;
        let l = l.trim_end();
        if l.is_empty() {
            break;
        }
        if let Some((k, v)) = l.split_once(':') {
            headers.push((k.trim().to_owned(), v.trim().to_owned()));
        }
    }
    let host = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("host"))
        .map(|(_, v)| v.clone())
        .unwrap_or_default();
    Some(Req {
        url: format!("https://{host}{path}"),
        headers,
    })
}

/// A complete response with content-length.
pub fn respond(s: &mut TcpStream, status: u16, headers: &[(&str, &str)], body: &[u8]) {
    let mut head = format!(
        "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (k, v) in headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    let _ = s.write_all(head.as_bytes());
    let _ = s.write_all(body);
}

/// Head with arbitrary headers (no content-length unless given), then `body`, then close.
pub fn respond_raw(s: &mut TcpStream, status: u16, headers: &[(&str, &str)], body: &[u8]) {
    let mut head = format!("HTTP/1.1 {status} X\r\nConnection: close\r\n");
    for (k, v) in headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    let _ = s.write_all(head.as_bytes());
    let _ = s.write_all(body);
}

/// Sends a 200 head and `prefix`, then never sends more until the client hangs up
/// (`hangingResponse`).
pub fn hang(s: &mut TcpStream, prefix: &[u8], content_length: Option<usize>) {
    let mut head = "HTTP/1.1 200 OK\r\n".to_owned();
    if let Some(n) = content_length {
        head.push_str(&format!("Content-Length: {n}\r\n"));
    }
    head.push_str("\r\n");
    let _ = s.write_all(head.as_bytes());
    let _ = s.write_all(prefix);
    let _ = s.flush();
    wait_close(s, Duration::from_secs(30));
}

/// Waits until the client closes the connection (or `max`).
pub fn wait_close(s: &mut TcpStream, max: Duration) {
    let _ = s.set_read_timeout(Some(Duration::from_millis(20)));
    let end = std::time::Instant::now() + max;
    let mut b = [0u8; 64];
    while std::time::Instant::now() < end {
        match s.read(&mut b) {
            Ok(0) => return,
            Ok(_) => {}
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => return,
        }
    }
}

/// A fresh temp folder, removed on drop.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("mdaw-packs-{tag}-{}", uuid_like()));
        std::fs::create_dir_all(&p).unwrap();
        Self(std::fs::canonicalize(&p).unwrap())
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn uuid_like() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!(
        "{}-{}-{}",
        std::process::id(),
        t,
        N.fetch_add(1, Ordering::SeqCst)
    )
}

/// Sorted names in a folder.
pub fn list_dir(p: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(p)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

pub fn read(p: impl AsRef<Path>) -> String {
    std::fs::read_to_string(p).unwrap()
}
