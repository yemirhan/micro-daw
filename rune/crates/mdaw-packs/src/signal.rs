//! `AbortController` / `AbortSignal` for worker threads: the first `abort` wins and keeps its
//! reason; [`Signal::with_timeout`] is `AbortSignal.timeout(ms)` (its reason is a timeout).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex};

/// Why a signal was aborted (`signal.reason`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AbortReason {
    /// `abort()` with no reason: a user cancel (`'Install cancelled'`).
    Cancel,
    /// `InstallPausedError`: app quit (`pauseAll`); the staging folder is kept.
    Paused,
    /// `AbortSignal.timeout()` fired (catalog / preview fetches).
    Timeout,
    /// `abort(new Error(message))`: a sibling download failed, or `Pack too large`.
    Error(String),
}

struct Inner {
    reason: Mutex<Option<AbortReason>>,
    cv: Condvar,
    flag: AtomicBool,
    deadline: Option<Instant>,
}

/// See the module docs. Cloning shares the signal.
#[derive(Clone)]
pub struct Signal(Arc<Inner>);

impl Default for Signal {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Signal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Signal").field(&self.reason()).finish()
    }
}

impl Signal {
    pub fn new() -> Self {
        Self(Arc::new(Inner {
            reason: Mutex::new(None),
            cv: Condvar::new(),
            flag: AtomicBool::new(false),
            deadline: None,
        }))
    }

    /// `AbortSignal.timeout(d)`.
    pub fn with_timeout(d: Duration) -> Self {
        Self(Arc::new(Inner {
            reason: Mutex::new(None),
            cv: Condvar::new(),
            flag: AtomicBool::new(false),
            deadline: Some(Instant::now() + d),
        }))
    }

    /// Aborts with `reason` unless already aborted (the first reason is kept).
    pub fn abort(&self, reason: AbortReason) {
        let mut r = self.0.reason.lock();
        if r.is_none() {
            *r = Some(reason);
            self.0.flag.store(true, Ordering::Release);
            self.0.cv.notify_all();
        }
    }

    /// Both handles are the same signal.
    pub fn same(&self, other: &Signal) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    pub fn aborted(&self) -> bool {
        if self.0.flag.load(Ordering::Acquire) {
            return true;
        }
        if let Some(d) = self.0.deadline
            && Instant::now() >= d
        {
            self.abort(AbortReason::Timeout);
            return true;
        }
        false
    }

    pub fn reason(&self) -> Option<AbortReason> {
        self.aborted();
        self.0.reason.lock().clone()
    }

    /// Sleeps `d` unless aborted first. True when aborted (before or during the wait).
    pub fn sleep(&self, d: Duration) -> bool {
        let end = Instant::now() + d;
        let mut r = self.0.reason.lock();
        loop {
            if r.is_some() {
                return true;
            }
            let now = Instant::now();
            if let Some(dl) = self.0.deadline
                && now >= dl
            {
                drop(r);
                self.abort(AbortReason::Timeout);
                return true;
            }
            if now >= end {
                return false;
            }
            let until = self.0.deadline.map_or(end, |dl| dl.min(end));
            self.0.cv.wait_until(&mut r, until);
        }
    }
}
