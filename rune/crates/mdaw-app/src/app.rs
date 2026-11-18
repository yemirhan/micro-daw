//! The app state (≙ `App.tsx` + the hooks' state) and its frame loop.

use std::collections::HashMap;
use std::time::Duration;

use mdaw_engine::engine::{EngineEvent, TransportState};
use mdaw_model::beat_helpers::{PadLayout, StepCell, StepGrid};
use mdaw_model::settings::{self as store, SettingsStore};
use mdaw_model::slicer::SliceRange;
use mdaw_model::types::{KitState, NoteRepeatRate, PadSettings};
use rune::{Live, LiveText, TextEdit, Tx, Waker};

use crate::buffers::PadBuffers;
use crate::services::{RunMode, Services};

/// `AppMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    Daw,
    Samples,
    Packs,
    Settings,
    Dev,
}

/// `DawView`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DawView {
    Beat,
    Song,
}

/// The start screen's phases (`StartScreen.tsx`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StartPhase {
    Idle,
    Starting,
    /// Exit transition running until `until` (UI clock).
    Exiting {
        until: f64,
    },
    Done,
}

/// Which popover / menu / select is open (only one at a time, like Radix's modal popovers).
#[derive(Debug, Clone, PartialEq)]
pub enum Menu {
    DrumTrack,
    Kit,
    Quantize,
    RepeatRate,
    Choke,
    Chop,
    /// The pad context menu at a window point.
    Pad {
        pad: usize,
        x: f32,
        y: f32,
        renaming: bool,
    },
    MidiDevice,
    Routing,
}

/// A press held on an on-screen pad (one at a time; released exactly once).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadHold {
    Pointer(usize),
    Key(usize),
}

/// The open undo group of continuous pad edits (`useBeatActions.editGroupOpen`): closed by
/// `endPadEdit` or after 700 ms idle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EditGroup {
    pub due: f64,
}

/// The step grid's gesture (`stepGesture`): a pointer drag (explicit end) or a burst of
/// wheel / key velocity nudges (closed after 400 ms idle).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StepGesture {
    Drag,
    Burst { due: f64 },
}

/// The step grid's pointer drag (`Drag` in StepSequencer.tsx).
#[derive(Debug, Clone, PartialEq)]
pub enum StepDrag {
    Paint {
        mode: mdaw_model::beat_helpers::PaintMode,
        visited: Vec<usize>,
        grouped: bool,
    },
    Velocity {
        pad: usize,
        step: usize,
        start_y: f32,
        start_vel: f64,
        last: f64,
        grouped: bool,
    },
}

/// The pad editor's chop panel (`ChopMode`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChopUi {
    Transients { sensitivity: f64 },
    Equal { count: usize },
}

/// A knob drag (`dragRef` + `pendingValue` in Knob.tsx).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KnobDrag {
    pub id: crate::ui::knob::KnobId,
    pub start_value: f64,
    pub pending: f64,
}

/// A row of the sample browser being dragged onto a pad (`application/x-micro-daw-sample`).
#[derive(Debug, Clone, PartialEq)]
pub struct SampleDrag {
    pub path: String,
    pub name: String,
    /// The pad under the pointer (the "Drop to load" highlight).
    pub over: Option<usize>,
    /// Window point of the pointer (drag ghost).
    pub at: [f32; 2],
}

/// The beat view's own UI state (BeatView / PadGrid / PadEditor / StepSequencer locals).
pub struct BeatUi {
    pub layout: PadLayout,
    pub grid: StepGrid,
    pub show_keys: bool,
    pub chop: ChopUi,
    /// Slices of the open chop panel (recomputed while it's open).
    pub slices: Vec<SliceRange>,
    pub hold: Option<PadHold>,
    pub step_focus: StepCell,
    pub step_kb_focus: bool,
    pub step_drag: Option<StepDrag>,
    /// The live-region text of the focused step cell (empty until used by keyboard).
    pub step_announce: String,
    pub trim_drag: Option<bool>,
    pub knob_drag: Option<KnobDrag>,
    pub bpm_drag: Option<(f32, f64, bool)>,
    pub bpm_edit: Option<TextEdit>,
    pub kit_name: TextEdit,
    pub rename: TextEdit,
    pub drop_over: Option<usize>,
}

