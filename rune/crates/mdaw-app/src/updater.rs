//! Phase 10: "Check for updates" without an in-place updater (plan §5, decided 2026-11-18).
//!
//! Electron's `electron-updater` asks GitHub (`owner: yemirhan`, `repo: micro-daw`,
//! `releaseType: 'release'`) for the latest release and offers it when `semver.gt(latest,
//! current)`; it then downloads and installs it on quit. The Rust app asks the GitHub REST API
//! for the same release (`/releases/latest`: the newest non-draft, non-prerelease one), compares
//! the versions with the same SemVer 2.0 precedence, and offers to open the release page in the
//! browser. No download, no install (Rune has no updater; Sparkle stays out of scope).
//!
//! States are the `updater:status` ones (`types/settings.ts`): `checking`, `available`,
//! `not-available`, `error`. `downloading` / `downloaded` never occur here.

use std::cmp::Ordering;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use mdaw_model::types::UpdateStatus;

/// `GET` of the latest published release.
pub const LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/yemirhan/micro-daw/releases/latest";
/// Where "Open release page" goes when the API's `html_url` is not a page of this repo.
pub const RELEASES_PAGE: &str = "https://github.com/yemirhan/micro-daw/releases/latest";

/// What a fetch returned: the HTTP status and body, or a network error's text.
pub type FetchResult = Result<(u16, String), String>;
/// Fetches [`LATEST_RELEASE_API`] (a thread runs it; tests inject their own).
pub type Fetcher = Arc<dyn Fn() -> FetchResult + Send + Sync>;

/// A parsed SemVer 2.0 version (`MAJOR.MINOR.PATCH[-PRE][+BUILD]`; a leading `v` / `=` is
/// accepted, like `semver.parse` in loose mode for tags).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Vec<String>,
}

/// A SemVer 2.0 identifier: non-empty, `[0-9A-Za-z-]` only.
fn valid_ident(i: &str) -> bool {
    !i.is_empty() && i.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
}

fn numeric(i: &str) -> bool {
    i.bytes().all(|c| c.is_ascii_digit())
}

impl Version {
    /// SemVer 2.0 (semver.org §2, §9, §10): three numbers without leading zeros; prerelease
    /// identifiers non-empty, `[0-9A-Za-z-]`, numeric ones without leading zeros; build
    /// metadata non-empty identifiers (ignored for precedence). A leading `v` / `=` is accepted
    /// (tags).
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let s = s
            .strip_prefix('v')
            .or_else(|| s.strip_prefix('='))
            .unwrap_or(s);
        let (s, build) = match s.split_once('+') {
            Some((v, b)) => (v, Some(b)),
            None => (s, None),
        };
        if let Some(b) = build
            && !b.split('.').all(valid_ident)
        {
            return None;
        }
        let (core, pre) = match s.split_once('-') {
            Some((c, p)) => (c, Some(p)),
            None => (s, None),
        };
        let mut parts = core.split('.');
        let num = |p: Option<&str>| -> Option<u64> {
            let p = p?;
            if p.is_empty() || !numeric(p) || (p.len() > 1 && p.starts_with('0')) {
                return None;
            }
            p.parse().ok()
        };
        let (major, minor, patch) = (num(parts.next())?, num(parts.next())?, num(parts.next())?);
        if parts.next().is_some() {
            return None;
        }
        let pre = match pre {
            None => Vec::new(),
            Some(p) => {
                let ids: Vec<String> = p.split('.').map(str::to_owned).collect();
                if ids
                    .iter()
                    .any(|i| !valid_ident(i) || (numeric(i) && i.len() > 1 && i.starts_with('0')))
                {
                    return None;
                }
                ids
            }
        };
        Some(Self {
            major,
            minor,
            patch,
            pre,
        })
    }
}

