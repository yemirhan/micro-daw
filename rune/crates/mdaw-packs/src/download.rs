//! Allowlisted, size-capped https downloads (`src/main/packs/download.ts`).
//!
//! - Every hop (the URL and each redirect target) must pass `isAllowedUrl`: redirects are
//!   followed by hand, at most [`MAX_REDIRECTS`], so an allowed host cannot bounce us elsewhere.
//! - The size is capped from `Content-Length` up front and again while streaming.
//! - Optional exact byte count and sha256 are verified on the streamed bytes.
//! - The idle timer is re-armed before the request and before every read; when it fires the
//!   download fails with [`DownloadError::Timeout`] (retryable), never as a user cancel. An
//!   aborted outer signal gives [`DownloadError::Aborted`], or `Timeout` when its reason is a
//!   timeout (`AbortSignal.timeout`, catalog / previews).
//! - Errors carry `retryable` (network, timeout, 5xx / 429 / 408, truncated or corrupt body);
//!   see [`download_to_file_with_retry`].
//!
//! Messages are the TS ones. Transport failures use Node fetch's texts (`fetch failed`,
//! `terminated`); OS errors (disk full, file exists) use Rust's `io::Error` text, which differs
//! from Node's `ENOSPC: ...` form.

use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use mdaw_model::js;
use mdaw_model::packs::{MAX_REDIRECTS, is_allowed_url};
use sha2::{Digest, Sha256};
use url::Url;

use crate::fault::{FsFaults, FsOp};
use crate::http::{self, Connector, HttpError, Interrupt};
use crate::signal::{AbortReason, Signal};

/// `DEFAULT_IDLE_MS`.
pub const DEFAULT_IDLE: Duration = Duration::from_millis(30_000);
/// `USER_AGENT`.
pub const USER_AGENT: &str = "micro-daw-packs";

/// `DownloadError` / `DownloadTimeoutError` / `DownloadAbortedError`, plus the non-download
/// errors the TS lets through unchanged (a failing `fs.open`): `Failed { retryable: false }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadError {
    Failed {
        message: String,
        retryable: bool,
    },
    /// `Download timed out: the server stopped responding` (retryable).
    Timeout,
    /// `Download cancelled` (name `AbortError`).
    Aborted,
}

impl DownloadError {
    fn fail(message: impl Into<String>, retryable: bool) -> Self {
        DownloadError::Failed {
            message: message.into(),
            retryable,
        }
    }

    pub fn message(&self) -> String {
        match self {
            DownloadError::Failed { message, .. } => message.clone(),
            DownloadError::Timeout => "Download timed out: the server stopped responding".into(),
            DownloadError::Aborted => "Download cancelled".into(),
        }
    }

    /// Worth another attempt.
    pub fn retryable(&self) -> bool {
        match self {
            DownloadError::Failed { retryable, .. } => *retryable,
            DownloadError::Timeout => true,
            DownloadError::Aborted => false,
        }
    }

    /// `isAbortError`.
    pub fn is_abort(&self) -> bool {
        matches!(self, DownloadError::Aborted)
    }
}

impl fmt::Display for DownloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for DownloadError {}

/// `DownloadOptions`.
#[derive(Clone, Copy)]
pub struct DownloadOptions<'a> {
    pub max_bytes: u64,
    pub signal: Option<&'a Signal>,
    /// Expected lower-case hex sha256 (compared lower-cased).
    pub sha256: Option<&'a str>,
    /// Expected exact byte count.
    pub expected_bytes: Option<u64>,
    /// Bytes received so far for this request.
    pub on_progress: Option<&'a (dyn Fn(u64) + Sync)>,
    /// Default [`DEFAULT_IDLE`] (also bounds the wait for the response head).
    pub idle_timeout: Option<Duration>,
    pub connector: &'a dyn Connector,
    pub faults: Option<&'a dyn FsFaults>,
}

impl<'a> DownloadOptions<'a> {
    pub fn new(max_bytes: u64, connector: &'a dyn Connector) -> Self {
        Self {
            max_bytes,
            signal: None,
            sha256: None,
            expected_bytes: None,
            on_progress: None,
            idle_timeout: None,
            connector,
            faults: None,
        }
    }
}

/// `new URL(url).host || 'invalid URL'`.
pub fn safe_host(url: &str) -> String {
    match Url::parse(url) {
        Ok(u) => {
            let h = match (u.host_str(), u.port()) {
                (Some(h), Some(p)) => format!("{h}:{p}"),
                (Some(h), None) => h.to_owned(),
                _ => String::new(),
            };
            if h.is_empty() {
                "invalid URL".into()
            } else {
                h
            }
        }
        Err(_) => "invalid URL".into(),
    }
}

