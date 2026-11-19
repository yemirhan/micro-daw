//! Phase 10 review finding 13: project and recovery file IO off the UI thread.
//!
//! The UI thread builds an immutable snapshot (the project JSON, a WAV's bytes, a path to read)
//! and submits a job; one worker thread runs the jobs in order and hands the results back,
//! which [`crate::App`] applies in its frame (`shell_poll`). Headless runs (tests, snapshots)
//! run each job inline, so a test sees its result at once, in the same order.

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::{Duration, Instant};

/// A finished job: what the UI thread applies.
pub type Done = Box<dyn FnOnce(&mut crate::App) + Send>;
type Job = Box<dyn FnOnce() -> Done + Send>;

pub struct IoWorker {
    sync: bool,
    jobs: Option<Sender<Job>>,
    done_tx: Sender<Done>,
    done_rx: Receiver<Done>,
    /// Jobs submitted and not applied yet.
    pub inflight: usize,
    waker: Option<rune::Waker>,
}

impl Default for IoWorker {
    fn default() -> Self {
        Self::new(true)
    }
}

impl IoWorker {
    /// `sync`: jobs run inline at submit (headless).
    pub fn new(sync: bool) -> Self {
        let (done_tx, done_rx) = channel();
        Self {
            sync,
            jobs: None,
            done_tx,
            done_rx,
            inflight: 0,
            waker: None,
        }
    }

    pub fn is_sync(&self) -> bool {
        self.sync
    }

    pub fn set_waker(&mut self, w: rune::Waker) {
        self.waker = Some(w);
    }

    fn sender(&mut self) -> &Sender<Job> {
        if self.jobs.is_none() {
            let (tx, rx) = channel::<Job>();
            let done = self.done_tx.clone();
            let waker = self.waker.clone();
            let _ = std::thread::Builder::new()
                .name("mdaw-project-io".into())
                .spawn(move || {
                    while let Ok(job) = rx.recv() {
                        let _ = done.send(job());
                        if let Some(w) = &waker {
                            w();
                        }
                    }
                });
            self.jobs = Some(tx);
        }
        self.jobs.as_ref().expect("just made")
    }

    /// Runs `job` (inline when sync) and queues what it returns for [`IoWorker::take`].
    pub fn submit(&mut self, job: impl FnOnce() -> Done + Send + 'static) {
        self.inflight += 1;
        if self.sync {
            let _ = self.done_tx.send(job());
            return;
        }
        let job: Job = Box::new(job);
        if let Err(e) = self.sender().send(job) {
            // The worker is gone (it panicked): run it here rather than lose it.
            let _ = self.done_tx.send((e.0)());
        }
    }

    /// The finished jobs, in order.
    pub fn take(&mut self) -> Vec<Done> {
        let mut out = Vec::new();
        while let Ok(d) = self.done_rx.try_recv() {
            out.push(d);
        }
        self.inflight = self.inflight.saturating_sub(out.len());
        out
    }

    /// Waits (at most `timeout`) for every submitted job and returns them all (quit).
    pub fn drain(&mut self, timeout: Duration) -> Vec<Done> {
        let deadline = Instant::now() + timeout;
        let mut out = Vec::new();
        while out.len() < self.inflight {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.done_rx.recv_timeout(left) {
                Ok(d) => out.push(d),
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => break,
            }
        }
        self.inflight = self.inflight.saturating_sub(out.len());
        out
    }
}

impl crate::App {
    /// Applies finished IO jobs. True when something was applied.
    pub fn io_poll(&mut self) -> bool {
        let done = self.io.take();
        let any = !done.is_empty();
        for d in done {
            d(self);
        }
        any
    }

    /// Submits `job` and, when the worker is inline, applies its result now (so the
    /// headless flows complete within the call, as before).
    pub fn io_submit(&mut self, job: impl FnOnce() -> Done + Send + 'static) {
        self.io.submit(job);
        if self.io.is_sync() {
            self.io_poll();
        }
    }
}
