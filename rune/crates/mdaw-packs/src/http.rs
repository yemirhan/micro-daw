//! A minimal blocking HTTP/1.1 GET client over rustls (the Electron main process uses Node's
//! `fetch`).
//!
//! Why not a full client (ureq / reqwest): the download rules need every wait to be
//! interruptible, by a cancel/pause from another thread or by the per-read idle timer
//! (`download.ts` arms it before the request and before every read). Here no call blocks for
//! longer than [`POLL`] without asking the caller's check whether to give up:
//!
//! - **DNS** runs on a helper thread; the caller polls its result and returns on a cancel (the
//!   OS lookup itself cannot be interrupted, the helper thread finishes it in the background).
//! - **TCP connect** is non-blocking (`poll(2)` on Unix) with one deadline for all addresses
//!   and a simple Happy Eyeballs: the next address starts after [`HAPPY_EYEBALLS_DELAY`] (or at
//!   once when an attempt fails), the first to connect wins. Windows: a sequential
//!   `connect_timeout` per address bounded by the remaining deadline (not cancellable mid-attempt).
//! - **TLS** is driven one socket read at a time (`read_tls` → `process_new_packets` → plaintext),
//!   so a server dripping a record byte by byte cannot keep rustls reading past a cancel.
//!
//! Only what the pack store needs: GET, no request body, `Connection: close` (one request per
//! connection, like a redirect hop or retry in the TS), `Content-Length` / chunked /
//! until-close bodies. `Accept-Encoding: identity` is sent; a server that compresses anyway is
//! decoded by `download.rs` (like Node's fetch).
//!
//! Framing follows Node's fetch (undici / llhttp, checked on the same raw responses): more than
//! one `Content-Length`, a non-digit length (`+5`, `-1`, `5x`, empty) or `Content-Length` with
//! `Transfer-Encoding` fail as `fetch failed`; the body is chunked only when the last
//! transfer coding is `chunked` (case-insensitive), any other `Transfer-Encoding` is read until
//! close. Memory is bounded: response head ≤ 64 KiB, a chunk-size line ≤ [`MAX_LINE`], trailers
//! ≤ [`MAX_TRAILER_BYTES`] / [`MAX_TRAILER_LINES`], at most [`MAX_INTERIM`] interim (1xx)
//! responses, and consumed bytes are dropped before every socket read.
//!
//! Connections come from a [`Connector`]: [`TlsConnector`] in the app (DNS + TCP + TLS with the
//! Mozilla roots of `webpki-roots`), a plain-TCP one in tests, which serves the allowlisted
//! `https://raw.githubusercontent.com/...` URLs from a local server.

use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::{Duration, Instant};

use url::{Host, Url};

