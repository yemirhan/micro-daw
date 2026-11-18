//! `PianoRollEditor.tsx`'s view: toolbar, the corner cell + ruler, the key column (piano keys
//! or drum rows) + note grid, the velocity gutter + lane. The canvases of the TS
//! (`NoteGrid`, `PianoRollRuler`, `VelocityLane`, `PianoKeyColumn`) are Rune shapes drawn in
//! viewport points; notes are elements (rounded, bordered, named). Logic: `crate::piano`.

pub mod toolbar;

use mdaw_model::RegionNote;
use mdaw_model::notes::midi_to_note_name;
use mdaw_model::piano_roll::{
    self as pr, BEATS_PER_BAR, LayoutKind, NoteEdge, PianoRollTool, is_black_key, is_shaded_row,
};
use rune::{Color, Cx, El, Phase, Shape, Tx, WheelEvent, canvas, col, el, row, text};

use crate::app::App;
use crate::piano::{
    Ctx, DRUM_MIN_PX, KEY_COL_W, LOOP_STRIP, NOTE_MIN_PX, PR_RULER_H, VEL_H, VEL_PAD_BOTTOM,
    VEL_PAD_TOP, WheelSource,
};
use crate::theme::{mono, t, ui};
use crate::ui::song::{cmix, track_color};

/// The canvases' theme colours (`buildColors` of `canvas.ts`).
pub struct Colors {
    pub background: Color,
    pub card: Color,
    pub fg: Color,
    pub muted_fg: Color,
    pub border: Color,
    pub primary: Color,
    pub primary_fg: Color,
    pub row_white: Color,
    pub row_black: Color,
    pub outside: Color,
    pub line_bar: Color,
    pub line_beat: Color,
    pub line_sub: Color,
    pub line_octave: Color,
    pub key_white: Color,
    pub key_black: Color,
    pub key_label: Color,
    pub key_hover: Color,
    pub selection: Color,
    pub selection_border: Color,
    pub cue: Color,
    pub loop_: Color,
    pub loop_fill: Color,
    pub ghost: Color,
    pub ruler: Color,
}

pub fn colors() -> Colors {
    let th = t();
    let (bg, card, fg) = (th.surface0, th.surface1, th.fg);
    Colors {
        background: bg,
        card,
        fg,
        muted_fg: th.muted_fg,
        border: th.border,
        primary: th.primary,
        primary_fg: th.primary_fg,
        row_white: cmix(card, bg, 0.70),
        row_black: cmix(bg, Color::BLACK, 0.86),
        outside: cmix(bg, Color::BLACK, 0.84),
        line_bar: fg.alpha(0.24),
        line_beat: fg.alpha(0.11),
        line_sub: fg.alpha(0.05),
        line_octave: fg.alpha(0.12),
        key_white: cmix(fg, card, 0.82),
        key_black: cmix(bg, Color::BLACK, 0.60),
        key_label: cmix(bg, fg, 0.70),
        key_hover: cmix(th.primary, fg, 0.55),
        selection: th.primary.alpha(0.14),
        selection_border: th.primary.alpha(0.70),
        cue: fg.alpha(0.45),
        loop_: th.primary.alpha(0.80),
        loop_fill: th.primary.alpha(0.09),
        ghost: th.muted_fg.alpha(0.30),
        ruler: cmix(card, bg, 0.80),
    }
}

/// `velocityRamp(base, 12)`.
pub struct Ramp {
    pub fill: Vec<Color>,
    pub selected: Color,
    pub border: Color,
    pub text: Color,
}

pub fn velocity_ramp(base: Color) -> Ramp {
    let bg = t().surface0;
    let fill = (0..12)
        .map(|i| {
            let tt = i as f32 / 11.0;
            let c = cmix(base, bg, (30.0 + tt * 70.0) / 100.0);
            if tt > 0.75 {
                cmix(Color::WHITE, c, (tt - 0.75) * 60.0 / 100.0)
            } else {
                c
            }
        })
        .collect();
    Ramp {
        fill,
        selected: cmix(Color::WHITE, base, 0.55),
        border: cmix(base, Color::BLACK, 0.55),
        text: cmix(Color::BLACK, base, 0.78),
    }
}

fn rect(sh: Shape, x: f32, y: f32, w: f32, h: f32, c: Color) -> Shape {
    if w <= 0.0 || h <= 0.0 {
        return sh;
    }
    sh.fill(vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]], c)
}

