//! A minimal blocking HTTP/1.1 GET client over rustls (the Electron main process uses Node's
//! `fetch`).
//!
//! Why not a full client (ureq / reqwest): the download rules need a body read that can be
//! interrupted at any moment, by a cancel/pause from another thread or by the per-read idle
//! timer (`download.ts` arms it before the request and before every read). Here every socket
//! read waits at most [`POLL`], then asks the caller's check whether to give up; a cancel takes
//! effect within [`POLL`], during the TLS handshake, the header wait or the body.
//!
//! Only what the pack store needs: GET, no request body, `Connection: close` (one request per
//! connection, like a redirect hop or retry in the TS), `Content-Length` / chunked /
//! until-close bodies, no `Accept-Encoding` (identity bodies: Node's fetch asks for gzip and
//! decompresses, so the byte counts the checks see are the same).
//!
//! Connections come from a [`Connector`]: [`TlsConnector`] in the app (DNS + TCP + TLS with the
//! Mozilla roots of `webpki-roots`), a plain-TCP one in tests, which serves the allowlisted
//! `https://raw.githubusercontent.com/...` URLs from a local server.

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

use url::Url;

/// Longest a socket read blocks before the caller's check runs again.
pub const POLL: Duration = Duration::from_millis(20);
/// Cap on the response head (status line + headers).
const MAX_HEAD: usize = 64 * 1024;

/// Why the caller's check stopped a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interrupt {
    /// The outer signal was aborted (cancel, pause, a sibling's failure, a total timeout).
    Aborted,
    /// The idle timer fired.
    TimedOut,
}

/// The caller's check, run before blocking and after every [`POLL`] without data.
pub type Check<'a> = &'a dyn Fn() -> Result<(), Interrupt>;

/// Request failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpError {
    Interrupted(Interrupt),
    /// A transport failure, with the message Node's fetch gives: `fetch failed` before a
    /// response, `terminated` when the body breaks off.
    Network(String),
}

impl From<Interrupt> for HttpError {
    fn from(i: Interrupt) -> Self {
        HttpError::Interrupted(i)
    }
}

/// A connection the client can read with a bounded wait.
pub trait Conn: Read + Write + Send {
    fn set_read_timeout(&mut self, d: Option<Duration>) -> io::Result<()>;
}

impl Conn for TcpStream {
    fn set_read_timeout(&mut self, d: Option<Duration>) -> io::Result<()> {
        TcpStream::set_read_timeout(self, d)
    }
}

/// Opens connections for URLs.
pub trait Connector: Send + Sync {
    /// Connects to `url`'s host (TLS for https). `timeout` bounds the TCP connect; the
    /// handshake runs under `check`.
    fn connect(
        &self,
        url: &Url,
        timeout: Duration,
        check: Check,
    ) -> Result<Box<dyn Conn>, HttpError>;
}

fn is_wait(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
    )
}

fn fetch_failed() -> HttpError {
    HttpError::Network("fetch failed".into())
}

fn tcp_connect(addrs: &[SocketAddr], timeout: Duration) -> Result<TcpStream, HttpError> {
    let mut last = None;
    for a in addrs {
        match TcpStream::connect_timeout(a, timeout.max(Duration::from_millis(1))) {
            Ok(s) => {
                let _ = s.set_nodelay(true);
                s.set_read_timeout(Some(POLL)).map_err(|_| fetch_failed())?;
                s.set_write_timeout(Some(timeout.max(Duration::from_secs(1))))
                    .map_err(|_| fetch_failed())?;
                return Ok(s);
            }
            Err(e) => last = Some(e),
        }
    }
    let _ = last;
    Err(fetch_failed())
}

/// TLS over TCP with the `webpki-roots` trust anchors (no client auth, TLS 1.2+).
pub struct TlsConnector {
    config: Arc<rustls::ClientConfig>,
}

impl Default for TlsConnector {
    fn default() -> Self {
        Self::new()
    }
}