/// The note-repeat service's UI mirror (`useNoteRepeat`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoteRepeat {
    pub enabled: bool,
    pub rate: NoteRepeatRate,
}

/// Values the frame loop moves without re-running the view.
pub struct LiveValues {
    /// Hit-light opacity per pad.
    pub flash: Vec<Live<f32>>,
    /// Press scale per pad.
    pub scale: Vec<Live<f32>>,
    /// Step-grid playhead: x offset and opacity.
    pub playhead_px: Live<[f32; 2]>,
    pub playhead_on: Live<f32>,
    /// "bar.beat" of the loop position LCD.
    pub loop_pos: LiveText,
    last_step: Option<i64>,
}

/// Everything the view reads (one state, like `App.tsx` + its hooks).
pub struct App {
    pub svc: Services,
    pub start: StartPhase,
    pub start_error: Option<String>,
    pub mode: AppMode,
    pub view: DawView,
    pub browser_open: bool,
    pub sidebar_open: bool,
    pub menu: Option<Menu>,
    pub beat: BeatUi,
    pub repeat: NoteRepeat,
    pub live: LiveValues,
    /// Pad buffers the view loads lazily (interior mutability: views read `&App`).
    pub buffers: std::cell::RefCell<PadBuffers>,
    pub clipboard: Option<PadSettings>,
    pub user_kits: Vec<KitState>,
    pub edit_group: Option<EditGroup>,
    pub step_gesture: Option<StepGesture>,
    pub sample_drag: Option<SampleDrag>,
    pub browser: crate::ui::browser::BrowserState,
    pub dock_open: bool,
    /// Computer-key pads held (key → pad).
    pub held_keys: HashMap<String, usize>,
    /// Doc version of the last load / save (`isDirty` = the doc changed since).
    pub clean_version: u64,
    /// `MDAW_SMOKE` scripted run state.
    pub smoke: Option<crate::smoke::Smoke>,
    /// The last pad flash (pad, peak light opacity), for tests.
    pub last_flash: Option<(usize, f32)>,
    /// The live instrument's preset (name, category label) for the top bar.
    pub preset: (String, &'static str),
    /// The step grid's cell width and row height (set by the view, read by the playhead).
    pub steps_geometry: std::cell::Cell<(f32, f32)>,
    /// The trim waveform's width (set by the view, read by the handle drags).
    pub trim_width: std::cell::Cell<f32>,
    /// UI clock (seconds; virtual in headless runs).
    pub now: f64,
    pub waker: Option<Waker>,
    /// Signature of what the view shows from the engine (re-render when it changes).
    seen: Seen,
}

#[derive(Debug, Clone, Default, PartialEq)]
struct Seen {
    doc: u64,
    kit: u64,
    packs: u64,
    count_in: u32,
    state: Option<TransportState>,
    recording: bool,
    can_undo: bool,
    can_redo: bool,
    devices: usize,
    active_notes: Vec<i64>,
    buffers: u64,
}

pub const EDIT_IDLE: f64 = 0.7;
pub const STEP_BURST_IDLE: f64 = 0.4;

fn read_layout(s: &SettingsStore) -> PadLayout {
    PadLayout::from_label(store::read_beat_pad_layout(s)).unwrap_or(PadLayout::FourByFour)
}

fn read_grid(s: &SettingsStore) -> StepGrid {
    StepGrid::from_label(store::read_beat_step_grid(s)).unwrap_or(StepGrid::S16)
}

impl App {
    /// The app over `settings` (persisted preferences are read from it).
    pub fn with_settings(mode: RunMode, settings: SettingsStore) -> Self {
        let layout = read_layout(&settings);
        let grid = read_grid(&settings);
        let show_keys = store::read_beat_key_hints(&settings);
        let browser_open = store::read_browser_open(&settings);
        let user_kits = store::read_kits(&settings, crate::actions::normalize_stored_kit);
        let browser = crate::ui::browser::BrowserState::new(&settings);
        let mut svc = Services::new(mode, settings);
        let preset = preset_label(&mut svc);
        let clean_version = svc.engine.doc().state_version();
        let selected = svc.engine.doc().get_selected_pad();
        Self {
            svc,
            start: StartPhase::Idle,
            start_error: None,
            mode: AppMode::Daw,
            view: DawView::Beat,
            browser_open,
            sidebar_open: false,
            menu: None,
            beat: BeatUi {
                layout,
                grid,
                show_keys,
                chop: ChopUi::Transients { sensitivity: 0.5 },
                slices: Vec::new(),
                hold: None,
                step_focus: StepCell {
                    pad: selected,
                    step: 0,
                },
                step_kb_focus: false,
                step_drag: None,
                step_announce: String::new(),
                trim_drag: None,
                knob_drag: None,
                bpm_drag: None,
                bpm_edit: None,
                kit_name: TextEdit::new(""),
                rename: TextEdit::new(""),
                drop_over: None,
            },
            repeat: NoteRepeat {
                enabled: false,
                rate: NoteRepeatRate::R16,
            },
            live: LiveValues {
                flash: (0..16).map(|_| Live::new(0.0f32)).collect(),
                scale: (0..16).map(|_| Live::new(1.0f32)).collect(),
                playhead_px: Live::new([0.0, 0.0]),
                playhead_on: Live::new(0.0),
                loop_pos: LiveText::new("1.1"),
                last_step: None,
            },
            buffers: std::cell::RefCell::new(PadBuffers::new(mode == RunMode::Headless)),
            clipboard: None,
            user_kits,
            edit_group: None,
            step_gesture: None,
            sample_drag: None,
            browser,
            dock_open: false,
            held_keys: HashMap::new(),
            preset,
            clean_version,
            last_flash: None,
            smoke: (mode == RunMode::Window && crate::smoke::enabled()).then(Default::default),
            steps_geometry: std::cell::Cell::new((18.0, 14.0)),
            trim_width: std::cell::Cell::new(1.0),
            now: 0.0,
            waker: None,
            seen: Seen::default(),
        }
    }