/// Numeric identifiers of any length compare by value: by length first (no leading zeros),
/// then digit by digit.
fn cmp_numeric(a: &str, b: &str) -> Ordering {
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    /// SemVer 2.0 precedence (what `semver.compare` implements): the core numbers, then a
    /// version without a prerelease ranks above one with; prerelease identifiers compare
    /// numerically when both are numeric, numeric below alphanumeric, else ASCII; a longer
    /// list wins a tie of its prefix.
    fn cmp(&self, o: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(o.major, o.minor, o.patch))
            .then_with(|| match (self.pre.is_empty(), o.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => {
                    for (a, b) in self.pre.iter().zip(&o.pre) {
                        let c = match (numeric(a), numeric(b)) {
                            (true, true) => cmp_numeric(a, b),
                            (true, false) => Ordering::Less,
                            (false, true) => Ordering::Greater,
                            (false, false) => a.cmp(b),
                        };
                        if c != Ordering::Equal {
                            return c;
                        }
                    }
                    self.pre.len().cmp(&o.pre.len())
                }
            })
    }
}

/// The check's outcome: the status, and the page "Open release page" opens.
pub fn evaluate(current: &str, fetched: FetchResult) -> (UpdateStatus, Option<String>) {
    let err = |m: String| (UpdateStatus::Error { message: m }, None);
    let (status, body) = match fetched {
        Ok(r) => r,
        Err(e) => return err(format!("Check failed: {e}")),
    };
    if status == 404 {
        // electron-updater's GitHub provider: no release to read.
        return err("No published versions on GitHub".into());
    }
    if !(200..300).contains(&status) {
        return err(format!("Check failed: HTTP {status}"));
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) else {
        return err("Check failed: unreadable release information".into());
    };
    let tag = v
        .get("tag_name")
        .and_then(|t| t.as_str())
        .unwrap_or_default();
    let Some(latest) = Version::parse(tag) else {
        return err(format!("Check failed: invalid version \"{tag}\""));
    };
    let Some(cur) = Version::parse(current) else {
        return err(format!("Check failed: invalid app version \"{current}\""));
    };
    if latest > cur {
        let page = v
            .get("html_url")
            .and_then(|u| u.as_str())
            .filter(|u| u.starts_with("https://github.com/yemirhan/micro-daw/releases/"))
            .unwrap_or(RELEASES_PAGE)
            .to_owned();
        let version = tag.trim_start_matches(['v', '=']).to_owned();
        (UpdateStatus::Available { version }, Some(page))
    } else {
        (UpdateStatus::NotAvailable, None)
    }
}

/// The whole check (DNS, connect, TLS, headers, body) ends within this.
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
/// No byte for this long (after the request went out) ends the check.
pub const CHECK_IDLE: Duration = Duration::from_secs(15);

/// The real fetch: TLS through the pack store's connector (webpki roots). Every wait inside
/// it polls a check at least every `POLL` (mdaw-packs `http.rs`), which ends the fetch after
/// [`CHECK_TIMEOUT`] in all or [`CHECK_IDLE`] without data (review finding 14).
pub fn github_fetcher() -> Fetcher {
    Arc::new(|| {
        use mdaw_packs::http::{Interrupt, TlsConnector, get};
        let url = url::Url::parse(LATEST_RELEASE_API).map_err(|e| e.to_string())?;
        let start = std::time::Instant::now();
        let progress = std::cell::Cell::new(start);
        let check = || {
            let now = std::time::Instant::now();
            if now.duration_since(start) >= CHECK_TIMEOUT {
                Err(Interrupt::Aborted)
            } else if now.duration_since(progress.get()) >= CHECK_IDLE {
                Err(Interrupt::TimedOut)
            } else {
                Ok(())
            }
        };
        let timed_out = |e: mdaw_packs::http::HttpError| -> String {
            if start.elapsed() >= CHECK_IDLE.min(CHECK_TIMEOUT) {
                "timed out".into()
            } else {
                format!("{e:?}")
            }
        };
        let mut res = get(
            &TlsConnector::new(),
            &url,
            concat!("micro-daw/", env!("CARGO_PKG_VERSION")),
            Duration::from_secs(10),
            &check,
        )
        .map_err(timed_out)?;
        let mut body = Vec::new();
        let mut buf = [0u8; 16 * 1024];
        loop {
            let n = res.read(&mut buf, &check).map_err(timed_out)?;
            if n == 0 {
                break;
            }
            progress.set(std::time::Instant::now());
            body.extend_from_slice(&buf[..n]);
            if body.len() > 4 * 1024 * 1024 {
                return Err("response too large".into());
            }
        }
        Ok((res.status, String::from_utf8_lossy(&body).into_owned()))
    })
}

