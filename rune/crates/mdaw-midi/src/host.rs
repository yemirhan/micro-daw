//! midir I/O: port of `MidiService`'s device handling (`src/services/MidiService.ts`).
//!
//! **Threads.**
//! - The **UI thread** owns [`MidiHost`]: enabling, the device list, selection, connecting, the
//!   raw-message listeners and the MIDI monitor, and the settings file.
//! - The **MIDI thread** is midir's callback thread (CoreMIDI / ALSA / WinMM). Its work per
//!   message is bounded: it locks [`MidiShared`] (router + held notes + timestamp mapper), maps
//!   the timestamp, runs the router (learn included; a profile save is only *queued*, see
//!   `settings.rs`), pushes the engine commands into the lock-free `LiveInput` ring, then copies
//!   the raw bytes into a bounded queue ([`RAW_QUEUE_CAPACITY`]) when a monitor or raw listener
//!   wants them, and wakes the UI. Monitor formatting and the raw listeners run on the UI
//!   thread ([`MidiHost::poll`]). No disk I/O, no formatting, no user callbacks.
//! - The **audio thread** never takes a MIDI lock: MIDI cannot block audio.
//! - **Deliberate deviation from plan §3** (which describes a published routing snapshot and a
//!   lock-free path): the router state stays behind one mutex shared by the MIDI thread and the
//!   UI thread (on-screen keyboard and pads, panic, routing mode, mute, learn). It keeps the
//!   TS's exact interleaving of UI and MIDI calls (learn consumption, held-note routes, panic vs
//!   a note in flight), which the routing golden checks call by call. What the UI does under it
//!   is a router call (µs); the measured worst case is in `docs/rune-port/review-phase-5.md`
//!   (`ui_lock_hold_is_short`). A UI hold delays MIDI by its length; it never loses an event.
//! - A **hot-plug watcher** thread enumerates the input ports every [`HOTPLUG_POLL_INTERVAL`]
//!   and is also woken by CoreMIDI setup notifications (`notify.rs`, macOS). When the list
//!   changed it flags it and wakes the UI, which runs [`MidiHost::poll`] → `handleDeviceChange`.
//!   On macOS the notification arrives immediately; elsewhere detection takes ≤ 500 ms.
//!
//! **Connection health** is separate from the selection: [`MidiHost::active_device_id`] is the
//! input chosen (desired), [`MidiHost::connection`] whether a connection to it is open, or the
//! error opening it. Connection changes are reported to [`MidiHost::on_connection_change`]
//! listeners. A failed connection is retried on the next hot-plug change and on a re-select of
//! the same device ([`MidiHost::select_device`]; a healthy one stays a no-op, like the TS), and
//! [`MidiHost::reconnect`] forces one. A source that disappeared and came back between two
//! polls (same port list) is detected through the CoreMIDI notifications and reconnected.
//!
//! **Semantics kept from the TS:** `enable` never panics or errors: it returns false when MIDI is
//! unavailable (midir's client creation is synchronous, so the TS 5 s timeout race has nothing to
//! race); auto-select (preferred, else `/mpk\s*mini/i`, else the first input; the first automatic
//! pick becomes preferred); `select_device` ignores unknown ids; losing an input (switch,
//! unplug, dispose) sends note-offs for every note held on it; the device-change listeners run
//! after every change. SysEx is not delivered (Web MIDI without the sysex permission); clock and
//! active sensing are (the monitor drops them).
//!
//! **Electron bug fixed in both apps (user decision 2026-11-18):** losing an input also lifts
//! its sustain pedal (CC 64 = 0 on each channel it held down), releasing the notes it sustained
//! (`devices::HeldNotes`).
//!
//! **Not in the TS (Electron gap fixed in Rust):** the preferred device is persisted
//! ([`MidiHost::set_persist`], wired by `settings::enable_with_settings`) and restored at enable
//! when `autoConnectLastDevice` is on ([`MidiHost::enable`]'s `last_device`).
//!
//! Device `manufacturer` is empty: midir exposes only the port name.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, bounded};
use midir::{Ignore, MidiInput, MidiInputConnection};
use parking_lot::Mutex;

