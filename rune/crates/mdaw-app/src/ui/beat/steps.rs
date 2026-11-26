//! `StepSequencer.tsx`: header (step size, readout, loop hint / gesture hint, Clear), row
//! labels, the 16-row grid drawn as one shape (beat shading, selected-row tint, cells with
//! velocity as alpha, bar lines), the playhead (a live offset: playback never re-runs the view),
//! the keyboard focus ring, pointer gestures (paint / erase, Alt-drag velocity), the wheel and
//! the keys of `stepKeys.ts`.

use mdaw_model::beat_helpers::{
    KeyEventLike, LoopMismatchInput, STEP_GRIDS, StepCell, StepGrid, StepKeyAction, StepRegion,
    adjust_velocity, build_step_matrix, cell_at, describe_loop_mismatch, describe_step,
    needs_toggle, nudge_velocity, pad_color, pad_label, pad_name, paint_mode_for, row_for_pad,
    step_beats_for, step_count, step_key_action, steps_per_beat,
};
use mdaw_model::types::{LoopMarkers, PAD_COUNT};
use rune::{Color, Cx, El, Key, KeyEvent, Phase, Shape, Tx, canvas, col, el, row, spacer, text};

use crate::app::{App, StepDrag};
use crate::icons;
use crate::theme::{css_color, mono, t, ui};
use crate::ui::widgets::{Button, Segmented, Size, Variant};

pub const LABEL_W: f32 = 132.0;
pub const MIN_CELL_W: f32 = 18.0;
pub const MIN_ROW_H: f32 = 14.0;

/// The grid geometry for a viewport `[w, h]`.
pub fn geometry(box_size: [f32; 2], steps: usize) -> (f32, f32) {
    let row_h = MIN_ROW_H.max((box_size[1] / PAD_COUNT as f32).floor());
    let cell_w = if box_size[0] > 0.0 {
        MIN_CELL_W.max((box_size[0] / steps as f32).floor())
    } else {
        MIN_CELL_W
    };
    (cell_w, row_h)
}

/// The step matrix of the active pattern (`buildStepMatrix`).
pub fn matrix(s: &App) -> (Vec<f32>, usize, f64) {
    let doc = s.svc.engine.doc();
    let sb = step_beats_for(s.beat.grid);
    let bars = doc.get_loop_settings().bars as f64;
    let steps = step_count(bars, sb);
    let start = s.pattern_start();
    let region = s
        .active_track()
        .and_then(|id| doc.get_pattern_region(&id).cloned());
    let m = build_step_matrix(region.as_ref().map(StepRegion::from), start, sb, steps);
    (m, steps, sb)
}

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Vec<[f32; 2]> {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    if r < 0.5 {
        return vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]];
    }
    let mut pts = Vec::with_capacity(16);
    let corners = [
        (x + w - r, y + r, -90.0f32),
        (x + w - r, y + h - r, 0.0),
        (x + r, y + h - r, 90.0),
        (x + r, y + r, 180.0),
    ];
    for (cx, cy, a0) in corners {
        for i in 0..=3 {
            let a = (a0 + 30.0 * i as f32).to_radians();
            pts.push([cx + r * a.cos(), cy + r * a.sin()]);
        }
    }
    pts
}

fn grid_shape(s: &App, m: &[f32], steps: usize, sb: f64, cell_w: f32, row_h: f32) -> Shape {
    let th = t();
    let kit = s.kit();
    let per_beat = steps_per_beat(sb);
    let per_bar = per_beat * 4;
    let grid_w = cell_w * steps as f32;
    let grid_h = row_h * PAD_COUNT as f32;
    let rect =
        |x: f32, y: f32, w: f32, h: f32| vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]];
    let mut shape = Shape::new();
    let mut st = 0;
    while st < steps {
        let c = if (st / per_beat).is_multiple_of(2) {
            th.surface0
        } else {
            th.surface1
        };
        shape = shape.fill(
            rect(st as f32 * cell_w, 0.0, per_beat as f32 * cell_w, grid_h),
            c,
        );
        st += per_beat;
    }
    let sel_row = row_for_pad(s.selected_pad()) as f32;
    shape = shape.fill(
        rect(0.0, sel_row * row_h, grid_w, row_h),
        th.white.alpha(0.045),
    );
    let inset = if cell_w >= 24.0 { 2.0 } else { 1.5 };
    let radius = (cell_w / 6.0).min(3.0);
    for pad in 0..PAD_COUNT {
        let y = row_for_pad(pad) as f32 * row_h;
        let color = css_color(&pad_color(kit.pads.get(pad), pad), th.primary);
        for step in 0..steps {
            let v = m[pad * steps + step];
            let x = step as f32 * cell_w;
            let fill: Color = if v > 0.0 {
                color.alpha(0.3 + 0.7 * v)
            } else if step % per_beat == 0 {
                th.surface3
            } else {
                th.surface2
            };
            shape = shape.fill(
                rounded_rect(
                    x + inset,
                    y + inset,
                    cell_w - inset * 2.0,
                    row_h - inset * 2.0,
                    radius,
                ),
                fill,
            );
        }
    }
    let mut b = per_bar;
    while b < steps {
        shape = shape.fill(
            rect(b as f32 * cell_w - 0.5, 0.0, 1.0, grid_h),
            th.white.alpha(0.16),
        );
        b += per_bar;
    }
    shape
}

