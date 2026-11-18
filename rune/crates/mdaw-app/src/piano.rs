//! The piano roll editor's state and logic (phase 8): `PianoRollEditor` (settings, viewport,
//! fit, follow, keys, clipboard), `NoteGrid`'s gestures, `VelocityLane`'s gestures,
//! `PianoRollRuler`'s position / loop drags and the key columns' audition. Every edit commits
//! through `editRegionNotes` with the TS labels; the pure maths is `mdaw_model::piano_roll`.

use mdaw_engine::engine::TransportState;
use mdaw_model::RegionNote;
use mdaw_model::piano_roll::{
    self as pr, DEFAULT_DRUM_DURATION, DEFAULT_NOTE_VELOCITY, HitTestOptions, LayoutKind, NoteEdge,
    PR_DEFAULT_DRUM_ROW_HEIGHT, PR_DEFAULT_PX_PER_BEAT, PR_DEFAULT_ROW_HEIGHT, PR_MAX_PX_PER_BEAT,
    PR_MAX_ROW_HEIGHT, PR_MIN_PX_PER_BEAT, PR_MIN_ROW_HEIGHT, PianoRollTool, QuantizeOptions,
    RowLayout,
};
use mdaw_model::settings::{PianoRollPrefs, read_piano_roll_prefs, write_piano_roll_prefs};

use crate::app::App;

/// `NOTE_MIN_PX` / `DRUM_MIN_PX`.
pub const NOTE_MIN_PX: f64 = 4.0;
pub const DRUM_MIN_PX: f64 = 9.0;
const DRAG_THRESHOLD: f32 = 3.0;
/// `PREVIEW_SEC`.
pub const PREVIEW_SEC: f64 = 0.3;
/// `RULER_HEIGHT` / `LOOP_STRIP` of the piano roll ruler.
pub const PR_RULER_H: f32 = 30.0;
pub const LOOP_STRIP: f32 = 10.0;
/// `VELOCITY_LANE_HEIGHT` / pads.
pub const VEL_H: f32 = 92.0;
pub const VEL_PAD_TOP: f32 = 10.0;
pub const VEL_PAD_BOTTOM: f32 = 6.0;
/// `KEY_COLUMN_WIDTH`.
pub const KEY_COL_W: f32 = 76.0;

/// `PianoRollViewport`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    pub ppb: f64,
    pub row_h: f64,
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

/// NoteGrid's `Gesture`.
#[derive(Debug, Clone, PartialEq)]
pub enum Gesture {
    Move {
        base: Vec<RegionNote>,
        indices: Vec<usize>,
        anchor: usize,
        start_beat: f64,
        start_row: f64,
        last_pitch: f64,
        moved: bool,
        duplicate: bool,
        collapse_to: Option<usize>,
        current: Vec<RegionNote>,
    },
    Resize {
        base: Vec<RegionNote>,
        indices: Vec<usize>,
        anchor: usize,
        edge: NoteEdge,
        start_beat: f64,
        moved: bool,
        current: Vec<RegionNote>,
    },
    Draw {
        index: usize,
        anchor_start: f64,
        moved: bool,
        current: Vec<RegionNote>,
    },
    Paint {
        base_len: usize,
        pitch: f64,
        current: Vec<RegionNote>,
        added: usize,
    },
    Band {
        start_beat: f64,
        start_row: f64,
        start: [f64; 2],
        moved: bool,
        additive: Option<Vec<usize>>,
    },
    Erase {
        base: Vec<RegionNote>,
        removed: Vec<usize>,
    },
}

/// VelocityLane's gesture.
#[derive(Debug, Clone, PartialEq)]
pub enum VelGesture {
    Set {
        base: Vec<RegionNote>,
        indices: Vec<usize>,
        current: Vec<RegionNote>,
    },
    Offset {
        base: Vec<RegionNote>,
        indices: Vec<usize>,
        start_y: f32,
        current: Vec<RegionNote>,
    },
    Ramp {
        base: Vec<RegionNote>,
        candidates: Option<Vec<usize>>,
        beat_a: f64,
        vel_a: f64,
        current: Vec<RegionNote>,
    },
}

/// The ruler's drag.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RulerDrag {
    Position,
    Loop {
        anchor: f64,
        start_x: f32,
        moved: bool,
        current: f64,
    },
}

/// The editor's state (`PianoRollEditorInner` + NoteGrid / VelocityLane / ruler locals).
pub struct PianoUi {
    pub tool: PianoRollTool,
    pub prefs: PianoRollPrefs,
    /// Selected note indices (JS `Set` insertion order).
    pub selection: Vec<usize>,
    pub draft: Option<Vec<RegionNote>>,
    /// Region-absolute cue beat.
    pub cue: f64,
    pub hover_row: Option<i64>,
    /// The status cursor: (region-relative snapped beat, pitch).
    pub cursor: Option<(f64, Option<f64>)>,
    pub vp: Viewport,
    pub row_zoom_acc: f64,
    pub did_fit: bool,
    /// The notes as last seen / last committed (external changes clear the selection).
    pub synced: Vec<RegionNote>,
    pub committed: Vec<RegionNote>,
    /// Module-level note clipboard (survives closing / reopening).
    pub clipboard: Vec<RegionNote>,
    pub gesture: Option<Gesture>,
    /// The rubber band in content points (x0, y0, x1, y1).
    pub band: Option<[f64; 4]>,
    pub vel_gesture: Option<VelGesture>,
    /// Velocity readout: (content x, y, text).
    pub readout: Option<(f64, f32, String)>,
    /// Ramp line: (x0 content, v0, x1 content, v1).
    pub ramp_line: Option<[f64; 4]>,
    pub ruler_drag: Option<RulerDrag>,
    pub draft_loop: Option<(f64, f64)>,
    /// Audition press on the key column: (row, pitch).
    pub key_press: Option<(i64, f64)>,
    /// The grid's measured size (the view writes it).
    pub measured: std::cell::Cell<[f64; 2]>,
}

