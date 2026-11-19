//! The HTTP client against scripted raw responses (phase-6 review findings 1, 5, 6, 7):
//! bounded memory, framing as Node's fetch reads it, content decoding, and how fast a cancel
//! or a deadline ends connect / handshake / body waits.
//!
//! Node's fetch (v26, undici) on the same raw bytes (scratch script, not part of the suite):
//! duplicate `Content-Length` (even identical, or `5, 5`) → `fetch failed`; `+5` / `-1` / `5x`
//! / empty → `fetch failed`; CL + TE → `fetch failed`; TE `notchunked` or `chunked, gzip` →
//! read until close; `gzip, chunked` / `CHUNKED` → chunked; `Content-Encoding` gzip, x-gzip,
//! deflate (zlib or raw), `gzip, gzip`, br → decoded; unknown / identity → passed through;
//! corrupt gzip → `terminated`; chunk size `+5` → `terminated`; any 1xx before the response →
//! `fetch failed` (here: up to 8 are skipped, see `http.rs`).

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::time::{Duration, Instant};

use flate2::Compression;
use flate2::write::{DeflateEncoder, GzEncoder, ZlibEncoder};
use mdaw_packs::download::{DownloadError, DownloadOptions, download_to_buffer, hex};
use mdaw_packs::http::{
    Check, Connector, FixedAddrConnector, HttpError, Interrupt, POLL, Response, TlsConnector, get,
    tcp_connect,
};
use mdaw_packs::signal::Signal;
use sha2::{Digest, Sha256};
use url::Url;

/// How late a cancel may land after its deadline: three polls, or 6× that on shared CI runners
/// (`CI` set), whose thread scheduling overshoots sleeps by tens of milliseconds.
fn late() -> Duration {
    3 * POLL
        * if std::env::var_os("CI").is_some() {
            6
        } else {
            1
        }
}

const URL: &str =
    "https://raw.githubusercontent.com/o/r/c0ffee0000000000000000000000000000000000/x.wav";

fn ok() -> Result<(), Interrupt> {
    Ok(())
}

/// Reads the request head; returns it.
fn read_head(s: &mut TcpStream) -> String {
    let mut got = Vec::new();
    let mut b = [0u8; 1024];
    while !got.windows(4).any(|w| w == b"\r\n\r\n") {
        match s.read(&mut b) {
            Ok(0) | Err(_) => break,
            Ok(n) => got.extend_from_slice(&b[..n]),
        }
    }
    String::from_utf8_lossy(&got).into_owned()
}

/// A one-shot server: accepts one connection, reads the request, runs `script`.
fn serve(script: impl FnOnce(TcpStream) + Send + 'static) -> SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    std::thread::spawn(move || {
        if let Ok((mut s, _)) = l.accept() {
            read_head(&mut s);
            script(s);
        }
    });
    addr
}

/// Serves `bytes` verbatim, then closes.
fn serve_raw(bytes: Vec<u8>) -> SocketAddr {
    serve(move |mut s| {
        let _ = s.write_all(&bytes);
    })
}

fn open(addr: SocketAddr) -> Result<Response, HttpError> {
    get(
        &FixedAddrConnector { addr },
        &Url::parse(URL).unwrap(),
        "t",
        Duration::from_secs(5),
        &ok,
    )
}

/// The whole body, or the first error.
fn body(addr: SocketAddr) -> Result<Vec<u8>, HttpError> {
    let mut r = open(addr)?;
    let mut out = Vec::new();
    let mut b = [0u8; 4096];
    loop {
        let n = r.read(&mut b, &ok)?;
        if n == 0 {
            return Ok(out);
        }
        out.extend_from_slice(&b[..n]);
    }
}

fn head(h: &str) -> Vec<u8> {
    format!("HTTP/1.1 200 OK\r\nConnection: close\r\n{h}\r\n").into_bytes()
}

fn resp(h: &str, body: &[u8]) -> Vec<u8> {
    let mut v = head(h);
    v.extend_from_slice(body);
    v
}

fn failed() -> HttpError {
    HttpError::Network("fetch failed".into())
}