fn row_label(s: &App, pad: usize, h: f32) -> El {
    let th = t();
    let kit = s.kit();
    let settings = kit.pads.get(pad);
    let color = css_color(&pad_color(settings, pad), th.primary);
    let name = pad_name(settings);
    let selected = pad == s.selected_pad();
    let has_sample = settings.is_some_and(|x| x.sample.is_some());
    let r = row()
        .key(format!("row-label-{pad}"))
        .role("button")
        .label(format!("Select pad {}", pad_label(pad)))
        .checked(selected)
        .h(h)
        .w_full()
        .items_center()
        .gap(6.0)
        .px(8.0)
        .child(
            el().w(3.0)
                .h((h * 0.6).min(12.0))
                .radius(1.5)
                .fill(color.alpha(if has_sample { 1.0 } else { 0.35 })),
        )
        .child(
            text(pad_label(pad))
                .font(mono(11.0, 400.0))
                .color(th.muted_fg)
                .w(18.0),
        )
        .child(
            text(if name.is_empty() {
                "—".to_owned()
            } else {
                name.clone()
            })
            .font(ui(11.0, if selected { 600.0 } else { 400.0 }).lh(11.0))
            .color(if selected {
                th.fg
            } else if name.is_empty() {
                th.muted_fg
            } else {
                th.fg.alpha(0.8)
            })
            .truncate()
            .min_w(0.0),
        )
        .on_press(move |s: &mut App| s.select_pad(pad));
    if selected {
        r.fill(th.surface3)
    } else {
        r.hover(th.surface2)
    }
}

