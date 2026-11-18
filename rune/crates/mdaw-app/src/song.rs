//! The Song view's state and actions (phase 8): `ArrangementView`'s local state (zoom, snap,
//! tool, region selection, rubber band, drags, context menus, the track-FX drawer), the
//! `arrangementActions` of `useArrangement.ts` the view calls, `App.tsx`'s Song-view state
//! (`editingRegion`, `showMixer`) and the Song transport. Every edit goes through the doc
//! engine (`ArrangementEngine::edit`), so undo labels are the doc's own.
//!
//! Electron bugs fixed in both apps (user decision 2026-11-18), see
//! `docs/rune-port/phase-8-song-view.md`:
//! - a region move / resize drag, a loop-handle drag and a track-FX knob drag are ONE undo
//!   step each (the TS pushed one per pointer move);
//! - the rubber band tests regions against the rows as drawn (group headers, collapsed groups,
//!   render order); the TS skipped advancing its running top for rows above the band, so a band
//!   below the first track selected nothing.

use std::cell::RefCell;
use std::collections::HashMap;

use mdaw_engine::engine::TransportState;
use mdaw_model::arrangement_helpers::snap_beat;
use mdaw_model::doc::MarkerUpdate;
use mdaw_model::doc::EffectName;
use mdaw_model::{AutomationParameter, InstrumentType, RegionNote, Track, TrackInstrument};
use rune::{Live, LiveText, TextEdit};
use serde_json::{Map, Value};

use crate::app::{App, Menu};
use crate::piano::PianoUi;

/// `HEADER_WIDTH` of the track-header column.
pub const HEADER_W: f32 = 224.0;
/// `RULER_HEIGHT`.
pub const RULER_H: f32 = 28.0;
/// A track row (`h-16`).
pub const TRACK_H: f32 = 64.0;
/// `AUTOMATION_LANE_HEIGHT`.
pub const AUTO_H: f32 = 48.0;
/// A group header row (`h-7`).
pub const GROUP_H: f32 = 28.0;
/// `DEFAULT_PX_PER_BEAT` / `MIN_PX_PER_BEAT` / `MAX_PX_PER_BEAT`.
pub const DEFAULT_PPB: f64 = 30.0;
pub const MIN_PPB: f64 = 10.0;
pub const MAX_PPB: f64 = 80.0;
/// `SNAP_VALUES` with their toolbar labels.
pub const SNAP_VALUES: [(f64, &str); 5] = [
    (0.25, "1/16"),
    (0.5, "1/8"),
    (1.0, "1/4"),
    (2.0, "1/2"),
    (4.0, "Bar"),
];
/// `DEFAULT_MARKER_NAMES`.
pub const DEFAULT_MARKER_NAMES: [&str; 8] = [
    "Intro",
    "Verse",
    "Chorus",
    "Bridge",
    "Drop",
    "Breakdown",
    "Outro",
    "Section",
];
/// `MIN_VOLUME` / `MAX_VOLUME` of tracks.
pub const MIN_VOLUME: f64 = -40.0;
pub const MAX_VOLUME: f64 = 0.0;
/// `ALL_AUTOMATION_PARAMS` (the automation menu's order).
pub const ALL_AUTOMATION_PARAMS: [AutomationParameter; 10] = [
    AutomationParameter::Volume,
    AutomationParameter::Pan,
    AutomationParameter::ReverbWet,
    AutomationParameter::DelayWet,
    AutomationParameter::ChorusDepth,
    AutomationParameter::DistortionWet,
    AutomationParameter::FilterCutoff,
    AutomationParameter::EqLow,
    AutomationParameter::EqMid,
    AutomationParameter::EqHigh,
];

/// `ArrangementTool`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrTool {
    Pointer,
    Cut,
}

/// `DragMode` of a region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionDragMode {
    Move,
    ResizeLeft,
    ResizeRight,
}

/// A region drag (`dragStart` + `dragMode` of RegionBlock).
#[derive(Debug, Clone, PartialEq)]
pub struct RegionDrag {
    pub track: String,
    pub region: String,
    pub mode: RegionDragMode,
    pub start_beat: f64,
    pub length: f64,
}

/// The rubber band, in content coordinates of the lanes (x from beat 0, y from the first row).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Band {
    pub start: [f32; 2],
    pub current: [f32; 2],
    pub active: bool,
}

/// Which loop handle is dragged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopEdge {
    Start,
    End,
}

/// A track-FX section (`SectionName`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FxSection {
    Filter,
    Eq,
    Compressor,
    Chorus,
    Delay,
    Distortion,
    Reverb,
}

impl FxSection {
    pub const ORDER: [FxSection; 7] = [
        FxSection::Filter,
        FxSection::Eq,
        FxSection::Compressor,
        FxSection::Chorus,
        FxSection::Delay,
        FxSection::Distortion,
        FxSection::Reverb,
    ];
    pub fn label(self) -> &'static str {
        match self {
            FxSection::Filter => "Filter",
            FxSection::Eq => "EQ",
            FxSection::Compressor => "Compressor",
            FxSection::Chorus => "Chorus",
            FxSection::Delay => "Delay",
            FxSection::Distortion => "Distortion",
            FxSection::Reverb => "Reverb",
        }
    }
    pub fn effect(self) -> EffectName {
        match self {
            FxSection::Filter => EffectName::Filter,
            FxSection::Eq => EffectName::Eq,
            FxSection::Compressor => EffectName::Compressor,
            FxSection::Chorus => EffectName::Chorus,
            FxSection::Delay => EffectName::Delay,
            FxSection::Distortion => EffectName::Distortion,
            FxSection::Reverb => EffectName::Reverb,
        }
    }
    /// `(fx[name] as { enabled }).enabled`.
    pub fn enabled(self, fx: &mdaw_model::TrackEffectState) -> bool {
        match self {
            FxSection::Filter => fx.filter.enabled,
            FxSection::Eq => fx.eq.enabled,
            FxSection::Compressor => fx.compressor.enabled,
            FxSection::Chorus => fx.chorus.enabled,
            FxSection::Delay => fx.delay.enabled,
            FxSection::Distortion => fx.distortion.enabled,
            FxSection::Reverb => fx.reverb.enabled,
        }
    }
    pub fn key(self) -> &'static str {
        self.effect().key()
    }
}