fn terminated() -> HttpError {
    HttpError::Network("terminated".into())
}

// --- Finding 1: bounded memory -------------------------------------------------------------

/// Reads until the body ends or fails; the receive buffer's capacity at the end.
fn drain_capacity(addr: SocketAddr) -> (Result<usize, HttpError>, usize, Duration) {
    let t = Instant::now();
    let mut r = open(addr).unwrap();
    let mut b = [0u8; 1];
    let mut res = Ok(1);
    while matches!(res, Ok(n) if n > 0) {
        res = r.read(&mut b, &ok);
    }
    (res, r.buffered_capacity(), t.elapsed())
}

/// The reviewers' probe: a 1-byte chunked body followed by a flood of trailers, written so a
/// partial line is always left at the end of a read. Before: the consumed bytes were kept while
/// a partial line remained (31 MB of trailers → 41.9 MB buffer, unbounded). After: the trailer
/// section fails at 64 KiB and the buffer stays at one read's worth.
#[test]
fn a_trailer_flood_fails_with_a_bounded_buffer() {
    let addr = serve(|mut s| {
        let _ =
            s.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1\r\nX\r\n0\r\n");
        let all =
            b"X-Trailer: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\r\n".repeat(500_000);
        for c in all.chunks(4093) {
            if s.write_all(c).is_err() {
                return;
            }
        }
    });
    let (res, cap, took) = drain_capacity(addr);
    eprintln!("trailer flood: {res:?}, buffer capacity {cap} B, {took:?}");
    assert_eq!(res, Err(terminated()));
    assert!(cap <= 128 * 1024, "capacity {cap}");
}

/// Many short trailer lines: the line count cap (128).
#[test]
fn too_many_trailer_lines_fail() {
    let mut v = head("Transfer-Encoding: chunked\r\n");
    v.extend_from_slice(b"1\r\nX\r\n0\r\n");
    v.extend_from_slice(&b"A: b\r\n".repeat(129));
    v.extend_from_slice(b"\r\n");
    assert_eq!(body(serve_raw(v)), Err(terminated()));
    let mut v = head("Transfer-Encoding: chunked\r\n");
    v.extend_from_slice(b"1\r\nX\r\n0\r\n");
    v.extend_from_slice(&b"A: b\r\n".repeat(128));
    v.extend_from_slice(b"\r\n");
    assert_eq!(body(serve_raw(v)).unwrap(), b"X");
}

/// A chunk extension that never ends (no CRLF): fails once the line passes 4 KiB.
#[test]
fn an_endless_chunk_extension_fails_with_a_bounded_buffer() {
    let addr = serve(|mut s| {
        let _ = s.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1;ext=");
        let junk = vec![b'a'; 1 << 20];
        for _ in 0..32 {
            if s.write_all(&junk).is_err() {
                return;
            }
        }
    });
    let (res, cap, _) = drain_capacity(addr);
    eprintln!("endless chunk extension: {res:?}, buffer capacity {cap} B");
    assert_eq!(res, Err(terminated()));
    assert!(cap <= 64 * 1024, "capacity {cap}");
}

/// The line limit holds even when the CRLF is already in the buffer.
#[test]
fn a_long_chunk_line_fails_even_with_its_crlf_buffered() {
    let mut v = head("Transfer-Encoding: chunked\r\n");
    v.extend_from_slice(b"1;");
    v.extend_from_slice(&vec![b'a'; 5000]);
    v.extend_from_slice(b"\r\nX\r\n0\r\n\r\n");
    assert_eq!(body(serve_raw(v)), Err(terminated()));
    // 4096 bytes exactly is fine.
    let mut v = head("Transfer-Encoding: chunked\r\n");
    v.extend_from_slice(b"1;");
    v.extend_from_slice(&vec![b'a'; 4094]);
    v.extend_from_slice(b"\r\nX\r\n0\r\n\r\n");
    assert_eq!(body(serve_raw(v)).unwrap(), b"X");
}

