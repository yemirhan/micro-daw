//! Wiring the router to the engine.
//!
//! - [`EngineLink`] is the router's output on the MIDI thread: engine actions go straight to the
//!   core through the MIDI producer's [`LiveInput`] (lock-free SPSC ring: the audio thread never
//!   waits for the MIDI thread). A full ring queues in the producer's outbox instead of dropping,
//!   and an **outbox retry thread** (`midi-outbox`) keeps pushing it every
//!   [`OUTBOX_RETRY_INTERVAL`] until it is empty, so commands left over after the last MIDI
//!   message (the note-offs of a burst) still reach the core when no further input arrives.
//!   The `LiveInput` has its own small lock, shared only by the MIDI thread and that retry thread
//!   (never the audio thread). Everything else is queued for the UI thread, which is woken
//!   through a coalesced [`Waker`].
//! - Song-recording captures of synth notes (`captureNoteOn/Off(note, …, false)`) go to the core
//!   too ([`LiveInput::capture_synth_note`]): the core stamps them at the note's frame with the
//!   transport beat and generation there, in order with the punch closes, so a slow UI frame
//!   never changes where (or whether) a note is recorded.
//! - [`MidiUi`] is the UI thread's half: it drains that queue every frame (or on wake) and
//!   applies it to the [`ArrangementEngine`]: the knob dispatches through the [`KnobRouter`]
//!   (soft takeover, undo grouping), and the legacy volume / effect CCs through
//!   `handle_legacy_cc` (the engine owns that pickup).
//!
//! **UI queue growth.** The queue is unbounded on purpose (an action is never dropped). Its
//! growth is bounded by the MIDI rate times a few actions per message (bus event, active notes,
//! knob): ≈ 3 kB per second of a 1 kHz MIDI stream. With the waker the UI drains it within a
//! frame; only a UI thread blocked for seconds lets it grow.
//!
//! Synth notes keep the TS live offset: they sound [`LIVE_OFFSET`] (4 ms) after the event's
//! audio time, like `audioEngine.noteOn` (`Tone.immediate() + LIVE_OFFSET`); pads sound at the
//! event's audio time, like `drumEngine.hit` (`Tone.immediate()`).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, unbounded};
use mdaw_engine::buffers::PadBufferCache;
use mdaw_engine::core::{LiveInput, Shared, SynthNoteOp};
use mdaw_engine::engine::{ArrangementEngine, LIVE_OFFSET};
use mdaw_model::PadIndex;
use mdaw_model::doc::LoopSettingsPatch;
use mdaw_model::knobs::{KnobDeps, KnobEngine, KnobRouter};
use mdaw_model::types::{KitState, LoopRecordSettings, PadParam, PadSettings};
use parking_lot::{Mutex, RwLock};
use serde_json::{Map, Value};

use crate::router::{Action, RouterOutput};

/// How often the outbox retry thread pushes commands still waiting for room in the ring (well
/// under one render quantum at 48 kHz: 2.7 ms).
pub const OUTBOX_RETRY_INTERVAL: Duration = Duration::from_millis(1);

/// A UI wake-up callback (the app's event-loop proxy).
pub type WakeFn = Arc<dyn Fn() + Send + Sync>;

/// "The MIDI side has something for the UI": a shared, settable waker, **coalesced**: after one
/// call it stays quiet until the UI acknowledges ([`Waker::ack`], done by [`MidiUi::pump`] and
/// [`crate::MidiHost::poll`]) so a burst wakes the UI once. Shared by the engine link (UI
/// actions), the host's raw-message queue (monitor rows, raw listeners) and the hot-plug
/// watcher; [`Waker::set`] works at any time, also after `enable`.
#[derive(Clone, Default)]
pub struct Waker(Arc<WakerInner>);

#[derive(Default)]
struct WakerInner {
    f: RwLock<Option<WakeFn>>,
    pending: AtomicBool,
}

impl Waker {
    pub fn set(&self, f: Option<WakeFn>) {
        *self.0.f.write() = f;
        self.0.pending.store(false, Ordering::Release);
    }
    /// Wake the UI unless a wake is already pending.
    pub fn wake(&self) {
        if self.0.pending.swap(true, Ordering::AcqRel) {
            return;
        }
        let f = self.0.f.read().clone();
        if let Some(f) = f {
            f();
        }
    }
    /// The UI is about to drain: the next event wakes it again.
    pub fn ack(&self) {
        self.0.pending.store(false, Ordering::Release);
    }
}