/// The Song view's popovers / menus (inside [`Menu::Song`]).
#[derive(Debug, Clone, PartialEq)]
pub enum SongMenu {
    /// `RegionContextMenu` at a window point.
    Region {
        x: f32,
        y: f32,
        track: String,
        region: String,
    },
    /// `MarkerContextMenu` at a window point.
    Marker {
        x: f32,
        y: f32,
        marker: String,
        renaming: bool,
    },
    /// A track header's "Automate" menu.
    Automation { track: String },
    /// A track header's instrument popover.
    Instrument { track: String },
    /// The toolbar's snap select.
    Snap,
    /// The transport's marker jump select.
    MarkerJump,
    /// Track FX selects.
    FilterType,
    LfoWave,
    /// Piano roll selects / popover.
    PrGrid,
    PrLength,
    PrShortcuts,
}

/// The instrument dock's local state (`InstrumentDock` in the Song view: pads and keys shown).
pub struct DockUi {
    pub show_pads: bool,
    pub show_keys: bool,
    /// `micro-daw:dock-pad-bank` (A = false, B = true).
    pub bank_b: bool,
}

/// Values the frame loop moves without re-running the view.
pub struct SongLive {
    /// Arrangement playhead x (viewport points) and its opacity.
    pub playhead: Live<[f32; 2]>,
    /// The transport LCD: "bar.beat.six" parts and the time.
    pub lcd_bar: LiveText,
    pub lcd_beat: LiveText,
    pub lcd_six: LiveText,
    pub lcd_time: LiveText,
    /// Mixer meter fills (vertical offset of the colour ramp) per track id; "" = master.
    pub meters: RefCell<HashMap<String, Live<[f32; 2]>>>,
    /// Mixer level per track id ("" = master), dB, read each frame while the mixer shows.
    pub levels: RefCell<HashMap<String, f64>>,
    pub clip: Live<f32>,
    /// Piano roll playhead x (grid viewport points).
    pub pr_playhead: Live<[f32; 2]>,
    pub pr_playhead_on: Live<f32>,
}

/// `ArrangementView` + `App.tsx` Song-view state.
pub struct SongUi {
    pub ppb: f64,
    pub snap: f64,
    pub tool: ArrTool,
    /// Selected regions in selection order: (region id, track id).
    pub selection: Vec<(String, String)>,
    /// Scroll of the timeline content (x in points from beat 0, y from the first row).
    pub scroll: [f32; 2],
    /// The timeline viewport (lanes area) size, measured by the view.
    pub viewport: std::cell::Cell<[f32; 2]>,
    pub region_drag: Option<RegionDrag>,
    pub band: Option<Band>,
    pub loop_drag: Option<LoopEdge>,
    /// A marker being dragged: (id, beat shown while dragging, moved).
    pub marker_drag: Option<(String, f64, bool)>,
    /// A pan dial drag: (track, start value).
    pub pan_drag: Option<(String, f64)>,
    /// A mixer fader drag: (track, start value).
    pub fader_drag: Option<(String, f64)>,
    /// A track-FX knob drag opened an undo group.
    pub fx_group: bool,
    pub fx_track: Option<String>,
    /// Expanded FX sections of the open drawer (reset when it opens on another track).
    pub fx_expanded: Vec<FxSection>,
    pub show_mixer: bool,
    /// `editingRegion`: (track id, region id).
    pub editing: Option<(String, String)>,
    pub piano: PianoUi,
    /// Group being renamed: (id, draft).
    pub group_rename: Option<(String, TextEdit)>,
    pub marker_rename: TextEdit,
    pub instrument_search: TextEdit,
    pub vibe: Option<mdaw_engine::synth::presets::PresetVibe>,
    pub dock: DockUi,
    /// The on-screen keyboard's pointer note (one pointer: the mouse).
    pub key_pointer: Option<i64>,
    pub live: SongLive,
    /// The last auto-follow target (avoids re-running the view every frame).
    pub last_follow: Option<f32>,
}

impl SongUi {
    pub fn new(bank_b: bool, piano: PianoUi) -> Self {
        Self {
            ppb: DEFAULT_PPB,
            snap: 1.0,
            tool: ArrTool::Pointer,
            selection: Vec::new(),
            scroll: [0.0, 0.0],
            viewport: std::cell::Cell::new([1000.0, 600.0]),
            region_drag: None,
            band: None,
            loop_drag: None,
            marker_drag: None,
            pan_drag: None,
            fader_drag: None,
            fx_group: false,
            fx_track: None,
            fx_expanded: Vec::new(),
            show_mixer: false,
            editing: None,
            piano,
            group_rename: None,
            marker_rename: TextEdit::new(""),
            instrument_search: TextEdit::new(""),
            vibe: None,
            dock: DockUi {
                show_pads: true,
                show_keys: true,
                bank_b,
            },
            key_pointer: None,
            live: SongLive {
                playhead: Live::new([0.0, 0.0]),
                lcd_bar: LiveText::new("  1"),
                lcd_beat: LiveText::new("1"),
                lcd_six: LiveText::new("1"),
                lcd_time: LiveText::new("0:00.0"),
                meters: RefCell::new(HashMap::new()),
                levels: RefCell::new(HashMap::new()),
                clip: Live::new(0.0),
                pr_playhead: Live::new([0.0, 0.0]),
                pr_playhead_on: Live::new(0.0),
            },
            last_follow: None,
        }
    }

    pub fn is_selected(&self, region: &str) -> bool {
        self.selection.iter().any(|(r, _)| r == region)
    }
}

/// A text field's draft with all of it selected (`input.focus(); input.select()`).
pub fn selected_edit(text: impl Into<String>) -> TextEdit {
    let text = text.into();
    let n = text.len();
    let mut e = TextEdit::new(text);
    e.set_selection(0, n);
    e
}

/// `formatBBS`: bars.beats.sixteenths (4/4), bar padded to 3 with spaces; negatives clamp to 0.
pub fn format_bbs(beats: f64) -> (String, String, String) {
    let b = beats.max(0.0);
    let bar = (b / 4.0).floor() as i64 + 1;
    let beat = (b % 4.0).floor() as i64 + 1;
    let six = ((b % 1.0) * 4.0).floor() as i64 + 1;
    (format!("{bar:>3}"), beat.to_string(), six.to_string())
}