use mdaw_model::controller::ProfileStore;

use crate::devices::{DeviceInfo, DeviceOp, DeviceSelector, HeldNotes, LastDevice};
use crate::engine_link::{EngineLink, WakeFn, Waker};
use crate::message::{MidiEvent, parse};
use crate::monitor::MonitorBuffer;
use crate::notify;
use crate::router::{CC_SUSTAIN, Router};
use crate::timing::{TimestampMapper, audio_time};

/// See the module docs.
pub const HOTPLUG_POLL_INTERVAL: Duration = Duration::from_millis(500);
/// Raw messages waiting for the UI (monitor, raw listeners); beyond it they are dropped and
/// counted ([`MidiStats::raw_dropped`]), never blocking the MIDI thread.
pub const RAW_QUEUE_CAPACITY: usize = 4096;
/// midir client names.
const CLIENT_NAME: &str = "micro-daw";

/// A raw-message listener: `(bytes, timestamp_ms)`; runs on the UI thread.
pub type RawListener = Box<dyn FnMut(&[u8], f64)>;
/// A device-change listener (`onDeviceChange`); runs on the UI thread.
pub type DeviceListener = Box<dyn FnMut(&[DeviceInfo])>;
/// A connection-change listener; runs on the UI thread.
pub type ConnectionListener = Box<dyn FnMut(&Connection)>;
/// Persists the preferred device; runs on the UI thread.
pub type PersistDevice = Box<dyn FnMut(&LastDevice)>;

/// The state of the connection to the selected input (see the module docs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Connection {
    Disconnected,
    Connected {
        id: String,
    },
    /// Opening the selected input failed (retried on the next hot-plug change or re-select).
    Failed {
        id: String,
        error: String,
    },
}

/// MIDI-thread counters.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MidiStats {
    /// Messages handled.
    pub messages: u64,
    /// Events delivered after their latency budget (played as soon as possible).
    pub late_events: u64,
    /// The worst lateness seen (ms).
    pub max_late_ms: f64,
}

/// One raw message for the UI thread.
struct RawRow {
    bytes: Vec<u8>,
    ms: f64,
}

/// The state the MIDI thread and the UI share (see the module docs).
pub struct MidiShared<St: ProfileStore> {
    pub router: Router<St>,
    pub link: EngineLink,
    held: HeldNotes,
    mapper: TimestampMapper,
    stats: MidiStats,
    epoch: Instant,
}

impl<St: ProfileStore> MidiShared<St> {
    pub fn new(router: Router<St>, link: EngineLink) -> Self {
        Self {
            router,
            link,
            held: HeldNotes::default(),
            mapper: TimestampMapper::new(),
            stats: MidiStats::default(),
            epoch: Instant::now(),
        }
    }

    /// Milliseconds since this state was created (the monitor's `performance.now()`).
    pub fn ms_since_start(&self, at: Instant) -> f64 {
        at.saturating_duration_since(self.epoch).as_secs_f64() * 1000.0
    }

    pub fn stats(&self) -> MidiStats {
        self.stats
    }

    /// One message from the active input. `timestamp_us`: midir's stamp (None: "now").
    /// Returns the instant the event happened (mapped from its timestamp).
    pub fn on_message(
        &mut self,
        timestamp_us: Option<u64>,
        bytes: &[u8],
        arrival: Instant,
    ) -> Instant {
        let instant = match timestamp_us {
            Some(ts) => self.mapper.map(ts, arrival),
            None => arrival,
        };
        self.stats.messages += 1;
        if let Some(ev) = parse(bytes) {
            let at = audio_time(self.link.shared(), instant);
            if let Some(e) = at
                && e.late_by > 0.0
            {
                self.stats.late_events += 1;
                self.stats.max_late_ms = self.stats.max_late_ms.max(e.late_by * 1000.0);
            }
            self.dispatch(ev, at.map(|e| e.time));
        }
        instant
    }