    /// The window app: settings from the data folder.
    pub fn window() -> Self {
        let dir = crate::services::data_dir();
        let _ = std::fs::create_dir_all(&dir);
        Self::with_settings(RunMode::Window, SettingsStore::open(&dir))
    }

    /// A headless app (snapshots, tests): in-memory settings, offline engine.
    pub fn headless() -> Self {
        Self::with_settings(RunMode::Headless, SettingsStore::in_memory())
    }

    pub fn started(&self) -> bool {
        !matches!(self.start, StartPhase::Idle | StartPhase::Starting)
    }

    pub fn transport(&self) -> TransportState {
        self.svc.engine.get_state()
    }

    /// `transportState !== 'stopped'`.
    pub fn playing(&self) -> bool {
        self.transport() != TransportState::Stopped
    }

    pub fn count_in(&self) -> u32 {
        self.svc.engine.get_count_in_remaining()
    }

    pub fn loop_recording(&self) -> bool {
        self.svc.engine.doc().is_loop_recording()
    }

    // --- start screen ----------------------------------------------------------------------

    /// `StartScreen.start` + `handleStart`.
    pub fn start_session(&mut self) {
        if self.start != StartPhase::Idle {
            return;
        }
        self.start = StartPhase::Starting;
        self.start_error = None;
        // MIDI failing to enable does not fail the start (webmidi's enable is caught in the TS).
        let _ = self.svc.start_session();
        self.start = StartPhase::Exiting {
            until: self.now + 0.38,
        };
    }

    // --- frame loop ------------------------------------------------------------------------