/// At most 8 interim responses are skipped (Node's fetch fails on any).
#[test]
fn interim_responses_are_bounded() {
    let mut v = b"HTTP/1.1 100 Continue\r\n\r\n".repeat(8);
    v.extend(resp("Content-Length: 5\r\n", b"hello"));
    assert_eq!(body(serve_raw(v)).unwrap(), b"hello");
    let addr = serve(|mut s| {
        let one = b"HTTP/1.1 100 Continue\r\n\r\n";
        for _ in 0..100_000 {
            if s.write_all(one).is_err() {
                return;
            }
        }
    });
    assert_eq!(open(addr).err(), Some(failed()));
}

/// A large Content-Length body streams through a buffer of one read.
#[test]
fn a_large_body_streams_through_a_small_buffer() {
    let addr = serve(|mut s| {
        let _ = s.write_all(&head("Content-Length: 33554432\r\n"));
        let junk = vec![7u8; 1 << 20];
        for _ in 0..32 {
            if s.write_all(&junk).is_err() {
                return;
            }
        }
    });
    let mut r = open(addr).unwrap();
    let mut b = vec![0u8; 64 * 1024];
    let mut total = 0usize;
    let mut cap = 0usize;
    loop {
        let n = r.read(&mut b, &ok).unwrap();
        cap = cap.max(r.buffered_capacity());
        if n == 0 {
            break;
        }
        total += n;
    }
    assert_eq!(total, 32 << 20);
    assert!(cap <= 64 * 1024, "capacity {cap}");
}

// --- Finding 6: framing ---------------------------------------------------------------------

#[test]
fn content_length_must_be_one_header_of_digits() {
    for h in [
        "Content-Length: 5\r\nContent-Length: 6\r\n",
        "Content-Length: 5\r\nContent-Length: 5\r\n",
        "Content-Length: 5, 5\r\n",
        "Content-Length: +5\r\n",
        "Content-Length: -1\r\n",
        "Content-Length: 5x\r\n",
        "Content-Length: \r\n",
        "Content-Length: 0x5\r\n",
        "Content-Length: 99999999999999999999999\r\n",
    ] {
        assert_eq!(
            open(serve_raw(resp(h, b"hello!"))).err(),
            Some(failed()),
            "{h:?}"
        );
    }
    assert_eq!(
        body(serve_raw(resp("Content-Length:  5 \r\n", b"hello!"))).unwrap(),
        b"hello"
    );
}

#[test]
fn content_length_with_transfer_encoding_fails() {
    let v = resp(
        "Content-Length: 5\r\nTransfer-Encoding: chunked\r\n",
        b"5\r\nhello\r\n0\r\n\r\n",
    );
    assert_eq!(open(serve_raw(v)).err(), Some(failed()));
}

#[test]
fn only_a_final_chunked_coding_is_chunked() {
    let chunked = b"5\r\nhello\r\n0\r\n\r\n";
    for (te, want) in [
        ("chunked", &b"hello"[..]),
        ("CHUNKED", b"hello"),
        ("gzip, chunked", b"hello"),
        ("gzip,chunked ", b"hello"),
        // Not chunked: read until close, like Node.
        ("notchunked", chunked),
        ("chunked, gzip", chunked),
        ("xchunked", chunked),
    ] {
        let v = resp(&format!("Transfer-Encoding: {te}\r\n"), chunked);
        assert_eq!(body(serve_raw(v)).unwrap(), want, "{te:?}");
    }
    // Two TE headers: the last coding of the combined list counts.
    let v = resp(
        "Transfer-Encoding: gzip\r\nTransfer-Encoding: chunked\r\n",
        chunked,
    );
    assert_eq!(body(serve_raw(v)).unwrap(), b"hello");
}

