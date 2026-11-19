//! `components/ui/Knob.tsx`: a rotary control rune-kit doesn't have. Same geometry (270° arc
//! from −135°, bipolar ranges drawn from zero), same input:
//!
//! - vertical drag: 200 px = full range (800 px with Shift); `onChange` per move, `onChangeEnd`
//!   on release (one undo step with the caller's grouping);
//! - wheel: ±`max(step, range × 0.015)`, `onChange` only (TS quirk kept: no `onChangeEnd`);
//! - double-click: default value, `onChange` + `onChangeEnd`;
//! - keys: arrows ±`max(step, range × 0.01)` (× 0.1 with Shift), Home / End, Delete /
//!   Backspace = default; each `onChange` + `onChangeEnd`;
//! - values snap to the step and clamp.

use rune::{El, Key, KeyEvent, LineCap, Phase, Shape, Tx, canvas, col, el, text};

use crate::app::{App, KnobDrag};
use crate::theme::{css_color, daw_label, mono, t};

/// Which knob (the drag state remembers it across the gesture).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KnobId {
    Gain,
    Pan,
    Pitch,
    Decay,
    Filter,
    Swing,
    /// A Song-view knob (phase 8): track FX parameter `n` of the open drawer, or `1000 + i`
    /// the mixer pan of track `i` (`crate::ui::song::knob_value`).
    Song(u16),
    /// A phase-9 knob (Live FX strip, synth editor: `crate::ui::p9::knob_value`).
    View(u16),
}

impl KnobId {
    pub fn key(self) -> &'static str {
        match self {
            KnobId::Gain => "knob-gain",
            KnobId::Pan => "knob-pan",
            KnobId::Pitch => "knob-pitch",
            KnobId::Decay => "knob-decay",
            KnobId::Filter => "knob-filter",
            KnobId::Swing => "knob-swing",
            KnobId::Song(_) => "knob-song",
            KnobId::View(_) => "knob-view",
        }
    }
}

/// The knob's range and formatting.
#[derive(Debug, Clone, Copy)]
pub struct KnobSpec {
    pub id: KnobId,
    pub label: &'static str,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub default: f64,
    pub small: bool,
    /// Hide the value text (`[&>span]:hidden` on the swing knob).
    pub hide_text: bool,
}

pub const DRAG_PX: f64 = 200.0;

impl KnobSpec {
    /// `clamp`: snap to the step, then clamp.
    pub fn clamp(&self, v: f64) -> f64 {
        let stepped = (v / self.step).round() * self.step;
        stepped.clamp(self.min, self.max)
    }

    /// Drag: value for a vertical travel `dy` (positive = up) from `start`.
    pub fn drag_value(&self, start: f64, dy: f64, shift: bool) -> f64 {
        let px = if shift { DRAG_PX * 4.0 } else { DRAG_PX };
        self.clamp(start + dy / px * (self.max - self.min))
    }

    /// Wheel: one tick up (`deltaY < 0`) or down.
    pub fn wheel_value(&self, value: f64, up: bool) -> f64 {
        let tick = self.step.max((self.max - self.min) * 0.015);
        self.clamp(value + if up { tick } else { -tick })
    }

    /// One keyboard tick (`max(step, range × 0.01)`, × 10 with Shift).
    pub fn tick(&self, shift: bool) -> f64 {
        self.step
            .max((self.max - self.min) * if shift { 0.1 } else { 0.01 })
    }

    /// Keyboard (`handleKeyDown`): the new value, or None when the key isn't the knob's.
    pub fn key_value(&self, value: f64, ev: &KeyEvent) -> Option<f64> {
        let tick = self
            .step
            .max((self.max - self.min) * if ev.mods.shift { 0.1 } else { 0.01 });
        match &ev.key {
            Key::Up | Key::Right => Some(self.clamp(value + tick)),
            Key::Down | Key::Left => Some(self.clamp(value - tick)),
            Key::Home => Some(self.min),
            Key::End => Some(self.max),
            Key::Delete | Key::Backspace => Some(self.default),
            _ => None,
        }
    }
}

/// A point on the knob circle, `deg` clockwise from 12 o'clock.
fn polar(c: f32, r: f32, deg: f32) -> [f32; 2] {
    let rad = (deg - 90.0).to_radians();
    [c + r * rad.cos(), c + r * rad.sin()]
}