/// The UI's own bound on a check (the fetch thread's deadlines plus a margin): past it the
/// check fails as "timed out" even if the thread is stuck somewhere no deadline reaches (an OS
/// DNS lookup), and "Check now" works again.
pub const UI_DEADLINE: Duration = Duration::from_secs(35);

/// `useAutoUpdater`'s state.
pub struct Updater {
    pub status: UpdateStatus,
    /// The release page of an available update.
    pub page: Option<String>,
    /// `autoCheckUpdates` ran its start-up check.
    pub checked_on_start: bool,
    fetcher: Option<Fetcher>,
    rx: Option<Receiver<FetchResult>>,
    /// When the running check is given up ([`UI_DEADLINE`] after it started).
    deadline: Option<std::time::Instant>,
    /// Runs the fetch inline (headless tests: no thread timing).
    pub sync: bool,
}

impl Default for Updater {
    fn default() -> Self {
        Self {
            status: UpdateStatus::Idle,
            page: None,
            checked_on_start: false,
            fetcher: None,
            rx: None,
            deadline: None,
            sync: false,
        }
    }
}

impl Updater {
    pub fn with_fetcher(fetcher: Fetcher, sync: bool) -> Self {
        Self {
            fetcher: Some(fetcher),
            sync,
            ..Self::default()
        }
    }

    pub fn checking(&self) -> bool {
        self.status == UpdateStatus::Checking
    }