#[test]
fn chunk_sizes_are_hex_digits() {
    let te = "Transfer-Encoding: chunked\r\n";
    for bad in [
        &b"+5\r\nhello\r\n0\r\n\r\n"[..],
        b" 5\r\nhello\r\n0\r\n\r\n",
        b"\r\nhello\r\n0\r\n\r\n",
        b"5x\r\nhello\r\n0\r\n\r\n",
        b"11111111111111111\r\n",
        b"5\r\nhelloXX0\r\n\r\n",
    ] {
        assert_eq!(body(serve_raw(resp(te, bad))), Err(terminated()), "{bad:?}");
    }
    for good in [
        &b"5;a=b\r\nhello\r\n0\r\nX-T: 1\r\n\r\n"[..],
        b"5 \r\nhello\r\n0\r\n\r\n",
        b"5 ;x\r\nhello\r\n0\r\n\r\n",
        b"00005\r\nhello\r\n0\r\n\r\n",
    ] {
        assert_eq!(
            body(serve_raw(resp(te, good))).unwrap(),
            b"hello",
            "{good:?}"
        );
    }
}

#[test]
fn the_request_asks_for_identity_bodies() {
    let (tx, rx) = crossbeam_channel::bounded(1);
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    std::thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        let _ = tx.send(read_head(&mut s));
        let _ = s.write_all(&resp("Content-Length: 0\r\n", b""));
    });
    body(addr).unwrap();
    let req = rx.recv().unwrap();
    assert!(req.contains("\r\nAccept-Encoding: identity\r\n"), "{req}");
}

// --- Finding 7: content encoding ------------------------------------------------------------

fn gzip(b: &[u8]) -> Vec<u8> {
    let mut e = GzEncoder::new(Vec::new(), Compression::default());
    e.write_all(b).unwrap();
    e.finish().unwrap()
}

fn zlib(b: &[u8]) -> Vec<u8> {
    let mut e = ZlibEncoder::new(Vec::new(), Compression::default());
    e.write_all(b).unwrap();
    e.finish().unwrap()
}

fn raw_deflate(b: &[u8]) -> Vec<u8> {
    let mut e = DeflateEncoder::new(Vec::new(), Compression::default());
    e.write_all(b).unwrap();
    e.finish().unwrap()
}

fn sha(b: &[u8]) -> String {
    hex(&Sha256::digest(b))
}

fn download(v: Vec<u8>, max: u64, expected: Option<&[u8]>) -> Result<Vec<u8>, DownloadError> {
    let c = FixedAddrConnector { addr: serve_raw(v) };
    let mut o = DownloadOptions::new(max, &c);
    let digest = expected.map(sha);
    o.sha256 = digest.as_deref();
    o.expected_bytes = expected.map(|e| e.len() as u64);
    download_to_buffer(URL, &o)
}

#[test]
fn compressed_bodies_are_decoded_before_the_checks() {
    let plain = b"hello world, hello world, hello world".repeat(100);
    for (ce, enc) in [
        ("gzip", gzip(&plain)),
        ("x-gzip", gzip(&plain)),
        ("GZIP", gzip(&plain)),
        ("deflate", zlib(&plain)),
        ("deflate", raw_deflate(&plain)),
        ("gzip, gzip", gzip(&gzip(&plain))),
        ("deflate, gzip", gzip(&zlib(&plain))),
    ] {
        let h = format!(
            "Content-Encoding: {ce}\r\nContent-Length: {}\r\n",
            enc.len()
        );
        // The size and sha256 checks see the decoded bytes, as in Node.
        let got = download(resp(&h, &enc), 1 << 20, Some(&plain)).unwrap();
        assert_eq!(got, plain, "{ce}");
        // Chunked transfer of the same.
        let mut chunked = format!("{:x}\r\n", enc.len()).into_bytes();
        chunked.extend_from_slice(&enc);
        chunked.extend_from_slice(b"\r\n0\r\n\r\n");
        let h = format!("Content-Encoding: {ce}\r\nTransfer-Encoding: chunked\r\n");
        assert_eq!(
            download(resp(&h, &chunked), 1 << 20, Some(&plain)).unwrap(),
            plain
        );
    }
}

#[test]
fn unknown_codings_pass_through_like_undici() {
    for ce in ["identity", "foo", "gzip, foo"] {
        let h = format!("Content-Encoding: {ce}\r\n");
        assert_eq!(
            download(resp(&h, b"hello"), 100, Some(b"hello")).unwrap(),
            b"hello",
            "{ce}"
        );
    }
}