    /// `simulateNoteOn / Off / ControlChange` and the parsed path: held-note (and pedal)
    /// bookkeeping, then the router, then push the core commands.
    pub fn dispatch(&mut self, ev: MidiEvent, time: Option<f64>) {
        match ev {
            MidiEvent::NoteOn { note, channel, .. } => self.held.note_on(note, channel),
            MidiEvent::NoteOff { note, channel } => self.held.note_off(note, channel),
            MidiEvent::ControlChange { cc, value, channel } => {
                self.held.control_change(cc, value, channel)
            }
        }
        self.router.handle_event(ev, time, &mut self.link);
        self.link.flush();
    }

    /// `releaseHeldNotes()`: note-offs for every note held on the input being lost, then its
    /// pedal up (CC 64 = 0) on every channel it held it down (Electron bug fixed in both apps).
    pub fn release_held_notes(&mut self) {
        for (note, channel) in self.held.take() {
            self.router
                .midi_note_off(i64::from(note), i64::from(channel), None, &mut self.link);
        }
        for channel in self.held.take_sustain() {
            self.router
                .handle_cc(CC_SUSTAIN, 0, i64::from(channel), None, &mut self.link);
        }
        self.link.flush();
    }

    /// Run `f` on the router with the engine link as its output (UI-thread calls: on-screen
    /// keyboard and pads, panic, routing mode, mute, learn), then push the core commands.
    pub fn with_router<R>(&mut self, f: impl FnOnce(&mut Router<St>, &mut EngineLink) -> R) -> R {
        let r = f(&mut self.router, &mut self.link);
        self.link.flush();
        r
    }

    fn reset_timing(&mut self) {
        self.mapper.reset();
    }
}

/// Port enumeration: (id, name) of every input.
fn enumerate(input: &MidiInput) -> Vec<DeviceInfo> {
    input
        .ports()
        .iter()
        .filter_map(|p| {
            let name = input.port_name(p).ok()?;
            Some(DeviceInfo::new(p.id(), name))
        })
        .collect()
}

struct Watcher {
    stop: Arc<AtomicBool>,
    changed: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    subscription: u64,
}

impl Drop for Watcher {
    fn drop(&mut self) {
        notify::events().unsubscribe(self.subscription);
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            t.thread().unpark();
            let _ = t.join();
        }
    }
}

/// See the module docs.
pub struct MidiHost<St: ProfileStore + Send + 'static> {
    shared: Arc<Mutex<MidiShared<St>>>,
    input: Option<MidiInput>,
    conn: Option<MidiInputConnection<()>>,
    connection: Connection,
    selector: DeviceSelector,
    devices: Vec<DeviceInfo>,
    enabled: bool,
    device_listeners: Vec<DeviceListener>,
    connection_listeners: Vec<ConnectionListener>,
    persist: Option<PersistDevice>,
    watcher: Option<Watcher>,
    waker: Waker,
    poll_interval: Duration,
    /// CoreMIDI removed-source count already handled.
    seen_removed: u64,
    raw_tx: Sender<RawRow>,
    raw_rx: Receiver<RawRow>,
    raw_dropped: Arc<AtomicU64>,
    /// Someone wants raw rows (monitor active or a raw listener).
    raw_wanted: Arc<AtomicBool>,
    raw_listeners: Vec<(u64, RawListener)>,
    next_raw_id: u64,
    monitor: MonitorBuffer,
    monitor_active: bool,
}