/// `formatTime`: `m:ss.s`.
pub fn format_time(beats: f64, bpm: f64) -> String {
    let sec = (beats * 60.0 / bpm).max(0.0);
    let m = (sec / 60.0).floor();
    let s = sec - m * 60.0;
    format!("{}:{:0>4}", m as i64, format!("{s:.1}"))
}

/// `panLabel` / `fmtPan`: "C", "L{n}" or "R{n}".
pub fn pan_label(pan: f64) -> String {
    if pan > 0.005 {
        format!("R{}", (pan * 100.0).round() as i64)
    } else if pan < -0.005 {
        format!("L{}", (pan.abs() * 100.0).round() as i64)
    } else {
        "C".to_owned()
    }
}

/// A track's row height: 64 + 48 per visible automation lane (`getTrackHeight`).
pub fn track_height(t: &Track) -> f32 {
    TRACK_H + visible_lanes(t).len() as f32 * AUTO_H
}

pub fn visible_lanes(t: &Track) -> Vec<&mdaw_model::AutomationLane> {
    t.automation
        .as_ref()
        .map(|l| l.iter().filter(|l| l.visible).collect())
        .unwrap_or_default()
}

/// One row of the header column / lanes, in render order (groups first, then ungrouped
/// tracks).
#[derive(Debug, Clone, PartialEq)]
pub enum Row {
    Group { id: String, top: f32 },
    Track { id: String, top: f32, height: f32 },
}

/// The render order of `ArrangementView` with each row's top (content y).
pub fn rows(tracks: &[Track], groups: &[mdaw_model::TrackGroup]) -> Vec<Row> {
    let mut out = Vec::new();
    let mut y = 0.0f32;
    let mut rendered: Vec<&str> = Vec::new();
    for g in groups {
        let gt: Vec<&Track> = g
            .track_ids
            .iter()
            .filter_map(|id| tracks.iter().find(|t| &t.id == id))
            .collect();
        if gt.is_empty() {
            continue;
        }
        out.push(Row::Group {
            id: g.id.clone(),
            top: y,
        });
        y += GROUP_H;
        for t in gt {
            rendered.push(&t.id);
            if g.collapsed {
                continue;
            }
            let h = track_height(t);
            out.push(Row::Track {
                id: t.id.clone(),
                top: y,
                height: h,
            });
            y += h;
        }
    }
    for t in tracks {
        if rendered.contains(&t.id.as_str()) {
            continue;
        }
        let h = track_height(t);
        out.push(Row::Track {
            id: t.id.clone(),
            top: y,
            height: h,
        });
        y += h;
    }
    out
}

/// The height of every row.
pub fn rows_height(rows: &[Row], tracks: &[Track]) -> f32 {
    rows.iter()
        .map(|r| match r {
            Row::Group { .. } => GROUP_H,
            Row::Track { height, .. } => *height,
        })
        .sum::<f32>()
        .max(if tracks.is_empty() { 0.0 } else { 0.0 })
}

/// The regions a rubber band from `a` to `b` (content points) touches: a track row overlapping
/// the band vertically, a region overlapping it horizontally. Rows as drawn (Electron bug fixed
/// in both apps, see the module docs).
pub fn band_hits(
    tracks: &[Track],
    rows: &[Row],
    a: [f32; 2],
    b: [f32; 2],
    ppb: f64,
) -> Vec<(String, String)> {
    let (x1, x2) = (a[0].min(b[0]) as f64, a[0].max(b[0]) as f64);
    let (y1, y2) = (a[1].min(b[1]), a[1].max(b[1]));
    let mut out = Vec::new();
    for r in rows {
        let Row::Track { id, top, height } = r else {
            continue;
        };
        let bottom = top + height;
        if bottom <= y1 || *top >= y2 {
            continue;
        }
        let Some(t) = tracks.iter().find(|t| &t.id == id) else {
            continue;
        };
        for region in &t.regions {
            let left = region.start_beat * ppb;
            let right = (region.start_beat + region.length_beats) * ppb;
            if right > x1 && left < x2 {
                out.push((region.id.clone(), t.id.clone()));
            }
        }
    }
    out
}

/// A track-FX parameter: section, JSON key, range, step, double-click default, format.
#[derive(Debug, Clone, Copy)]
pub struct FxParam {
    pub section: FxSection,
    pub key: &'static str,
    pub label: &'static str,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub default: f64,
    pub fmt: fn(f64) -> String,
}

fn fmt_hz(v: f64) -> String {
    if v >= 1000.0 {
        format!("{:.1}k", v / 1000.0)
    } else {
        format!("{}", v.round() as i64)
    }
}
fn fmt_pct(v: f64) -> String {
    format!("{}%", (v * 100.0).round() as i64)
}
fn fmt_db(v: f64) -> String {
    format!("{}{:.1}dB", if v > 0.0 { "+" } else { "" }, v)
}
fn fmt_ms(v: f64) -> String {
    format!("{}ms", (v * 1000.0).round() as i64)
}
fn fmt_q(v: f64) -> String {
    format!("{v:.1}")
}
fn fmt_ratio(v: f64) -> String {
    format!("{v:.1}:1")
}
fn fmt_rate(v: f64) -> String {
    format!("{v:.1}Hz")
}
fn fmt_thresh(v: f64) -> String {
    format!("{}dB", crate::song::js_num(v))
}