fn wheel(source: WheelSource) -> impl Fn(&mut App, WheelEvent, &mut Tx) -> bool + Clone + 'static {
    move |s: &mut App, e: WheelEvent, _tx: &mut Tx| {
        // Rune's deltas move the content; the DOM's scroll it.
        s.pr_wheel(
            source,
            -e.dx as f64,
            -e.dy as f64,
            e.x as f64,
            e.y as f64,
            e.mods,
        );
        true
    }
}

pub fn view(s: &App, cx: &mut Cx<App>) -> El {
    let th = t();
    let Some(c) = s.piano_ctx() else {
        return row()
            .key("piano-roll-gone")
            .grow(1.0)
            .center()
            .gap(8.0)
            .child(
                text("This region no longer exists.")
                    .font(ui(14.0, 400.0))
                    .color(th.muted_fg),
            )
            .child(
                text("Back to arrangement")
                    .key("piano-roll-back-link")
                    .role("link")
                    .font(ui(14.0, 400.0))
                    .color(th.muted_fg)
                    .on_press(|s: &mut App| s.close_piano_roll()),
            );
    };
    if let Some([w, h]) = cx.layout_size("pr-grid") {
        s.song.piano.measured.set([w as f64, h as f64]);
    }
    let cols = colors();
    let snap = s.song.piano.prefs.snap;
    col()
        .key("piano-roll")
        .grow(1.0)
        .min_h(0.0)
        .min_w(0.0)
        .fill(th.surface0)
        .child(toolbar::view(s, &c, cx))
        .child(
            row()
                .w_full()
                .child(
                    row()
                        .key("pr-corner")
                        .w(KEY_COL_W)
                        .h(PR_RULER_H)
                        .items_end()
                        .px(8.0)
                        .pb(4.0)
                        .fill(th.surface1)
                        .border_sides([0.0, 1.0, 1.0, 0.0], th.border)
                        .child(
                            text(if snap > 0.0 {
                                format!("grid {}", pr::format_beats(snap))
                            } else {
                                "grid off".into()
                            })
                            .font(mono(9.0, 400.0))
                            .color(th.muted_fg),
                        ),
                )
                .child(ruler(s, &c, &cols)),
        )
        .child(
            row()
                .grow(1.0)
                .min_h(0.0)
                .w_full()
                .child(if c.is_drum {
                    drum_keys(s, &c)
                } else {
                    piano_keys(s, &c, &cols)
                })
                .child(grid(s, &c, &cols)),
        )
        .child(
            row()
                .w_full()
                .border_sides([1.0, 0.0, 0.0, 0.0], th.border)
                .child(vel_gutter())
                .child(velocity_lane(s, &c, &cols)),
        )
}

// --- grid --------------------------------------------------------------------------------------