    /// `poll`: engine, MIDI and packs work of one frame, timers, live values. True when the
    /// view must re-run.
    pub fn poll(&mut self, now: f64, tx: &mut Tx) -> bool {
        self.now = now;
        let mut changed = false;
        if let StartPhase::Exiting { until } = self.start
            && now >= until
        {
            self.start = StartPhase::Done;
            changed = true;
        }
        self.svc.advance(now);
        for e in self.svc.engine.take_ui_events() {
            if let EngineEvent::PadHit { pad, velocity, .. } = e {
                self.flash(pad, velocity);
            }
        }
        changed |= self.svc.pump_midi();
        changed |= self.svc.pump_packs();
        changed |= self.buffers.get_mut().poll();
        changed |= self.timers(now);
        self.svc.autosave(now);
        self.update_live();
        let seen = self.seen_now();
        if seen != self.seen {
            self.seen = seen;
            changed = true;
        }
        // Keep frames coming while something moves on its own.
        let busy = self.playing()
            || self.count_in() > 0
            || self.svc.packs.busy()
            || self.buffers.borrow().loading()
            || matches!(self.start, StartPhase::Exiting { .. })
            || self.edit_group.is_some()
            || matches!(self.step_gesture, Some(StepGesture::Burst { .. }));
        if busy {
            tx.wake_after(Duration::from_millis(16));
        } else {
            tx.wake_after(Duration::from_millis(100));
        }
        changed
    }

    fn seen_now(&self) -> Seen {
        let doc = self.svc.engine.doc();
        let um = doc.undo_manager();
        Seen {
            doc: doc.state_version(),
            kit: doc.kit_version(),
            packs: self.svc.packs.version(),
            count_in: self.count_in(),
            state: Some(self.transport()),
            recording: doc.is_loop_recording(),
            can_undo: um.can_undo(),
            can_redo: um.can_redo(),
            devices: self.svc.midi.host.devices().len(),
            active_notes: self.active_notes(),
            buffers: self.buffers.borrow().version(),
        }
    }

    /// Held notes for the live readouts (TopBar, dock).
    pub fn active_notes(&self) -> Vec<i64> {
        let mut notes: Vec<i64> = self
            .svc
            .midi
            .host
            .shared()
            .lock()
            .router
            .active_notes()
            .iter()
            .map(|n| n.note)
            .collect();
        notes.sort_unstable();
        notes.dedup();
        notes
    }

    /// Undo-group idle timers (`setTimeout` in the hooks).
    fn timers(&mut self, now: f64) -> bool {
        let mut changed = false;
        if let Some(g) = self.edit_group
            && now >= g.due
        {
            self.end_pad_edit();
            changed = true;
        }
        if let Some(StepGesture::Burst { due }) = self.step_gesture
            && now >= due
        {
            self.end_step_gesture();
            changed = true;
        }
        changed
    }

    /// The pad flash (PadGrid's `flash`): light `0.35 + 0.6 v` → 0 over 260 ms, scale
    /// 0.965 → 1 over 140 ms.
    pub fn flash(&mut self, pad: usize, velocity: f64) {
        let v = if velocity > 1.0 {
            velocity / 127.0
        } else {
            velocity
        }
        .clamp(0.25, 1.0) as f32;
        self.last_flash = Some((pad, 0.35 + 0.6 * v));
        if let Some(l) = self.live.flash.get(pad) {
            l.snap(0.35 + 0.6 * v);
            l.set(0.0);
        }
        if let Some(s) = self.live.scale.get(pad) {
            s.snap(0.965);
            s.set(1.0);
        }
    }