/// What went wrong inside the `try` of `streamBody`, before the `catch` maps it.
enum Fail {
    Dl(DownloadError),
    /// A non-DownloadError (network `TypeError`, a failing write): retryable `DownloadError`.
    Other(String),
    Interrupted(Interrupt),
}

impl From<HttpError> for Fail {
    fn from(e: HttpError) -> Self {
        match e {
            HttpError::Interrupted(i) => Fail::Interrupted(i),
            HttpError::Network(m) => Fail::Other(m),
        }
    }
}

impl From<Interrupt> for Fail {
    fn from(i: Interrupt) -> Self {
        Fail::Interrupted(i)
    }
}

/// `openAllowed`.
fn open_allowed(
    url: &str,
    opts: &DownloadOptions,
    check: http::Check,
) -> Result<http::Response, Fail> {
    let mut current = url.to_owned();
    let idle = opts.idle_timeout.unwrap_or(DEFAULT_IDLE);
    for _hop in 0..=MAX_REDIRECTS {
        if !is_allowed_url(&current) {
            return Err(Fail::Dl(DownloadError::fail(
                format!("Host not allowed: {}", safe_host(&current)),
                false,
            )));
        }
        let parsed = Url::parse(&current).map_err(|_| Fail::Other("fetch failed".into()))?;
        let res = http::get(opts.connector, &parsed, USER_AGENT, idle, check)?;
        if (300..400).contains(&res.status) {
            let Some(loc) = res.header("location").map(str::to_owned) else {
                return Err(Fail::Dl(DownloadError::fail(
                    format!("Redirect without location ({})", res.status),
                    false,
                )));
            };
            drop(res);
            current = parsed
                .join(&loc)
                .map_err(|_| Fail::Other("Invalid URL".into()))?
                .to_string();
            continue;
        }
        if !(200..300).contains(&res.status) {
            let s = res.status;
            return Err(Fail::Dl(DownloadError::fail(
                format!("HTTP {s} for {}", safe_host(&current)),
                s >= 500 || s == 429 || s == 408,
            )));
        }
        return Ok(res);
    }
    Err(Fail::Dl(DownloadError::fail("Too many redirects", false)))
}

/// `streamBody`: streams the body into `sink`, enforcing the cap. Returns the byte count and the
/// hex sha256.
pub fn stream_body(
    url: &str,
    opts: &DownloadOptions,
    sink: &mut dyn FnMut(&[u8]) -> io::Result<()>,
) -> Result<(u64, String), DownloadError> {
    if opts.signal.is_some_and(Signal::aborted) {
        return Err(DownloadError::Aborted);
    }
    let idle = opts.idle_timeout.unwrap_or(DEFAULT_IDLE);
    let deadline = std::cell::Cell::new(Instant::now() + idle);
    let timed_out = std::cell::Cell::new(false);
    let check = || -> Result<(), Interrupt> {
        if opts.signal.is_some_and(Signal::aborted) {
            return Err(Interrupt::Aborted);
        }
        if Instant::now() >= deadline.get() {
            timed_out.set(true);
            return Err(Interrupt::TimedOut);
        }
        Ok(())
    };
    let arm = || deadline.set(Instant::now() + idle);

    let mut run = || -> Result<(u64, String), Fail> {
        arm();
        let mut res = open_allowed(url, opts, &check)?;
        if let Some(cl) = res.header("content-length") {
            let declared = js::string_to_number(cl);
            if declared.is_finite() && declared > opts.max_bytes as f64 {
                return Err(Fail::Dl(DownloadError::fail(
                    format!("File too large ({} bytes)", js::number_to_string(declared)),
                    false,
                )));
            }
        }
        if matches!(res.status, 204 | 205) {
            return Err(Fail::Dl(DownloadError::fail("Empty response", false)));
        }
        // Node's fetch decodes `Content-Encoding`; the size and hash checks see decoded bytes.
        let codings = content_codings(res.header("content-encoding"))?;
        let mut body: Box<dyn Read + '_> = Box::new(BodyRead {
            res: &mut res,
            check: &check,
        });
        for c in codings.iter().rev() {
            body = match c {
                Coding::Gzip => Box::new(flate2::read::MultiGzDecoder::new(body)),
                Coding::Deflate => Box::new(DeflateAuto::new(body)),
            };
        }
        let mut hash = Sha256::new();
        let mut bytes: u64 = 0;
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            arm();
            let n = read_body(body.as_mut(), &mut buf)?;
            if n == 0 {
                break;
            }
            bytes += n as u64;
            if bytes > opts.max_bytes {
                return Err(Fail::Dl(DownloadError::fail(
                    format!("File too large (over {} bytes)", opts.max_bytes),
                    false,
                )));
            }
            hash.update(&buf[..n]);
            sink(&buf[..n]).map_err(|e| Fail::Other(e.to_string()))?;
            if let Some(p) = opts.on_progress {
                p(bytes);
            }
        }
        if timed_out.get() {
            return Err(Fail::Dl(DownloadError::Timeout));
        }
        if opts.signal.is_some_and(Signal::aborted) {
            return Err(Fail::Dl(DownloadError::Aborted));
        }
        if let Some(exp) = opts.expected_bytes
            && bytes != exp
        {
            return Err(Fail::Dl(DownloadError::fail(
                format!("Size mismatch ({bytes} bytes, expected {exp})"),
                true,
            )));
        }
        let sha = hex(&hash.finalize());
        if let Some(want) = opts.sha256
            && !want.is_empty()
            && want.to_lowercase() != sha
        {
            return Err(Fail::Dl(DownloadError::fail("Checksum mismatch", true)));
        }
        Ok((bytes, sha))
    };

    run().map_err(|f| {
        if let Some(sig) = opts.signal
            && sig.aborted()
        {
            return if sig.reason() == Some(AbortReason::Timeout) {
                DownloadError::Timeout
            } else {
                DownloadError::Aborted
            };
        }
        if timed_out.get() {
            return DownloadError::Timeout;
        }
        match f {
            Fail::Dl(e) => e,
            Fail::Other(m) => DownloadError::fail(m, true),
            // Only reachable when the signal / timer said so (handled above).
            Fail::Interrupted(Interrupt::TimedOut) => DownloadError::Timeout,
            Fail::Interrupted(Interrupt::Aborted) => DownloadError::Aborted,
        }
    })
}