fn grid(s: &App, c: &Ctx, k: &Colors) -> El {
    let vp = s.song.piano.vp;
    let (w, h) = (vp.width as f32, vp.height as f32);
    let (ppb, rh) = (vp.ppb, vp.row_h);
    let x = |b: f64| (b * ppb - vp.left) as f32;
    let start_beat = vp.left / ppb;
    let end_beat = (vp.left + vp.width) / ppb;
    let rows = c.layout.row_count as i64;
    let first_row = ((vp.top / rh).floor() as i64).max(0);
    let last_row = (((vp.top + vp.height) / rh).ceil() as i64).min(rows - 1);
    let hover = s.song.piano.hover_row;
    let mut sh = rect(Shape::new(), 0.0, 0.0, w, h, k.outside);
    for r in first_row..=last_row {
        let y = (r as f64 * rh - vp.top) as f32;
        let c0 = if is_shaded_row(&c.layout, r as f64) {
            k.row_black
        } else {
            k.row_white
        };
        sh = rect(sh, 0.0, y, w, rh as f32, c0);
        if hover == Some(r) {
            sh = rect(sh, 0.0, y, w, rh as f32, k.selection);
        }
    }
    let rows_bottom = (rows as f64 * rh - vp.top) as f32;
    let vh = h.min(rows_bottom);
    // Alternate bars.
    let first_bar = ((start_beat + c.start) / BEATS_PER_BAR).floor() as i64;
    let last_bar = ((end_beat + c.start) / BEATS_PER_BAR).ceil() as i64;
    for bar in first_bar..=last_bar {
        if bar % 2 == 0 {
            continue;
        }
        let bx = x(bar as f64 * BEATS_PER_BAR - c.start);
        sh = rect(sh, bx, 0.0, (BEATS_PER_BAR * ppb) as f32, vh, k.line_sub);
    }
    if rh >= 9.0 || c.layout.kind == LayoutKind::Drum {
        for r in first_row..=last_row {
            let y = ((r + 1) as f64 * rh - vp.top).round() as f32 - 1.0;
            sh = rect(sh, 0.0, y, w, 1.0, k.line_sub);
        }
    }
    if c.layout.kind == LayoutKind::Piano {
        for r in first_row..=last_row {
            if c.layout.row_to_pitch(r as f64) as i64 % 12 != 0 {
                continue;
            }
            let y = ((r + 1) as f64 * rh - vp.top).round() as f32 - 1.0;
            sh = rect(sh, 0.0, y, w, 1.0, k.line_octave);
        }
    }
    let lines = pr::compute_grid_lines(
        start_beat - 1.0,
        end_beat + 1.0,
        ppb,
        snap(s),
        c.start,
        pr::GRID_MIN_SPACING,
    );
    for level in [2u8, 1, 0] {
        let col = match level {
            0 => k.line_bar,
            1 => k.line_beat,
            _ => k.line_sub,
        };
        for l in lines.iter().filter(|l| l.level == level) {
            sh = rect(sh, x(l.beat).round(), 0.0, 1.0, vh, col);
        }
    }
    let end_x = x(c.length);
    if end_x < w {
        sh = rect(sh, end_x.max(0.0), 0.0, w, vh, k.outside.alpha(0.55));
        sh = rect(sh, end_x.round(), 0.0, 1.0, vh, k.muted_fg);
    }
    let min_w = if c.is_drum { DRUM_MIN_PX } else { NOTE_MIN_PX };
    // Ghost notes (other regions of the track).
    if let Some(track) = s.find_track(&c.track) {
        for r in &track.regions {
            if r.id == c.region || r.audio.is_some() {
                continue;
            }
            for n in &r.notes {
                let row = c.layout.pitch_to_row(n.note);
                if row < first_row as f64 || row > last_row as f64 {
                    continue;
                }
                let nx = x(n.start_beat + r.start_beat - c.start);
                let nw = pr::note_visual_width(n, ppb, min_w) as f32;
                if nx > w || nx + nw < 0.0 {
                    continue;
                }
                sh = rect(
                    sh,
                    nx,
                    (row * rh - vp.top) as f32 + 2.0,
                    nw,
                    rh as f32 - 4.0,
                    k.ghost,
                );
            }
        }
    }
    let mut g = el()
        .key("pr-grid")
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .clip_rounded(0.0)
        .cursor(match s.song.piano.tool {
            PianoRollTool::Draw => rune::Cursor::Crosshair,
            PianoRollTool::Eraser => rune::Cursor::Cell,
            PianoRollTool::Pointer => rune::Cursor::Default,
        })
        .child(canvas(sh).absolute(0.0, 0.0).size(w, h).inert())
        .on_wheel(wheel(WheelSource::Grid))
        .on_drag(|s: &mut App, d: &mut rune::Drag, tx: &mut Tx| {
            let m = tx.mods();
            match d.phase {
                Phase::Down => s.pr_grid_down(d.x, d.y, d.clicks, m),
                Phase::Move => {
                    s.pr_autoscroll(d.x, d.y);
                    s.pr_grid_move(d.x, d.y, d.dx, d.dy, m);
                }
                Phase::Up => s.pr_grid_up(d.cancelled()),
            }
        })
        .on_pointer_move_local(|s: &mut App, p: rune::PointerEvent, _tx: &mut Tx| {
            if s.song.piano.gesture.is_none() {
                s.pr_grid_hover(p.x, p.y);
            }
        })
        .on_hover_change(|s: &mut App, inside: bool, _tx: &mut Tx| {
            if !inside {
                s.pr_grid_leave();
            }
        })
        .on_secondary_press_local(|s: &mut App, p: rune::PointerEvent, _tx: &mut Tx| {
            s.pr_grid_secondary(p.x, p.y);
        });
    // Notes: unselected first, selected on top.
    let base = track_color(
        &s.find_track(&c.track)
            .map(|t| t.color.clone())
            .unwrap_or_default(),
    );
    let ramp = velocity_ramp(base);
    let sel = &s.song.piano.selection;
    let order: Vec<usize> = (0..c.notes.len())
        .filter(|i| !sel.contains(i))
        .chain(sel.iter().copied().filter(|i| *i < c.notes.len()))
        .collect();
    for i in order {
        let n = &c.notes[i];
        if let Some(e) = note_el(s, c, k, &ramp, n, i, sel.contains(&i), first_row, last_row) {
            g = g.child(e);
        }
    }
    // Cue + playhead.
    let playing = s.song_state() != mdaw_engine::engine::TransportState::Stopped;
    if !playing {
        let cx = x(s.song.piano.cue - c.start).round() + 0.5;
        if cx >= 0.0 && cx <= w {
            let mut dash = Shape::new();
            let mut y = 0.0;
            while y < h {
                dash = rect(dash, cx - 0.5, y, 1.0, 3.0, k.cue);
                y += 6.0;
            }
            g = g.child(
                canvas(dash)
                    .key("pr-cue")
                    .absolute(0.0, 0.0)
                    .size(w, h)
                    .inert(),
            );
        }
    }
    g = g.child(
        el().key("pr-playhead")
            .absolute(0.0, 0.0)
            .w(1.5)
            .h(h)
            .fill(k.primary)
            .live_offset(&s.song.live.pr_playhead)
            .live_opacity(&s.song.live.pr_playhead_on)
            .inert(),
    );
    if let Some([x0, y0, x1, y1]) = s.song.piano.band {
        let bx = (x0.min(x1) - vp.left) as f32;
        let by = (y0.min(y1) - vp.top) as f32;
        g = g.child(
            el().key("pr-band")
                .absolute(bx, by)
                .size((x1 - x0).abs() as f32, (y1 - y0).abs() as f32)
                .fill(k.selection)
                .border(1.0, k.selection_border)
                .inert(),
        );
    }
    g
}