    /// Playhead + loop position, from the engine position (no view re-run).
    fn update_live(&mut self) {
        let doc = self.svc.engine.doc();
        let bars = doc.get_loop_settings().bars as f64;
        let start = self.pattern_start();
        let playing = self.playing();
        let pos = self.svc.engine.get_position();
        if playing {
            let len = bars * 4.0;
            let rel = ((pos - start) % len + len) % len;
            self.live.loop_pos.set(format!(
                "{}.{}",
                (rel / 4.0).floor() as i64 + 1,
                (rel % 4.0).floor() as i64 + 1
            ));
        } else {
            self.live.loop_pos.set("1.1");
        }
        let sb = mdaw_model::beat_helpers::step_beats_for(self.beat.grid);
        let step = if playing {
            mdaw_model::beat_helpers::playhead_step(pos, start, bars * 4.0, sb)
                .map(|s| s.floor() as i64)
        } else {
            None
        };
        if step != self.live.last_step {
            self.live.last_step = step;
            match step {
                Some(s) => {
                    self.live.playhead_on.snap(1.0);
                    let (cell_w, _) = self.steps_geometry.get();
                    self.live.playhead_px.snap([s as f32 * cell_w, 0.0]);
                }
                None => {
                    self.live.playhead_on.snap(0.0);
                }
            }
        }
    }

    /// The current playhead step (None = hidden), for tests.
    pub fn playhead(&self) -> Option<i64> {
        self.live.last_step
    }

    /// `patternStartBeat = loopMarkers.start ?? patternRegion.start ?? 0`.
    pub fn pattern_start(&self) -> f64 {
        let doc = self.svc.engine.doc();
        if let Some(m) = doc.get_loop_markers() {
            return m.start_beat;
        }
        doc.get_active_drum_track_id()
            .and_then(|id| doc.get_pattern_region(&id).map(|r| r.start_beat))
            .unwrap_or(0.0)
    }

    /// Writes a preference (errors ignored, like the TS `writePref`).
    pub fn with_settings_mut(&mut self, f: impl FnOnce(&mut SettingsStore)) {
        f(&mut self.svc.settings.borrow_mut());
    }
}

/// The live preset's name and category label (`PRESET_CATEGORIES`).
pub fn preset_label(svc: &mut Services) -> (String, &'static str) {
    use mdaw_engine::synth::presets::PRESET_CATEGORIES;
    let p = svc.engine.live().current_preset();
    let m = p.meta();
    let cat = PRESET_CATEGORIES
        .iter()
        .find(|c| c.0 == m.category)
        .map_or("Sound", |c| c.1);
    (m.name.clone(), cat)
}

impl App {
    /// `project.isDirty`.
    pub fn dirty(&self) -> bool {
        self.svc.engine.doc().state_version() != self.clean_version
    }

    pub fn toggle_menu(&mut self, m: Menu) {
        if self.menu.as_ref() == Some(&m) {
            self.close_menu();
        } else {
            self.menu = Some(m);
        }
    }

    pub fn close_menu(&mut self) {
        if self.menu == Some(Menu::Chop) {
            self.beat.slices.clear();
        }
        self.menu = None;
    }

    /// `switchMode`: `allNotesOff()` on every change.
    pub fn switch_mode(&mut self, mode: AppMode) {
        if mode == self.mode {
            return;
        }
        self.svc.with_router(|r, out| r.all_notes_off(out));
        self.mode = mode;
        self.menu = None;
    }

    /// `useAudioInput.toggleMute`.
    pub fn toggle_mute(&mut self) {
        self.svc.with_router(|r, out| {
            let m = r.muted();
            r.set_muted(!m, out);
        });
    }

    /// Commits the typed BPM (`parseFloat`, clamp + round; invalid → discarded).
    pub fn commit_bpm(&mut self) {
        let Some(edit) = self.beat.bpm_edit.take() else {
            return;
        };
        if let Ok(v) = edit.text.trim().parse::<f64>()
            && v.is_finite()
        {
            self.set_bpm(crate::ui::beat::transport::clamp_bpm(v));
        }
    }

    /// Releases the held on-screen pad press, exactly once.
    pub fn release_pad_hold(&mut self) {
        if let Some(h) = self.beat.hold.take() {
            let pad = match h {
                PadHold::Pointer(p) | PadHold::Key(p) => p,
            };
            self.svc.drum_release(pad);
        }
    }
}
