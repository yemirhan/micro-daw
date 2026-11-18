//! Small math helpers, coordinate transforms, zoom maths, snapping and text formatting.

use super::types::*;

// ---------------------------------------------------------------------------
// JS numeric semantics
// ---------------------------------------------------------------------------

/// `Math.round`: halves round toward +inf. Exact for every double (unlike `(x + 0.5).floor()`,
/// which is off for 0.49999999999999994 and odd integers above 2^52). NaN/±inf pass through.
pub fn js_round(x: f64) -> f64 {
    if !x.is_finite() {
        return x;
    }
    let f = x.floor();
    if x - f >= 0.5 { f + 1.0 } else { f }
}

/// `Math.max(a, b)`: NaN if either side is NaN.
pub fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::NAN } else { a.max(b) }
}

/// `Math.min(a, b)`: NaN if either side is NaN.
pub fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::NAN } else { a.min(b) }
}

/// `String(n)` / template-literal formatting of a JS number.
pub fn js_num(n: f64) -> String {
    if n.is_nan() {
        return "NaN".into();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    if n == 0.0 {
        return "0".into(); // also -0
    }
    let a = n.abs();
    if (1e-6..1e21).contains(&a) {
        // Rust's Display prints the shortest round-trip digits without an exponent, as JS does here.
        return format!("{n}");
    }
    // Exponent form: Rust "1.5e-7" / "1e21" -> JS "1.5e-7" / "1e+21".
    let s = format!("{n:e}");
    match s.split_once('e') {
        Some((m, e)) if !e.starts_with('-') => format!("{m}e+{e}"),
        _ => s,
    }
}

/// `n.toFixed(2)` for the values `format_beats` can reach (finite, < 1e21; JS rounds exact ties
/// up while Rust rounds them to even, but no tick-grid value is an exact 2-decimal tie that
/// reaches this path: such values are multiples of 1/8 and are caught by the fraction search).
fn js_to_fixed2(n: f64) -> String {
    if !n.is_finite() {
        return js_num(n);
    }
    format!("{n:.2}")
}

// ---------------------------------------------------------------------------
// Small math helpers
// ---------------------------------------------------------------------------

/// `v < lo ? lo : v > hi ? hi : v` (never panics, NaN passes through).
pub fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

/// Round a beat value to the internal tick grid (kills float drift like 0.30000000000000004).
/// `|| 0`: -0 and NaN become 0.
pub fn round_beat(beat: f64) -> f64 {
    let r = js_round(beat * TICKS_PER_BEAT) / TICKS_PER_BEAT;
    if r == 0.0 || r.is_nan() { 0.0 } else { r }
}

pub fn clamp_velocity(v: f64) -> f64 {
    clamp(v, MIN_VELOCITY, 1.0)
}

/// 0..1 velocity -> MIDI 1..127 for display.
pub fn velocity_to_midi(v: f64) -> f64 {
    js_round(clamp_velocity(v) * 127.0)
}

pub fn is_multiple_of(value: f64, step: f64) -> bool {
    if step <= 0.0 {
        return false;
    }
    let r = value / step;
    (r - js_round(r)).abs() < 1e-4
}

// ---------------------------------------------------------------------------
// Coordinate transforms
// ---------------------------------------------------------------------------

pub fn beat_to_x(beat: f64, px_per_beat: f64) -> f64 {
    beat * px_per_beat
}

pub fn x_to_beat(x: f64, px_per_beat: f64) -> f64 {
    x / px_per_beat
}

pub fn row_to_y(row: f64, row_height: f64) -> f64 {
    row * row_height
}

pub fn y_to_row(y: f64, row_height: f64) -> f64 {
    (y / row_height).floor()
}

/// Legacy alias of [`beat_to_x`].
pub fn beat_to_grid_x(beat: f64, px_per_beat: f64) -> f64 {
    beat_to_x(beat, px_per_beat)
}

/// Legacy alias of [`x_to_beat`].
pub fn grid_x_to_beat(x: f64, px_per_beat: f64) -> f64 {
    x_to_beat(x, px_per_beat)
}

/// Keep the content point under the cursor fixed while zooming.
/// `cursor` is the cursor offset inside the viewport (px).
pub fn zoom_scroll(scroll: f64, cursor: f64, old_scale: f64, new_scale: f64) -> f64 {
    if old_scale <= 0.0 {
        return scroll;
    }
    js_max(0.0, ((scroll + cursor) / old_scale) * new_scale - cursor)
}

/// Default `sensitivity` of [`zoom_from_wheel`].
pub const ZOOM_WHEEL_SENSITIVITY: f64 = 0.0025;

/// Multiplicative zoom step from a wheel delta, clamped to [min, max].
/// (`sensitivity` defaults to [`ZOOM_WHEEL_SENSITIVITY`] in TS.)
pub fn zoom_from_wheel(current: f64, delta_y: f64, min: f64, max: f64, sensitivity: f64) -> f64 {
    let factor = (-delta_y * sensitivity).exp();
    clamp(current * factor, min, max)
}

/// Map a 0..1 slider position to a log scale between min and max (and back).
pub fn slider_to_zoom(t: f64, min: f64, max: f64) -> f64 {
    min * (max / min).powf(clamp(t, 0.0, 1.0))
}

pub fn zoom_to_slider(v: f64, min: f64, max: f64) -> f64 {
    clamp((v / min).ln() / (max / min).ln(), 0.0, 1.0)
}

// ---------------------------------------------------------------------------
// Snapping
// ---------------------------------------------------------------------------

/// Round to the nearest grid line. `offset` = absolute beat of the region start.
pub fn snap_beat(beat: f64, snap: f64, offset: f64) -> f64 {
    if snap <= 0.0 {
        return round_beat(beat);
    }
    round_beat(js_round((beat + offset) / snap) * snap - offset)
}

pub fn snap_floor(beat: f64, snap: f64, offset: f64) -> f64 {
    if snap <= 0.0 {
        return round_beat(beat);
    }
    round_beat(((beat + offset) / snap + EPS).floor() * snap - offset)
}

pub fn snap_ceil(beat: f64, snap: f64, offset: f64) -> f64 {
    if snap <= 0.0 {
        return round_beat(beat);
    }
    round_beat(((beat + offset) / snap - EPS).ceil() * snap - offset)
}

/// Legacy name: `snapBeat(beat, snap)`.
pub fn snap_beat_pr(beat: f64, snap_value: f64) -> f64 {
    snap_beat(beat, snap_value, 0.0)
}

/// Legacy name: `snapFloor(beat, snap)`.
pub fn snap_beat_floor(beat: f64, snap_value: f64) -> f64 {
    snap_floor(beat, snap_value, 0.0)
}

/// Step used for keyboard nudges: the grid, or a 1/16 when snap is off.
pub fn nudge_step(snap: f64) -> f64 {
    if snap > 0.0 { snap } else { 0.25 }
}

/// `findSnapOption(beats, options = SNAP_OPTIONS)`.
pub fn find_snap_option(beats: f64, options: &[SnapOption]) -> Option<&SnapOption> {
    options.iter().find(|o| (o.beats - beats).abs() < 1e-6)
}

/// Human readable length: "1/16", "3/16", "1/8 T", "2 bars", "0.37 beats".
pub fn format_beats(beats: f64) -> String {
    if beats <= 0.0 {
        return "0".into();
    }
    if let Some(opt) = find_snap_option(beats, &NOTE_LENGTH_OPTIONS).or_else(|| find_snap_option(beats, &SNAP_OPTIONS))
    {
        return opt.label.into();
    }
    if is_multiple_of(beats, BEATS_PER_BAR) {
        let bars = js_round(beats / BEATS_PER_BAR);
        return format!("{} bar{}", js_num(bars), if bars == 1.0 { "" } else { "s" });
    }
    let whole = beats / 4.0;
    for den in [2.0, 4.0, 8.0, 16.0, 32.0, 64.0, 3.0, 6.0, 12.0, 24.0, 48.0] {
        let num = whole * den;
        if (num - js_round(num)).abs() < 1e-6 {
            return format!("{}/{}", js_num(js_round(num)), js_num(den));
        }
    }
    format!("{} beats", js_to_fixed2(beats))
}

/// Absolute beat -> "bar.beat.sixteenth" (1-based), e.g. 0 -> "1.1.1".
pub fn format_position(abs_beat: f64) -> String {
    let b = js_max(0.0, abs_beat);
    let bar = (b / BEATS_PER_BAR + EPS).floor() + 1.0;
    let beat_in_bar = ((b % BEATS_PER_BAR) + EPS).floor() + 1.0;
    let sixteenth = (((b % 1.0) + EPS) * 4.0).floor() + 1.0;
    format!("{}.{}.{}", js_num(bar), js_num(js_min(beat_in_bar, BEATS_PER_BAR)), js_num(js_min(sixteenth, 4.0)))
}