impl PianoUi {
    pub fn new(prefs: PianoRollPrefs) -> Self {
        Self {
            tool: PianoRollTool::Pointer,
            prefs,
            selection: Vec::new(),
            draft: None,
            cue: 0.0,
            hover_row: None,
            cursor: None,
            vp: Viewport {
                ppb: PR_DEFAULT_PX_PER_BEAT,
                row_h: PR_DEFAULT_ROW_HEIGHT,
                left: 0.0,
                top: 0.0,
                width: 0.0,
                height: 0.0,
            },
            row_zoom_acc: PR_DEFAULT_ROW_HEIGHT,
            did_fit: false,
            synced: Vec::new(),
            committed: Vec::new(),
            clipboard: Vec::new(),
            gesture: None,
            band: None,
            vel_gesture: None,
            readout: None,
            ramp_line: None,
            ruler_drag: None,
            draft_loop: None,
            key_press: None,
            measured: std::cell::Cell::new([0.0, 0.0]),
        }
    }
}

/// What the editor shows for the open region.
pub struct Ctx {
    pub track: String,
    pub region: String,
    pub is_drum: bool,
    pub start: f64,
    pub length: f64,
    pub notes: Vec<RegionNote>,
    pub layout: RowLayout,
}

/// `createDrumLayout(unique sorted pitches)` / `createPianoLayout()`.
pub fn layout_for(is_drum: bool, notes: &[RegionNote]) -> RowLayout {
    if is_drum {
        let mut p: Vec<f64> = notes.iter().map(|n| n.note).collect();
        p.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        p.dedup();
        pr::create_drum_layout(&p)
    } else {
        pr::create_piano_layout_default()
    }
}

impl App {
    /// The editor's region, or None when it no longer exists.
    pub fn piano_ctx(&self) -> Option<Ctx> {
        let (tid, rid) = self.song.editing.clone()?;
        let t = self.find_track(&tid)?;
        let r = t.regions.iter().find(|r| r.id == rid)?;
        let is_drum = t.instrument.kind == mdaw_model::InstrumentType::Drums;
        let notes = self
            .song
            .piano
            .draft
            .clone()
            .unwrap_or_else(|| r.notes.clone());
        let layout = layout_for(is_drum, &notes);
        Some(Ctx {
            track: tid,
            region: rid,
            is_drum,
            start: r.start_beat,
            length: r.length_beats,
            notes,
            layout,
        })
    }

    /// The region's committed notes (not the draft).
    pub fn piano_region_notes(&self) -> Vec<RegionNote> {
        self.song
            .editing
            .as_ref()
            .and_then(|(t, r)| {
                self.find_track(t)?
                    .regions
                    .iter()
                    .find(|x| &x.id == r)
                    .map(|x| x.notes.clone())
            })
            .unwrap_or_default()
    }

    /// Opening the editor (the inner component mounts per region).
    pub fn piano_open(&mut self) {
        let prefs = read_piano_roll_prefs(&self.svc.settings.borrow());
        let clipboard = std::mem::take(&mut self.song.piano.clipboard);
        let mut p = PianoUi::new(prefs);
        p.clipboard = clipboard;
        if let Some((t, r)) = self.song.editing.clone()
            && let Some(track) = self.find_track(&t)
            && let Some(region) = track.regions.iter().find(|x| x.id == r)
        {
            let drum = track.instrument.kind == mdaw_model::InstrumentType::Drums;
            p.vp.row_h = if drum {
                PR_DEFAULT_DRUM_ROW_HEIGHT
            } else {
                PR_DEFAULT_ROW_HEIGHT
            };
            p.row_zoom_acc = p.vp.row_h;
            p.cue = region.start_beat;
            p.synced = region.notes.clone();
            p.committed = region.notes.clone();
        }
        self.song.piano = p;
    }

    fn save_piano_prefs(&mut self) {
        let p = self.song.piano.prefs;
        let _ = write_piano_roll_prefs(&mut self.svc.settings.borrow_mut(), &p);
    }

    pub fn set_pr_snap(&mut self, snap: f64) {
        self.song.piano.prefs.snap = snap;
        self.save_piano_prefs();
    }

    pub fn set_pr_draw_length(&mut self, beats: f64) {
        self.song.piano.prefs.draw_length = beats;
        self.save_piano_prefs();
    }

    pub fn set_pr_follow(&mut self, on: bool) {
        self.song.piano.prefs.follow = on;
        self.save_piano_prefs();
    }

    pub fn set_pr_tool(&mut self, t: PianoRollTool) {
        self.song.piano.tool = t;
    }