/// The outbox retry thread (see the module docs).
struct Retry {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Retry {
    fn spawn(live: Arc<Mutex<LiveInput>>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let s = stop.clone();
        let thread = std::thread::Builder::new()
            .name("midi-outbox".into())
            .spawn(move || {
                while !s.load(Ordering::Acquire) {
                    let queued = {
                        let mut l = live.lock();
                        l.flush();
                        l.queued()
                    };
                    if queued > 0 {
                        std::thread::sleep(OUTBOX_RETRY_INTERVAL);
                    } else {
                        std::thread::park();
                    }
                }
            })
            .ok();
        Self { stop, thread }
    }
    fn kick(&self) {
        if let Some(t) = &self.thread {
            t.thread().unpark();
        }
    }
}

impl Drop for Retry {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            t.thread().unpark();
            let _ = t.join();
        }
    }
}

/// The router's output on the MIDI thread (see the module docs).
pub struct EngineLink {
    live: Arc<Mutex<LiveInput>>,
    shared: Arc<Shared>,
    synth_slot: usize,
    ui: Sender<Action>,
    waker: Waker,
    retry: Retry,
    /// UI actions queued since the last [`Self::flush`].
    ui_queued: bool,
}

impl EngineLink {
    /// `synth_slot`: [`ArrangementEngine::live_synth_slot`]. Returns the link and the UI
    /// thread's receiver.
    pub fn new(live: LiveInput, synth_slot: usize) -> (Self, Receiver<Action>) {
        let (ui, rx) = unbounded();
        let shared = live.shared_arc();
        let live = Arc::new(Mutex::new(live));
        (
            Self {
                retry: Retry::spawn(live.clone()),
                live,
                shared,
                synth_slot,
                ui,
                waker: Waker::default(),
                ui_queued: false,
            },
            rx,
        )
    }

    /// Push commands still waiting for room in the ring (the retry thread takes over if some
    /// are left), and wake the UI if actions were queued for it.
    pub fn flush(&mut self) {
        let queued = {
            let mut l = self.live.lock();
            l.flush();
            l.queued()
        };
        if queued > 0 {
            self.retry.kick();
        }
        if std::mem::take(&mut self.ui_queued) {
            self.waker.wake();
        }
    }

    /// Commands waiting in the MIDI producer's outbox.
    pub fn queued(&self) -> usize {
        self.live.lock().queued()
    }

    /// The core's published clock.
    pub fn shared(&self) -> &Shared {
        &self.shared
    }

    /// The waker shared with the host (set it there: [`crate::MidiHost::set_waker`]).
    pub fn waker(&self) -> &Waker {
        &self.waker
    }

    fn synth(&mut self, op: SynthNoteOp, time: Option<f64>) {
        self.live
            .lock()
            .synth_note_at(self.synth_slot, op, time, LIVE_OFFSET);
    }
}

impl RouterOutput for EngineLink {
    fn emit(&mut self, action: Action) {
        match action {
            Action::PadDown {
                pad,
                velocity,
                time,
            } => self.live.lock().pad_down(pad, velocity, time),
            Action::PadHit {
                pad,
                velocity,
                time,
            } => self.live.lock().pad_hit(pad, velocity, time),
            Action::PadUp { pad, time } => self.live.lock().pad_up(pad, time),
            Action::SynthOn {
                note,
                velocity,
                time,
            } => self.synth(
                SynthNoteOp::On {
                    note: note as f64,
                    velocity,
                },
                time,
            ),
            Action::SynthOff { note, time } => {
                self.synth(SynthNoteOp::Off { note: note as f64 }, time)
            }
            Action::SynthReleaseAll { time } => self.synth(SynthNoteOp::ReleaseAll, time),
            Action::RepeatReleaseAll { time } => self.live.lock().repeat_release_all(time),
            Action::PadMute { muted, time } => self.live.lock().set_pad_mute(muted, time),
            Action::CaptureNoteOn {
                note,
                velocity,
                time,
            } => self
                .live
                .lock()
                .capture_synth_note(note as f64, velocity, true, time),
            Action::CaptureNoteOff { note, time } => {
                self.live
                    .lock()
                    .capture_synth_note(note as f64, 0.0, false, time)
            }
            other => {
                // the UI thread is gone only at shutdown
                let _ = self.ui.send(other);
                self.ui_queued = true;
            }
        }
    }
}

/// [`KnobEngine`] over the [`ArrangementEngine`] (`KnobEngine = Pick<ArrangementEngine, …>`).
pub struct EngineKnobs<'a>(pub &'a mut ArrangementEngine);