#[test]
fn brotli_and_zstd_are_rejected_without_retry() {
    for ce in ["br", "zstd", "gzip, br"] {
        let h = format!("Content-Encoding: {ce}\r\n");
        let e = download(resp(&h, b"\x0b\x02\x80hello\x03"), 100, None).unwrap_err();
        let t = ce.rsplit(", ").next().unwrap();
        assert_eq!(
            e,
            DownloadError::Failed {
                message: format!("Unsupported content encoding: {t}"),
                retryable: false
            },
            "{ce}"
        );
    }
}

#[test]
fn a_corrupt_or_truncated_stream_is_a_retryable_terminated() {
    let e = download(
        resp("Content-Encoding: gzip\r\n", b"not gzip at all"),
        100,
        None,
    )
    .unwrap_err();
    assert_eq!(
        e,
        DownloadError::Failed {
            message: "terminated".into(),
            retryable: true
        }
    );
    let g = gzip(b"hello world");
    let e = download(
        resp("Content-Encoding: gzip\r\n", &g[..g.len() - 6]),
        100,
        None,
    )
    .unwrap_err();
    assert!(e.retryable(), "{e:?}");
}

/// A zip bomb (256 MiB of zeros, 255 KB on the wire) fails at the cap on the decoded bytes,
/// streaming: nothing near the decoded size is buffered.
#[test]
fn a_zip_bomb_fails_at_the_decoded_cap() {
    let mut e = GzEncoder::new(Vec::new(), Compression::best());
    let zeros = vec![0u8; 1 << 20];
    for _ in 0..256 {
        e.write_all(&zeros).unwrap();
    }
    let bomb = e.finish().unwrap();
    let t = Instant::now();
    let h = format!(
        "Content-Encoding: gzip\r\nContent-Length: {}\r\n",
        bomb.len()
    );
    let err = download(resp(&h, &bomb), 10 * 1024 * 1024, None).unwrap_err();
    eprintln!(
        "zip bomb ({} B on the wire): {err:?} after {:?}",
        bomb.len(),
        t.elapsed()
    );
    assert_eq!(
        err,
        DownloadError::Failed {
            message: format!("File too large (over {} bytes)", 10 * 1024 * 1024),
            retryable: false
        }
    );
}

// --- Finding 5: cancellation and deadlines ---------------------------------------------------

/// A check that aborts `after` from now, recording when it first said so.
fn cancel_after(after: Duration) -> (Instant, impl Fn() -> Result<(), Interrupt>) {
    let t0 = Instant::now();
    (t0, move || {
        if t0.elapsed() >= after {
            Err(Interrupt::Aborted)
        } else {
            Ok(())
        }
    })
}

/// Unroutable addresses (blackholed on this network: a connect waits until its timeout).
fn blackholes(n: u16) -> Vec<SocketAddr> {
    (0..n)
        .map(|i| SocketAddr::from(([10, 255, 255, 1], 443 + i)))
        .collect()
}

/// Before: `connect_timeout` per address, not cancellable (one blackholed address with a 3 s
/// timeout: cancel due at 50 ms returned after 3.0 s; 8 addresses × the 8 s catalog timeout ≈
/// 64 s). After: one deadline for all addresses, a cancel within one POLL.
#[test]
fn a_cancel_interrupts_a_connect_to_blackholed_addresses() {
    let (t0, check) = cancel_after(Duration::from_millis(50));
    let r = tcp_connect(&blackholes(8), Duration::from_secs(8), &check);
    let took = t0.elapsed();
    eprintln!(
        "blackholed connect, cancel at 50 ms: {:?} after {took:?}",
        r.as_ref().err()
    );
    assert!(r.is_err());
    assert!(took < Duration::from_millis(50) + late(), "{took:?}");
}

