//! Wiring the router to the engine.
//!
//! - [`EngineLink`] is the router's output on the MIDI thread: engine actions go straight to the
//!   core through the MIDI producer's [`LiveInput`] (lock-free SPSC ring: the audio thread never
//!   waits for the MIDI thread, and a full ring queues in the producer's outbox instead of
//!   dropping); everything else is queued for the UI thread. Call [`EngineLink::flush`] after
//!   each MIDI message (the host does) so queued commands are pushed as soon as there is room.
//! - [`MidiUi`] is the UI thread's half: it drains that queue every frame and applies it to the
//!   [`ArrangementEngine`]: synth-note captures for Song recording, the knob dispatches through
//!   the [`KnobRouter`] (soft takeover, undo grouping), and the legacy volume / effect CCs
//!   through `handle_legacy_cc` (the engine owns that pickup).
//!
//! Synth notes keep the TS live offset: they sound [`LIVE_OFFSET`] (4 ms) after the event's
//! audio time, like `audioEngine.noteOn` (`Tone.immediate() + LIVE_OFFSET`); pads sound at the
//! event's audio time, like `drumEngine.hit` (`Tone.immediate()`).

use crossbeam_channel::{Receiver, Sender, unbounded};
use mdaw_engine::buffers::PadBufferCache;
use mdaw_engine::core::{LiveInput, SynthNoteOp};
use mdaw_engine::engine::{ArrangementEngine, LIVE_OFFSET};
use mdaw_model::PadIndex;
use mdaw_model::doc::LoopSettingsPatch;
use mdaw_model::knobs::{KnobDeps, KnobEngine, KnobRouter};
use mdaw_model::types::{KitState, LoopRecordSettings, PadParam, PadSettings};
use serde_json::{Map, Value};

use crate::router::{Action, RouterOutput};

/// The router's output on the MIDI thread (see the module docs).
pub struct EngineLink {
    live: LiveInput,
    synth_slot: usize,
    ui: Sender<Action>,
}

impl EngineLink {
    /// `synth_slot`: [`ArrangementEngine::live_synth_slot`]. Returns the link and the UI
    /// thread's receiver.
    pub fn new(live: LiveInput, synth_slot: usize) -> (Self, Receiver<Action>) {
        let (ui, rx) = unbounded();
        (
            Self {
                live,
                synth_slot,
                ui,
            },
            rx,
        )
    }

    /// Push commands still waiting for room in the ring.
    pub fn flush(&mut self) {
        self.live.flush();
    }

    pub fn live(&self) -> &LiveInput {
        &self.live
    }

    fn synth(&mut self, op: SynthNoteOp, time: Option<f64>) {
        self.live
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
            } => self.live.pad_down(pad, velocity, time),
            Action::PadHit {
                pad,
                velocity,
                time,
            } => self.live.pad_hit(pad, velocity, time),
            Action::PadUp { pad, time } => self.live.pad_up(pad, time),
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
            Action::RepeatReleaseAll { time } => self.live.repeat_release_all(time),
            Action::PadMute { muted, time } => self.live.set_pad_mute(muted, time),
            other => {
                // the UI thread is gone only at shutdown
                let _ = self.ui.send(other);
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
    pub knobs: KnobRouter,
}

impl MidiUi {
    /// `rx` from [`EngineLink::new`]; `clock_ms`: a monotonic millisecond clock (the knob
    /// gestures' 400 ms idle timer).
    pub fn new(rx: Receiver<Action>, clock_ms: Box<dyn Fn() -> f64>) -> Self {
        Self {
            rx,
            knobs: KnobRouter::new(clock_ms, None),
        }
    }

    /// Apply everything the MIDI thread queued, then close an idle knob gesture. Returns the
    /// non-engine actions applied (the UI reads bus events / sustain / active notes from them).
    pub fn pump(&mut self, engine: &mut ArrangementEngine) -> Vec<Action> {
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