impl KnobEngine for EngineKnobs<'_> {
    fn get_active_drum_track_id(&self) -> Option<String> {
        self.0.doc().get_active_drum_track_id()
    }
    fn ensure_drum_track(&mut self) -> String {
        self.0.edit(|d| d.ensure_drum_track())
    }
    fn get_selected_pad(&self) -> PadIndex {
        self.0.doc().get_selected_pad()
    }
    fn get_kit(&self, track_id: &str) -> KitState {
        self.0.doc().get_kit(track_id)
    }
    fn update_pad(&mut self, track_id: &str, pad: PadIndex, param: PadParam, value: f64) {
        let mut patch = Map::new();
        patch.insert(param.id().to_owned(), Value::from(value));
        self.0.edit(|d| d.update_pad(track_id, pad, &patch));
    }
    fn begin_undo_group(&mut self, label: &str) {
        self.0.edit(|d| d.begin_undo_group(label));
    }
    fn end_undo_group(&mut self) {
        self.0.edit(|d| d.end_undo_group());
    }
    fn set_loop_swing(&mut self, swing: f64) {
        let patch = LoopSettingsPatch {
            swing: Some(swing),
            ..LoopSettingsPatch::default()
        };
        self.0.edit(|d| d.set_loop_settings(&patch));
    }
    fn get_loop_settings(&self) -> LoopRecordSettings {
        self.0.doc().get_loop_settings()
    }
    fn set_bpm(&mut self, bpm: f64) {
        self.0.edit(|d| d.set_bpm(bpm));
    }
    fn get_bpm(&self) -> f64 {
        self.0.doc().get_bpm()
    }
    fn set_track_volume(&mut self, track_id: &str, db: f64) {
        self.0.edit(|d| d.set_track_volume(track_id, db));
    }
    fn track_volume(&self, track_id: &str) -> Option<f64> {
        self.0
            .doc()
            .get_tracks()
            .iter()
            .find(|t| t.id == track_id)
            .map(|t| t.volume)
    }
}

/// `KnobRouterDeps` of `installKnobRouting()`: the master bus volume and the cached pad buffer
/// length (`getCachedPadBuffer(source)?.duration`). The master volume write is held here and
/// applied after the knob ([`KnobRouter::handle`] needs the engine and the deps at once; the
/// master target touches nothing else, so the order of effects is the TS one).
pub struct EngineKnobDeps {
    master: f64,
    set_master: Option<f64>,
    cache: PadBufferCache,
}

impl EngineKnobDeps {
    pub fn new(engine: &ArrangementEngine) -> Self {
        Self {
            master: engine.get_master_volume_db(),
            set_master: None,
            cache: engine.cache().clone(),
        }
    }
    /// Apply the held master volume write.
    pub fn apply(self, engine: &mut ArrangementEngine) {
        if let Some(db) = self.set_master {
            engine.set_master_volume_db(db);
        }
    }
}

impl KnobDeps for EngineKnobDeps {
    fn set_master_volume(&mut self, db: f64) {
        self.master = db;
        self.set_master = Some(db);
    }
    fn get_master_volume(&self) -> f64 {
        self.master
    }
    fn pad_buffer_duration(&self, pad: &PadSettings) -> Option<f64> {
        let sample = pad.sample.as_ref()?;
        self.cache.get_cached(&sample.source).map(|b| b.duration())
    }
}

/// The UI thread's half (see the module docs).
pub struct MidiUi {
    rx: Receiver<Action>,
    waker: Waker,
    pub knobs: KnobRouter,
}

impl MidiUi {
    /// `rx` from [`EngineLink::new`], `waker` = [`EngineLink::waker`]; `clock_ms`: a monotonic
    /// millisecond clock (the knob gestures' 400 ms idle timer).
    pub fn new(rx: Receiver<Action>, waker: Waker, clock_ms: Box<dyn Fn() -> f64>) -> Self {
        Self {
            rx,
            waker,
            knobs: KnobRouter::new(clock_ms, None),
        }
    }

    /// Apply everything the MIDI thread queued, then close an idle knob gesture. Returns the
    /// non-engine actions applied (the UI reads bus events / sustain / active notes from them).
    /// Call it every frame and whenever the waker fires.
    pub fn pump(&mut self, engine: &mut ArrangementEngine) -> Vec<Action> {
        self.waker.ack();
        let mut seen = Vec::new();
        while let Ok(a) = self.rx.try_recv() {
            self.apply(engine, &a);
            seen.push(a);
        }
        self.knobs.poll(&mut EngineKnobs(engine));
        seen
    }

    /// Apply one queued action.
    pub fn apply(&mut self, engine: &mut ArrangementEngine, action: &Action) {
        match *action {
            // the MIDI path sends these to the core (`EngineLink`); a caller that routes into a
            // plain action list can still apply them here (UI-thread capture, the TS way)
            Action::CaptureNoteOn {
                note,
                velocity,
                time,
            } => engine.capture_note_on(note as f64, velocity, time),
            Action::CaptureNoteOff { note, time } => engine.capture_note_off(note as f64, time),
            Action::Knob(k) => {
                let mut deps = EngineKnobDeps::new(engine);
                self.knobs
                    .handle(&mut EngineKnobs(engine), &mut deps, k.target, k.value01);
                deps.apply(engine);
            }
            Action::LegacyCc { cc, value, master } => {
                engine.handle_legacy_cc(cc, value, master);
            }
            _ => {}
        }
    }
}