pub fn view(s: &App, cx: &mut Cx<App>) -> El {
    let th = t();
    let doc = s.svc.engine.doc();
    let bars = doc.get_loop_settings().bars;
    let (m, steps, sb) = matrix(s);
    let box_size = cx.layout_size("steps-box").unwrap_or([0.0, 0.0]);
    let (cell_w, row_h) = geometry(box_size, steps);
    s.steps_geometry.set((cell_w, row_h));
    let grid_w = cell_w * steps as f32;
    let grid_h = row_h * PAD_COUNT as f32;
    let width = cx.layout_size("step-sequencer").map_or(1400.0, |w| w[0]);

    // Header
    let grid_sel = Segmented::new(
        "Step size",
        s.beat.grid,
        STEP_GRIDS
            .iter()
            .map(|g| (*g, g.label().to_owned()))
            .collect(),
    )
    .height(24.0)
    .key("step-size")
    .build(|s: &mut App, g: StepGrid| s.set_grid(g));
    let playing = s.playing();
    let active_loop: Option<LoopMarkers> =
        if doc.is_loop_recording() || s.svc.engine.is_pattern_playback() {
            s.svc.engine.get_active_loop_range()
        } else {
            doc.get_loop_markers()
        };
    let loop_enabled = if doc.is_loop_recording() || s.svc.engine.is_pattern_playback() {
        active_loop.is_some()
    } else {
        doc.is_loop_enabled()
    };
    let hint = describe_loop_mismatch(&LoopMismatchInput {
        r#loop: active_loop,
        loop_enabled,
        playing,
        pattern_start_beat: s.pattern_start(),
        bars: bars as f64,
    });
    let has_notes = m.iter().any(|v| *v > 0.0)
        || s.active_track()
            .and_then(|id| doc.get_pattern_region(&id).map(|r| !r.notes.is_empty()))
            .unwrap_or(false);
    let mut header = row()
        .key("steps-header")
        .h(32.0)
        .w_full()
        .items_center()
        .gap(12.0)
        .px(12.0)
        .child(text("Steps").font(ui(12.0, 600.0)).color(th.fg))
        .child(grid_sel)
        .child(
            text(format!(
                "{steps} steps · {bars} bar{}",
                if bars == 1 { "" } else { "s" }
            ))
            .font(mono(11.0, 400.0))
            .color(th.muted_fg),
        )
        .child(spacer());
    if let Some(h) = hint {
        header = header.child(
            row()
                .key("loop-hint")
                .min_w(0.0)
                .items_center()
                .gap(6.0)
                .child(
                    text(h)
                        .font(ui(11.0, 400.0))
                        .color(th.warn)
                        .truncate()
                        .min_w(0.0)
                        .help("The transport loops a different range than this grid shows"),
                )
                .child(
                    Button::new(Variant::Outline, Size::Xs)
                        .icon(&icons::REPEAT)
                        .label("Loop pattern")
                        .tip("Loop exactly this pattern", None)
                        .key("loop-pattern")
                        .build(|s: &mut App, _| s.loop_pattern()),
                ),
        );
    } else if width >= 760.0 {
        let alt = if cfg!(target_os = "macos") {
            "Option"
        } else {
            "Alt"
        };
        header = header.child(
            text(format!(
                "Click to toggle, drag to paint, {alt}-drag or scroll for velocity"
            ))
            .key("steps-hint")
            .font(ui(11.0, 400.0))
            .color(th.muted_fg)
            .truncate()
            .min_w(0.0),
        );
    }
    header = header.child(
        Button::new(Variant::Ghost, Size::Xs)
            .icon(&icons::TRASH_2)
            .label("Clear")
            .tip("Clear the whole pattern", Some("⇧⌫"))
            .disabled(!has_notes)
            .key("clear-pattern")
            .build(|s: &mut App, _| s.clear_pattern()),
    );

    // Labels
    let mut labels = col()
        .w(LABEL_W)
        .fill(th.surface1)
        .border_sides([0.0, 1.0, 0.0, 0.0], th.border);
    for r in 0..PAD_COUNT {
        labels = labels.child(row_label(s, PAD_COUNT - 1 - r, row_h));
    }

    // Grid
    let focus = StepCell {
        pad: s.beat.step_focus.pad,
        step: s.beat.step_focus.step.min(steps.saturating_sub(1)),
    };
    let ring = el()
        .key("step-focus")
        .absolute(
            focus.step as f32 * cell_w,
            row_for_pad(focus.pad) as f32 * row_h,
        )
        .size(cell_w, row_h)
        .radius(4.0)
        .border(2.0, th.primary)
        .opacity(if s.beat.step_kb_focus { 1.0 } else { 0.0 });
    let playhead = el()
        .key("playhead")
        .absolute(0.0, 0.0)
        .size(cell_w, grid_h)
        .live_offset(&s.live.playhead_px)
        .live_opacity(&s.live.playhead_on)
        .fill(th.fg.alpha(0.07))
        .child(el().absolute(0.0, 0.0).size(2.0, grid_h).fill(th.play));
    let canvas_el = canvas(grid_shape(s, &m, steps, sb, cell_w, row_h))
        .key("step-grid")
        .role("grid")
        .label(format!(
            "Pattern grid, {PAD_COUNT} pads by {steps} steps. Arrow keys move, Space toggles, plus and minus set velocity."
        ))
        .absolute(0.0, 0.0)
        .size(grid_w, grid_h)
        .focusable(true)
        .no_focus_ring()
        .cursor(rune::Cursor::Pointer)
        .on_drag(|s: &mut App, d: &mut rune::Drag, tx: &mut Tx| s.step_drag(d, tx))
        .on_wheel(|s: &mut App, e: rune::WheelEvent, _tx: &mut Tx| s.step_wheel(e))
        .on_key(|s: &mut App, ev: &KeyEvent, tx: &mut Tx| s.step_key(ev, tx));
    let grid = el()
        .size(grid_w, grid_h)
        .child(canvas_el)
        .child(ring)
        .child(playhead);
    let viewport = el()
        .key("steps-box")
        .grow(1.0)
        .min_w(0.0)
        .min_h(0.0)
        .scroll_x()
        .child(grid);
    // CSS border-t takes a pixel of layout (border-box), Rune's borders don't.
    let body = row()
        .grow(1.0)
        .min_h(0.0)
        .pt(1.0)
        .border_sides([1.0, 0.0, 0.0, 0.0], th.border)
        .child(labels)
        .child(viewport);
    let announce = el()
        .key("step-announce")
        .role("status")
        .label(s.beat.step_announce.clone())
        .live_region(rune::LiveRegion::Polite)
        .size(0.0, 0.0);
    let mut sec = col()
        .key("step-sequencer")
        .role("region")
        .label("Step sequencer")
        .grow(1.0)
        .min_h(0.0)
        .min_w(0.0)
        .child(header)
        .child(body)
        .child(announce);
    if s.loop_recording() && s.count_in() == 0 {
        sec = sec.box_shadow(rune::Shadow {
            x: 0.0,
            y: 2.0,
            blur: 0.0,
            spread: 0.0,
            color: th.rec,
            inset: true,
        });
    }
    sec
}