/// `String(v)` of a JS number (integers without a fraction).
pub fn js_num(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

/// The knobs of `TrackEffectsPanel`, in drawing order.
pub const FX_PARAMS: [FxParam; 18] = [
    FxParam {
        section: FxSection::Filter,
        key: "cutoff",
        label: "Cutoff",
        min: 60.0,
        max: 18000.0,
        step: 1.0,
        default: 18000.0,
        fmt: fmt_hz,
    },
    FxParam {
        section: FxSection::Filter,
        key: "resonance",
        label: "Reso",
        min: 0.0,
        max: 20.0,
        step: 0.1,
        default: 1.0,
        fmt: fmt_q,
    },
    FxParam {
        section: FxSection::Filter,
        key: "lfoRate",
        label: "LFO Rate",
        min: 0.1,
        max: 20.0,
        step: 0.1,
        default: 1.0,
        fmt: fmt_rate,
    },
    FxParam {
        section: FxSection::Filter,
        key: "lfoDepth",
        label: "LFO Dep",
        min: 0.0,
        max: 1.0,
        step: 0.01,
        default: 0.0,
        fmt: fmt_pct,
    },
    FxParam {
        section: FxSection::Eq,
        key: "low",
        label: "Low",
        min: -12.0,
        max: 12.0,
        step: 0.5,
        default: 0.0,
        fmt: fmt_db,
    },
    FxParam {
        section: FxSection::Eq,
        key: "mid",
        label: "Mid",
        min: -12.0,
        max: 12.0,
        step: 0.5,
        default: 0.0,
        fmt: fmt_db,
    },
    FxParam {
        section: FxSection::Eq,
        key: "high",
        label: "High",
        min: -12.0,
        max: 12.0,
        step: 0.5,
        default: 0.0,
        fmt: fmt_db,
    },
    FxParam {
        section: FxSection::Compressor,
        key: "threshold",
        label: "Thresh",
        min: -60.0,
        max: 0.0,
        step: 1.0,
        default: -24.0,
        fmt: fmt_thresh,
    },
    FxParam {
        section: FxSection::Compressor,
        key: "ratio",
        label: "Ratio",
        min: 1.0,
        max: 20.0,
        step: 0.5,
        default: 4.0,
        fmt: fmt_ratio,
    },
    FxParam {
        section: FxSection::Compressor,
        key: "attack",
        label: "Attack",
        min: 0.001,
        max: 1.0,
        step: 0.001,
        default: 0.003,
        fmt: fmt_ms,
    },
    FxParam {
        section: FxSection::Compressor,
        key: "release",
        label: "Release",
        min: 0.01,
        max: 1.0,
        step: 0.01,
        default: 0.25,
        fmt: fmt_ms,
    },
    FxParam {
        section: FxSection::Chorus,
        key: "depth",
        label: "Depth",
        min: 0.0,
        max: 1.0,
        step: 0.01,
        default: 0.5,
        fmt: fmt_pct,
    },
    FxParam {
        section: FxSection::Delay,
        key: "time",
        label: "Time",
        min: 0.01,
        max: 1.0,
        step: 0.01,
        default: 0.25,
        fmt: fmt_ms,
    },
    FxParam {
        section: FxSection::Delay,
        key: "feedback",
        label: "Fdbk",
        min: 0.0,
        max: 0.95,
        step: 0.01,
        default: 0.3,
        fmt: fmt_pct,
    },
    FxParam {
        section: FxSection::Delay,
        key: "wet",
        label: "Mix",
        min: 0.0,
        max: 1.0,
        step: 0.01,
        default: 0.3,
        fmt: fmt_pct,
    },
    FxParam {
        section: FxSection::Distortion,
        key: "amount",
        label: "Drive",
        min: 0.0,
        max: 1.0,
        step: 0.01,
        default: 0.3,
        fmt: fmt_pct,
    },
    FxParam {
        section: FxSection::Distortion,
        key: "wet",
        label: "Mix",
        min: 0.0,
        max: 1.0,
        step: 0.01,
        default: 0.5,
        fmt: fmt_pct,
    },
    FxParam {
        section: FxSection::Reverb,
        key: "wet",
        label: "Wet",
        min: 0.0,
        max: 1.0,
        step: 0.01,
        default: 0.3,
        fmt: fmt_pct,
    },
];

/// A track's current value of an FX parameter (`fx[section][key]`, through the doc's JSON).
pub fn fx_value(t: &Track, p: &FxParam) -> f64 {
    let fx = t.effects.clone().unwrap_or_default();
    serde_json::to_value(&fx)
        .ok()
        .and_then(|v| v.get(p.section.key())?.get(p.key)?.as_f64())
        .unwrap_or(p.default)
}

impl App {
    // --- lookups ------------------------------------------------------------------------------

    pub fn tracks(&self) -> &[Track] {
        self.svc.engine.doc().get_tracks()
    }

    pub fn find_track(&self, id: &str) -> Option<&Track> {
        self.tracks().iter().find(|t| t.id == id)
    }

    pub fn song_rows(&self) -> Vec<Row> {
        let doc = self.svc.engine.doc();
        let groups = doc.get_groups().to_vec();
        rows(doc.get_tracks(), &groups)
    }

    pub fn song_state(&self) -> TransportState {
        self.svc.engine.get_state()
    }

    /// `hasClipboard()` of the region clipboard.
    pub fn has_region_clipboard(&self) -> bool {
        self.svc.engine.doc().has_clipboard()
    }

    // --- tracks -------------------------------------------------------------------------------

    /// `addTrack(type)` (preset 0).
    pub fn add_song_track(&mut self, kind: InstrumentType) {
        self.svc.engine.edit(|d| {
            d.add_track(kind, 0);
        });
    }

    pub fn remove_song_track(&mut self, id: &str) {
        self.svc.engine.edit(|d| d.remove_track(id));
        self.song.selection.retain(|(_, t)| t != id);
    }

    pub fn set_track_instrument(&mut self, id: &str, kind: InstrumentType, preset: i64) {
        let inst = TrackInstrument {
            kind,
            preset_index: preset,
            synth_overrides: None,
            kit: None,
            extra: Map::new(),
        };
        self.svc.engine.edit(|d| d.set_track_instrument(id, &inst));
    }

    pub fn set_track_volume(&mut self, id: &str, db: f64) {
        self.svc.engine.edit(|d| d.set_track_volume(id, db));
    }

    pub fn set_track_pan(&mut self, id: &str, pan: f64) {
        self.svc.engine.edit(|d| d.set_track_pan(id, pan));
    }

    pub fn toggle_track_mute(&mut self, id: &str) {
        let m = self.find_track(id).is_some_and(|t| t.muted);
        self.svc.engine.edit(|d| d.set_track_mute(id, !m));
    }

    pub fn toggle_track_solo(&mut self, id: &str) {
        let m = self.find_track(id).is_some_and(|t| t.solo);
        self.svc.engine.edit(|d| d.set_track_solo(id, !m));
    }

    /// The arm toggle: arm this track, or disarm (`armTrack(armed === id ? null : id)`).
    pub fn toggle_arm(&mut self, id: &str) {
        let armed = self.svc.engine.doc().get_armed_track_id() == Some(id);
        self.svc.engine.edit(|d| {
            d.arm_track_for_recording(if armed { None } else { Some(id) });
        });
    }

    /// The FX button: toggle this track's drawer (one track at a time).
    pub fn toggle_fx(&mut self, id: &str) {
        if self.song.fx_track.as_deref() == Some(id) {
            self.song.fx_track = None;
        } else {
            self.song.fx_track = Some(id.to_owned());
            // Sections that are on start expanded (the panel remounts per track).
            let fx = self
                .find_track(id)
                .and_then(|t| t.effects.clone())
                .unwrap_or_default();
            self.song.fx_expanded = FxSection::ORDER
                .into_iter()
                .filter(|s| s.enabled(&fx))
                .collect();
        }
    }

    pub fn toggle_fx_section(&mut self, s: FxSection) {
        if let Some(i) = self.song.fx_expanded.iter().position(|x| *x == s) {
            self.song.fx_expanded.remove(i);
        } else {
            self.song.fx_expanded.push(s);
        }
    }

    /// `setTrackEffect(track, effect, params)`.
    pub fn set_track_effect(&mut self, id: &str, effect: EffectName, params: Map<String, Value>) {
        self.svc
            .engine
            .edit(|d| d.set_track_effect(id, effect, &params));
    }

    /// A knob of the FX drawer moved: inside a drag, the whole gesture is one undo step.
    pub fn set_fx_param(&mut self, p: &FxParam, v: f64) {
        let Some(id) = self.song.fx_track.clone() else {
            return;
        };
        if self.beat.knob_drag.is_some() && !self.song.fx_group {
            self.svc
                .engine
                .edit(|d| d.begin_undo_group("Change Track Effect"));
            self.song.fx_group = true;
        }
        let mut m = Map::new();
        m.insert(p.key.to_owned(), serde_json::json!(v));
        self.set_track_effect(&id, p.section.effect(), m);
    }

    /// The knob's `onChangeEnd`: closes the drag's undo group.
    pub fn end_fx_knob(&mut self) {
        if std::mem::take(&mut self.song.fx_group) {
            self.svc.engine.edit(|d| d.end_undo_group());
        }
    }

    pub fn toggle_fx_enabled(&mut self, s: FxSection) {
        let Some(id) = self.song.fx_track.clone() else {
            return;
        };
        let fx = self
            .find_track(&id)
            .and_then(|t| t.effects.clone())
            .unwrap_or_default();
        let mut m = Map::new();
        m.insert("enabled".into(), Value::Bool(!s.enabled(&fx)));
        self.set_track_effect(&id, s.effect(), m);
    }

    // --- automation -----------------------------------------------------------------------------

    /// A parameter clicked in the automation menu: remove its lane if it exists, else add it.
    pub fn toggle_automation_param(&mut self, id: &str, p: AutomationParameter) {
        let exists = self
            .find_track(id)
            .and_then(|t| t.automation.as_ref())
            .is_some_and(|l| l.iter().any(|l| l.parameter == p));
        self.svc.engine.edit(|d| {
            if exists {
                d.remove_automation_lane(id, p);
            } else {
                d.add_automation_lane(id, p);
            }
        });
    }

    /// "Clear all": every lane removed.
    pub fn clear_automation(&mut self, id: &str) {
        let lanes: Vec<AutomationParameter> = self
            .find_track(id)
            .and_then(|t| t.automation.as_ref())
            .map(|l| l.iter().map(|l| l.parameter).collect())
            .unwrap_or_default();
        self.svc.engine.edit(|d| {
            for p in lanes {
                d.remove_automation_lane(id, p);
            }
        });
    }

    pub fn set_automation_point(&mut self, id: &str, p: AutomationParameter, beat: f64, v: f64) {
        let snap = self.song.snap;
        self.svc
            .engine
            .edit(|d| d.set_automation_point(id, p, beat, v, snap));
    }

    pub fn delete_automation_point(&mut self, id: &str, p: AutomationParameter, index: usize) {
        self.svc
            .engine
            .edit(|d| d.delete_automation_point(id, p, index));
    }

    // --- regions --------------------------------------------------------------------------------

    /// `handleSelectRegion`.
    pub fn select_region(&mut self, track: &str, region: &str, add: bool) {
        let sel = &mut self.song.selection;
        if add {
            if let Some(i) = sel.iter().position(|(r, _)| r == region) {
                sel.remove(i);
            } else {
                sel.push((region.to_owned(), track.to_owned()));
            }
        } else {
            sel.clear();
            sel.push((region.to_owned(), track.to_owned()));
        }
    }

    pub fn clear_selection(&mut self) {
        self.song.selection.clear();
    }

    /// Mod+A: every region of every track.
    pub fn select_all_regions(&mut self) {
        let all: Vec<(String, String)> = self
            .tracks()
            .iter()
            .flat_map(|t| t.regions.iter().map(|r| (r.id.clone(), t.id.clone())))
            .collect();
        self.song.selection = all;
    }

    /// Delete / Backspace: the selected regions, else the region under the open context menu.
    pub fn delete_selected_regions(&mut self) {
        if !self.song.selection.is_empty() {
            let sel = std::mem::take(&mut self.song.selection);
            self.svc.engine.edit(|d| {
                for (r, t) in &sel {
                    d.remove_region(t, r);
                }
            });
            return;
        }
        if let Some(Menu::Song(SongMenu::Region { track, region, .. })) = self.menu.clone() {
            self.svc.engine.edit(|d| d.remove_region(&track, &region));
            self.menu = None;
        }
    }

    /// RegionBlock pointer down (pointer tool): select, then start a move / resize drag. The
    /// drag is one undo step.
    pub fn begin_region_drag(&mut self, track: &str, region: &str, local_x: f32, w: f32, add: bool) {
        self.select_region(track, region, add);
        let Some(r) = self
            .find_track(track)
            .and_then(|t| t.regions.iter().find(|r| r.id == region))
        else {
            return;
        };
        const HANDLE: f32 = 6.0;
        let mode = if local_x < HANDLE {
            RegionDragMode::ResizeLeft
        } else if local_x > w - HANDLE {
            RegionDragMode::ResizeRight
        } else {
            RegionDragMode::Move
        };
        let (start_beat, length) = (r.start_beat, r.length_beats);
        let label = if mode == RegionDragMode::Move {
            "Move Region"
        } else {
            "Trim Region"
        };
        self.svc.engine.edit(|d| d.begin_undo_group(label));
        self.song.region_drag = Some(RegionDrag {
            track: track.to_owned(),
            region: region.to_owned(),
            mode,
            start_beat,
            length,
        });
    }

    /// RegionBlock pointer move during a drag (`dx` window points since the press).
    pub fn move_region_drag(&mut self, dx: f32) {
        let Some(d) = self.song.region_drag.clone() else {
            return;
        };
        let snap = self.song.snap;
        let delta = dx as f64 / self.song.ppb;
        let min_len = if snap == 0.0 { 0.25 } else { snap };
        match d.mode {
            RegionDragMode::Move => {
                let b = snap_beat(d.start_beat + delta, snap).max(0.0);
                self.svc
                    .engine
                    .edit(|doc| doc.move_region(&d.track, &d.region, b));
            }
            RegionDragMode::ResizeRight => {
                let start = self
                    .find_track(&d.track)
                    .and_then(|t| t.regions.iter().find(|r| r.id == d.region))
                    .map_or(d.start_beat, |r| r.start_beat);
                let len = snap_beat(d.length + delta, snap).max(min_len);
                self.svc
                    .engine
                    .edit(|doc| doc.resize_region(&d.track, &d.region, Some(start), Some(len)));
            }
            RegionDragMode::ResizeLeft => {
                let start = snap_beat(d.start_beat + delta, snap).max(0.0);
                let len = d.length + (d.start_beat - start);
                if len >= min_len {
                    self.svc
                        .engine
                        .edit(|doc| doc.resize_region(&d.track, &d.region, Some(start), Some(len)));
                }
            }
        }
    }

    pub fn end_region_drag(&mut self) {
        if self.song.region_drag.take().is_some() {
            self.svc.engine.edit(|d| d.end_undo_group());
        }
    }

    /// The cut tool: split at the snapped beat under the cursor.
    pub fn cut_region_at(&mut self, track: &str, region: &str, local_x: f32) {
        let Some(start) = self
            .find_track(track)
            .and_then(|t| t.regions.iter().find(|r| r.id == region))
            .map(|r| r.start_beat)
        else {
            return;
        };
        let local = snap_beat(local_x as f64 / self.song.ppb, self.song.snap);
        self.svc
            .engine
            .edit(|d| d.split_region(track, region, start + local));
    }

    pub fn split_region_at_playhead(&mut self, track: &str, region: &str) {
        let pos = self.svc.engine.get_position();
        self.svc.engine.edit(|d| d.split_region(track, region, pos));
    }

    pub fn duplicate_region(&mut self, track: &str, region: &str) {
        self.svc.engine.edit(|d| d.duplicate_region(track, region));
    }

    pub fn copy_region(&mut self, track: &str, region: &str) {
        self.svc.engine.edit(|d| d.copy_region(track, region));
    }

    /// Paste at the playhead on that track.
    pub fn paste_region(&mut self, track: &str) {
        let pos = self.svc.engine.get_position();
        self.svc.engine.edit(|d| d.paste_region(track, pos));
    }

    pub fn quantize_region(&mut self, track: &str, region: &str) {
        let snap = self.song.snap;
        self.svc
            .engine
            .edit(|d| d.quantize_region_notes(track, region, snap));
    }

    pub fn remove_region(&mut self, track: &str, region: &str) {
        self.svc.engine.edit(|d| d.remove_region(track, region));
        self.song.selection.retain(|(r, _)| r != region);
    }

    /// Double-click on a MIDI region: open the piano roll.
    pub fn open_piano_roll(&mut self, track: &str, region: &str) {
        let is_audio = self
            .find_track(track)
            .and_then(|t| t.regions.iter().find(|r| r.id == region))
            .is_some_and(|r| r.audio.is_some());
        if is_audio {
            return;
        }
        self.song.editing = Some((track.to_owned(), region.to_owned()));
        self.menu = None;
        self.piano_open();
    }

    pub fn close_piano_roll(&mut self) {
        self.song.editing = None;
        self.menu = None;
    }

    // --- rubber band ------------------------------------------------------------------------------

    pub fn band_down(&mut self, at: [f32; 2]) {
        self.song.band = Some(Band {
            start: at,
            current: at,
            active: false,
        });
    }

    pub fn band_move(&mut self, at: [f32; 2]) {
        if let Some(b) = &mut self.song.band {
            if !b.active && ((at[0] - b.start[0]).abs() > 3.0 || (at[1] - b.start[1]).abs() > 3.0)
            {
                b.active = true;
            }
            if b.active {
                b.current = at;
            }
        }
    }

    pub fn band_up(&mut self, shift: bool) {
        let Some(b) = self.song.band.take() else {
            return;
        };
        if b.active {
            let rows = self.song_rows();
            let hits = band_hits(self.tracks(), &rows, b.start, b.current, self.song.ppb);
            let mut next = if shift {
                self.song.selection.clone()
            } else {
                Vec::new()
            };
            for h in hits {
                if !next.iter().any(|(r, _)| *r == h.0) {
                    next.push(h);
                }
            }
            self.song.selection = next;
        } else if !shift {
            self.clear_selection();
        }
    }

    // --- groups -------------------------------------------------------------------------------------

    /// The toolbar's "Group": the selected regions' tracks, or all tracks.
    pub fn create_group_from_selection(&mut self) {
        let mut ids: Vec<String> = Vec::new();
        for (_, t) in &self.song.selection {
            if !ids.contains(t) {
                ids.push(t.clone());
            }
        }
        if ids.is_empty() {
            ids = self.tracks().iter().map(|t| t.id.clone()).collect();
        }
        if !ids.is_empty() {
            self.svc.engine.edit(|d| {
                d.create_group("Group", &ids);
            });
        }
    }

    /// Mod+G: a group "Group" of every track.
    pub fn group_all_tracks(&mut self) {
        let ids: Vec<String> = self.tracks().iter().map(|t| t.id.clone()).collect();
        if !ids.is_empty() {
            self.svc.engine.edit(|d| {
                d.create_group("Group", &ids);
            });
        }
    }

    pub fn toggle_group_collapsed(&mut self, id: &str) {
        self.svc.engine.edit(|d| d.toggle_group_collapsed(id));
    }

    pub fn rename_group(&mut self, id: &str, name: &str) {
        self.svc.engine.edit(|d| d.rename_group(id, name));
    }

    pub fn remove_group(&mut self, id: &str) {
        self.svc.engine.edit(|d| d.remove_group(id));
    }

    /// Group mute: all muted → unmute all, else mute all.
    pub fn toggle_group_mute(&mut self, id: &str) {
        let all = self.group_tracks(id).iter().all(|t| t.muted);
        self.svc.engine.edit(|d| d.set_group_mute(id, !all));
    }

    /// Group solo: any solo → unsolo, else solo.
    pub fn toggle_group_solo(&mut self, id: &str) {
        let any = self.group_tracks(id).iter().any(|t| t.solo);
        self.svc.engine.edit(|d| d.set_group_solo(id, !any));
    }

    pub fn group_tracks(&self, id: &str) -> Vec<&Track> {
        let doc = self.svc.engine.doc();
        let Some(g) = doc.get_groups().iter().find(|g| g.id == id) else {
            return Vec::new();
        };
        g.track_ids
            .iter()
            .filter_map(|tid| doc.get_tracks().iter().find(|t| &t.id == tid))
            .collect()
    }

    /// Commit the group rename draft (trimmed, non-empty); always ends editing.
    pub fn commit_group_rename(&mut self) {
        if let Some((id, edit)) = self.song.group_rename.take() {
            let name = edit.text.trim().to_owned();
            if !name.is_empty() {
                self.rename_group(&id, &name);
            }
        }
    }

    // --- markers ----------------------------------------------------------------------------------

    /// "Marker" / Shift+M: a marker at the playhead, named by the count.
    pub fn add_marker_at_playhead(&mut self) {
        let n = self.svc.engine.doc().get_markers().len();
        let name = DEFAULT_MARKER_NAMES[n % DEFAULT_MARKER_NAMES.len()];
        let pos = self.svc.engine.get_position();
        self.svc.engine.edit(|d| {
            d.add_marker(name, pos, None);
        });
    }

    pub fn seek_to_marker(&mut self, id: &str) {
        if let Some(b) = self.svc.engine.doc().marker_seek_target(id) {
            self.svc.engine.set_position(b);
        }
    }

    pub fn seek_next_marker(&mut self) {
        let pos = self.svc.engine.get_position();
        if let Some(b) = self.svc.engine.doc().next_marker_target(pos) {
            self.svc.engine.set_position(b);
        }
    }

    pub fn seek_previous_marker(&mut self) {
        let pos = self.svc.engine.get_position();
        if let Some(b) = self.svc.engine.doc().previous_marker_target(pos) {
            self.svc.engine.set_position(b);
        }
    }

    pub fn update_marker(&mut self, id: &str, u: MarkerUpdate) {
        self.svc.engine.edit(|d| d.update_marker(id, &u));
    }

    pub fn remove_marker(&mut self, id: &str) {
        self.svc.engine.edit(|d| d.remove_marker(id));
    }

    /// The marker menu's rename: Enter / blur commits (trimmed, non-empty).
    pub fn commit_marker_rename(&mut self) {
        let Some(Menu::Song(SongMenu::Marker {
            marker, renaming, ..
        })) = &mut self.menu
        else {
            return;
        };
        if !*renaming {
            return;
        }
        *renaming = false;
        let id = marker.clone();
        let name = self.song.marker_rename.text.trim().to_owned();
        if !name.is_empty() {
            self.update_marker(
                &id,
                MarkerUpdate {
                    name: Some(name),
                    ..Default::default()
                },
            );
        }
    }

    // --- loop / transport ---------------------------------------------------------------------------

    pub fn toggle_song_loop(&mut self) {
        let on = self.svc.engine.doc().is_loop_enabled();
        self.svc.engine.edit(|d| d.set_loop_enabled(!on));
    }

    pub fn set_loop_markers(&mut self, a: f64, b: f64) {
        self.svc.engine.edit(|d| d.set_loop_markers(a, b));
    }

    pub fn begin_loop_drag(&mut self, edge: LoopEdge) {
        self.svc
            .engine
            .edit(|d| d.begin_undo_group("Set Loop Markers"));
        self.song.loop_drag = Some(edge);
    }

    /// A loop handle at content x: snapped, clamped to the song, the loop at least 1 beat.
    pub fn move_loop_drag(&mut self, x: f32) {
        let (Some(edge), Some(m)) = (self.song.loop_drag, self.svc.engine.doc().get_loop_markers())
        else {
            return;
        };
        let len = self.svc.engine.doc().get_arrangement().length_beats;
        let beat = snap_beat(x as f64 / self.song.ppb, self.song.snap).clamp(0.0, len);
        match edge {
            LoopEdge::Start => self.set_loop_markers(beat.min(m.end_beat - 1.0), m.end_beat),
            LoopEdge::End => self.set_loop_markers(m.start_beat, beat.max(m.start_beat + 1.0)),
        }
    }

    pub fn end_loop_drag(&mut self) {
        if self.song.loop_drag.take().is_some() {
            self.svc.engine.edit(|d| d.end_undo_group());
        }
    }

    /// The ruler click: seek to the snapped beat (`setPosition`).
    pub fn ruler_seek(&mut self, x: f32) {
        let b = snap_beat(x as f64 / self.song.ppb, self.song.snap).max(0.0);
        self.svc.engine.set_position(b);
    }

    /// `togglePlay` of the Song view: stop if playing or recording, else play.
    pub fn song_toggle_play(&mut self) {
        if self.song_state() == TransportState::Stopped {
            self.song_play();
        } else {
            self.song_stop();
        }
    }

    pub fn song_play(&mut self) {
        self.svc.engine.play();
    }

    /// Stop (or stop recording while recording).
    pub fn song_stop(&mut self) {
        if self.song_state() == TransportState::Recording {
            self.svc.engine.stop_recording_to_track();
        } else {
            self.svc.engine.stop();
        }
    }

    pub fn song_record(&mut self) {
        self.svc.engine.start_recording_to_track(None);
    }

    /// R in the Song view: stop recording if recording; start if stopped.
    pub fn song_r_key(&mut self) {
        match self.song_state() {
            TransportState::Recording => {
                self.svc.engine.stop_recording_to_track();
            }
            TransportState::Stopped => self.song_record(),
            _ => {}
        }
    }

    pub fn toggle_song_metronome(&mut self) {
        let on = self.svc.engine.is_metronome_enabled();
        self.svc.engine.set_metronome_enabled(!on);
    }

    // --- zoom / scroll ---------------------------------------------------------------------------

    /// Content width of the lanes (`beatToPx(lengthBeats)`).
    pub fn content_width(&self) -> f32 {
        (self.svc.engine.doc().get_arrangement().length_beats * self.song.ppb) as f32
    }

    /// Clamp the scroll into the content.
    pub fn clamp_song_scroll(&mut self) {
        let [vw, vh] = self.song.viewport.get();
        let rows = self.song_rows();
        let ch = rows_height(&rows, self.tracks()) + 64.0;
        let max_x = (self.content_width() - vw).max(0.0);
        let max_y = (ch - vh).max(0.0);
        self.song.scroll[0] = self.song.scroll[0].clamp(0.0, max_x);
        self.song.scroll[1] = self.song.scroll[1].clamp(0.0, max_y);
    }

    /// Set the zoom keeping the beat at viewport x `anchor` in place.
    pub fn song_zoom(&mut self, ppb: f64, anchor: Option<f32>) {
        let next = ppb.clamp(MIN_PPB, MAX_PPB).round();
        if next == self.song.ppb {
            return;
        }
        let view_x = anchor.unwrap_or(0.0).max(0.0);
        let beat = (view_x + self.song.scroll[0]) as f64 / self.song.ppb;
        self.song.ppb = next;
        if anchor.is_some() {
            self.song.scroll[0] = (beat * next) as f32 - view_x;
        }
        self.clamp_song_scroll();
    }

    /// Ctrl/⌘ + wheel: `−deltaY × 0.5` px per beat.
    pub fn song_wheel_zoom(&mut self, dom_delta_y: f64, view_x: f32) {
        let next = (self.song.ppb - dom_delta_y * 0.5)
            .clamp(MIN_PPB, MAX_PPB)
            .round();
        self.song_zoom(next, Some(view_x));
    }

    /// Auto-follow while rolling: when the playhead leaves the view (50 px right margin),
    /// `scrollLeft = playheadX − 100`. True when the scroll changed.
    pub fn song_follow(&mut self) -> bool {
        let st = self.song_state();
        if st != TransportState::Playing && st != TransportState::Recording {
            return false;
        }
        let x = (self.svc.engine.get_position() * self.song.ppb) as f32;
        let [vw, _] = self.song.viewport.get();
        let sl = self.song.scroll[0];
        if x > sl + vw - 50.0 || x < sl {
            self.song.scroll[0] = x - 100.0;
            self.clamp_song_scroll();
            return self.song.scroll[0] != sl;
        }
        false
    }

    // --- piano roll edits ---------------------------------------------------------------------------

    /// `editRegionNotes(track, region, notes, label, newLength)`.
    pub fn edit_region_notes(
        &mut self,
        track: &str,
        region: &str,
        notes: Vec<RegionNote>,
        label: &str,
        new_len: Option<f64>,
    ) {
        self.svc
            .engine
            .edit(|d| d.edit_region_notes(track, region, notes, Some(label), new_len));
    }

    /// `previewNote(trackId, midi, velocity, 0.3)`.
    pub fn preview_note(&mut self, track: &str, midi: f64, velocity: f64) {
        self.svc.engine.preview_note(track, midi, velocity, 0.3);
    }

    // --- frame loop -----------------------------------------------------------------------------

    /// The Song view's per-frame work: follow scroll, LCD, playhead and meters. True when the
    /// view must re-run.
    pub fn song_poll(&mut self) -> bool {
        let pos = self.svc.engine.get_position();
        let bpm = self.svc.engine.doc().get_bpm();
        let (bar, beat, six) = format_bbs(pos);
        let l = &self.song.live;
        l.lcd_bar.set(bar);
        l.lcd_beat.set(beat);
        l.lcd_six.set(six);
        l.lcd_time.set(format_time(pos, bpm));
        let x = (pos * self.song.ppb) as f32 - self.song.scroll[0];
        l.playhead.snap([x, 0.0]);
        let mut changed = false;
        if self.song.region_drag.is_none() && self.song.band.is_none() {
            changed |= self.song_follow();
        }
        if self.song.show_mixer {
            self.read_levels();
        }
        changed |= self.piano_poll(pos);
        changed
    }

    /// `useMixer`: every track's level and the master, each frame.
    fn read_levels(&self) {
        let ids: Vec<String> = self.tracks().iter().map(|t| t.id.clone()).collect();
        let mut levels = self.song.live.levels.borrow_mut();
        let meters = self.song.live.meters.borrow();
        for id in ids {
            let db = self.svc.engine.get_track_level(&id);
            if let Some(m) = meters.get(&id) {
                m.snap([0.0, crate::ui::song::mixer::meter_offset(db, true)]);
            }
            levels.insert(id, db);
        }
        let master = self.svc.engine.get_master_level();
        if let Some(m) = meters.get("") {
            m.snap([0.0, crate::ui::song::mixer::meter_offset(master, false)]);
        }
        self.song
            .live
            .clip
            .snap(if master.is_finite() && master > -0.5 {
                1.0
            } else {
                0.0
            });
        levels.insert(String::new(), master);
    }
}
