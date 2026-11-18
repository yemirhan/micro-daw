//! Remote sound previews (`src/main/packs/preview.ts`): allowlisted hosts only, at most 10 MiB
//! each, size + sha256 verified when the catalog gives them, a byte-bounded LRU (24 entries /
//! 48 MiB, at least one entry kept) and in-flight de-duplication (a second request for a URL
//! being fetched waits for that fetch).

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use mdaw_model::packs::{MAX_PREVIEW_BYTES, is_allowed_url};
use parking_lot::{Condvar, Mutex};

use crate::download::{DownloadError, DownloadOptions, download_to_buffer};
use crate::http::Connector;
use crate::signal::Signal;

/// The fetch timeout (`AbortSignal.timeout(20_000)`).
pub const PREVIEW_TIMEOUT: Duration = Duration::from_millis(20_000);

/// `PreviewCacheOptions`.
#[derive(Debug, Clone, Copy)]
pub struct PreviewCacheOptions {
    pub max_entries: usize,
    pub max_total_bytes: usize,
    pub timeout: Duration,
}

impl Default for PreviewCacheOptions {
    fn default() -> Self {
        Self {
            max_entries: 24,
            max_total_bytes: 48 * 1024 * 1024,
            timeout: PREVIEW_TIMEOUT,
        }
    }
}

type Shared = Arc<(Mutex<Option<Result<Arc<Vec<u8>>, DownloadError>>>, Condvar)>;

struct State {
    /// Least recently used first.
    cache: VecDeque<(String, Arc<Vec<u8>>)>,
    total: usize,
    pending: Vec<(String, Shared)>,
}

/// `PreviewFetcher`.
pub struct PreviewFetcher {
    opts: PreviewCacheOptions,
    connector: Arc<dyn Connector>,
    state: Mutex<State>,
}

impl PreviewFetcher {
    pub fn new(connector: Arc<dyn Connector>, opts: PreviewCacheOptions) -> Self {
        Self {
            opts,
            connector,
            state: Mutex::new(State {
                cache: VecDeque::new(),
                total: 0,
                pending: Vec::new(),
            }),
        }
    }

    /// Bytes of the file at `url` (a fresh copy per call). Blocks.
    pub fn fetch(
        &self,
        url: &str,
        size: Option<u64>,
        sha256: Option<&str>,
    ) -> Result<Vec<u8>, DownloadError> {
        if !is_allowed_url(url) {
            return Err(DownloadError::Failed {
                message: "Preview URL not allowed".into(),
                retryable: false,
            });
        }
        self.get(url, size, sha256).map(|b| b.as_ref().clone())
    }

    fn get(
        &self,
        url: &str,
        size: Option<u64>,
        sha256: Option<&str>,
    ) -> Result<Arc<Vec<u8>>, DownloadError> {
        let shared = {
            let mut st = self.state.lock();
            if let Some(i) = st.cache.iter().position(|(k, _)| k == url) {
                let hit = st.cache.remove(i).expect("present");
                let bytes = hit.1.clone();
                st.cache.push_back(hit);
                return Ok(bytes);
            }
            if let Some((_, p)) = st.pending.iter().find(|(k, _)| k == url) {
                let p = p.clone();
                drop(st);
                return wait(&p);
            }
            let p: Shared = Arc::new((Mutex::new(None), Condvar::new()));
            st.pending.push((url.to_owned(), p.clone()));
            p
        };
        let signal = Signal::with_timeout(self.opts.timeout);
        let mut d = DownloadOptions::new(MAX_PREVIEW_BYTES, &*self.connector);
        d.expected_bytes = size;
        d.sha256 = sha256;
        d.signal = Some(&signal);
        let res = download_to_buffer(url, &d).map(Arc::new);
        {
            let mut st = self.state.lock();
            if let Ok(bytes) = &res {
                put(&mut st, &self.opts, url, bytes.clone());
            }
            st.pending.retain(|(k, _)| k != url);
        }
        *shared.0.lock() = Some(res.clone());
        shared.1.notify_all();
        res
    }

    /// Cached URLs, least recently used first.
    pub fn keys(&self) -> Vec<String> {
        self.state
            .lock()
            .cache
            .iter()
            .map(|(k, _)| k.clone())
            .collect()
    }

    pub fn clear(&self) {
        let mut st = self.state.lock();
        st.cache.clear();
        st.total = 0;
    }
}

fn wait(p: &Shared) -> Result<Arc<Vec<u8>>, DownloadError> {
    let mut g = p.0.lock();
    while g.is_none() {
        p.1.wait(&mut g);
    }
    g.clone().expect("settled")
}

fn put(st: &mut State, opts: &PreviewCacheOptions, url: &str, bytes: Arc<Vec<u8>>) {
    if let Some(i) = st.cache.iter().position(|(k, _)| k == url) {
        let (_, prev) = st.cache.remove(i).expect("present");
        st.total -= prev.len();
    }
    st.total += bytes.len();
    st.cache.push_back((url.to_owned(), bytes));
    while st.cache.len() > opts.max_entries
        || (st.total > opts.max_total_bytes && st.cache.len() > 1)
    {
        if let Some((_, b)) = st.cache.pop_front() {
            st.total -= b.len();
        }
    }
}