#[test]
fn one_deadline_bounds_all_addresses() {
    let t0 = Instant::now();
    let r = tcp_connect(&blackholes(8), Duration::from_millis(600), &ok);
    let took = t0.elapsed();
    eprintln!(
        "8 blackholed addresses, 600 ms timeout: {:?} after {took:?}",
        r.as_ref().err()
    );
    assert_eq!(r.err().map(|e| e.to_string()), Some("fetch failed".into()));
    assert!(took < Duration::from_millis(600) + late(), "{took:?}");
}

/// Happy Eyeballs: a live address after a blackholed one connects ≈250 ms in, not after the
/// first one's timeout.
#[test]
fn a_live_address_after_a_blackholed_one_connects_quickly() {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let live = l.local_addr().unwrap();
    let t0 = Instant::now();
    let mut addrs = blackholes(1);
    addrs.push(live);
    let s = tcp_connect(&addrs, Duration::from_secs(8), &ok).unwrap();
    let took = t0.elapsed();
    eprintln!("blackhole then live: connected after {took:?}");
    assert_eq!(s.peer_addr().unwrap(), live);
    assert!(took < Duration::from_millis(1000), "{took:?}");
    // A refused address is skipped at once.
    let refused = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap()
    };
    let t0 = Instant::now();
    let s = tcp_connect(&[refused, live], Duration::from_secs(8), &ok).unwrap();
    assert_eq!(s.peer_addr().unwrap(), live);
    assert!(
        t0.elapsed() < Duration::from_millis(200),
        "{:?}",
        t0.elapsed()
    );
}

fn tls_server_config() -> Arc<rustls::ServerConfig> {
    use rustls::pki_types::pem::PemObject;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    let certs: Vec<CertificateDer> =
        CertificateDer::pem_slice_iter(include_bytes!("fixtures/tls/localhost.pem"))
            .collect::<Result<_, _>>()
            .unwrap();
    let key = PrivateKeyDer::from_pem_slice(include_bytes!("fixtures/tls/localhost.key")).unwrap();
    Arc::new(
        rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .unwrap(),
    )
}

fn test_tls_connector() -> TlsConnector {
    use rustls::pki_types::CertificateDer;
    use rustls::pki_types::pem::PemObject;
    let mut roots = rustls::RootCertStore::empty();
    for c in CertificateDer::pem_slice_iter(include_bytes!("fixtures/tls/ca.pem")) {
        roots.add(c.unwrap()).unwrap();
    }
    TlsConnector::with_roots(roots)
}

/// A TLS server on `localhost`: handshake, then `respond(conn, sock)`.
fn tls_serve(
    respond: impl FnOnce(&mut rustls::ServerConnection, &mut TcpStream) + Send + 'static,
) -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let cfg = tls_server_config();
    std::thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        let mut c = rustls::ServerConnection::new(cfg).unwrap();
        while c.is_handshaking() {
            if c.complete_io(&mut s).is_err() {
                return;
            }
        }
        // The request.
        let mut got = Vec::new();
        let mut b = [0u8; 1024];
        while !got.windows(4).any(|w| w == b"\r\n\r\n") {
            if c.complete_io(&mut s).is_err() {
                return;
            }
            match c.reader().read(&mut b) {
                Ok(n) => got.extend_from_slice(&b[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => return,
            }
        }
        respond(&mut c, &mut s);
    });
    port
}

/// Sanity: a full TLS request through the local CA, via `localhost` (DNS on the helper thread,
/// Happy Eyeballs over ::1 / 127.0.0.1).
#[test]
fn a_tls_request_works_end_to_end() {
    let port = tls_serve(|c, s| {
        c.writer()
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello")
            .unwrap();
        c.send_close_notify();
        while c.wants_write() {
            let _ = c.write_tls(s);
        }
    });
    let url = Url::parse(&format!("https://localhost:{port}/x")).unwrap();
    let mut r = get(
        &test_tls_connector(),
        &url,
        "t",
        Duration::from_secs(5),
        &ok,
    )
    .unwrap();
    let mut b = [0u8; 16];
    let n = r.read(&mut b, &ok).unwrap();
    assert_eq!(&b[..n], b"hello");
    assert_eq!(r.read(&mut b, &ok).unwrap(), 0);
}