fn snap(s: &App) -> f64 {
    s.song.piano.prefs.snap
}

#[allow(clippy::too_many_arguments)]
fn note_el(
    s: &App,
    c: &Ctx,
    k: &Colors,
    ramp: &Ramp,
    n: &RegionNote,
    i: usize,
    sel: bool,
    first_row: i64,
    last_row: i64,
) -> Option<El> {
    let vp = s.song.piano.vp;
    let row = c.layout.pitch_to_row(n.note);
    if row < first_row as f64 || row > last_row as f64 || row < 0.0 {
        return None;
    }
    let min_w = if c.is_drum { DRUM_MIN_PX } else { NOTE_MIN_PX };
    let nx = (n.start_beat * vp.ppb - vp.left) as f32;
    let w = pr::note_visual_width(n, vp.ppb, min_w) as f32;
    if nx > vp.width as f32 || nx + w < 0.0 {
        return None;
    }
    let ny = (row * vp.row_h - vp.top) as f32 + 1.0;
    let nh = (vp.row_h as f32 - 2.0).max(2.0);
    let r = if c.is_drum {
        c.layout
            .row_color(row as i64)
            .map(|col| velocity_ramp(track_color(col)))
    } else {
        None
    };
    let r = r.as_ref().unwrap_or(ramp);
    let bucket = pr::velocity_bucket(n.velocity, 12.0) as usize;
    let fill = if sel {
        r.selected
    } else {
        r.fill[bucket.min(11)]
    };
    let radius = (vp.row_h as f32 / 4.0).min(3.0);
    let mut e = el()
        .key(format!("pr-note-{i}"))
        .label(format!(
            "{} {}",
            midi_to_note_name(n.note),
            pr::format_position(n.start_beat + c.start)
        ))
        .checked(sel)
        .absolute(nx + 0.5, ny + 0.5)
        .size((w - 1.0).max(1.0), nh - 1.0)
        .radius(radius)
        .fill(fill)
        .border(
            if sel { 1.5 } else { 1.0 },
            if sel { k.fg } else { r.border },
        )
        .cursor(match s.song.piano.tool {
            PianoRollTool::Draw => rune::Cursor::Pointer,
            PianoRollTool::Eraser => rune::Cursor::Cell,
            PianoRollTool::Pointer => rune::Cursor::Grab,
        });
    if !c.is_drum && w > 10.0 && nh >= 8.0 {
        e = e.child(
            el().absolute(1.5, nh - 3.5)
                .size(((w - 4.0) * n.velocity as f32).max(1.0), 1.5)
                .fill(r.border.alpha(0.5)),
        );
    }
    if vp.row_h >= 11.0 && !c.is_drum && w > 28.0 {
        e = e.child(
            text(midi_to_note_name(n.note))
                .font(mono((vp.row_h as f32 - 5.0).clamp(8.0, 10.0), 600.0))
                .color(if sel { k.background } else { r.text })
                .absolute(3.5, 0.0)
                .h(nh - 1.0)
                .max_w(w - 6.0)
                .center_y()
                .truncate(),
        );
    }
    if !c.is_drum && s.song.piano.tool != PianoRollTool::Eraser && w >= 12.0 {
        for (edge, ex) in [(NoteEdge::Start, 0.0), (NoteEdge::End, w - 6.0)] {
            e = e.child(
                el().key(format!("pr-note-{i}-{}", edge.as_str()))
                    .absolute(ex, 0.0)
                    .size(6.0, nh)
                    .cursor(rune::Cursor::ResizeColumn),
            );
        }
    }
    Some(e)
}