impl TlsConnector {
    pub fn new() -> Self {
        let roots = rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .expect("ring supports the default TLS versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
        Self {
            config: Arc::new(config),
        }
    }
}

struct TlsConn(rustls::StreamOwned<rustls::ClientConnection, TcpStream>);

impl Read for TlsConn {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl Write for TlsConn {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl Conn for TlsConn {
    fn set_read_timeout(&mut self, d: Option<Duration>) -> io::Result<()> {
        self.0.sock.set_read_timeout(d)
    }
}

impl Connector for TlsConnector {
    fn connect(
        &self,
        url: &Url,
        timeout: Duration,
        check: Check,
    ) -> Result<Box<dyn Conn>, HttpError> {
        if url.scheme() != "https" {
            return Err(fetch_failed());
        }
        let host = url.host_str().ok_or_else(fetch_failed)?.to_owned();
        let port = url.port_or_known_default().unwrap_or(443);
        check()?;
        let addrs: Vec<SocketAddr> = (host.as_str(), port)
            .to_socket_addrs()
            .map_err(|_| fetch_failed())?
            .collect();
        check()?;
        let mut sock = tcp_connect(&addrs, timeout)?;
        let name = rustls::pki_types::ServerName::try_from(host).map_err(|_| fetch_failed())?;
        let mut conn =
            rustls::ClientConnection::new(self.config.clone(), name).map_err(|_| fetch_failed())?;
        while conn.is_handshaking() {
            check()?;
            match conn.complete_io(&mut sock) {
                Ok(_) => {}
                Err(e) if is_wait(&e) => {}
                Err(_) => return Err(fetch_failed()),
            }
        }
        Ok(Box::new(TlsConn(rustls::StreamOwned::new(conn, sock))))
    }
}

/// Plain TCP to a fixed address whatever the URL says (tests: a local server stands in for
/// raw.githubusercontent.com; the allowlist still checks the https URLs).
pub struct FixedAddrConnector {
    pub addr: SocketAddr,
}

impl Connector for FixedAddrConnector {
    fn connect(
        &self,
        _url: &Url,
        timeout: Duration,
        check: Check,
    ) -> Result<Box<dyn Conn>, HttpError> {
        check()?;
        Ok(Box::new(tcp_connect(&[self.addr], timeout)?))
    }
}

#[derive(Debug)]
enum Body {
    Length(u64),
    /// Bytes left in the current chunk (0: a chunk header comes next).
    Chunked {
        left: u64,
        first: bool,
    },
    UntilClose,
    Done,
}

/// A response whose body is read with [`Response::read`].
pub struct Response {
    pub status: u16,
    headers: Vec<(String, String)>,
    conn: Box<dyn Conn>,
    /// Received but not yet consumed bytes.
    buf: Vec<u8>,
    pos: usize,
    body: Body,
    eof: bool,
}

impl std::fmt::Debug for Response {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Response")
            .field("status", &self.status)
            .finish()
    }
}

impl Response {
    /// The first header with this name (case-insensitive).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Reads more bytes from the connection into `buf`. Ok(false) at EOF.
    fn fill(&mut self, check: Check) -> Result<bool, HttpError> {
        if self.eof {
            return Ok(false);
        }
        if self.pos > 0 && self.pos == self.buf.len() {
            self.buf.clear();
            self.pos = 0;
        }
        let mut tmp = [0u8; 16 * 1024];
        loop {
            check()?;
            match self.conn.read(&mut tmp) {
                Ok(0) => {
                    self.eof = true;
                    return Ok(false);
                }
                Ok(n) => {
                    self.buf.extend_from_slice(&tmp[..n]);
                    return Ok(true);
                }
                Err(e) if is_wait(&e) => {}
                // A TLS peer that closes without close_notify: EOF for until-close bodies,
                // a broken body otherwise (decided by the caller).
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                    self.eof = true;
                    return Ok(false);
                }
                Err(_) => return Err(HttpError::Network("terminated".into())),
            }
        }
    }

    fn avail(&self) -> &[u8] {
        &self.buf[self.pos..]
    }

    /// Reads a CRLF-terminated line of the chunked framing.
    fn line(&mut self, check: Check) -> Result<String, HttpError> {
        loop {
            if let Some(i) = self.avail().windows(2).position(|w| w == b"\r\n") {
                let line = String::from_utf8_lossy(&self.avail()[..i]).into_owned();
                self.pos += i + 2;
                return Ok(line);
            }
            if self.avail().len() > 4096 || !self.fill(check)? {
                return Err(HttpError::Network("terminated".into()));
            }
        }
    }