/// Before: the handshake ran in rustls' `complete_io`, which keeps reading while a record
/// trickles in (a 16 KB handshake record at 1 byte / 5 ms: cancel due at 50 ms returned after
/// 26.5 s). After: one socket read per check.
#[test]
fn a_cancel_interrupts_a_dripping_tls_handshake() {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        let mut b = [0u8; 4096];
        let _ = s.read(&mut b);
        if s.write_all(&[0x16, 0x03, 0x03, 0x40, 0x00]).is_err() {
            return;
        }
        for _ in 0..4000 {
            std::thread::sleep(Duration::from_millis(5));
            if s.write_all(&[0x02]).is_err() {
                return;
            }
        }
    });
    let (t0, check) = cancel_after(Duration::from_millis(50));
    let url = Url::parse(&format!("https://127.0.0.1:{port}/")).unwrap();
    let r = test_tls_connector().connect(&url, Duration::from_secs(5), &check);
    let took = t0.elapsed();
    eprintln!(
        "dripping TLS handshake, cancel at 50 ms: {:?} after {took:?}",
        r.as_ref().err()
    );
    assert_eq!(r.err(), Some(HttpError::Interrupted(Interrupt::Aborted)));
    assert!(took < Duration::from_millis(50) + late(), "{took:?}");
}

/// The reviewers' case: the body arrives as one 16 KB TLS record dripped 1 byte / 2 ms (seen:
/// cancel due at 50 ms returned at 736 ms). After: within a POLL.
#[test]
fn a_cancel_interrupts_a_dripping_tls_body() {
    let port = tls_serve(|c, s| {
        let body = vec![b'x'; 16_000];
        c.writer()
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 16000\r\n\r\n")
            .unwrap();
        while c.wants_write() {
            let _ = c.write_tls(s);
        }
        c.writer().write_all(&body).unwrap();
        let mut wire = Vec::new();
        while c.wants_write() {
            c.write_tls(&mut wire).unwrap();
        }
        for b in wire {
            std::thread::sleep(Duration::from_millis(2));
            if s.write_all(&[b]).is_err() {
                return;
            }
        }
    });
    let url = Url::parse(&format!("https://localhost:{port}/x")).unwrap();
    let mut r = get(
        &test_tls_connector(),
        &url,
        "t",
        Duration::from_secs(5),
        &ok,
    )
    .unwrap();
    let (t0, check) = cancel_after(Duration::from_millis(50));
    let check: Check = &check;
    let mut b = [0u8; 1024];
    let res = r.read(&mut b, check);
    let took = t0.elapsed();
    eprintln!("dripping TLS body, cancel at 50 ms: {res:?} after {took:?}");
    assert_eq!(res, Err(HttpError::Interrupted(Interrupt::Aborted)));
    assert!(took < Duration::from_millis(50) + late(), "{took:?}");
}

/// A cancelled download through `stream_body` over a plain dripping body.
#[test]
fn a_cancelled_download_returns_within_a_poll() {
    let c = FixedAddrConnector {
        addr: serve(|mut s| {
            let _ = s.write_all(&head("Content-Length: 100000\r\n"));
            for _ in 0..100_000 {
                std::thread::sleep(Duration::from_millis(1));
                if s.write_all(b"x").is_err() {
                    return;
                }
            }
        }),
    };
    let sig = Signal::new();
    let s2 = sig.clone();
    let t0 = Instant::now();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        s2.abort(mdaw_packs::AbortReason::Cancel);
    });
    let mut o = DownloadOptions::new(1 << 20, &c);
    o.signal = Some(&sig);
    let e = download_to_buffer(URL, &o).unwrap_err();
    let took = t0.elapsed();
    eprintln!("cancelled plain download: {e:?} after {took:?}");
    assert_eq!(e, DownloadError::Aborted);
    assert!(took < Duration::from_millis(50) + late(), "{took:?}");
}

/// `Connector` is object safe and the test connector is usable through it.
#[test]
fn connectors_are_object_safe() {
    let c: Box<dyn Connector> = Box::new(test_tls_connector());
    let _ = c;
}