// --- ruler -------------------------------------------------------------------------------------

fn ruler(s: &App, c: &Ctx, k: &Colors) -> El {
    let vp = s.song.piano.vp;
    let w = vp.width as f32;
    let hh = PR_RULER_H;
    let x = |b: f64| (b * vp.ppb - vp.left) as f32;
    let start_beat = vp.left / vp.ppb;
    let end_beat = (vp.left + vp.width) / vp.ppb;
    let mut sh = rect(Shape::new(), 0.0, 0.0, w, hh, k.ruler);
    let end_x = x(c.length);
    if end_x < w {
        sh = rect(sh, end_x.max(0.0), 0.0, w - end_x.max(0.0), hh, k.outside);
    }
    sh = rect(sh, 0.0, 0.0, w, LOOP_STRIP, k.loop_fill);
    let doc = s.svc.engine.doc();
    let loop_on = doc.is_loop_enabled();
    let shown = s.song.piano.draft_loop.or_else(|| {
        doc.get_loop_markers()
            .map(|m| (m.start_beat - c.start, m.end_beat - c.start))
    });
    if let Some((a, b)) = shown {
        let (lx0, lx1) = (x(a), x(b));
        let alpha = if loop_on || s.song.piano.draft_loop.is_some() {
            1.0
        } else {
            0.4
        };
        sh = rect(
            sh,
            lx0,
            1.0,
            lx1 - lx0,
            LOOP_STRIP - 2.0,
            k.loop_.alpha(0.8 * alpha),
        );
    }
    for l in pr::compute_grid_lines(
        start_beat - 1.0,
        end_beat + 1.0,
        vp.ppb,
        snap(s),
        c.start,
        8.0,
    ) {
        let lx = x(l.beat).round();
        let h = match l.level {
            0 => hh - LOOP_STRIP,
            1 => 7.0,
            _ => 4.0,
        };
        sh = rect(
            sh,
            lx,
            hh - h,
            1.0,
            h,
            if l.level == 0 {
                k.line_bar
            } else {
                k.line_beat
            },
        );
    }
    if end_x >= 0.0 && end_x <= w {
        sh = rect(
            sh,
            end_x.round(),
            LOOP_STRIP,
            1.0,
            hh - LOOP_STRIP,
            k.muted_fg,
        );
    }
    let playing = s.song_state() != mdaw_engine::engine::TransportState::Stopped;
    let tri = |sh: Shape, beat: f64, col: Color| {
        let px = x(beat);
        if px < -8.0 || px > w + 8.0 {
            return sh;
        }
        let sh = sh.fill(
            vec![[px - 5.0, hh - 9.0], [px + 5.0, hh - 9.0], [px, hh - 1.0]],
            col,
        );
        rect(
            sh,
            px.round() - 0.5,
            LOOP_STRIP,
            1.0,
            hh - LOOP_STRIP - 9.0,
            col,
        )
    };
    if !playing {
        sh = tri(sh, s.song.piano.cue - c.start, k.cue);
    } else {
        sh = tri(sh, s.svc.engine.get_position() - c.start, k.primary);
    }
    sh = rect(sh, 0.0, hh - 1.0, w, 1.0, k.border);
    let mut r = el()
        .key("pr-ruler")
        .grow(1.0)
        .min_w(0.0)
        .h(hh)
        .clip_rounded(0.0)
        .cursor(rune::Cursor::Pointer)
        .help("Click/drag: set playhead · Drag the top strip (or Shift+drag): set loop · Click the strip: toggle loop")
        .child(canvas(sh).absolute(0.0, 0.0).size(w, hh).inert());
    for l in pr::compute_ruler_labels(start_beat - 4.0, end_beat, vp.ppb, c.start, 40.0) {
        r = r.child(
            text(l.label.clone())
                .font(mono(10.0, 600.0))
                .color(if l.major { k.fg } else { k.muted_fg })
                .absolute(x(l.beat) + 4.0, LOOP_STRIP + 3.0)
                .inert(),
        );
    }
    r.on_wheel(wheel(WheelSource::Ruler))
        .on_drag(|s: &mut App, d: &mut rune::Drag, tx: &mut Tx| {
            let m = tx.mods();
            match d.phase {
                Phase::Down => s.ruler_down(d.x, d.y, m),
                Phase::Move => s.ruler_move(d.x, m),
                Phase::Up => s.ruler_up(),
            }
        })
}