/// Longest a socket read blocks before the caller's check runs again.
pub const POLL: Duration = Duration::from_millis(20);
/// Happy Eyeballs: delay before the next address is tried while earlier attempts are pending.
pub const HAPPY_EYEBALLS_DELAY: Duration = Duration::from_millis(250);
/// Cap on the response head (status line + headers).
const MAX_HEAD: usize = 64 * 1024;
/// Cap on one line of the chunked framing (chunk size + extensions, a trailer line).
pub const MAX_LINE: usize = 4096;
/// Cap on the whole trailer section of a chunked body.
pub const MAX_TRAILER_BYTES: usize = 64 * 1024;
/// Cap on the number of trailer lines.
pub const MAX_TRAILER_LINES: usize = 128;
/// Interim (1xx) responses skipped before the final one; more fail the request.
pub const MAX_INTERIM: usize = 8;
/// Bytes read from the socket at a time.
const READ_CHUNK: usize = 16 * 1024;

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

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpError::Interrupted(i) => write!(f, "interrupted ({i:?})"),
            HttpError::Network(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for HttpError {}

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
    /// Connects to `url`'s host (TLS for https). `timeout` bounds DNS + TCP connect together;
    /// every wait (DNS, connect, handshake) runs under `check`.
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

fn terminated() -> HttpError {
    HttpError::Network("terminated".into())
}

/// Resolves `host:port`: an IP literal directly, a name on a helper thread whose result is
/// polled under `check` (a cancel returns at once; the lookup finishes in the background).
pub fn resolve(host: &Host<&str>, port: u16, check: Check) -> Result<Vec<SocketAddr>, HttpError> {
    let name = match host {
        Host::Ipv4(ip) => return Ok(vec![SocketAddr::new(IpAddr::V4(*ip), port)]),
        Host::Ipv6(ip) => return Ok(vec![SocketAddr::new(IpAddr::V6(*ip), port)]),
        Host::Domain(d) => (*d).to_owned(),
    };
    check()?;
    let (tx, rx) = crossbeam_channel::bounded(1);
    std::thread::Builder::new()
        .name("packs-dns".into())
        .spawn(move || {
            let r = (name.as_str(), port)
                .to_socket_addrs()
                .map(|it| it.collect::<Vec<_>>());
            let _ = tx.send(r);
        })
        .map_err(|_| fetch_failed())?;
    loop {
        check()?;
        match rx.recv_timeout(POLL) {
            Ok(Ok(a)) if !a.is_empty() => return Ok(a),
            Ok(_) => return Err(fetch_failed()),
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(_) => return Err(fetch_failed()),
        }
    }
}

/// RFC 8305 order: alternate address families, starting with the first one returned.
fn interleave(addrs: &[SocketAddr]) -> Vec<SocketAddr> {
    let Some(first) = addrs.first() else {
        return vec![];
    };
    let (mut a, mut b): (Vec<_>, Vec<_>) = addrs
        .iter()
        .copied()
        .partition(|x| x.is_ipv4() == first.is_ipv4());
    a.reverse();
    b.reverse();
    let mut out = Vec::with_capacity(addrs.len());
    while !a.is_empty() || !b.is_empty() {
        out.extend(a.pop());
        out.extend(b.pop());
    }
    out
}

fn configure(s: TcpStream, timeout: Duration) -> Result<TcpStream, HttpError> {
    let _ = s.set_nodelay(true);
    s.set_read_timeout(Some(POLL)).map_err(|_| fetch_failed())?;
    s.set_write_timeout(Some(timeout.max(Duration::from_secs(1))))
        .map_err(|_| fetch_failed())?;
    Ok(s)
}

/// TCP connect to the first of `addrs` that answers, all within `timeout`, every wait at most
/// [`POLL`] between two `check`s (see the module docs: Happy Eyeballs on Unix).
pub fn tcp_connect(
    addrs: &[SocketAddr],
    timeout: Duration,
    check: Check,
) -> Result<TcpStream, HttpError> {
    let deadline = Instant::now() + timeout.max(Duration::from_millis(1));
    let order = interleave(addrs);
    let s = connect_any(&order, deadline, check)?;
    configure(s, timeout)
}

#[cfg(unix)]
fn connect_any(
    order: &[SocketAddr],
    deadline: Instant,
    check: Check,
) -> Result<TcpStream, HttpError> {
    use std::os::fd::OwnedFd;
    let mut next = 0usize;
    let mut next_start = Instant::now();
    let mut pending: Vec<OwnedFd> = Vec::new();
    loop {
        check()?;
        let now = Instant::now();
        if now >= deadline {
            return Err(fetch_failed());
        }
        if next < order.len() && (pending.is_empty() || now >= next_start) {
            let a = order[next];
            next += 1;
            next_start = now + HAPPY_EYEBALLS_DELAY;
            match nb::start(&a) {
                Ok((fd, true)) => return nb::finish(fd).map_err(|_| fetch_failed()),
                Ok((fd, false)) => pending.push(fd),
                Err(_) => next_start = now,
            }
            continue;
        }
        if pending.is_empty() {
            return Err(fetch_failed());
        }
        let mut wait = POLL.min(deadline - now);
        if next < order.len() {
            wait = wait.min(next_start.saturating_duration_since(now));
        }
        let ready = nb::poll_writable(&pending, wait).map_err(|_| fetch_failed())?;
        for i in (0..pending.len()).rev() {
            if !ready[i] {
                continue;
            }
            let fd = pending.remove(i);
            if nb::so_error(&fd).is_ok() {
                return nb::finish(fd).map_err(|_| fetch_failed());
            }
            // Failed: try the next address now.
            next_start = Instant::now();
        }
    }
}

#[cfg(not(unix))]
fn connect_any(
    order: &[SocketAddr],
    deadline: Instant,
    check: Check,
) -> Result<TcpStream, HttpError> {
    for a in order {
        check()?;
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        if let Ok(s) = TcpStream::connect_timeout(a, left) {
            return Ok(s);
        }
    }
    Err(fetch_failed())
}

/// Non-blocking connect with libc (std has no interruptible connect).
#[cfg(unix)]
mod nb {
    use std::io;
    use std::mem::{size_of, zeroed};
    use std::net::{SocketAddr, TcpStream};
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::time::Duration;

    fn cvt(r: libc::c_int) -> io::Result<libc::c_int> {
        if r < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(r)
        }
    }

    fn sockaddr(a: &SocketAddr) -> (libc::sockaddr_storage, libc::socklen_t) {
        // SAFETY: an all-zero sockaddr_storage is valid; the casts below write a sockaddr_in /
        // sockaddr_in6, both smaller than and aligned within sockaddr_storage.
        let mut st: libc::sockaddr_storage = unsafe { zeroed() };
        match a {
            SocketAddr::V4(v4) => {
                let sin = unsafe { &mut *(&mut st as *mut _ as *mut libc::sockaddr_in) };
                sin.sin_family = libc::AF_INET as libc::sa_family_t;
                sin.sin_port = v4.port().to_be();
                sin.sin_addr = libc::in_addr {
                    s_addr: u32::from_ne_bytes(v4.ip().octets()),
                };
                #[cfg(any(
                    target_os = "macos",
                    target_os = "ios",
                    target_os = "freebsd",
                    target_os = "openbsd",
                    target_os = "netbsd",
                    target_os = "dragonfly"
                ))]
                {
                    sin.sin_len = size_of::<libc::sockaddr_in>() as u8;
                }
                (st, size_of::<libc::sockaddr_in>() as libc::socklen_t)
            }
            SocketAddr::V6(v6) => {
                let sin6 = unsafe { &mut *(&mut st as *mut _ as *mut libc::sockaddr_in6) };
                sin6.sin6_family = libc::AF_INET6 as libc::sa_family_t;
                sin6.sin6_port = v6.port().to_be();
                sin6.sin6_flowinfo = v6.flowinfo();
                sin6.sin6_addr = libc::in6_addr {
                    s6_addr: v6.ip().octets(),
                };
                sin6.sin6_scope_id = v6.scope_id();
                #[cfg(any(
                    target_os = "macos",
                    target_os = "ios",
                    target_os = "freebsd",
                    target_os = "openbsd",
                    target_os = "netbsd",
                    target_os = "dragonfly"
                ))]
                {
                    sin6.sin6_len = size_of::<libc::sockaddr_in6>() as u8;
                }
                (st, size_of::<libc::sockaddr_in6>() as libc::socklen_t)
            }
        }
    }

    /// Starts a non-blocking connect. `(fd, true)` when it connected at once.
    pub fn start(a: &SocketAddr) -> io::Result<(OwnedFd, bool)> {
        let domain = if a.is_ipv4() {
            libc::AF_INET
        } else {
            libc::AF_INET6
        };
        // SAFETY: plain libc calls on a descriptor this function owns.
        let raw = cvt(unsafe { libc::socket(domain, libc::SOCK_STREAM, 0) })?;
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        unsafe {
            cvt(libc::fcntl(raw, libc::F_SETFD, libc::FD_CLOEXEC))?;
            let fl = cvt(libc::fcntl(raw, libc::F_GETFL))?;
            cvt(libc::fcntl(raw, libc::F_SETFL, fl | libc::O_NONBLOCK))?;
        }
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            // std sets this on its own sockets: a write to a closed peer must not raise SIGPIPE.
            let one: libc::c_int = 1;
            unsafe {
                cvt(libc::setsockopt(
                    raw,
                    libc::SOL_SOCKET,
                    libc::SO_NOSIGPIPE,
                    &one as *const _ as *const libc::c_void,
                    size_of::<libc::c_int>() as libc::socklen_t,
                ))?;
            }
        }
        let (st, len) = sockaddr(a);
        let r = unsafe { libc::connect(raw, &st as *const _ as *const libc::sockaddr, len) };
        if r == 0 {
            return Ok((fd, true));
        }
        let e = io::Error::last_os_error();
        if e.raw_os_error() == Some(libc::EINPROGRESS) {
            Ok((fd, false))
        } else {
            Err(e)
        }
    }

    /// Waits at most `wait` for any of `fds` to become writable (connected or failed).
    pub fn poll_writable(fds: &[OwnedFd], wait: Duration) -> io::Result<Vec<bool>> {
        let mut p: Vec<libc::pollfd> = fds
            .iter()
            .map(|f| libc::pollfd {
                fd: f.as_raw_fd(),
                events: libc::POLLOUT,
                revents: 0,
            })
            .collect();
        let ms = wait.as_millis().min(i32::MAX as u128) as libc::c_int;
        // SAFETY: `p` is a valid array of `p.len()` pollfds.
        let r = unsafe { libc::poll(p.as_mut_ptr(), p.len() as libc::nfds_t, ms) };
        if r < 0 {
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::Interrupted {
                return Ok(vec![false; fds.len()]);
            }
            return Err(e);
        }
        Ok(p.iter().map(|x| x.revents != 0).collect())
    }

    /// The connect's outcome (`SO_ERROR`).
    pub fn so_error(fd: &OwnedFd) -> io::Result<()> {
        let mut err: libc::c_int = 0;
        let mut len = size_of::<libc::c_int>() as libc::socklen_t;
        // SAFETY: `err` / `len` are valid for getsockopt's out parameters.
        cvt(unsafe {
            libc::getsockopt(
                fd.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_ERROR,
                &mut err as *mut _ as *mut libc::c_void,
                &mut len,
            )
        })?;
        if err != 0 {
            return Err(io::Error::from_raw_os_error(err));
        }
        Ok(())
    }

    /// Back to blocking mode, as a std stream.
    pub fn finish(fd: OwnedFd) -> io::Result<TcpStream> {
        let raw = fd.as_raw_fd();
        // SAFETY: fcntl on a descriptor we own.
        unsafe {
            let fl = cvt(libc::fcntl(raw, libc::F_GETFL))?;
            cvt(libc::fcntl(raw, libc::F_SETFL, fl & !libc::O_NONBLOCK))?;
        }
        Ok(TcpStream::from(fd))
    }
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
        Self::with_roots(rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        })
    }

    /// With other trust anchors (tests: a local CA).
    pub fn with_roots(roots: rustls::RootCertStore) -> Self {
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

/// A TLS connection driven one socket read at a time (see the module docs).
struct TlsConn {
    tls: rustls::ClientConnection,
    sock: TcpStream,
}

impl TlsConn {
    /// One socket read (at most [`POLL`]) fed to rustls. Ok(false) at the socket's EOF; a
    /// `WouldBlock` / `TimedOut` error when nothing arrived.
    fn read_tls_once(&mut self) -> io::Result<bool> {
        let n = self.tls.read_tls(&mut self.sock)?;
        if n == 0 {
            return Ok(false);
        }
        self.tls
            .process_new_packets()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(true)
    }

    fn write_pending(&mut self) -> io::Result<()> {
        while self.tls.wants_write() {
            self.tls.write_tls(&mut self.sock)?;
        }
        Ok(())
    }

    fn handshake(&mut self, check: Check) -> Result<(), HttpError> {
        while self.tls.is_handshaking() {
            check()?;
            if self.tls.wants_write() {
                self.write_pending().map_err(|_| fetch_failed())?;
                continue;
            }
            match self.read_tls_once() {
                Ok(true) => {}
                Ok(false) => return Err(fetch_failed()),
                Err(e) if is_wait(&e) => {}
                Err(_) => return Err(fetch_failed()),
            }
        }
        self.write_pending().map_err(|_| fetch_failed())
    }
}

impl Read for TlsConn {
    /// Plaintext already decrypted, or one bounded socket read; `WouldBlock` when that read
    /// completed no record yet (the caller checks and calls again).
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.tls.reader().read(buf) {
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
            r => return r,
        }
        let got = self.read_tls_once()?;
        self.write_pending()?;
        match self.tls.reader().read(buf) {
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => Err(if got {
                io::ErrorKind::WouldBlock.into()
            } else {
                io::ErrorKind::UnexpectedEof.into()
            }),
            r => r,
        }
    }
}