    /// Reads up to `out.len()` body bytes; Ok(0) at the end of the body. A body cut short (EOF
    /// before its declared length or final chunk) is `Network("terminated")`.
    pub fn read(&mut self, out: &mut [u8], check: Check) -> Result<usize, HttpError> {
        loop {
            match self.body {
                Body::Done => return Ok(0),
                Body::Length(0) => {
                    self.body = Body::Done;
                    return Ok(0);
                }
                Body::Length(left) => {
                    if self.avail().is_empty() && !self.fill(check)? {
                        return Err(HttpError::Network("terminated".into()));
                    }
                    let n = (left.min(out.len() as u64) as usize).min(self.avail().len());
                    out[..n].copy_from_slice(&self.avail()[..n]);
                    self.pos += n;
                    self.body = Body::Length(left - n as u64);
                    return Ok(n);
                }
                Body::UntilClose => {
                    if self.avail().is_empty() && !self.fill(check)? {
                        self.body = Body::Done;
                        return Ok(0);
                    }
                    let n = out.len().min(self.avail().len());
                    out[..n].copy_from_slice(&self.avail()[..n]);
                    self.pos += n;
                    return Ok(n);
                }
                Body::Chunked { left: 0, first } => {
                    if !first {
                        // CRLF after the previous chunk's data.
                        let l = self.line(check)?;
                        if !l.is_empty() {
                            return Err(HttpError::Network("terminated".into()));
                        }
                    }
                    let l = self.line(check)?;
                    let size = l.split(';').next().unwrap_or("").trim();
                    let size = u64::from_str_radix(size, 16)
                        .map_err(|_| HttpError::Network("terminated".into()))?;
                    if size == 0 {
                        // Trailers until the empty line.
                        while !self.line(check)?.is_empty() {}
                        self.body = Body::Done;
                        return Ok(0);
                    }
                    self.body = Body::Chunked {
                        left: size,
                        first: false,
                    };
                }
                Body::Chunked { left, .. } => {
                    if self.avail().is_empty() && !self.fill(check)? {
                        return Err(HttpError::Network("terminated".into()));
                    }
                    let n = (left.min(out.len() as u64) as usize).min(self.avail().len());
                    out[..n].copy_from_slice(&self.avail()[..n]);
                    self.pos += n;
                    self.body = Body::Chunked {
                        left: left - n as u64,
                        first: false,
                    };
                    return Ok(n);
                }
            }
        }
    }
}

/// `GET url` with the pack store's headers. `connect_timeout` bounds the TCP connect; every
/// other wait runs under `check`.
pub fn get(
    connector: &dyn Connector,
    url: &Url,
    user_agent: &str,
    connect_timeout: Duration,
    check: Check,
) -> Result<Response, HttpError> {
    let mut conn = connector.connect(url, connect_timeout, check)?;
    let host = match (url.host_str(), url.port()) {
        (Some(h), Some(p)) => format!("{h}:{p}"),
        (Some(h), None) => h.to_owned(),
        _ => return Err(fetch_failed()),
    };
    let target = &url[url::Position::BeforePath..url::Position::AfterQuery];
    let target = if target.is_empty() { "/" } else { target };
    let req = format!(
        "GET {target} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: {user_agent}\r\nAccept: */*\r\nConnection: close\r\n\r\n"
    );
    check()?;
    conn.write_all(req.as_bytes()).map_err(|_| fetch_failed())?;
    conn.flush().map_err(|_| fetch_failed())?;
    let _ = conn.set_read_timeout(Some(POLL));
    let mut res = Response {
        status: 0,
        headers: Vec::new(),
        conn,
        buf: Vec::new(),
        pos: 0,
        body: Body::Done,
        eof: false,
    };
    loop {
        // Head: parse what we have; read more while it is partial.
        let parsed = {
            let mut hs = [httparse::EMPTY_HEADER; 128];
            let mut r = httparse::Response::new(&mut hs);
            match r.parse(&res.buf) {
                Ok(httparse::Status::Complete(n)) => {
                    let status = r.code.unwrap_or(0);
                    let headers: Vec<(String, String)> = r
                        .headers
                        .iter()
                        .map(|h| {
                            (
                                h.name.to_owned(),
                                String::from_utf8_lossy(h.value).trim().to_owned(),
                            )
                        })
                        .collect();
                    Some((n, status, headers))
                }
                Ok(httparse::Status::Partial) => None,
                Err(_) => return Err(fetch_failed()),
            }
        };
        match parsed {
            Some((n, status, headers)) if (100..200).contains(&status) => {
                // Interim response: drop it and read the final one.
                let _ = headers;
                res.buf.drain(..n);
            }
            Some((n, status, headers)) => {
                res.pos = n;
                res.status = status;
                res.headers = headers;
                break;
            }
            None => {
                if res.buf.len() > MAX_HEAD || !res.fill(check)? {
                    return Err(fetch_failed());
                }
            }
        }
    }
    let chunked = res
        .header("transfer-encoding")
        .is_some_and(|v| v.to_ascii_lowercase().contains("chunked"));
    res.body = if matches!(res.status, 204 | 304) {
        Body::Done
    } else if chunked {
        Body::Chunked {
            left: 0,
            first: true,
        }
    } else if let Some(len) = res.header("content-length") {
        match len.trim().parse::<u64>() {
            Ok(n) => Body::Length(n),
            Err(_) => return Err(fetch_failed()),
        }
    } else {
        Body::UntilClose
    };
    Ok(res)
}