fn arc(c: f32, r: f32, from: f32, to: f32) -> Vec<[f32; 2]> {
    let (a, b) = if from <= to { (from, to) } else { (to, from) };
    let n = ((b - a) / 4.0).ceil().max(2.0) as usize;
    (0..=n)
        .map(|i| polar(c, r, a + (b - a) * i as f32 / n as f32))
        .collect()
}

/// Draws the knob and wires its handlers. `on_change(app, v)` / `on_end(app)` are the
/// `onChange` / `onChangeEnd` props.
pub fn knob(
    spec: KnobSpec,
    value: f64,
    display: String,
    color: &str,
    on_change: impl Fn(&mut App, f64) + Clone + 'static,
    on_end: Option<fn(&mut App)>,
    app: &App,
) -> El {
    knob_keyed(
        spec,
        spec.id.key().to_owned(),
        value,
        display,
        color,
        on_change,
        on_end,
        app,
    )
}

/// [`knob`] with an explicit element key (several Song-view knobs share a [`KnobId`] kind).
#[allow(clippy::too_many_arguments)]
pub fn knob_keyed(
    spec: KnobSpec,
    key: String,
    value: f64,
    display: String,
    color: &str,
    on_change: impl Fn(&mut App, f64) + Clone + 'static,
    on_end: Option<fn(&mut App)>,
    app: &App,
) -> El {
    let th = t();
    let px: f32 = if spec.small { 34.0 } else { 42.0 };
    let sw: f32 = if spec.small { 2.5 } else { 3.0 };
    let radius = px / 2.0 - sw / 2.0 - 1.0;
    let c = px / 2.0;
    // While dragging, show the pending value (Knob's local `displayValue`).
    let shown = match app.beat.knob_drag {
        Some(d) if d.id == spec.id => d.pending,
        _ => value,
    };
    let norm = |v: f64| ((v - spec.min) / (spec.max - spec.min)) as f32;
    let n = norm(shown).clamp(0.0, 1.0);
    let angle = -135.0 + n * 270.0;
    let bipolar = spec.min < 0.0 && spec.max > 0.0;
    let origin = if bipolar {
        -135.0 + norm(0.0) * 270.0
    } else {
        -135.0
    };
    let accent = css_color(color, th.primary);
    let body_r = radius - sw - 1.5;
    let mut shape = Shape::new().stroke_with(
        arc(c, radius, -135.0, 135.0),
        false,
        sw,
        LineCap::Round,
        th.surface4,
    );
    if (angle - origin).abs() > 0.5 {
        shape = shape.stroke_with(
            arc(c, radius, origin, angle),
            false,
            sw,
            LineCap::Round,
            accent,
        );
    }
    let pointer = Shape::new().line(
        polar(c, body_r * 0.3, angle),
        polar(c, body_r - 1.5, angle),
        if spec.small { 1.75 } else { 2.0 },
        LineCap::Round,
        th.fg,
    );
    let body = el()
        .absolute(c - body_r, c - body_r)
        .size(body_r * 2.0, body_r * 2.0)
        .radius(body_r)
        .fill(th.surface3)
        .border(1.0, th.black.alpha(0.45))
        .child(
            el().absolute(1.0, 1.0)
                .size(body_r * 2.0 - 2.0, body_r * 2.0 - 2.0)
                .radius(body_r - 1.0)
                .border(1.0, th.white.alpha(0.07)),
        );
    let dial = el()
        .key(key)
        .size(px, px)
        .radius(px / 2.0)
        .role("slider")
        .label(spec.label)
        // aria-valuemin / -max / -now / -valuetext.
        .range_value(shown, spec.min, spec.max)
        .range_step(spec.tick(false))
        .a11y_value(display.clone())
        .focusable(true)
        .child(canvas(shape).absolute(0.0, 0.0).size(px, px))
        .child(body)
        .child(canvas(pointer).absolute(0.0, 0.0).size(px, px))
        .cursor(rune::Cursor::ResizeRow)
        .on_drag({
            let on_change = on_change.clone();
            move |s: &mut App, d: &mut rune::Drag, tx: &mut Tx| match d.phase {
                Phase::Down => {
                    let v = current_value(s, spec.id);
                    s.beat.knob_drag = Some(KnobDrag {
                        id: spec.id,
                        start_value: v,
                        pending: v,
                    });
                }
                Phase::Move => {
                    let Some(mut k) = s.beat.knob_drag else {
                        return;
                    };
                    let next = spec.drag_value(k.start_value, -d.dy as f64, tx.mods().shift);
                    if next != k.pending {
                        k.pending = next;
                        s.beat.knob_drag = Some(k);
                        on_change(s, next);
                    }
                }
                Phase::Up => {
                    if s.beat.knob_drag.take().is_some()
                        && let Some(end) = on_end
                    {
                        end(s);
                    }
                }
            }
        })
        .on_wheel({
            let on_change = on_change.clone();
            move |s: &mut App, e: rune::WheelEvent, _tx: &mut Tx| {
                if e.dy == 0.0 {
                    return false;
                }
                let v = current_value(s, spec.id);
                // A wheel delta's sign follows the content: dy > 0 scrolls the content down (the
                // DOM's deltaY < 0), so it turns the knob up.
                let next = spec.wheel_value(v, e.dy > 0.0);
                if next != v {
                    on_change(s, next);
                }
                true
            }
        })
        .on_double_press({
            let on_change = on_change.clone();
            move |s: &mut App| {
                on_change(s, spec.default);
                if let Some(end) = on_end {
                    end(s);
                }
            }
        })
        .on_key({
            let on_change = on_change.clone();
            move |s: &mut App, ev: &KeyEvent, _tx: &mut Tx| {
                let v = current_value(s, spec.id);
                let Some(next) = spec.key_value(v, ev) else {
                    return false;
                };
                commit(s, v, next, &on_change, on_end);
                true
            }
        })
        // Assistive technology's increment / decrement (the keys go through `on_key` above,
        // which runs first); Page Up / Down are ten ticks.
        .on_step({
            let on_change = on_change.clone();
            move |s: &mut App, delta: i32, tx: &mut Tx| {
                let v = current_value(s, spec.id);
                let next = spec.clamp(v + delta as f64 * spec.tick(tx.mods().shift));
                commit(s, v, next, &on_change, on_end);
            }
        })
        .on_step_limit(move |s: &mut App, max: bool, _tx: &mut Tx| {
            let v = current_value(s, spec.id);
            commit(
                s,
                v,
                if max { spec.max } else { spec.min },
                &on_change,
                on_end,
            );
        });
    let mut out = col().items_center().gap(4.0).child(dial);
    if !spec.hide_text {
        out = out
            .child(
                text(display)
                    .font(mono(10.0, 400.0).lh(10.0))
                    .color(th.fg.alpha(0.85)),
            )
            .child(
                text(spec.label)
                    .font(daw_label(9.5).lh(9.5))
                    .text_case(rune::TextCase::Uppercase)
                    .color(th.muted_fg),
            );
    }
    out
}