/// A content coding Node's fetch decodes.
enum Coding {
    Gzip,
    Deflate,
}

/// `Content-Encoding` as undici reads it: `gzip` / `x-gzip` / `deflate` are decoded (several
/// codings in reverse order, at most 5); any other token (`identity` included) turns decoding
/// off and the body passes through as is. `br` / `zstd` (Node decodes them; no decoder here)
/// fail without retry.
fn content_codings(header: Option<&str>) -> Result<Vec<Coding>, Fail> {
    let Some(h) = header else {
        return Ok(vec![]);
    };
    let tokens: Vec<String> = h
        .split(',')
        .map(|t| t.trim().to_ascii_lowercase())
        .filter(|t| !t.is_empty())
        .collect();
    let mut out = Vec::new();
    for t in &tokens {
        match t.as_str() {
            "gzip" | "x-gzip" => out.push(Coding::Gzip),
            "deflate" => out.push(Coding::Deflate),
            "br" | "zstd" => {}
            _ => return Ok(vec![]),
        }
    }
    if let Some(t) = tokens.iter().find(|t| *t == "br" || *t == "zstd") {
        return Err(Fail::Dl(DownloadError::fail(
            format!("Unsupported content encoding: {t}"),
            false,
        )));
    }
    if out.len() > 5 {
        return Err(Fail::Dl(DownloadError::fail(
            "Too many content encodings",
            false,
        )));
    }
    Ok(out)
}

/// An HTTP error carried through the decoders' `io::Error`.
#[derive(Debug)]
struct Carried(HttpError);

impl fmt::Display for Carried {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for Carried {}

/// The raw body as a `Read` for the decoders.
struct BodyRead<'r, 'c> {
    res: &'r mut http::Response,
    check: http::Check<'c>,
}

impl Read for BodyRead<'_, '_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        self.res
            .read(out, self.check)
            .map_err(|e| io::Error::other(Carried(e)))
    }
}

/// One read of (decoded) body bytes: an HTTP error comes back as itself, a decoding error
/// (corrupt or truncated stream) as Node's `terminated` (retryable).
fn read_body(r: &mut dyn Read, buf: &mut [u8]) -> Result<usize, Fail> {
    loop {
        match r.read(buf) {
            Ok(n) => return Ok(n),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => {
                if let Some(c) = e.get_ref().and_then(|x| x.downcast_ref::<Carried>()) {
                    return Err(c.0.clone().into());
                }
                return Err(Fail::Other("terminated".into()));
            }
        }
    }
}