impl<St: ProfileStore + Send + 'static> MidiHost<St> {
    pub fn new(shared: Arc<Mutex<MidiShared<St>>>) -> Self {
        let waker = shared.lock().link.waker().clone();
        let (raw_tx, raw_rx) = bounded(RAW_QUEUE_CAPACITY);
        Self {
            shared,
            input: None,
            conn: None,
            connection: Connection::Disconnected,
            selector: DeviceSelector::new(),
            devices: Vec::new(),
            enabled: false,
            device_listeners: Vec::new(),
            connection_listeners: Vec::new(),
            persist: None,
            watcher: None,
            waker,
            poll_interval: HOTPLUG_POLL_INTERVAL,
            seen_removed: notify::events().sources_removed(),
            raw_tx,
            raw_rx,
            raw_dropped: Arc::new(AtomicU64::new(0)),
            raw_wanted: Arc::new(AtomicBool::new(false)),
            raw_listeners: Vec::new(),
            next_raw_id: 0,
            monitor: MonitorBuffer::default(),
            monitor_active: false,
        }
    }

    pub fn shared(&self) -> &Arc<Mutex<MidiShared<St>>> {
        &self.shared
    }

    /// Wakes the UI (the app's event-loop proxy) when MIDI has something for it: UI actions
    /// queued ([`crate::MidiUi::pump`]), raw messages, a port-list change ([`Self::poll`]).
    /// Coalesced; can be set (or replaced) at any time, also after [`Self::enable`].
    pub fn set_waker(&mut self, waker: WakeFn) {
        self.waker.set(Some(waker));
    }

    /// Called with the preferred device whenever a change worth persisting happens (see
    /// `devices.rs`). `settings::enable_with_settings` wires it to the settings store.
    pub fn set_persist(&mut self, persist: PersistDevice) {
        self.persist = Some(persist);
    }

    /// The hot-plug watcher's interval (tests use a short one).
    pub fn set_poll_interval(&mut self, interval: Duration) {
        self.poll_interval = interval;
    }

    /// `enable()`: never fails; false when MIDI is unavailable. `last_device`: the persisted
    /// preferred device, passed when `settings.midi.autoConnectLastDevice` is on.
    pub fn enable(&mut self, last_device: Option<LastDevice>) -> bool {
        if self.enabled {
            return true;
        }
        // the first CoreMIDI client of the process must live on a pumped run loop
        notify::ensure_started();
        let input = match std::panic::catch_unwind(|| MidiInput::new(CLIENT_NAME)) {
            Ok(Ok(i)) => i,
            _ => return false,
        };
        self.devices = enumerate(&input);
        self.input = Some(input);
        self.enabled = true;
        self.seen_removed = notify::events().sources_removed();
        self.selector = DeviceSelector::with_last_device(last_device);
        self.start_watcher();
        let ops = self.selector.auto_select(&self.devices);
        self.apply(ops);
        true
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// `getDevices()`.
    pub fn devices(&self) -> &[DeviceInfo] {
        if self.enabled { &self.devices } else { &[] }
    }

    /// `getActiveDeviceId()`: the input selected (even if its connection failed to open; see
    /// [`Self::connection`]).
    pub fn active_device_id(&self) -> Option<&str> {
        self.selector.active()
    }

    /// True while a connection to the selected input is open.
    pub fn is_connected(&self) -> bool {
        matches!(self.connection, Connection::Connected { .. })
    }

    /// The connection state (see the module docs).
    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    pub fn preferred_device_id(&self) -> Option<&str> {
        self.selector.preferred()
    }

    /// MIDI-thread counters, plus raw rows dropped because the UI did not drain them.
    pub fn stats(&self) -> (MidiStats, u64) {
        (
            self.shared.lock().stats(),
            self.raw_dropped.load(Ordering::Relaxed),
        )
    }

    /// `selectDevice(id)`. Re-selecting the selected input is a no-op while its connection is
    /// healthy (TS); after a failure (or a dead connection) it reconnects.
    pub fn select_device(&mut self, id: &str) {
        let reselect = self.selector.active() == Some(id);
        let ops = self.selector.select(id, &self.devices);
        self.apply(ops);
        if reselect && !self.is_connected() {
            self.reconnect();
        }
    }

    /// Close and reopen the connection to the selected input (held notes are released).
    /// Returns whether it is connected now.
    pub fn reconnect(&mut self) -> bool {
        let Some(id) = self.selector.active().map(str::to_owned) else {
            return false;
        };
        self.detach();
        self.attach(&id);
        self.is_connected()
    }

    /// `onDeviceChange(cb)`.
    pub fn on_device_change(&mut self, cb: DeviceListener) {
        self.device_listeners.push(cb);
    }

    /// Connection changes (opened, closed, failed with its error).
    pub fn on_connection_change(&mut self, cb: ConnectionListener) {
        self.connection_listeners.push(cb);
    }

    /// `onRawMessage(cb)`: returns the id for [`Self::remove_raw_listener`]. Listeners run on
    /// the UI thread, in [`Self::poll`].
    pub fn add_raw_listener(&mut self, l: RawListener) -> u64 {
        self.next_raw_id += 1;
        self.raw_listeners.push((self.next_raw_id, l));
        self.update_raw_wanted();
        self.next_raw_id
    }
    pub fn remove_raw_listener(&mut self, id: u64) {
        self.raw_listeners.retain(|(i, _)| *i != id);
        self.update_raw_wanted();
    }

    /// The MIDI monitor's buffer (the Dev view drains it with `take`).
    pub fn monitor(&mut self) -> &mut MonitorBuffer {
        &mut self.monitor
    }
    /// Feed the monitor (the Dev view's MIDI tab is open).
    pub fn set_monitor_active(&mut self, on: bool) {
        self.monitor_active = on;
        self.update_raw_wanted();
    }

    fn update_raw_wanted(&self) {
        self.raw_wanted.store(
            self.monitor_active || !self.raw_listeners.is_empty(),
            Ordering::Relaxed,
        );
    }

    /// Run the raw listeners and the monitor on the messages queued by the MIDI thread.
    pub fn drain_raw(&mut self) -> usize {
        let mut n = 0;
        while let Ok(row) = self.raw_rx.try_recv() {
            n += 1;
            if self.monitor_active {
                self.monitor.on_raw(&row.bytes, row.ms);
            }
            for (_, l) in &mut self.raw_listeners {
                l(&row.bytes, row.ms);
            }
        }
        n
    }

    /// Call every UI frame and whenever the waker fires: drains the raw messages and, if the
    /// watcher saw a change, re-enumerates. Returns true when the device list changed.
    pub fn poll(&mut self) -> bool {
        self.waker.ack();
        self.drain_raw();
        let flagged = self
            .watcher
            .as_ref()
            .is_some_and(|w| w.changed.swap(false, Ordering::Relaxed));
        if flagged { self.refresh() } else { false }
    }

    /// Enumerate now and run `handleDeviceChange` if the list differs; retry a failed
    /// connection; reconnect a source replaced between two polls. Returns true when the list
    /// changed.
    pub fn refresh(&mut self) -> bool {
        let Some(input) = &self.input else {
            return false;
        };
        let devices = enumerate(input);
        let removed = notify::events().sources_removed();
        let source_removed = removed != self.seen_removed;
        self.seen_removed = removed;
        if devices == self.devices {
            // same list, but a source went away meanwhile (and came back): its connection is
            // dead even if the id is the same
            if (source_removed || !self.is_connected()) && self.selector.active().is_some() {
                self.reconnect();
            }
            return false;
        }
        self.devices = devices;
        let ops = self.selector.device_change(&self.devices);
        let attached = ops.iter().any(|o| matches!(o, DeviceOp::Attach(_)));
        self.apply(ops);
        if !attached && self.selector.active().is_some() && (!self.is_connected() || source_removed)
        {
            self.reconnect();
        }
        let devices = self.devices.clone();
        for cb in &mut self.device_listeners {
            cb(&devices);
        }
        true
    }

    fn apply(&mut self, ops: Vec<DeviceOp>) {
        for op in ops {
            match op {
                DeviceOp::Detach => self.detach(),
                DeviceOp::Attach(id) => self.attach(&id),
            }
        }
        if self.selector.take_preferred_change()
            && let Some(d) = self.selector.persisted_preferred(&self.devices)
            && let Some(p) = &mut self.persist
        {
            p(&d);
        }
    }

    fn set_connection(&mut self, c: Connection) {
        if self.connection == c {
            return;
        }
        self.connection = c;
        let c = self.connection.clone();
        for cb in &mut self.connection_listeners {
            cb(&c);
        }
    }

    fn detach(&mut self) {
        if let Some(c) = self.conn.take() {
            c.close();
        }
        {
            let mut s = self.shared.lock();
            s.release_held_notes();
            s.reset_timing();
        }
        self.set_connection(Connection::Disconnected);
    }

    fn attach(&mut self, id: &str) {
        match self.open(id) {
            Ok(c) => {
                self.conn = Some(c);
                self.set_connection(Connection::Connected { id: id.to_owned() });
            }
            Err(error) => self.set_connection(Connection::Failed {
                id: id.to_owned(),
                error,
            }),
        }
    }

    fn open(&self, id: &str) -> Result<MidiInputConnection<()>, String> {
        let mut input = MidiInput::new(CLIENT_NAME).map_err(|e| e.to_string())?;
        input.ignore(Ignore::Sysex);
        let port = input
            .find_port_by_id(id.to_owned())
            .ok_or_else(|| format!("MIDI input {id} not found"))?;
        let shared = self.shared.clone();
        let raw_tx = self.raw_tx.clone();
        let dropped = self.raw_dropped.clone();
        let wanted = self.raw_wanted.clone();
        let waker = self.waker.clone();
        let epoch = self.shared.lock().epoch;
        input
            .connect(
                &port,
                "micro-daw-in",
                move |ts, bytes, _| {
                    let arrival = Instant::now();
                    let instant = shared.lock().on_message(Some(ts), bytes, arrival);
                    if wanted.load(Ordering::Relaxed) {
                        let ms = instant.saturating_duration_since(epoch).as_secs_f64() * 1000.0;
                        let row = RawRow {
                            bytes: bytes.to_vec(),
                            ms,
                        };
                        if raw_tx.try_send(row).is_err() {
                            dropped.fetch_add(1, Ordering::Relaxed);
                        }
                        waker.wake();
                    }
                },
                (),
            )
            .map_err(|e| e.to_string())
    }

    fn start_watcher(&mut self) {
        let stop = Arc::new(AtomicBool::new(false));
        let changed = Arc::new(AtomicBool::new(false));
        let (s, c) = (stop.clone(), changed.clone());
        let waker = self.waker.clone();
        let interval = self.poll_interval;
        let initial: Vec<DeviceInfo> = self.devices.clone();
        let thread = std::thread::Builder::new()
            .name("midi-hotplug".into())
            .spawn(move || {
                let Ok(input) = MidiInput::new("micro-daw-hotplug") else {
                    return;
                };
                let mut last = initial;
                while !s.load(Ordering::Relaxed) {
                    std::thread::park_timeout(interval);
                    if s.load(Ordering::Relaxed) {
                        break;
                    }
                    let now = enumerate(&input);
                    if now != last {
                        last = now;
                        c.store(true, Ordering::Relaxed);
                        waker.wake();
                    }
                }
            })
            .ok();
        // CoreMIDI setup notifications: flag and wake at once, and let the watcher re-read
        let (c, w) = (changed.clone(), self.waker.clone());
        let unparker = thread.as_ref().map(|t| t.thread().clone());
        let subscription = notify::events().subscribe(Arc::new(move || {
            c.store(true, Ordering::Relaxed);
            w.wake();
            if let Some(t) = &unparker {
                t.unpark();
            }
        }));
        self.watcher = Some(Watcher {
            stop,
            changed,
            thread,
            subscription,
        });
    }

    /// `dispose()`: detach (held notes released), forget the preferred device, stop watching.
    pub fn dispose(&mut self) {
        let ops = self.selector.dispose();
        self.apply(ops);
        self.watcher = None;
        self.device_listeners.clear();
        self.input = None;
        self.enabled = false;
    }
}

impl<St: ProfileStore + Send + 'static> Drop for MidiHost<St> {
    fn drop(&mut self) {
        if let Some(c) = self.conn.take() {
            c.close();
        }
        self.watcher = None;
    }
}