impl Write for TlsConn {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.tls.writer().write(buf)?;
        self.write_pending()?;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.tls.writer().flush()?;
        self.write_pending()
    }
}

impl Conn for TlsConn {
    fn set_read_timeout(&mut self, d: Option<Duration>) -> io::Result<()> {
        self.sock.set_read_timeout(d)
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
        let host = url.host().ok_or_else(fetch_failed)?;
        let port = url.port_or_known_default().unwrap_or(443);
        let deadline = Instant::now() + timeout;
        let addrs = resolve(&host, port, check)?;
        let left = deadline.saturating_duration_since(Instant::now());
        let sock = tcp_connect(&addrs, left, check)?;
        let name = match host {
            Host::Domain(d) => rustls::pki_types::ServerName::try_from(d.to_owned()),
            Host::Ipv4(ip) => Ok(rustls::pki_types::ServerName::from(IpAddr::V4(ip))),
            Host::Ipv6(ip) => Ok(rustls::pki_types::ServerName::from(IpAddr::V6(ip))),
        }
        .map_err(|_| fetch_failed())?;
        let tls =
            rustls::ClientConnection::new(self.config.clone(), name).map_err(|_| fetch_failed())?;
        let mut conn = TlsConn { tls, sock };
        conn.handshake(check)?;
        Ok(Box::new(conn))
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
        Ok(Box::new(tcp_connect(&[self.addr], timeout, check)?))
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
    /// Received bytes; `buf[..pos]` are consumed (dropped before the next socket read).
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

    fn header_values<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.headers
            .iter()
            .filter(move |(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Tests: capacity of the receive buffer (bounded-memory checks).
    #[doc(hidden)]
    pub fn buffered_capacity(&self) -> usize {
        self.buf.capacity()
    }

    /// Reads more bytes from the connection into `buf` (after dropping the consumed ones).
    /// Ok(false) at EOF.
    fn fill(&mut self, check: Check) -> Result<bool, HttpError> {
        if self.eof {
            return Ok(false);
        }
        if self.pos > 0 {
            self.buf.drain(..self.pos);
            self.pos = 0;
        }
        let mut tmp = [0u8; READ_CHUNK];
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
                Err(_) => return Err(terminated()),
            }
        }
    }

    fn avail(&self) -> &[u8] {
        &self.buf[self.pos..]
    }

    /// Reads a CRLF-terminated line of the chunked framing, at most `max` bytes before the CRLF
    /// (checked whether or not the CRLF is already buffered).
    fn line(&mut self, check: Check, max: usize) -> Result<Vec<u8>, HttpError> {
        loop {
            let a = self.avail();
            let scan = &a[..a.len().min(max + 2)];
            if let Some(i) = scan.windows(2).position(|w| w == b"\r\n") {
                let line = a[..i].to_vec();
                self.pos += i + 2;
                return Ok(line);
            }
            if a.len() >= max + 2 || !self.fill(check)? {
                return Err(terminated());
            }
        }
    }

    /// The chunk-size line: hex digits, then optional whitespace / `;extensions` (llhttp).
    fn chunk_size(&mut self, check: Check) -> Result<u64, HttpError> {
        let l = self.line(check, MAX_LINE)?;
        let end = l
            .iter()
            .position(|&b| b == b';' || b == b' ' || b == b'\t')
            .unwrap_or(l.len());
        let (digits, rest) = l.split_at(end);
        let rest_ok = rest
            .iter()
            .position(|&b| b != b' ' && b != b'\t')
            .is_none_or(|i| rest[i] == b';');
        if digits.is_empty() || digits.len() > 16 || !rest_ok {
            return Err(terminated());
        }
        let mut n = 0u64;
        for &d in digits {
            let v = (d as char).to_digit(16).ok_or_else(terminated)?;
            n = (n << 4) | v as u64;
        }
        Ok(n)
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
                        return Err(terminated());
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
                        if !self.line(check, 0)?.is_empty() {
                            return Err(terminated());
                        }
                    }
                    let size = self.chunk_size(check)?;
                    if size == 0 {
                        // Trailers until the empty line, bounded.
                        let (mut lines, mut bytes) = (0usize, 0usize);
                        loop {
                            let l = self.line(check, MAX_LINE)?;
                            if l.is_empty() {
                                break;
                            }
                            lines += 1;
                            bytes += l.len() + 2;
                            if lines > MAX_TRAILER_LINES || bytes > MAX_TRAILER_BYTES {
                                return Err(terminated());
                            }
                        }
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
                        return Err(terminated());
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

    /// The body framing from the final response's head (see the module docs).
    fn framing(&self) -> Result<Body, HttpError> {
        let cls: Vec<&str> = self.header_values("content-length").collect();
        let te: Vec<&str> = self.header_values("transfer-encoding").collect();
        if cls.len() > 1 || (!cls.is_empty() && !te.is_empty()) {
            return Err(fetch_failed());
        }
        let length = match cls.first() {
            Some(v) => {
                if v.is_empty() || !v.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(fetch_failed());
                }
                Some(v.parse::<u64>().map_err(|_| fetch_failed())?)
            }
            None => None,
        };
        if matches!(self.status, 204 | 304) {
            return Ok(Body::Done);
        }
        if !te.is_empty() {
            let last = te
                .iter()
                .flat_map(|v| v.split(','))
                .map(str::trim)
                .rfind(|t| !t.is_empty());
            return Ok(if last.is_some_and(|t| t.eq_ignore_ascii_case("chunked")) {
                Body::Chunked {
                    left: 0,
                    first: true,
                }
            } else {
                Body::UntilClose
            });
        }
        Ok(match length {
            Some(n) => Body::Length(n),
            None => Body::UntilClose,
        })
    }
}

/// `GET url` with the pack store's headers. `connect_timeout` bounds DNS + TCP connect; every
/// wait runs under `check`.
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
        "GET {target} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: {user_agent}\r\nAccept: */*\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n"
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
    let mut interim = 0usize;
    loop {
        // Head: parse what we have; read more while it is partial.
        let parsed = {
            let mut hs = [httparse::EMPTY_HEADER; 128];
            let mut r = httparse::Response::new(&mut hs);
            match r.parse(res.avail()) {
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
            Some((n, status, _)) if (100..200).contains(&status) => {
                // Interim response: drop it and read the final one (bounded).
                interim += 1;
                if interim > MAX_INTERIM {
                    return Err(fetch_failed());
                }
                res.pos += n;
            }
            Some((n, status, headers)) => {
                res.pos += n;
                res.status = status;
                res.headers = headers;
                break;
            }
            None => {
                if res.avail().len() > MAX_HEAD || !res.fill(check)? {
                    return Err(fetch_failed());
                }
            }
        }
    }
    res.body = res.framing()?;
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interleave_alternates_families() {
        let a4 = |n: u8| SocketAddr::from(([10, 0, 0, n], 1));
        let a6 = |n: u16| SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, n], 1));
        assert_eq!(
            interleave(&[a6(1), a6(2), a4(1), a4(2), a4(3)]),
            vec![a6(1), a4(1), a6(2), a4(2), a4(3)]
        );
        assert_eq!(interleave(&[a4(1), a4(2)]), vec![a4(1), a4(2)]);
    }
}