impl App {
    /// The grid's pointer gesture (`onPointerDown` / `onPointerMove` / `endDrag`).
    pub fn step_drag(&mut self, d: &rune::Drag, tx: &mut Tx) {
        let (m, steps, sb) = matrix(self);
        let (cell_w, row_h) = self.steps_geometry.get();
        let cell = cell_at(d.x as f64, d.y as f64, cell_w as f64, row_h as f64, steps);
        match d.phase {
            Phase::Down => {
                let Some(cell) = cell else { return };
                self.end_step_drag();
                self.beat.step_kb_focus = false;
                self.beat.step_focus = cell;
                let v = m[cell.pad * steps + cell.step] as f64;
                if cell.pad != self.selected_pad() {
                    self.select_pad(cell.pad);
                }
                if tx.mods().alt {
                    if v > 0.0 {
                        self.beat.step_drag = Some(StepDrag::Velocity {
                            pad: cell.pad,
                            step: cell.step,
                            start_y: d.y,
                            start_vel: v,
                            last: v,
                            grouped: false,
                        });
                    }
                    return;
                }
                self.beat.step_drag = Some(StepDrag::Paint {
                    mode: paint_mode_for(v),
                    visited: Vec::new(),
                    grouped: false,
                });
                self.paint_cell(cell.pad, cell.step, &m, steps, sb);
            }
            Phase::Move => match self.beat.step_drag.clone() {
                Some(StepDrag::Velocity {
                    pad,
                    step,
                    start_y,
                    start_vel,
                    last,
                    grouped,
                }) => {
                    let next = adjust_velocity(start_vel, (start_y - d.y) as f64, 120.0);
                    if next != last {
                        if !grouped {
                            self.begin_step_gesture("Step velocity");
                        }
                        self.beat.step_drag = Some(StepDrag::Velocity {
                            pad,
                            step,
                            start_y,
                            start_vel,
                            last: next,
                            grouped: true,
                        });
                        self.set_step_velocity(pad, step, sb, next);
                    }
                }
                Some(StepDrag::Paint { .. }) => {
                    if let Some(c) = cell {
                        self.paint_cell(c.pad, c.step, &m, steps, sb);
                    }
                }
                None => {}
            },
            Phase::Up => self.end_step_drag(),
        }
    }

    /// `endDrag`: closes the gesture's undo step if one was opened.
    pub fn end_step_drag(&mut self) {
        let grouped = match self.beat.step_drag.take() {
            Some(StepDrag::Paint { grouped, .. }) | Some(StepDrag::Velocity { grouped, .. }) => {
                grouped
            }
            None => false,
        };
        if grouped {
            self.end_step_gesture();
        }
    }