/// `deflate` like undici: zlib-wrapped when the first byte says so (`(b & 0x0f) == 8`), raw
/// deflate otherwise.
enum DeflateAuto<'a> {
    Start(Option<Box<dyn Read + 'a>>),
    Zlib(flate2::read::ZlibDecoder<Peeked<'a>>),
    Raw(flate2::read::DeflateDecoder<Peeked<'a>>),
}

/// The body with its first byte put back.
type Peeked<'a> = io::Chain<io::Cursor<[u8; 1]>, Box<dyn Read + 'a>>;

impl<'a> DeflateAuto<'a> {
    fn new(inner: Box<dyn Read + 'a>) -> Self {
        DeflateAuto::Start(Some(inner))
    }
}

impl Read for DeflateAuto<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if let DeflateAuto::Start(inner) = self {
            let mut inner = inner.take().expect("started once");
            let mut first = [0u8; 1];
            loop {
                match inner.read(&mut first) {
                    Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                    Ok(_) => break,
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                    Err(e) => return Err(e),
                }
            }
            let src = io::Cursor::new(first).chain(inner);
            *self = if first[0] & 0x0f == 0x08 {
                DeflateAuto::Zlib(flate2::read::ZlibDecoder::new(src))
            } else {
                DeflateAuto::Raw(flate2::read::DeflateDecoder::new(src))
            };
        }
        match self {
            DeflateAuto::Zlib(d) => d.read(out),
            DeflateAuto::Raw(d) => d.read(out),
            DeflateAuto::Start(_) => unreachable!(),
        }
    }
}

/// Lower-case hex.
pub fn hex(bytes: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(H[(b >> 4) as usize] as char);
        s.push(H[(b & 15) as usize] as char);
    }
    s
}

/// `downloadToFile`: `dest` is created exclusively; on any failure the partial file is removed.
pub fn download_to_file(
    url: &str,
    dest: &Path,
    opts: &DownloadOptions,
) -> Result<(u64, String), DownloadError> {
    let open = || -> io::Result<fs::File> {
        if let Some(f) = opts.faults {
            f.before(FsOp::Open, dest, None)?;
        }
        OpenOptions::new().write(true).create_new(true).open(dest)
    };
    // A failing open is thrown as-is in the TS (not a DownloadError: never retried).
    let mut file = open().map_err(|e| DownloadError::fail(e.to_string(), false))?;
    let mut result = stream_body(url, opts, &mut |chunk| file.write_all(chunk));
    // Durability (`docs/rune-port/packs-dir-protocol.md` §3): the verified bytes reach the disk
    // before the file counts as done. A failing fsync (ENOSPC, EIO) is not retried.
    if result.is_ok() {
        let synced = match opts.faults {
            Some(f) => f.before(FsOp::Sync, dest, None),
            None => Ok(()),
        }
        .and_then(|_| file.sync_all());
        if let Err(e) = synced {
            result = Err(DownloadError::fail(e.to_string(), false));
        }
    }
    if let Some(f) = opts.faults {
        f.before_close(dest);
    }
    drop(file);
    if result.is_err() {
        let _ = fs::remove_file(dest);
    }
    result
}

/// `downloadToBuffer` (catalog, previews).
pub fn download_to_buffer(url: &str, opts: &DownloadOptions) -> Result<Vec<u8>, DownloadError> {
    let mut out = Vec::new();
    stream_body(url, opts, &mut |chunk| {
        out.extend_from_slice(chunk);
        Ok(())
    })?;
    Ok(out)
}

/// `onRetry(attempt about to start, error)`.
pub type OnRetry<'a> = &'a dyn Fn(u32, &DownloadError);

/// `RetryOptions` (without `onRetry`, passed separately).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryOptions {
    /// Total attempts (default 3).
    pub attempts: u32,
    /// The delay before attempt n+1 is `base_delay · 3^(n-1)` (default 500 ms → 500, 1500).
    pub base_delay: Duration,
}

impl Default for RetryOptions {
    fn default() -> Self {
        Self {
            attempts: 3,
            base_delay: Duration::from_millis(500),
        }
    }
}

/// `downloadToFileWithRetry`: retries retryable errors (unless the signal is aborted) with
/// backoff; `on_retry(attempt about to start, error)` runs before each wait.
pub fn download_to_file_with_retry(
    url: &str,
    dest: &Path,
    opts: &DownloadOptions,
    retry: RetryOptions,
    on_retry: Option<OnRetry>,
) -> Result<(u64, String), DownloadError> {
    let attempts = retry.attempts.max(1);
    let mut attempt = 1u32;
    loop {
        match download_to_file(url, dest, opts) {
            Ok(r) => return Ok(r),
            Err(err) => {
                let retryable = err.retryable() && !opts.signal.is_some_and(Signal::aborted);
                if !retryable || attempt >= attempts {
                    return Err(err);
                }
                if let Some(f) = on_retry {
                    f(attempt + 1, &err);
                }
                let delay = retry.base_delay * 3u32.pow(attempt - 1);
                match opts.signal {
                    Some(s) => {
                        if s.sleep(delay) {
                            return Err(DownloadError::Aborted);
                        }
                    }
                    None => std::thread::sleep(delay),
                }
                attempt += 1;
            }
        }
    }
}
