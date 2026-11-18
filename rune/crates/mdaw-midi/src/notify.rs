//! MIDI setup-change notifications and CoreMIDI run-loop ownership.
//!
//! **Why a run-loop thread (macOS).** CoreMIDI delivers its setup notifications, and refreshes
//! the process's view of the sources / destinations made by *other* processes and by hardware,
//! on the run loop of the thread that created the process's **first** MIDI client. A client
//! created on a thread that never runs its run loop (a test thread, a watcher thread, or the UI
//! thread before the event loop starts) leaves the port list frozen for external changes (the
//! phase-5 review's external-process probe saw no hot-plug that way). [`ensure_started`]
//! therefore creates the first client on a dedicated `midi-runloop` thread that runs its
//! `CFRunLoop` for the life of the process, and turns its notifications into [`SetupEvents`]:
//! a change counter, a counter of removed sources, and subscriber callbacks (the hot-plug
//! watchers, which wake the UI). [`crate::MidiHost::enable`] calls it before creating any midir
//! client; the app should call it first thing at startup.
//!
//! On other platforms (ALSA, WinMM) there are no notifications here: the host's polling watcher
//! is the only hot-plug source.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use parking_lot::Mutex;

/// A subscriber callback (runs on the notification thread).
pub type SetupListener = Arc<dyn Fn() + Send + Sync>;

/// Process-wide setup-change state (see the module docs).
#[derive(Default)]
pub struct SetupEvents {
    changes: AtomicU64,
    sources_removed: AtomicU64,
    next_id: AtomicU64,
    listeners: Mutex<Vec<(u64, SetupListener)>>,
}

impl SetupEvents {
    /// Setup notifications so far.
    pub fn changes(&self) -> u64 {
        self.changes.load(Ordering::Acquire)
    }
    /// Sources removed so far: a source removed and re-added between two polls leaves the
    /// port list unchanged, but its connection is dead (the host reconnects).
    pub fn sources_removed(&self) -> u64 {
        self.sources_removed.load(Ordering::Acquire)
    }
    pub fn subscribe(&self, f: SetupListener) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        self.listeners.lock().push((id, f));
        id
    }
    pub fn unsubscribe(&self, id: u64) {
        self.listeners.lock().retain(|(i, _)| *i != id);
    }
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    fn notify(&self, source_removed: bool) {
        if source_removed {
            self.sources_removed.fetch_add(1, Ordering::AcqRel);
        }
        self.changes.fetch_add(1, Ordering::AcqRel);
        let ls: Vec<SetupListener> = self
            .listeners
            .lock()
            .iter()
            .map(|(_, f)| f.clone())
            .collect();
        for f in ls {
            f();
        }
    }
}

/// The process-wide [`SetupEvents`].
pub fn events() -> &'static SetupEvents {
    static EVENTS: OnceLock<SetupEvents> = OnceLock::new();
    EVENTS.get_or_init(SetupEvents::default)
}

/// Start the notification run-loop thread once per process (macOS; elsewhere a no-op). Returns
/// whether OS setup notifications are delivered.
pub fn ensure_started() -> bool {
    #[cfg(target_os = "macos")]
    {
        mac::start()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::sync::OnceLock;
    use std::time::Duration;

    use core_foundation::runloop::{CFRunLoop, CFRunLoopRunResult, kCFRunLoopDefaultMode};
    use coremidi::{AnyObject, Client, Notification};

    pub fn start() -> bool {
        static STARTED: OnceLock<bool> = OnceLock::new();
        *STARTED.get_or_init(|| {
            let (tx, rx) = std::sync::mpsc::sync_channel(1);
            let spawned = std::thread::Builder::new()
                .name("midi-runloop".into())
                .spawn(move || {
                    let client =
                        Client::new_with_notifications("micro-daw-notify", |n: &Notification| {
                            match n {
                                Notification::ObjectRemoved(info) => {
                                    super::events().notify(matches!(
                                        info.child,
                                        AnyObject::Source(_) | AnyObject::ExternalSource(_)
                                    ))
                                }
                                Notification::SetupChanged
                                | Notification::ObjectAdded(_)
                                | Notification::PropertyChanged(_) => super::events().notify(false),
                                _ => {}
                            }
                        });
                    let ok = client.is_ok();
                    let _ = tx.send(ok);
                    if !ok {
                        return;
                    }
                    // the client lives (and its run loop runs) for the rest of the process
                    let _client = client;
                    loop {
                        let r = CFRunLoop::run_in_mode(
                            unsafe { kCFRunLoopDefaultMode },
                            Duration::from_millis(500),
                            false,
                        );
                        if matches!(r, CFRunLoopRunResult::Finished) {
                            // no source attached (yet): do not spin
                            std::thread::sleep(Duration::from_millis(50));
                        }
                    }
                });
            spawned.is_ok() && rx.recv_timeout(Duration::from_secs(2)).unwrap_or(false)
        })
    }
}