/// A keyboard / assistive change: `onChange` + `onChangeEnd` when the value moved.
fn commit(
    s: &mut App,
    from: f64,
    to: f64,
    on_change: &impl Fn(&mut App, f64),
    on_end: Option<fn(&mut App)>,
) {
    if to != from {
        on_change(s, to);
        if let Some(end) = on_end {
            end(s);
        }
    }
}

/// The knob's current value from the app state (the selected pad's settings, or swing).
pub fn current_value(s: &App, id: KnobId) -> f64 {
    use mdaw_model::beat_helpers::to_log01;
    if id == KnobId::Swing {
        return s.svc.engine.doc().get_loop_settings().swing;
    }
    if let KnobId::Song(n) = id {
        return crate::ui::song::knob_value(s, n);
    }
    if let KnobId::View(n) = id {
        return crate::ui::p9::knob_value(s, n);
    }
    let Some(p) = s.pad_settings(s.selected_pad()) else {
        return 0.0;
    };
    match id {
        KnobId::Gain => p.gain_db,
        KnobId::Pan => p.pan,
        KnobId::Pitch => p.pitch,
        KnobId::Decay => to_log01(p.decay, DECAY_MIN, DECAY_MAX),
        KnobId::Filter => to_log01(p.filter_cutoff, CUTOFF_MIN, CUTOFF_MAX),
        KnobId::Swing | KnobId::Song(_) | KnobId::View(_) => unreachable!(),
    }
}

pub const DECAY_MIN: f64 = 0.005;
pub const DECAY_MAX: f64 = 4.0;
pub const CUTOFF_MIN: f64 = 20.0;
pub const CUTOFF_MAX: f64 = 20000.0;