// --- key columns -------------------------------------------------------------------------------

fn lit_pitches(s: &App, c: &Ctx) -> Vec<f64> {
    let mut v: Vec<f64> = s
        .pr_selected(c.notes.len())
        .into_iter()
        .map(|i| c.notes[i].note)
        .collect();
    if let Some((_, p)) = s.song.piano.key_press {
        v.push(p);
    }
    v
}

fn key_drag(s: &mut App, d: &mut rune::Drag, _tx: &mut Tx) {
    match d.phase {
        Phase::Down => s.pr_key_at(d.y, true),
        Phase::Move => s.pr_key_at(d.y, false),
        Phase::Up => s.pr_key_up(),
    }
}

fn piano_keys(s: &App, c: &Ctx, k: &Colors) -> El {
    let vp = s.song.piano.vp;
    let (w, h, rh) = (KEY_COL_W, vp.height as f32, vp.row_h);
    let rows = c.layout.row_count as i64;
    let first = ((vp.top / rh).floor() as i64 - 1).max(0);
    let last = (((vp.top + vp.height) / rh).ceil() as i64 + 1).min(rows - 1);
    let black_w = (w * 0.6).round();
    let lit = lit_pitches(s, c);
    let hover = s.song.piano.hover_row;
    let mut sh = rect(Shape::new(), 0.0, 0.0, w, h, k.key_white);
    for r in first..=last {
        let p = c.layout.row_to_pitch(r as f64);
        let y = (r as f64 * rh - vp.top) as f32;
        if is_black_key(p) {
            continue;
        }
        let on = lit.contains(&p);
        if on || hover == Some(r) {
            sh = rect(
                sh,
                0.0,
                y,
                w,
                rh as f32,
                if on {
                    k.primary.alpha(0.85)
                } else {
                    k.key_hover.alpha(0.35)
                },
            );
        }
        let pc = p as i64 % 12;
        if pc == 0 || pc == 5 {
            sh = rect(sh, 0.0, (y + rh as f32).round() - 1.0, w, 1.0, k.border);
        }
    }
    for r in first..=last {
        let p = c.layout.row_to_pitch(r as f64);
        if !is_black_key(p) {
            continue;
        }
        let y = (r as f64 * rh - vp.top) as f32;
        sh = rect(
            sh,
            black_w,
            (y + rh as f32 / 2.0).round(),
            w - black_w,
            1.0,
            k.border,
        );
        let on = lit.contains(&p);
        sh = rect(
            sh,
            0.0,
            y + 0.5,
            black_w,
            rh as f32 - 1.0,
            if on { k.primary } else { k.key_black },
        );
        if hover == Some(r) && !on {
            sh = rect(
                sh,
                0.0,
                y + 0.5,
                black_w,
                rh as f32 - 1.0,
                k.key_hover.alpha(0.35),
            );
        }
    }
    sh = rect(sh, w - 1.0, 0.0, 1.0, h, k.border);
    let mut col_el = el()
        .key("pr-keys")
        .w(w)
        .h_full()
        .clip_rounded(0.0)
        .cursor(rune::Cursor::Pointer)
        .help("Click or drag across the keys to audition")
        .child(canvas(sh).absolute(0.0, 0.0).size(w, h).inert());
    if rh >= 7.0 {
        for r in first..=last {
            let p = c.layout.row_to_pitch(r as f64);
            let is_c = p as i64 % 12 == 0;
            if !is_c && hover != Some(r) {
                continue;
            }
            let y = (r as f64 * rh - vp.top) as f32;
            let on = lit.contains(&p);
            let black = is_black_key(p);
            let colr = if on {
                k.primary_fg
            } else if black {
                k.fg
            } else {
                k.key_label
            };
            let t = text(midi_to_note_name(p))
                .font(mono(9.0, 600.0))
                .color(colr)
                .h(rh as f32)
                .center_y()
                .inert();
            col_el = col_el.child(if black {
                t.absolute(black_w + 4.0, y)
            } else {
                row()
                    .absolute(0.0, y)
                    .w(w - 5.0)
                    .h(rh as f32)
                    .justify_end()
                    .inert()
                    .child(t)
            });
        }
    }
    col_el.on_wheel(wheel(WheelSource::Keys)).on_drag(key_drag)
}