    /// Selection limited to existing notes, sorted (`selectedIdx`).
    pub fn pr_selected(&self, len: usize) -> Vec<usize> {
        let mut v: Vec<usize> = self
            .song
            .piano
            .selection
            .iter()
            .copied()
            .filter(|i| *i < len)
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    // --- viewport -------------------------------------------------------------------------------

    fn pr_content_size(&self, c: &Ctx) -> (f64, f64) {
        let vp = self.song.piano.vp;
        let mut end = 0.0f64;
        for n in &c.notes {
            end = end.max(n.start_beat + n.duration_beats);
        }
        let view_beats = vp.width / vp.ppb;
        let content_beats = c.length.max(end) + 8.0f64.max(view_beats * 0.5);
        (
            vp.width.max((content_beats * vp.ppb).ceil()),
            vp.height.max(c.layout.row_count as f64 * vp.row_h),
        )
    }

    /// Scrolls (clamped to the content, as the browser clamps).
    pub fn pr_scroll_to(&mut self, left: Option<f64>, top: Option<f64>) {
        let Some(c) = self.piano_ctx() else { return };
        let (cw, ch) = self.pr_content_size(&c);
        let vp = &mut self.song.piano.vp;
        if let Some(l) = left {
            vp.left = l.clamp(0.0, (cw - vp.width).max(0.0));
        }
        if let Some(t) = top {
            vp.top = t.clamp(0.0, (ch - vp.height).max(0.0));
        }
    }

    /// `zoomX(next, cursorX)`.
    pub fn pr_zoom_x(&mut self, next: f64, cursor_x: Option<f64>) {
        let vp = self.song.piano.vp;
        let px = pr::clamp(next, PR_MIN_PX_PER_BEAT, PR_MAX_PX_PER_BEAT);
        if px == vp.ppb {
            return;
        }
        let anchor = cursor_x.unwrap_or(vp.width / 2.0);
        let left = pr::zoom_scroll(vp.left, anchor, vp.ppb, px);
        self.song.piano.vp.ppb = px;
        self.pr_scroll_to(Some(left), None);
    }

    /// `zoomY(nextRow, cursorY)`.
    pub fn pr_zoom_y(&mut self, next: f64, cursor_y: Option<f64>) {
        let p = &mut self.song.piano;
        p.row_zoom_acc = pr::clamp(next, PR_MIN_ROW_HEIGHT, PR_MAX_ROW_HEIGHT);
        let rh = p.row_zoom_acc.round();
        if rh == p.vp.row_h {
            return;
        }
        let anchor = cursor_y.unwrap_or(p.vp.height / 2.0);
        let top = pr::zoom_scroll(p.vp.top, anchor, p.vp.row_h, rh);
        p.vp.row_h = rh;
        self.pr_scroll_to(None, Some(top));
    }

    /// The wheel over a pane (`makeWheel(source)`): `dom_dx` / `dom_dy` are DOM deltas
    /// (positive = scroll right / down), `x` / `y` the pointer in the grid viewport.
    pub fn pr_wheel(&mut self, source: WheelSource, dom_dx: f64, dom_dy: f64, x: f64, y: f64, m: rune::Mods) {
        let vp = self.song.piano.vp;
        if m.cmd && !m.shift {
            self.pr_zoom_x(
                pr::zoom_from_wheel(
                    vp.ppb,
                    dom_dy,
                    PR_MIN_PX_PER_BEAT,
                    PR_MAX_PX_PER_BEAT,
                    pr::ZOOM_WHEEL_SENSITIVITY,
                ),
                Some(x),
            );
            return;
        }
        if m.alt || (m.cmd && m.shift) {
            let delta = if dom_dy != 0.0 { dom_dy } else { dom_dx };
            let acc = self.song.piano.row_zoom_acc;
            self.pr_zoom_y(acc * (-delta * 0.003).exp(), Some(y));
            return;
        }
        match source {
            WheelSource::Ruler | WheelSource::Lane => {
                let d = if dom_dx != 0.0 { dom_dx } else { dom_dy };
                self.pr_scroll_to(Some(vp.left + d), None);
            }
            WheelSource::Keys => self.pr_scroll_to(None, Some(vp.top + dom_dy)),
            WheelSource::Grid => self.pr_scroll_to(Some(vp.left + dom_dx), Some(vp.top + dom_dy)),
        }
    }

    /// `fit()`: zoom to the region / notes and centre the pitches.
    pub fn pr_fit(&mut self) {
        let Some(c) = self.piano_ctx() else { return };
        let notes = self.piano_region_notes();
        let vp = self.song.piano.vp;
        if vp.width <= 0.0 || vp.height <= 0.0 {
            return;
        }
        let f = pr::fit_view(&notes, &c.layout, c.length, vp.width, vp.height);
        let mut rh = f.row_height;
        if !c.is_drum {
            rh = pr::clamp(rh, 12.0, 22.0);
        }
        let top = pr::center_scroll_top(&notes, &c.layout, rh, vp.height);
        let p = &mut self.song.piano;
        p.row_zoom_acc = rh;
        p.vp.ppb = f.px_per_beat;
        p.vp.row_h = rh;
        self.pr_scroll_to(Some(0.0), Some(top));
    }

    // --- frame ------------------------------------------------------------------------------------

    /// External note changes, the first fit, follow. True when the view must re-run.
    pub fn piano_poll(&mut self, pos: f64) -> bool {
        if self.song.editing.is_none() {
            return false;
        }
        let mut changed = false;
        let [w, h] = self.song.piano.measured.get();
        let vp = &mut self.song.piano.vp;
        if (w, h) != (vp.width, vp.height) && w > 0.0 {
            vp.width = w;
            vp.height = h;
            changed = true;
        }
        // Notes changed underneath (undo / redo / recording): selection and draft go.
        let notes = self.piano_region_notes();
        if notes != self.song.piano.synced {
            self.song.piano.synced = notes.clone();
            if notes != self.song.piano.committed {
                self.song.piano.selection.clear();
                self.song.piano.draft = None;
            }
            changed = true;
        }
        if !self.song.piano.did_fit && self.song.piano.vp.width > 0.0 && self.song.piano.vp.height > 0.0
        {
            self.song.piano.did_fit = true;
            self.pr_fit();
            changed = true;
        }
        // Playhead (live) + follow.
        let Some(c) = self.piano_ctx() else {
            return changed;
        };
        let playing = self.svc.engine.get_state() != TransportState::Stopped;
        let vp = self.song.piano.vp;
        if playing {
            let rel = pos - c.start;
            self.song
                .live
                .pr_playhead
                .snap([(rel * vp.ppb - vp.left) as f32, 0.0]);
            self.song.live.pr_playhead_on.snap(1.0);
            if self.song.piano.prefs.follow
                && let Some(next) =
                    pr::follow_scroll(rel * vp.ppb, vp.left, vp.width, pr::FOLLOW_MARGIN)
                && next != vp.left
            {
                self.pr_scroll_to(Some(next), None);
                changed = true;
            }
        } else {
            self.song.live.pr_playhead_on.snap(0.0);
        }
        changed
    }

    // --- commits ------------------------------------------------------------------------------------

    /// `commit(next, label, selection)`: one undo step; grows the region when needed.
    pub fn pr_commit(&mut self, next: Vec<RegionNote>, label: &str, sel: Vec<usize>) {
        let Some((track, region)) = self.song.editing.clone() else {
            return;
        };
        let length = self
            .find_track(&track)
            .and_then(|t| t.regions.iter().find(|r| r.id == region))
            .map_or(0.0, |r| r.length_beats);
        let grow = pr::required_region_length(&next, length);
        let p = &mut self.song.piano;
        p.committed = next.clone();
        p.synced = next.clone();
        p.draft = None;
        p.selection = sel;
        self.edit_region_notes(
            &track,
            &region,
            next,
            label,
            (grow > length).then_some(grow),
        );
    }

    fn pr_preview(&mut self, pitch: f64, velocity: f64) {
        if let Some((t, _)) = self.song.editing.clone() {
            self.preview_note(&t, pitch, velocity);
        }
    }

    /// `quantize()`: the selection, or every note, to the grid ("Quantize Notes").
    pub fn pr_quantize(&mut self) {
        let Some(c) = self.piano_ctx() else { return };
        let snap = self.song.piano.prefs.snap;
        let notes = self.piano_region_notes();
        if snap <= 0.0 || notes.is_empty() {
            return;
        }
        let sel = self.pr_selected(c.notes.len());
        let opts = QuantizeOptions {
            indices: (!sel.is_empty()).then_some(&sel[..]),
            offset: c.start,
            ..Default::default()
        };
        let next = pr::quantize_notes(&notes, snap, &opts);
        self.pr_commit(next, "Quantize Notes", sel);
    }

    /// `pasteAt()`: the playhead (or the cue), snapped.
    fn pr_paste_at(&self, c: &Ctx) -> f64 {
        let playing = self.svc.engine.get_state() != TransportState::Stopped;
        let rel = if playing {
            self.svc.engine.get_position() - c.start
        } else {
            self.song.piano.cue - c.start
        };
        let at = rel.max(0.0);
        let snap = self.song.piano.prefs.snap;
        if snap > 0.0 {
            pr::snap_floor(at, snap, c.start).max(0.0)
        } else {
            at
        }
    }

    /// The editor's keys (`handleKeyDown`). True when handled.
    pub fn pr_key(&mut self, ev: &rune::KeyEvent) -> bool {
        use rune::Key;
        let Some(c) = self.piano_ctx() else {
            return false;
        };
        let notes = self.piano_region_notes();
        let snap = self.song.piano.prefs.snap;
        let step = pr::nudge_step(snap);
        let sel = self.pr_selected(c.notes.len());
        let has_sel = !sel.is_empty();
        let ch = match &ev.key {
            Key::Text(t) => t.to_lowercase(),
            Key::Command(c) => c.to_lowercase().to_string(),
            _ => String::new(),
        };
        if ev.mods.cmd {
            match ch.as_str() {
                "a" => self.song.piano.selection = (0..notes.len()).collect(),
                "c" => {
                    if has_sel {
                        self.song.piano.clipboard = pr::copy_notes(&notes, &sel);
                    }
                }
                "x" => {
                    if has_sel {
                        self.song.piano.clipboard = pr::copy_notes(&notes, &sel);
                        self.pr_commit(pr::delete_notes(&notes, &sel), "Cut Notes", Vec::new());
                    }
                }
                "v" => {
                    if !self.song.piano.clipboard.is_empty() {
                        let at = self.pr_paste_at(&c);
                        let res =
                            pr::paste_notes(&notes, &self.song.piano.clipboard, at, Some(c.is_drum));
                        self.pr_commit(res.notes, "Paste Notes", res.selection);
                    }
                }
                "d" => {
                    if has_sel {
                        let res = pr::duplicate_notes(&notes, &sel, snap);
                        self.pr_commit(res.notes, "Duplicate Notes", res.selection);
                    }
                }
                _ => return false,
            }
            return true;
        }
        match (&ev.key, ch.as_str()) {
            (_, " ") => {
                if self.svc.engine.get_state() != TransportState::Stopped {
                    self.svc.engine.stop();
                } else {
                    let cue = self.song.piano.cue;
                    self.svc.engine.play_from(cue);
                }
            }
            (Key::Delete | Key::Backspace, _) => {
                if has_sel {
                    let label = if sel.len() > 1 {
                        "Delete Notes"
                    } else {
                        "Delete Note"
                    };
                    self.pr_commit(pr::delete_notes(&notes, &sel), label, Vec::new());
                }
            }
            (Key::Up | Key::Down, _) => {
                if has_sel {
                    let dir = if matches!(ev.key, Key::Up) { -1.0 } else { 1.0 };
                    let rows = if c.is_drum {
                        dir
                    } else {
                        dir * if ev.mods.shift { 12.0 } else { 1.0 }
                    };
                    let next = pr::move_notes(&notes, &sel, 0.0, rows, &c.layout);
                    let single = (sel.len() == 1).then(|| (next[sel[0]].note, next[sel[0]].velocity));
                    self.pr_commit(next, "Transpose Notes", sel);
                    if let Some((n, v)) = single {
                        self.pr_preview(n, v);
                    }
                }
            }
            (Key::Left | Key::Right, _) => {
                if has_sel {
                    let dir = if matches!(ev.key, Key::Right) { 1.0 } else { -1.0 };
                    if ev.mods.alt && !c.is_drum {
                        let next = pr::change_lengths(&notes, &sel, dir * step, step.min(1.0 / 16.0));
                        self.pr_commit(next, "Change Length", sel);
                    } else {
                        let next = pr::move_notes(&notes, &sel, dir * step, 0.0, &c.layout);
                        self.pr_commit(next, "Move Notes", sel);
                    }
                }
            }
            (_, "q") => self.pr_quantize(),
            (_, "v") => self.set_pr_tool(PianoRollTool::Pointer),
            (_, "d") | (_, "b") => self.set_pr_tool(PianoRollTool::Draw),
            (_, "e") => self.set_pr_tool(PianoRollTool::Eraser),
            (_, "f") => self.pr_fit(),
            (_, "=") | (_, "+") => {
                let px = self.song.piano.vp.ppb;
                self.pr_zoom_x(px * 1.25, None);
            }
            (_, "-") => {
                let px = self.song.piano.vp.ppb;
                self.pr_zoom_x(px / 1.25, None);
            }
            _ => return false,
        }
        true
    }

    // --- grid gestures ------------------------------------------------------------------------------

    fn pr_hit(&self, c: &Ctx, x: f64, y: f64, with_selection: bool) -> Option<pr::NoteHit> {
        let vp = self.song.piano.vp;
        let sel = self.song.piano.selection.clone();
        pr::hit_test_note(
            &c.notes,
            x,
            y,
            &c.layout,
            vp.ppb,
            vp.row_h,
            &HitTestOptions {
                resizable: !c.is_drum,
                selected: with_selection.then_some(&sel[..]),
                min_width_px: if c.is_drum { DRUM_MIN_PX } else { NOTE_MIN_PX },
                ..Default::default()
            },
        )
    }

    fn pr_new_note(&self, c: &Ctx, beat: f64, row: f64, free: bool) -> RegionNote {
        let p = &self.song.piano;
        let snap = p.prefs.snap;
        let start = if free || snap <= 0.0 {
            pr::round_beat(beat)
        } else {
            pr::snap_floor(beat, snap, c.start)
        }
        .max(0.0);
        RegionNote {
            note: c.layout.row_to_pitch(row),
            start_beat: start,
            duration_beats: if c.is_drum {
                DEFAULT_DRUM_DURATION.min(if snap > 0.0 { snap } else { DEFAULT_DRUM_DURATION })
            } else {
                p.prefs.draw_length
            },
            velocity: DEFAULT_NOTE_VELOCITY,
            is_drum: c.is_drum,
        }
    }

    /// Grid pointer down at grid viewport point (x, y). `clicks` = 2 on a double press.
    pub fn pr_grid_down(&mut self, x: f32, y: f32, clicks: u32, m: rune::Mods) {
        let Some(c) = self.piano_ctx() else { return };
        let vp = self.song.piano.vp;
        let cx = x as f64 + vp.left;
        let cy = y as f64 + vp.top;
        let row = (cy / vp.row_h).floor();
        if row < 0.0 || row >= c.layout.row_count as f64 {
            return;
        }
        let beat = cx / vp.ppb;
        let free = m.cmd;
        let hit = self.pr_hit(&c, cx, cy, true);
        let tool = self.song.piano.tool;

        if tool == PianoRollTool::Eraser {
            let mut removed = Vec::new();
            if let Some(h) = hit {
                removed.push(h.index);
            }
            if !removed.is_empty() {
                self.song.piano.draft = Some(pr::delete_notes(&c.notes, &removed));
            }
            self.song.piano.gesture = Some(Gesture::Erase {
                base: c.notes.clone(),
                removed,
            });
            return;
        }

        if let Some(h) = hit {
            let idx = h.index;
            if clicks == 2 && tool == PianoRollTool::Pointer && h.edge == NoteEdge::Body {
                self.pr_commit(pr::delete_notes(&c.notes, &[idx]), "Delete Note", Vec::new());
                return;
            }
            let cur = self.song.piano.selection.clone();
            let mut collapse_to = None;
            let sel = if m.shift {
                let s = pr::apply_click_selection(&cur, idx, true);
                self.song.piano.selection = s.clone();
                if !s.contains(&idx) {
                    return;
                }
                s
            } else {
                if cur.contains(&idx) && cur.len() > 1 {
                    collapse_to = Some(idx);
                }
                let s = pr::apply_click_selection(&cur, idx, false);
                self.song.piano.selection = s.clone();
                s
            };
            let note = c.notes[idx].clone();
            self.pr_preview(note.note, note.velocity);
            let mut indices = sel.clone();
            indices.sort_unstable();
            if h.edge != NoteEdge::Body {
                self.song.piano.gesture = Some(Gesture::Resize {
                    base: c.notes.clone(),
                    indices,
                    anchor: idx,
                    edge: h.edge,
                    start_beat: beat,
                    moved: false,
                    current: c.notes.clone(),
                });
                return;
            }
            let (mut base, mut move_idx, mut anchor) = (c.notes.clone(), indices.clone(), idx);
            let duplicate = m.alt;
            if duplicate {
                let res = pr::append_copies(&c.notes, &indices, 0.0, 0.0, None);
                base = res.notes;
                anchor = res.selection[indices.iter().position(|i| *i == idx).unwrap_or(0)];
                move_idx = res.selection;
            }
            self.song.piano.gesture = Some(Gesture::Move {
                current: base.clone(),
                base,
                indices: move_idx,
                anchor,
                start_beat: beat,
                start_row: row,
                last_pitch: note.note,
                moved: false,
                duplicate,
                collapse_to,
            });
            return;
        }

        let draw_here = tool == PianoRollTool::Draw || (tool == PianoRollTool::Pointer && clicks == 2);
        if draw_here {
            let note = self.pr_new_note(&c, beat, row, free);
            let mut next = c.notes.clone();
            next.push(note.clone());
            self.pr_preview(note.note, note.velocity);
            self.song.piano.selection = vec![next.len() - 1];
            self.song.piano.gesture = Some(if c.is_drum {
                Gesture::Paint {
                    base_len: c.notes.len(),
                    pitch: note.note,
                    current: next.clone(),
                    added: 1,
                }
            } else {
                Gesture::Draw {
                    index: next.len() - 1,
                    anchor_start: note.start_beat,
                    moved: false,
                    current: next.clone(),
                }
            });
            self.song.piano.draft = Some(next);
            return;
        }

        self.song.piano.gesture = Some(Gesture::Band {
            start_beat: beat,
            start_row: row,
            start: [cx, cy],
            moved: false,
            additive: m.shift.then(|| self.song.piano.selection.clone()),
        });
    }

    /// Hover over the grid (no button): row highlight and status cursor.
    pub fn pr_grid_hover(&mut self, x: f32, y: f32) -> bool {
        let Some(c) = self.piano_ctx() else {
            return false;
        };
        let vp = self.song.piano.vp;
        let cx = x as f64 + vp.left;
        let cy = y as f64 + vp.top;
        let row = (cy / vp.row_h).floor();
        let in_rows = row >= 0.0 && row < c.layout.row_count as f64;
        let beat = cx / vp.ppb;
        let snap = self.song.piano.prefs.snap;
        let snapped = if snap > 0.0 {
            pr::snap_floor(beat, snap, c.start)
        } else {
            pr::round_beat(beat)
        };
        let hover = in_rows.then_some(row as i64);
        let cursor = Some((snapped.max(0.0), in_rows.then(|| c.layout.row_to_pitch(row))));
        let p = &mut self.song.piano;
        if p.hover_row != hover || p.cursor != cursor {
            p.hover_row = hover;
            p.cursor = cursor;
            return true;
        }
        false
    }

    pub fn pr_grid_leave(&mut self) {
        if self.song.piano.gesture.is_some() {
            return;
        }
        self.song.piano.hover_row = None;
        self.song.piano.cursor = None;
    }

    /// Grid drag move: `dx` / `dy` window travel since the press, (x, y) the grid point.
    pub fn pr_grid_move(&mut self, x: f32, y: f32, dx: f32, dy: f32, m: rune::Mods) {
        self.pr_grid_hover(x, y);
        let Some(c) = self.piano_ctx() else { return };
        let Some(mut g) = self.song.piano.gesture.take() else {
            return;
        };
        let vp = self.song.piano.vp;
        let cx = x as f64 + vp.left;
        let cy = y as f64 + vp.top;
        let row = (cy / vp.row_h).floor();
        let beat = cx / vp.ppb;
        let free = m.cmd || (!matches!(g, Gesture::Move { .. }) && m.alt);
        let snap = if free { 0.0 } else { self.song.piano.prefs.snap };
        match &mut g {
            Gesture::Move {
                base,
                indices,
                anchor,
                start_beat,
                start_row,
                last_pitch,
                moved,
                duplicate,
                current,
                ..
            } => {
                if !*moved && dx.hypot(dy) < DRAG_THRESHOLD {
                    self.song.piano.gesture = Some(g);
                    return;
                }
                *moved = true;
                let a = &base[*anchor];
                let d_beat = pr::snapped_move_delta(a.start_beat, beat - *start_beat, snap, c.start);
                let d_row = row - *start_row;
                *current = pr::move_notes(base, indices, d_beat, d_row, &c.layout);
                if *duplicate {
                    self.song.piano.selection = indices.clone();
                }
                self.song.piano.draft = Some(current.clone());
                let pitch = current[*anchor].note;
                if pitch != *last_pitch {
                    *last_pitch = pitch;
                    let v = current[*anchor].velocity;
                    self.pr_preview(pitch, v);
                }
            }
            Gesture::Resize {
                base,
                indices,
                anchor,
                edge,
                start_beat,
                moved,
                current,
            } => {
                if !*moved && dx.abs() < DRAG_THRESHOLD {
                    self.song.piano.gesture = Some(g);
                    return;
                }
                *moved = true;
                *current =
                    pr::resize_notes(base, indices, *edge, *anchor, beat - *start_beat, snap, c.start);
                self.song.piano.draft = Some(current.clone());
            }
            Gesture::Draw {
                index,
                anchor_start,
                moved,
                current,
            } => {
                if !*moved && dx.abs() < DRAG_THRESHOLD {
                    self.song.piano.gesture = Some(g);
                    return;
                }
                *moved = true;
                let span = pr::draw_span(*anchor_start, beat, snap, c.start);
                current[*index].start_beat = span.start_beat;
                current[*index].duration_beats = span.duration_beats;
                self.song.piano.draft = Some(current.clone());
            }
            Gesture::Paint {
                pitch,
                current,
                added,
                ..
            } => {
                let psnap = self.song.piano.prefs.snap;
                let step = if psnap > 0.0 { psnap } else { DEFAULT_DRUM_DURATION };
                let b = if free {
                    pr::round_beat(beat)
                } else {
                    pr::snap_floor(beat, step, c.start)
                }
                .max(0.0);
                let exists = current
                    .iter()
                    .any(|n| n.note == *pitch && (n.start_beat - b).abs() < step / 2.0);
                if !exists {
                    current.push(RegionNote {
                        note: *pitch,
                        start_beat: b,
                        duration_beats: DEFAULT_DRUM_DURATION.min(step),
                        velocity: DEFAULT_NOTE_VELOCITY,
                        is_drum: true,
                    });
                    *added += 1;
                    self.song.piano.draft = Some(current.clone());
                    let p = *pitch;
                    self.pr_preview(p, DEFAULT_NOTE_VELOCITY);
                }
            }
            Gesture::Band {
                start_beat,
                start_row,
                start,
                moved,
                additive,
            } => {
                if !*moved && (cx - start[0]).hypot(cy - start[1]) < DRAG_THRESHOLD as f64 {
                    self.song.piano.gesture = Some(g);
                    return;
                }
                *moved = true;
                self.song.piano.band = Some([start[0], start[1], cx, cy]);
                let inside = pr::notes_in_rect(&c.notes, *start_beat, beat, *start_row, row, &c.layout);
                let mut next = additive.clone().unwrap_or_default();
                for i in inside {
                    if !next.contains(&i) {
                        next.push(i);
                    }
                }
                self.song.piano.selection = next;
            }
            Gesture::Erase { base, removed } => {
                let hit = pr::hit_test_note(
                    base,
                    cx,
                    cy,
                    &c.layout,
                    vp.ppb,
                    vp.row_h,
                    &HitTestOptions {
                        resizable: false,
                        min_width_px: if c.is_drum { DRUM_MIN_PX } else { NOTE_MIN_PX },
                        ..Default::default()
                    },
                );
                if let Some(h) = hit
                    && !removed.contains(&h.index)
                {
                    removed.push(h.index);
                    self.song.piano.draft = Some(pr::delete_notes(base, removed));
                }
            }
        }
        self.song.piano.gesture = Some(g);
    }

    /// Grid pointer up (`finish`); `cancelled` = Escape / lost pointer.
    pub fn pr_grid_up(&mut self, cancelled: bool) {
        let Some(g) = self.song.piano.gesture.take() else {
            return;
        };
        self.song.piano.band = None;
        if cancelled {
            self.song.piano.draft = None;
            return;
        }
        match g {
            Gesture::Move {
                moved,
                current,
                duplicate,
                indices,
                collapse_to,
                ..
            } => {
                if moved {
                    let label = if duplicate {
                        "Duplicate Notes"
                    } else if indices.len() > 1 {
                        "Move Notes"
                    } else {
                        "Move Note"
                    };
                    self.pr_commit(current, label, indices);
                } else {
                    self.song.piano.draft = None;
                    if let Some(i) = collapse_to {
                        self.song.piano.selection = vec![i];
                    }
                }
            }
            Gesture::Resize {
                moved,
                current,
                indices,
                edge,
                anchor,
                ..
            } => {
                if moved {
                    let len = current[anchor].duration_beats;
                    let label = if indices.len() > 1 {
                        "Resize Notes"
                    } else {
                        "Resize Note"
                    };
                    self.pr_commit(current, label, indices);
                    if edge == NoteEdge::End {
                        self.set_pr_draw_length(len);
                    }
                } else {
                    self.song.piano.draft = None;
                }
            }
            Gesture::Draw {
                index,
                moved,
                current,
                ..
            } => {
                let len = current[index].duration_beats;
                self.pr_commit(current, "Draw Note", vec![index]);
                if moved {
                    self.set_pr_draw_length(len);
                }
            }
            Gesture::Paint {
                base_len,
                current,
                added,
                ..
            } => {
                let label = if added > 1 { "Draw Notes" } else { "Draw Note" };
                self.pr_commit(current, label, (base_len..base_len + added).collect());
            }
            Gesture::Band { moved, additive, .. } => {
                if !moved && additive.is_none() {
                    self.song.piano.selection.clear();
                }
            }
            Gesture::Erase { base, removed } => {
                if removed.is_empty() {
                    self.song.piano.draft = None;
                } else {
                    let label = if removed.len() > 1 {
                        "Delete Notes"
                    } else {
                        "Delete Note"
                    };
                    self.pr_commit(pr::delete_notes(&base, &removed), label, Vec::new());
                }
            }
        }
    }

    /// Right-click on the grid: erase the note under the pointer (one note; Rune's secondary
    /// button has no drag, so right-drag erasing is the eraser tool only).
    pub fn pr_grid_secondary(&mut self, x: f32, y: f32) {
        let Some(c) = self.piano_ctx() else { return };
        let vp = self.song.piano.vp;
        let (cx, cy) = (x as f64 + vp.left, y as f64 + vp.top);
        if let Some(h) = self.pr_hit(&c, cx, cy, true) {
            self.pr_commit(pr::delete_notes(&c.notes, &[h.index]), "Delete Note", Vec::new());
        }
    }

    /// Escape during a gesture cancels it (before the editor / App see the key).
    pub fn pr_cancel_gesture(&mut self) -> bool {
        if self.song.piano.gesture.is_some() {
            self.pr_grid_up(true);
            return true;
        }
        false
    }

    // --- velocity lane ------------------------------------------------------------------------------

    fn vel_usable() -> f32 {
        VEL_H - VEL_PAD_TOP - VEL_PAD_BOTTOM
    }

    pub fn vel_to_y(v: f64) -> f32 {
        VEL_PAD_TOP + (1.0 - v as f32) * Self::vel_usable()
    }

    fn y_to_vel(y: f32) -> f64 {
        pr::clamp_velocity(1.0 - ((y - VEL_PAD_TOP) / Self::vel_usable()) as f64)
    }

    pub fn vel_down(&mut self, x: f32, y: f32, alt: bool) {
        let Some(c) = self.piano_ctx() else { return };
        let vp = self.song.piano.vp;
        let cx = x as f64 + vp.left;
        let vel = Self::y_to_vel(y);
        let selected = (!self.song.piano.selection.is_empty()).then(|| self.song.piano.selection.clone());
        let mut hits = match &selected {
            Some(s) => pr::notes_at_x(&c.notes, cx, vp.ppb, 6.0, Some(s)),
            None => Vec::new(),
        };
        if hits.is_empty() {
            hits = pr::notes_at_x(&c.notes, cx, vp.ppb, 6.0, None);
        }
        if !hits.is_empty() && !alt {
            let multi = selected
                .as_ref()
                .is_some_and(|s| hits.iter().any(|i| s.contains(i)) && s.len() > 1);
            if multi {
                self.song.piano.vel_gesture = Some(VelGesture::Offset {
                    base: c.notes.clone(),
                    indices: selected.unwrap_or_default(),
                    start_y: y,
                    current: c.notes.clone(),
                });
                self.song.piano.readout = Some((cx, y, "±0".into()));
                return;
            }
            let current = pr::set_velocities(&c.notes, &hits, vel);
            self.song.piano.draft = Some(current.clone());
            self.song.piano.vel_gesture = Some(VelGesture::Set {
                base: c.notes.clone(),
                indices: hits,
                current,
            });
            self.song.piano.readout = Some((cx, y, format!("{}", pr::velocity_to_midi(vel))));
            return;
        }
        self.song.piano.vel_gesture = Some(VelGesture::Ramp {
            base: c.notes.clone(),
            candidates: selected,
            beat_a: cx / vp.ppb,
            vel_a: vel,
            current: c.notes.clone(),
        });
        self.song.piano.ramp_line = Some([cx, vel, cx, vel]);
    }

    pub fn vel_move(&mut self, x: f32, y: f32) {
        let vp = self.song.piano.vp;
        let cx = x as f64 + vp.left;
        let vel = Self::y_to_vel(y);
        let Some(mut g) = self.song.piano.vel_gesture.take() else {
            return;
        };
        match &mut g {
            VelGesture::Set {
                base,
                indices,
                current,
            } => {
                *current = pr::set_velocities(base, indices, vel);
                self.song.piano.draft = Some(current.clone());
                self.song.piano.readout = Some((cx, y, format!("{}", pr::velocity_to_midi(vel))));
            }
            VelGesture::Offset {
                base,
                indices,
                start_y,
                current,
            } => {
                let delta = ((*start_y - y) / Self::vel_usable()) as f64;
                *current = pr::offset_velocities(base, indices, delta);
                self.song.piano.draft = Some(current.clone());
                let d = (delta * 127.0).round() as i64;
                self.song.piano.readout =
                    Some((cx, y, format!("{}{d}", if d >= 0 { "+" } else { "" })));
            }
            VelGesture::Ramp {
                base,
                candidates,
                beat_a,
                vel_a,
                current,
            } => {
                *current = pr::ramp_velocities(
                    base,
                    candidates.as_deref(),
                    *beat_a,
                    *vel_a,
                    cx / vp.ppb,
                    vel,
                );
                self.song.piano.draft = Some(current.clone());
                self.song.piano.ramp_line = Some([*beat_a * vp.ppb, *vel_a, cx, vel]);
                self.song.piano.readout = Some((
                    cx,
                    y,
                    format!(
                        "{} → {}",
                        pr::velocity_to_midi(*vel_a),
                        pr::velocity_to_midi(vel)
                    ),
                ));
            }
        }
        self.song.piano.vel_gesture = Some(g);
    }

    pub fn vel_up(&mut self, cancelled: bool) {
        let g = self.song.piano.vel_gesture.take();
        self.song.piano.readout = None;
        self.song.piano.ramp_line = None;
        let Some(g) = g else { return };
        let (base, current) = match g {
            VelGesture::Set { base, current, .. }
            | VelGesture::Offset { base, current, .. }
            | VelGesture::Ramp { base, current, .. } => (base, current),
        };
        if cancelled || current == base {
            self.song.piano.draft = None;
            return;
        }
        let sel = self.pr_selected(current.len());
        self.pr_commit(current, "Edit Velocity", sel);
    }

    // --- ruler --------------------------------------------------------------------------------------

    /// Ruler pointer down at viewport x, y.
    pub fn ruler_down(&mut self, x: f32, y: f32, m: rune::Mods) {
        let Some(c) = self.piano_ctx() else { return };
        let vp = self.song.piano.vp;
        let free = m.alt || m.cmd;
        let beat = (x as f64 + vp.left) / vp.ppb;
        let snap = self.song.piano.prefs.snap;
        if y < LOOP_STRIP + 2.0 || m.shift {
            let anchor = if free {
                beat
            } else {
                pr::snap_floor(beat, if snap > 0.0 { snap } else { 1.0 }, c.start)
            };
            self.song.piano.ruler_drag = Some(RulerDrag::Loop {
                anchor,
                start_x: x,
                moved: false,
                current: anchor,
            });
            return;
        }
        self.song.piano.ruler_drag = Some(RulerDrag::Position);
        let rel = if free {
            beat
        } else {
            pr::snap_beat(beat, snap, c.start)
        };
        self.pr_set_position(rel.max(-c.start));
    }

    pub fn ruler_move(&mut self, x: f32, m: rune::Mods) {
        let Some(c) = self.piano_ctx() else { return };
        let vp = self.song.piano.vp;
        let free = m.alt || m.cmd;
        let beat = (x as f64 + vp.left) / vp.ppb;
        let snap = self.song.piano.prefs.snap;
        match self.song.piano.ruler_drag {
            Some(RulerDrag::Position) => {
                let rel = if free {
                    beat
                } else {
                    pr::snap_beat(beat, snap, c.start)
                };
                self.pr_set_position(rel.max(-c.start));
            }
            Some(RulerDrag::Loop {
                anchor,
                start_x,
                mut moved,
                ..
            }) => {
                if (x - start_x).abs() > 3.0 {
                    moved = true;
                }
                if !moved {
                    return;
                }
                let cur = if free {
                    beat
                } else {
                    pr::snap_beat(beat, if snap > 0.0 { snap } else { 1.0 }, c.start)
                }
                .max(-c.start);
                self.song.piano.ruler_drag = Some(RulerDrag::Loop {
                    anchor,
                    start_x,
                    moved,
                    current: cur,
                });
                let (a, b) = (anchor.min(cur), anchor.max(cur));
                self.song.piano.draft_loop = (b > a).then_some((a, b));
            }
            None => {}
        }
    }

    pub fn ruler_up(&mut self) {
        let drag = self.song.piano.ruler_drag.take();
        let Some(RulerDrag::Loop {
            anchor,
            moved,
            current,
            ..
        }) = drag
        else {
            return;
        };
        self.song.piano.draft_loop = None;
        if !moved {
            self.toggle_song_loop();
            return;
        }
        let (a, b) = (anchor.min(current), anchor.max(current));
        if b - a > 1e-3 {
            self.pr_set_loop(a, b);
        }
    }

    /// `handleSetPosition(rel)`: seek while playing, else set the cue.
    pub fn pr_set_position(&mut self, rel: f64) {
        let Some(c) = self.piano_ctx() else { return };
        let abs = (rel + c.start).max(0.0);
        if self.svc.engine.get_state() != TransportState::Stopped {
            self.svc.engine.seek(abs);
        } else {
            self.song.piano.cue = abs;
        }
    }

    /// `handleSetLoop(a, b)`: set the loop, switching it on if it was off.
    pub fn pr_set_loop(&mut self, a: f64, b: f64) {
        let Some(c) = self.piano_ctx() else { return };
        let (s, e) = ((a + c.start).max(0.0), (b + c.start).max(0.0));
        let on = self.svc.engine.doc().is_loop_enabled();
        self.set_loop_markers(s, e);
        if !on {
            self.toggle_song_loop();
        }
    }

    // --- key columns --------------------------------------------------------------------------------

    /// Press / drag across the key column at viewport y: audition each new row.
    pub fn pr_key_at(&mut self, y: f32, down: bool) {
        let Some(c) = self.piano_ctx() else { return };
        let vp = self.song.piano.vp;
        let row = ((y as f64 + vp.top) / vp.row_h).floor();
        if row < 0.0 || row >= c.layout.row_count as f64 {
            return;
        }
        let r = row as i64;
        if !down && self.song.piano.key_press.map(|k| k.0) == Some(r) {
            return;
        }
        if !down && self.song.piano.key_press.is_none() {
            return;
        }
        let pitch = c.layout.row_to_pitch(row);
        self.song.piano.key_press = Some((r, pitch));
        self.pr_preview(pitch, DEFAULT_NOTE_VELOCITY);
    }

    pub fn pr_key_up(&mut self) {
        self.song.piano.key_press = None;
    }
}

/// Which pane a wheel event came from (`makeWheel(source)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WheelSource {
    Grid,
    Ruler,
    Keys,
    Lane,
}

/// The status text (`status`): one selected note's row · position · length · velocity, or the
/// hover cursor.
pub fn status_text(s: &App, c: &Ctx) -> String {
    let sel = s.pr_selected(c.notes.len());
    if sel.len() == 1 {
        let n = &c.notes[sel[0]];
        return format!(
            "{} · {} · {} · vel {}",
            c.layout.row_label(c.layout.pitch_to_row(n.note) as i64),
            pr::format_position(n.start_beat + c.start),
            pr::format_beats(n.duration_beats),
            (n.velocity * 127.0).round() as i64
        );
    }
    if let Some((beat, pitch)) = s.song.piano.cursor {
        let label = pitch.map(|p| {
            if c.is_drum {
                c.layout.row_label(c.layout.pitch_to_row(p) as i64)
            } else {
                mdaw_model::notes::midi_to_note_name(p)
            }
        });
        return match label {
            Some(l) if !l.is_empty() => format!("{} · {l}", pr::format_position(beat + c.start)),
            _ => pr::format_position(beat + c.start),
        };
    }
    String::new()
}

/// `LayoutKind` helper for the view.
pub fn is_drum_layout(l: &RowLayout) -> bool {
    l.kind == LayoutKind::Drum
}