    /// `checkForUpdates()`: `checking`, then the answer. Without a fetcher (headless snapshots
    /// and tests that inject none) nothing happens.
    pub fn check(&mut self) {
        let Some(f) = self.fetcher.clone() else {
            return;
        };
        if self.checking() {
            return;
        }
        self.status = UpdateStatus::Checking;
        self.page = None;
        if self.sync {
            self.apply(f());
            return;
        }
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(f());
        });
        self.rx = Some(rx);
        self.deadline = Some(std::time::Instant::now() + UI_DEADLINE);
    }

    /// A finished fetch, or the check given up at its deadline. True when the status changed.
    pub fn poll(&mut self, now: std::time::Instant) -> bool {
        let Some(rx) = &self.rx else {
            return false;
        };
        match rx.try_recv() {
            Ok(r) => {
                self.rx = None;
                self.deadline = None;
                self.apply(r);
                true
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                if self.deadline.is_some_and(|d| now >= d) {
                    // The thread finishes (or not) on its own; its answer is dropped.
                    self.rx = None;
                    self.deadline = None;
                    self.apply(Err("timed out".into()));
                    return true;
                }
                false
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.rx = None;
                self.apply(Err("Check failed".into()));
                true
            }
        }
    }

    fn apply(&mut self, r: FetchResult) {
        let (status, page) = evaluate(env!("CARGO_PKG_VERSION"), r);
        self.status = status;
        self.page = page;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap_or_else(|| panic!("{s}"))
    }

    #[test]
    fn semver_precedence_matches_the_spec_order() {
        // semver.org §11's example order.
        let order = [
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.beta",
            "1.0.0-beta",
            "1.0.0-beta.2",
            "1.0.0-beta.11",
            "1.0.0-rc.1",
            "1.0.0",
            "1.0.1",
            "1.1.0",
            "2.0.0",
        ];
        for w in order.windows(2) {
            assert!(v(w[0]) < v(w[1]), "{} < {}", w[0], w[1]);
        }
        assert_eq!(v("v1.4.0"), v("1.4.0"));
        assert_eq!(v("1.4.0+build.7"), v("1.4.0"));
        for bad in [
            "1.4",
            "01.4.0",
            "1.4.0.1",
            "1.4.0-",
            "x",
            "1.4.0-a..b",
            // Review finding 14: leading zeros in a numeric prerelease id, empty / malformed
            // build metadata, non-ASCII digits.
            "1.4.0-01",
            "1.4.0-rc.007",
            "1.4.0+",
            "1.4.0+a..b",
            "1.4.0+a_b",
            "1.4.0-é",
            "+1.4.0",
        ] {
            assert!(Version::parse(bad).is_none(), "{bad}");
        }
        // Valid edge cases.
        for ok in [
            "1.4.0-0",
            "1.4.0-0a",
            "1.4.0-x-y.1",
            "1.4.0+001",
            "1.4.0-rc.1+exp.sha.5114f85",
        ] {
            assert!(Version::parse(ok).is_some(), "{ok}");
        }
        // Numeric prerelease ids beyond u64 compare by value, not as text or saturated.
        assert!(v("1.0.0-99999999999999999999") < v("1.0.0-100000000000000000000"));
        assert!(v("1.0.0-18446744073709551616") > v("1.0.0-18446744073709551615"));
        assert!(v("1.0.0-9") < v("1.0.0-10"));
        assert!(
            v("1.0.0-99999999999999999999") < v("1.0.0-a"),
            "numeric below alpha"
        );
    }

    #[test]
    fn a_stuck_check_times_out_and_can_run_again() {
        let mut u = Updater::with_fetcher(
            Arc::new(|| {
                std::thread::sleep(Duration::from_secs(3600));
                Ok((200, String::new()))
            }),
            false,
        );
        u.check();
        let t0 = std::time::Instant::now();
        assert!(!u.poll(t0));
        assert_eq!(u.status, UpdateStatus::Checking);
        assert!(u.poll(t0 + UI_DEADLINE));
        assert_eq!(
            u.status,
            UpdateStatus::Error {
                message: "Check failed: timed out".into()
            }
        );
        // "Check now" works again.
        u.fetcher = Some(Arc::new(|| Ok((200, r#"{"tag_name":"v99.0.0"}"#.into()))));
        u.sync = true;
        u.check();
        assert!(matches!(u.status, UpdateStatus::Available { .. }));
    }

    #[test]
    fn evaluate_reports_like_electron_updater() {
        let rel = |tag: &str| {
            Ok((
                200,
                format!(
                    r#"{{"tag_name":"{tag}","html_url":"https://github.com/yemirhan/micro-daw/releases/tag/{tag}"}}"#
                ),
            ))
        };
        let (s, page) = evaluate("1.4.0", rel("v1.5.0"));
        assert_eq!(
            s,
            UpdateStatus::Available {
                version: "1.5.0".into()
            }
        );
        assert_eq!(
            page.as_deref(),
            Some("https://github.com/yemirhan/micro-daw/releases/tag/v1.5.0")
        );
        assert_eq!(
            evaluate("1.4.0", rel("v1.4.0")).0,
            UpdateStatus::NotAvailable
        );
        assert_eq!(
            evaluate("1.4.0", rel("1.3.9")).0,
            UpdateStatus::NotAvailable
        );
        // A prerelease of the next version is newer (gt), the same one's prerelease is not.
        assert!(matches!(
            evaluate("1.4.0", rel("1.4.1-beta.1")).0,
            UpdateStatus::Available { .. }
        ));
        assert_eq!(
            evaluate("1.4.0", rel("1.4.0-rc.1")).0,
            UpdateStatus::NotAvailable
        );
        // A foreign page is not opened.
        let (_, page) = evaluate(
            "1.0.0",
            Ok((
                200,
                r#"{"tag_name":"2.0.0","html_url":"https://evil.example/x"}"#.into(),
            )),
        );
        assert_eq!(page.as_deref(), Some(RELEASES_PAGE));
        let msg = |r| match evaluate("1.4.0", r).0 {
            UpdateStatus::Error { message } => message,
            o => panic!("{o:?}"),
        };
        assert_eq!(
            msg(Ok((404, "{}".into()))),
            "No published versions on GitHub"
        );
        assert_eq!(msg(Ok((503, String::new()))), "Check failed: HTTP 503");
        assert_eq!(msg(Err("offline".into())), "Check failed: offline");
        assert_eq!(
            msg(Ok((200, r#"{"tag_name":"latest"}"#.into()))),
            "Check failed: invalid version \"latest\""
        );
    }

    #[test]
    fn the_check_runs_on_a_thread_and_polls_in() {
        let mut u = Updater::with_fetcher(
            Arc::new(|| Ok((200, r#"{"tag_name":"v99.0.0"}"#.into()))),
            false,
        );
        u.check();
        assert_eq!(u.status, UpdateStatus::Checking);
        let t = std::time::Instant::now();
        while !u.poll(std::time::Instant::now()) {
            assert!(t.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            u.status,
            UpdateStatus::Available {
                version: "99.0.0".into()
            }
        );
        assert_eq!(u.page.as_deref(), Some(RELEASES_PAGE));
    }
}