fn drum_keys(s: &App, c: &Ctx) -> El {
    let th = t();
    let vp = s.song.piano.vp;
    let lit = lit_pitches(s, c);
    let hover = s.song.piano.hover_row;
    let mut col_el = el()
        .key("pr-drum-keys")
        .w(KEY_COL_W)
        .h_full()
        .clip_rounded(0.0)
        .fill(th.surface1)
        .border_sides([0.0, 1.0, 0.0, 0.0], th.border);
    for r in 0..c.layout.row_count as i64 {
        let p = c.layout.row_to_pitch(r as f64);
        let y = (r as f64 * vp.row_h - vp.top) as f32;
        if y > vp.height as f32 || y + (vp.row_h as f32) < 0.0 {
            continue;
        }
        let on = lit.contains(&p);
        let bg = if on {
            th.primary.alpha(0.25)
        } else if hover == Some(r) {
            th.surface3
        } else if r % 2 == 1 {
            th.surface0.alpha(0.6)
        } else {
            th.surface1
        };
        let dot = c.layout.row_color(r).map_or(th.muted_fg, track_color);
        let mut b = row()
            .key(format!("pr-drum-row-{r}"))
            .role("button")
            .label(c.layout.row_label(r))
            .help(format!("{} (MIDI {})", c.layout.row_label(r), p as i64))
            .absolute(0.0, y)
            .w(KEY_COL_W - 1.0)
            .h(vp.row_h as f32)
            .items_center()
            .gap(6.0)
            .px(8.0)
            .fill(bg)
            .border_sides([0.0, 0.0, 1.0, 0.0], th.border.alpha(0.6))
            .child(el().size(8.0, 8.0).radius(4.0).fill(dot));
        if vp.row_h >= 12.0 {
            b = b.child(
                text(c.layout.row_label(r))
                    .font(mono(10.0, 600.0))
                    .color(if on { th.fg } else { th.muted_fg })
                    .truncate(),
            );
        }
        col_el = col_el.child(b);
    }
    col_el.on_wheel(wheel(WheelSource::Keys)).on_drag(key_drag)
}

// --- velocity ----------------------------------------------------------------------------------

fn vel_gutter() -> El {
    let th = t();
    let mut g = el()
        .key("pr-vel-gutter")
        .w(KEY_COL_W)
        .h(VEL_H)
        .fill(th.surface1)
        .border_sides([0.0, 1.0, 0.0, 0.0], th.border)
        .child(
            text("Vel")
                .font(ui(10.0, 600.0).tracking(0.25))
                .text_case(rune::TextCase::Uppercase)
                .color(th.muted_fg)
                .absolute(8.0, 6.0),
        );
    for (label, v) in [(127, 1.0f32), (64, 0.5), (1, 0.0)] {
        let y = VEL_PAD_TOP + (1.0 - v) * (VEL_H - VEL_PAD_TOP - VEL_PAD_BOTTOM);
        g = g.child(
            row()
                .absolute(0.0, y - 5.0)
                .w(KEY_COL_W - 8.0)
                .justify_end()
                .child(
                    text(label.to_string())
                        .font(mono(9.0, 400.0))
                        .color(th.muted_fg),
                ),
        );
    }
    g
}