    /// `paint`: each cell once per drag, toggled only when it needs it; the undo step opens on
    /// the first toggle.
    fn paint_cell(&mut self, pad: usize, step: usize, m: &[f32], steps: usize, sb: f64) {
        let key = pad * steps + step;
        let Some(StepDrag::Paint {
            mode,
            visited,
            grouped,
        }) = &mut self.beat.step_drag
        else {
            return;
        };
        if visited.contains(&key) {
            return;
        }
        visited.push(key);
        if !needs_toggle(*mode, m.get(key).copied().unwrap_or(0.0) as f64) {
            return;
        }
        let label = if *mode == mdaw_model::beat_helpers::PaintMode::Draw {
            "Paint steps"
        } else {
            "Erase steps"
        };
        let first = !*grouped;
        *grouped = true;
        if first {
            // begin_step_gesture ends any previous gesture: keep this drag's state.
            let drag = self.beat.step_drag.take();
            self.begin_step_gesture(label);
            self.beat.step_drag = drag;
        }
        self.toggle_step(pad, step, sb);
    }

    /// The wheel over a filled cell, vertical only: ±0.05 as one burst-grouped undo step.
    pub fn step_wheel(&mut self, e: rune::WheelEvent) -> bool {
        let (m, steps, sb) = matrix(self);
        let (cell_w, row_h) = self.steps_geometry.get();
        let Some(cell) = cell_at(e.x as f64, e.y as f64, cell_w as f64, row_h as f64, steps) else {
            return false;
        };
        let v = m[cell.pad * steps + cell.step] as f64;
        if v <= 0.0 || e.dy.abs() < e.dx.abs() {
            return false;
        }
        let next = adjust_velocity(v, if e.dy > 0.0 { 6.0 } else { -6.0 }, 120.0);
        if next != v {
            self.nudge_step_velocity(cell.pad, cell.step, sb, next);
        }
        true
    }

    /// The grid's keys (`stepKeyAction`).
    pub fn step_key(&mut self, ev: &KeyEvent, tx: &mut Tx) -> bool {
        // A grid the pointer clicked leaves Space to the transport (`keys::pointer_focus_key`);
        // once the keyboard moves in it (its ring shows), Space toggles the focused step.
        if matches!(&ev.key, Key::Text(t) if t == " ")
            && !self.beat.step_kb_focus
            && !tx.focus_visible()
        {
            return false;
        }
        let (m, steps, sb) = matrix(self);
        let key = match &ev.key {
            Key::Right => "ArrowRight".to_owned(),
            Key::Left => "ArrowLeft".to_owned(),
            Key::Up => "ArrowUp".to_owned(),
            Key::Down => "ArrowDown".to_owned(),
            Key::Home => "Home".to_owned(),
            Key::End => "End".to_owned(),
            Key::Enter => "Enter".to_owned(),
            Key::Text(t) => t.clone(),
            Key::Command(c) => c.to_string(),
            _ => return false,
        };
        let e = KeyEventLike {
            key,
            code: String::new(),
            meta: ev.mods.logo,
            ctrl: ev.mods.ctrl || ev.mods.cmd,
            alt: ev.mods.alt,
            shift: ev.mods.shift,
        };
        let cell = StepCell {
            pad: self.beat.step_focus.pad,
            step: self.beat.step_focus.step.min(steps.saturating_sub(1)),
        };
        let Some(action) = step_key_action(&e, cell, steps, steps_per_beat(sb)) else {
            return false;
        };
        self.beat.step_kb_focus = true;
        match action {
            StepKeyAction::Move(c) => {
                self.beat.step_focus = c;
                if c.pad != self.selected_pad() {
                    self.select_pad(c.pad);
                }
            }
            StepKeyAction::Toggle => {
                if !tx.key_repeat() {
                    self.toggle_step(cell.pad, cell.step, sb);
                }
            }
            StepKeyAction::Velocity { delta } => {
                let v = m[cell.pad * steps + cell.step] as f64;
                if v > 0.0 {
                    let next = nudge_velocity(v, delta);
                    if next != v {
                        self.nudge_step_velocity(cell.pad, cell.step, sb, next);
                    }
                }
            }
        }
        // Announce the focused cell with its result.
        let (m2, steps2, _) = matrix(self);
        let c = StepCell {
            pad: self.beat.step_focus.pad,
            step: self.beat.step_focus.step.min(steps2.saturating_sub(1)),
        };
        let kit = self.kit();
        let name = match pad_name(kit.pads.get(c.pad)) {
            n if n.is_empty() => format!("Pad {}", pad_label(c.pad)),
            n => n,
        };
        self.beat.step_announce = describe_step(&name, c.step, m2[c.pad * steps2 + c.step] as f64);
        tx.scroll_into_view("step-focus");
        true
    }
}
