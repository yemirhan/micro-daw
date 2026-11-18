//! midir I/O: port of `MidiService`'s device handling (`src/services/MidiService.ts`).
//!
//! **Threads.**
//! - The **UI thread** owns [`MidiHost`]: enabling, the device list, selection, connecting.
//! - The **MIDI thread** is midir's callback thread (CoreMIDI / ALSA / WinMM). For every message
//!   it locks [`MidiShared`] (router + held notes + timestamp mapper + raw listeners) and runs
//!   the router, whose engine actions go to the core through the lock-free `LiveInput` ring.
//!   That mutex is only ever shared by the MIDI thread and the UI thread, never by the audio
//!   thread, so the audio callback cannot be blocked by MIDI.
//! - A **hot-plug watcher** thread enumerates the input ports every [`HOTPLUG_POLL_INTERVAL`]
//!   (500 ms; midir has no port-change notifications) and, when the list differs, flags it and
//!   calls the waker so the UI runs [`MidiHost::poll`], which re-enumerates and applies
//!   `handleDeviceChange`. Detection latency is therefore ≤ 500 ms + one UI poll.
//!
//! **Semantics kept from the TS:** `enable` never panics or errors: it returns false when MIDI is
//! unavailable (midir's client creation is synchronous, so the TS 5 s timeout race has nothing to
//! race); auto-select (preferred, else `/mpk\s*mini/i`, else the first input; the first automatic
//! pick becomes preferred); `select_device` ignores unknown ids; re-selecting the active input is
//! a no-op; losing an input (switch, unplug, dispose) sends note-offs for every note held on it;
//! the device-change listeners run after every change. SysEx is not delivered (Web MIDI without
//! the sysex permission); clock and active sensing are (the monitor drops them).
//!
//! **Not in the TS (Electron gap fixed in Rust):** the preferred device is persisted
//! ([`MidiHost::set_persist`]) and restored at enable when `autoConnectLastDevice` is on
//! ([`MidiHost::enable`]'s `last_device`).
//!
//! Device `manufacturer` is empty: midir exposes only the port name.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use midir::{Ignore, MidiInput, MidiInputConnection};
use parking_lot::Mutex;

use mdaw_model::controller::ProfileStore;

use crate::devices::{DeviceInfo, DeviceOp, DeviceSelector, HeldNotes, LastDevice};
use crate::engine_link::EngineLink;
use crate::message::{MidiEvent, parse};
use crate::monitor::MonitorBuffer;
use crate::router::Router;
use crate::timing::{TimestampMapper, audio_time};

/// See the module docs.
pub const HOTPLUG_POLL_INTERVAL: Duration = Duration::from_millis(500);
/// midir client names.
const CLIENT_NAME: &str = "micro-daw";

/// A raw-message listener: `(bytes, timestamp_ms)`; runs on the MIDI thread.
pub type RawListener = Box<dyn FnMut(&[u8], f64) + Send>;
/// A device-change listener (`onDeviceChange`); runs on the UI thread.
pub type DeviceListener = Box<dyn FnMut(&[DeviceInfo])>;
/// Persists the preferred device; runs on the UI thread.
pub type PersistDevice = Box<dyn FnMut(&LastDevice)>;

/// The state the MIDI thread and the UI share (see the module docs).
pub struct MidiShared<St: ProfileStore> {
    pub router: Router<St>,
    pub link: EngineLink,
    held: HeldNotes,
    mapper: TimestampMapper,
    raw: Vec<(u64, RawListener)>,
    next_raw_id: u64,
    /// The MIDI monitor's buffer (the Dev view drains it).
    pub monitor: MonitorBuffer,
    pub monitor_active: bool,
    epoch: Instant,
}

impl<St: ProfileStore> MidiShared<St> {
    pub fn new(router: Router<St>, link: EngineLink) -> Self {
        Self {
            router,
            link,
            held: HeldNotes::default(),
            mapper: TimestampMapper::new(),
            raw: Vec::new(),
            next_raw_id: 0,
            monitor: MonitorBuffer::default(),
            monitor_active: false,
            epoch: Instant::now(),
        }
    }

    /// Milliseconds since this state was created (the monitor's `performance.now()`).
    pub fn ms_since_start(&self, at: Instant) -> f64 {
        at.saturating_duration_since(self.epoch).as_secs_f64() * 1000.0
    }

    /// One message from the active input. `timestamp_us`: midir's stamp (None: "now").
    pub fn on_message(&mut self, timestamp_us: Option<u64>, bytes: &[u8], arrival: Instant) {
        let instant = match timestamp_us {
            Some(ts) => self.mapper.map(ts, arrival),
            None => arrival,
        };
        let ms = self.ms_since_start(instant);
        if self.monitor_active {
            self.monitor.on_raw(bytes, ms);
        }
        for (_, l) in &mut self.raw {
            l(bytes, ms);
        }
        let Some(ev) = parse(bytes) else { return };
        let time = audio_time(self.link.live().shared(), instant);
        self.dispatch(ev, time);
    }

    /// `simulateNoteOn / Off / ControlChange` and the parsed path: held-note bookkeeping, then
    /// the router, then push the core commands.
    pub fn dispatch(&mut self, ev: MidiEvent, time: Option<f64>) {
        match ev {
            MidiEvent::NoteOn { note, channel, .. } => self.held.note_on(note, channel),
            MidiEvent::NoteOff { note, channel } => self.held.note_off(note, channel),
            MidiEvent::ControlChange { .. } => {}
        }
        self.router.handle_event(ev, time, &mut self.link);
        self.link.flush();
    }