fn velocity_lane(s: &App, c: &Ctx, k: &Colors) -> El {
    let vp = s.song.piano.vp;
    let w = vp.width as f32;
    let h = VEL_H;
    let x = |b: f64| (b * vp.ppb - vp.left) as f32;
    let base = track_color(
        &s.find_track(&c.track)
            .map(|t| t.color.clone())
            .unwrap_or_default(),
    );
    let ramp = velocity_ramp(base);
    let mut sh = rect(Shape::new(), 0.0, 0.0, w, h, k.row_black);
    for v in [0.25, 0.5, 0.75, 1.0] {
        sh = rect(sh, 0.0, App::vel_to_y(v).round(), w, 1.0, k.line_sub);
    }
    let end_x = x(c.length);
    if end_x < w {
        sh = rect(sh, end_x.max(0.0), 0.0, w, h, k.outside.alpha(0.55));
    }
    let bottom = h - VEL_PAD_BOTTOM;
    let sel = &s.song.piano.selection;
    let order: Vec<usize> = (0..c.notes.len())
        .filter(|i| !sel.contains(i))
        .chain(sel.iter().copied().filter(|i| *i < c.notes.len()))
        .collect();
    for i in order {
        let n = &c.notes[i];
        let is_sel = sel.contains(&i);
        let sx = x(n.start_beat);
        if sx < -6.0 || sx > w + 6.0 {
            continue;
        }
        let top = App::vel_to_y(n.velocity);
        let col = if is_sel {
            ramp.selected
        } else {
            ramp.fill[pr::velocity_bucket(n.velocity, 12.0) as usize]
        };
        let bar_w = ((n.duration_beats * vp.ppb) as f32 - 1.0).clamp(2.0, 14.0);
        sh = rect(
            sh,
            sx,
            top,
            bar_w,
            bottom - top,
            col.alpha(if is_sel { 0.35 } else { 0.22 }),
        );
        sh = rect(
            sh,
            sx.round(),
            top,
            if is_sel { 2.0 } else { 1.5 },
            bottom - top,
            col,
        );
        let rr = if is_sel { 3.5 } else { 3.0 };
        let circle: Vec<[f32; 2]> = (0..16)
            .map(|j| {
                let a = j as f32 / 16.0 * std::f32::consts::TAU;
                [sx + 1.0 + rr * a.cos(), top + rr * a.sin()]
            })
            .collect();
        sh = sh.fill(circle.clone(), col);
        if is_sel {
            sh = sh.stroke_with(circle, true, 1.0, rune::LineCap::Butt, k.fg);
        }
    }
    if let Some([x0, v0, x1, v1]) = s.song.piano.ramp_line {
        let (ax, ay) = ((x0 - vp.left) as f32, App::vel_to_y(v0));
        let (bx, by) = ((x1 - vp.left) as f32, App::vel_to_y(v1));
        let len = ((bx - ax).powi(2) + (by - ay).powi(2)).sqrt().max(1.0);
        let mut d = 0.0;
        while d < len {
            let e = (d + 4.0).min(len);
            let p = |t: f32| [ax + (bx - ax) * t / len, ay + (by - ay) * t / len];
            sh = sh.line(p(d), p(e), 1.0, rune::LineCap::Butt, k.fg);
            d += 7.0;
        }
    }
    sh = rect(sh, 0.0, 0.0, w, 1.0, k.border);
    sh = rect(sh, 0.0, h - 1.0, w, 1.0, k.border);
    let mut lane = el()
        .key("pr-velocity")
        .grow(1.0)
        .min_w(0.0)
        .h(h)
        .clip_rounded(0.0)
        .cursor(rune::Cursor::ResizeRow)
        .help("Drag a stem to set velocity · Drag empty space (or Alt+drag) to draw a ramp")
        .child(canvas(sh).absolute(0.0, 0.0).size(w, h).inert())
        .on_wheel(wheel(WheelSource::Lane))
        .on_drag(
            |s: &mut App, d: &mut rune::Drag, tx: &mut Tx| match d.phase {
                Phase::Down => s.vel_down(d.x, d.y, tx.mods().alt),
                Phase::Move => s.vel_move(d.x, d.y),
                Phase::Up => s.vel_up(d.cancelled()),
            },
        );
    if let Some((rx, ry, txt)) = &s.song.piano.readout {
        let tw = txt.chars().count() as f32 * 6.0 + 8.0;
        let x0 = ((*rx - vp.left) as f32 + 8.0).max(2.0).min(w - tw - 2.0);
        let y0 = (*ry - 8.0).min(h - 18.0).max(2.0);
        lane = lane.child(
            row()
                .key("pr-vel-readout")
                .absolute(x0, y0)
                .h(15.0)
                .px(4.0)
                .items_center()
                .fill(k.background.alpha(0.9))
                .inert()
                .child(text(txt.clone()).font(mono(10.0, 600.0)).color(k.fg)),
        );
    }
    lane
}

impl App {
    /// Auto-scroll near the grid's edges while dragging (24-point zone; 12 / 8 points).
    pub fn pr_autoscroll(&mut self, x: f32, y: f32) {
        let vp = self.song.piano.vp;
        let (w, h) = (vp.width as f32, vp.height as f32);
        let edge = 24.0;
        if w < edge * 4.0 || h < edge * 4.0 {
            return;
        }
        let mut left = None;
        let mut top = None;
        if x > w - edge {
            left = Some(vp.left + 12.0);
        } else if x < edge {
            left = Some(vp.left - 12.0);
        }
        if y > h - edge {
            top = Some(vp.top + 8.0);
        } else if y < edge {
            top = Some(vp.top - 8.0);
        }
        if left.is_some() || top.is_some() {
            self.pr_scroll_to(left, top);
        }
    }
}