    /// `releaseHeldNotes()`: note-offs for every note held on the input being lost.
    pub fn release_held_notes(&mut self) {
        for (note, channel) in self.held.take() {
            self.router
                .midi_note_off(i64::from(note), i64::from(channel), None, &mut self.link);
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

    /// `onRawMessage(cb)`: returns the id for [`Self::remove_raw_listener`].
    pub fn add_raw_listener(&mut self, l: RawListener) -> u64 {
        self.next_raw_id += 1;
        self.raw.push((self.next_raw_id, l));
        self.next_raw_id
    }
    pub fn remove_raw_listener(&mut self, id: u64) {
        self.raw.retain(|(i, _)| *i != id);
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
}

impl Drop for Watcher {
    fn drop(&mut self) {
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
    selector: DeviceSelector,
    devices: Vec<DeviceInfo>,
    enabled: bool,
    device_listeners: Vec<DeviceListener>,
    persist: Option<PersistDevice>,
    watcher: Option<Watcher>,
    waker: Option<Arc<dyn Fn() + Send + Sync>>,
    poll_interval: Duration,
}

impl<St: ProfileStore + Send + 'static> MidiHost<St> {
    pub fn new(shared: Arc<Mutex<MidiShared<St>>>) -> Self {
        Self {
            shared,
            input: None,
            conn: None,
            selector: DeviceSelector::new(),
            devices: Vec::new(),
            enabled: false,
            device_listeners: Vec::new(),
            persist: None,
            watcher: None,
            waker: None,
            poll_interval: HOTPLUG_POLL_INTERVAL,
        }
    }

    pub fn shared(&self) -> &Arc<Mutex<MidiShared<St>>> {
        &self.shared
    }

    /// Called from the hot-plug watcher thread when the port list changed (wake the UI so it
    /// calls [`Self::poll`]).
    pub fn set_waker(&mut self, waker: Arc<dyn Fn() + Send + Sync>) {
        self.waker = Some(waker);
    }

    /// Called with the new preferred device whenever it changes (persist it in settings).
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
        let input = match std::panic::catch_unwind(|| MidiInput::new(CLIENT_NAME)) {
            Ok(Ok(i)) => i,
            _ => return false,
        };
        self.devices = enumerate(&input);
        self.input = Some(input);
        self.enabled = true;
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

    /// `getActiveDeviceId()` (the input listened to, even if its connection failed to open).
    pub fn active_device_id(&self) -> Option<&str> {
        self.selector.active()
    }

    /// True while a connection is open.
    pub fn is_connected(&self) -> bool {
        self.conn.is_some()
    }

    pub fn preferred_device_id(&self) -> Option<&str> {
        self.selector.preferred()
    }

    /// `selectDevice(id)`.
    pub fn select_device(&mut self, id: &str) {
        let ops = self.selector.select(id, &self.devices);
        self.apply(ops);
    }

    /// `onDeviceChange(cb)`.
    pub fn on_device_change(&mut self, cb: DeviceListener) {
        self.device_listeners.push(cb);
    }

    /// Re-enumerate if the watcher saw a change (call every UI frame / on wake). Returns true
    /// when the device list changed.
    pub fn poll(&mut self) -> bool {
        let flagged = self
            .watcher
            .as_ref()
            .is_some_and(|w| w.changed.swap(false, Ordering::Relaxed));
        if flagged { self.refresh() } else { false }
    }

    /// Enumerate now and run `handleDeviceChange` if the list differs. Returns true on change.
    pub fn refresh(&mut self) -> bool {
        let Some(input) = &self.input else {
            return false;
        };
        let devices = enumerate(input);
        if devices == self.devices {
            return false;
        }
        self.devices = devices;
        let ops = self.selector.device_change(&self.devices);
        self.apply(ops);
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
            && let Some(id) = self.selector.preferred()
            && let Some(d) = self.devices.iter().find(|d| d.id == id)
            && let Some(p) = &mut self.persist
        {
            p(&LastDevice {
                id: d.id.clone(),
                name: d.name.clone(),
            });
        }
    }

    fn detach(&mut self) {
        if let Some(c) = self.conn.take() {
            c.close();
        }
        let mut s = self.shared.lock();
        s.release_held_notes();
        s.reset_timing();
    }

    fn attach(&mut self, id: &str) {
        let Ok(mut input) = MidiInput::new(CLIENT_NAME) else {
            return;
        };
        input.ignore(Ignore::Sysex);
        let Some(port) = input.find_port_by_id(id.to_owned()) else {
            return;
        };
        let shared = self.shared.clone();
        let conn = input.connect(
            &port,
            "micro-daw-in",
            move |ts, bytes, _| {
                let arrival = Instant::now();
                shared.lock().on_message(Some(ts), bytes, arrival);
            },
            (),
        );
        if let Ok(c) = conn {
            self.conn = Some(c);
        }
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
                        if let Some(w) = &waker {
                            w();
                        }
                    }
                }
            })
            .ok();
        self.watcher = Some(Watcher {
            stop,
            changed,
            thread,
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
