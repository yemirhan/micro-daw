//! The pad editor's sound-shaping row (Rust-only): section tabs (Amp / Filter / Pitch / Drive),
//! the section's knobs and a curve display. The Amp display is an editable AHDSR envelope: drag
//! its handles (attack, hold, decay + sustain, release).
//!
//! The pad's `decay` is the envelope's release (Amp tab, "Release") and `filterCutoff` the
//! filter's cutoff (Filter tab, "Cutoff"); both keep their knob keys (`knob-decay`,
//! `knob-filter`). Every edit goes through `edit_pad` (one "Edit pad" undo step per gesture).

use mdaw_dsp::pad_shape::{Drive, decay_curve};
use mdaw_model::beat_helpers::{
    fmt_pitch, format_hz, format_seconds, from_log01, pad_color, to_log01,
};
use mdaw_model::types::{PadFilterType, PadSettings, PadShape};
use rune::{El, LineCap, Phase, Shape, Tx, canvas, col, el, row};
use serde_json::json;

use crate::app::App;
use crate::theme::{css_color, t};
use crate::ui::knob::{
    CUTOFF_MAX, CUTOFF_MIN, DECAY_MAX, DECAY_MIN, KnobId, KnobSpec, knob, knob_keyed,
};
use crate::ui::widgets::{Segmented, lcd};

/// The open section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShapeTab {
    #[default]
    Amp,
    Filter,
    Pitch,
    Drive,
}

/// A shaping knob (`KnobId::Shape(n as u16)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ctl {
    Attack = 0,
    Hold,
    AmpDecay,
    Sustain,
    Resonance,
    FilterEnv,
    FilterEnvDecay,
    PitchEnv,
    PitchEnvDecay,
    Drive,
}

const CTLS: [Ctl; 10] = [
    Ctl::Attack,
    Ctl::Hold,
    Ctl::AmpDecay,
    Ctl::Sustain,
    Ctl::Resonance,
    Ctl::FilterEnv,
    Ctl::FilterEnvDecay,
    Ctl::PitchEnv,
    Ctl::PitchEnvDecay,
    Ctl::Drive,
];

/// Longest attack / hold (seconds); their knobs are cubic (fine control near 0).
pub const TIME_MAX: f64 = 2.0;
const PITCH_DECAY_MAX: f64 = 2.0;

/// Attack / hold seconds ↔ knob position (`sec = 2·t³`).
pub fn time_to01(sec: f64) -> f64 {
    (sec / TIME_MAX).clamp(0.0, 1.0).cbrt()
}
pub fn time_from01(t: f64) -> f64 {
    let s = TIME_MAX * t.clamp(0.0, 1.0).powi(3);
    (s * 10_000.0).round() / 10_000.0
}

impl Ctl {
    pub fn from_index(n: u16) -> Option<Ctl> {
        CTLS.get(n as usize).copied()
    }

    fn key(self) -> &'static str {
        match self {
            Ctl::Attack => "knob-attack",
            Ctl::Hold => "knob-hold",
            Ctl::AmpDecay => "knob-amp-decay",
            Ctl::Sustain => "knob-sustain",
            Ctl::Resonance => "knob-resonance",
            Ctl::FilterEnv => "knob-filter-env",
            Ctl::FilterEnvDecay => "knob-filter-env-decay",
            Ctl::PitchEnv => "knob-pitch-env",
            Ctl::PitchEnvDecay => "knob-pitch-env-decay",
            Ctl::Drive => "knob-drive",
        }
    }

    /// (label, min, max, step, default) in knob units.
    fn spec(self) -> (&'static str, f64, f64, f64, f64) {
        let d = PadShape::default();
        match self {
            Ctl::Attack => ("Attack", 0.0, 1.0, 0.001, 0.0),
            Ctl::Hold => ("Hold", 0.0, 1.0, 0.001, 0.0),
            Ctl::AmpDecay => (
                "Decay",
                0.0,
                1.0,
                0.001,
                to_log01(d.amp_decay, DECAY_MIN, DECAY_MAX),
            ),
            Ctl::Sustain => ("Sustain", 0.0, 1.0, 0.01, 1.0),
            Ctl::Resonance => ("Reso", 0.0, 1.0, 0.01, 0.0),
            Ctl::FilterEnv => ("Env", -6.0, 6.0, 0.1, 0.0),
            Ctl::FilterEnvDecay => (
                "Env Dec",
                0.0,
                1.0,
                0.001,
                to_log01(d.filter_env_decay, DECAY_MIN, DECAY_MAX),
            ),
            Ctl::PitchEnv => ("Amount", -24.0, 48.0, 1.0, 0.0),
            Ctl::PitchEnvDecay => (
                "Time",
                0.0,
                1.0,
                0.001,
                to_log01(d.pitch_env_decay, DECAY_MIN, PITCH_DECAY_MAX),
            ),
            Ctl::Drive => ("Drive", 0.0, 1.0, 0.01, 0.0),
        }
    }

    /// The knob value of a pad's setting.
    pub fn value(self, s: &PadShape) -> f64 {
        match self {
            Ctl::Attack => time_to01(s.amp_attack),
            Ctl::Hold => time_to01(s.amp_hold),
            Ctl::AmpDecay => to_log01(s.amp_decay, DECAY_MIN, DECAY_MAX),
            Ctl::Sustain => s.amp_sustain,
            Ctl::Resonance => s.filter_resonance,
            Ctl::FilterEnv => s.filter_env,
            Ctl::FilterEnvDecay => to_log01(s.filter_env_decay, DECAY_MIN, DECAY_MAX),
            Ctl::PitchEnv => s.pitch_env,
            Ctl::PitchEnvDecay => to_log01(s.pitch_env_decay, DECAY_MIN, PITCH_DECAY_MAX),
            Ctl::Drive => s.drive,
        }
    }

    /// The pad patch for a knob value.
    pub fn patch(self, v: f64) -> serde_json::Value {
        let r3 = |x: f64| (x * 1000.0).round() / 1000.0;
        match self {
            Ctl::Attack => json!({ "ampAttack": time_from01(v) }),
            Ctl::Hold => json!({ "ampHold": time_from01(v) }),
            Ctl::AmpDecay => json!({ "ampDecay": r3(from_log01(v, DECAY_MIN, DECAY_MAX)) }),
            Ctl::Sustain => json!({ "ampSustain": v }),
            Ctl::Resonance => json!({ "filterResonance": v }),
            Ctl::FilterEnv => json!({ "filterEnv": v }),
            Ctl::FilterEnvDecay => {
                json!({ "filterEnvDecay": r3(from_log01(v, DECAY_MIN, DECAY_MAX)) })
            }
            Ctl::PitchEnv => json!({ "pitchEnv": v }),
            Ctl::PitchEnvDecay => {
                json!({ "pitchEnvDecay": r3(from_log01(v, DECAY_MIN, PITCH_DECAY_MAX)) })
            }
            Ctl::Drive => json!({ "drive": v }),
        }
    }

    /// The value text under the knob.
    pub fn display(self, v: f64) -> String {
        let pct = |x: f64| format!("{}%", (x * 100.0).round());
        match self {
            Ctl::Attack | Ctl::Hold => format_seconds(time_from01(v)),
            Ctl::AmpDecay | Ctl::FilterEnvDecay => {
                format_seconds(from_log01(v, DECAY_MIN, DECAY_MAX))
            }
            Ctl::PitchEnvDecay => format_seconds(from_log01(v, DECAY_MIN, PITCH_DECAY_MAX)),
            Ctl::Sustain | Ctl::Resonance | Ctl::Drive => pct(v),
            Ctl::FilterEnv => {
                if v == 0.0 {
                    "Off".into()
                } else {
                    format!("{v:+.1}oct")
                }
            }
            Ctl::PitchEnv => {
                if v == 0.0 {
                    "Off".into()
                } else {
                    fmt_pitch(v)
                }
            }
        }
    }
}

/// `current_value` of a shaping knob (`KnobId::Shape(n)`).
pub fn knob_value(s: &App, n: u16) -> f64 {
    let (Some(c), Some(p)) = (Ctl::from_index(n), s.pad_settings(s.selected_pad())) else {
        return 0.0;
    };
    c.value(&p.shape)
}

fn ctl_knob(s: &App, c: Ctl, settings: &PadSettings, color: &str) -> El {
    let (label, min, max, step, default) = c.spec();
    let id = KnobId::Shape(c as u16);
    let v = c.value(&settings.shape);
    let shown = match s.beat.knob_drag {
        Some(d) if d.id == id => d.pending,
        _ => v,
    };
    let end: Option<fn(&mut App)> = Some(|s: &mut App| s.end_pad_edit());
    knob_keyed(
        KnobSpec {
            id,
            label,
            min,
            max,
            step,
            default,
            small: true,
            hide_text: false,
        },
        c.key().to_owned(),
        v,
        c.display(shown),
        color,
        move |s, v| {
            let p = s.selected_pad();
            s.edit_pad(p, c.patch(v))
        },
        end,
        s,
    )
}

fn pending(s: &App, id: KnobId, v: f64) -> f64 {
    match s.beat.knob_drag {
        Some(d) if d.id == id => d.pending,
        _ => v,
    }
}

/// The release knob (the pad's `decay`; key `knob-decay`).
fn release_knob(s: &App, settings: &PadSettings, color: &str) -> El {
    let t = to_log01(settings.decay, DECAY_MIN, DECAY_MAX);
    knob(
        KnobSpec {
            id: KnobId::Decay,
            label: "Release",
            min: 0.0,
            max: 1.0,
            step: 0.001,
            default: to_log01(0.05, DECAY_MIN, DECAY_MAX),
            small: true,
            hide_text: false,
        },
        t,
        format_seconds(from_log01(
            pending(s, KnobId::Decay, t),
            DECAY_MIN,
            DECAY_MAX,
        )),
        color,
        |s, t| {
            let p = s.selected_pad();
            s.edit_pad(p, json!({ "decay": from_log01(t, DECAY_MIN, DECAY_MAX) }))
        },
        Some(|s: &mut App| s.end_pad_edit()),
        s,
    )
}

/// The filter's cutoff knob (the pad's `filterCutoff`; key `knob-filter`).
fn cutoff_knob(s: &App, settings: &PadSettings, color: &str) -> El {
    let t = to_log01(settings.filter_cutoff, CUTOFF_MIN, CUTOFF_MAX);
    let lp = settings.shape.filter_type == PadFilterType::Lp;
    knob(
        KnobSpec {
            id: KnobId::Filter,
            label: "Cutoff",
            min: 0.0,
            max: 1.0,
            step: 0.001,
            default: 1.0,
            small: true,
            hide_text: false,
        },
        t,
        {
            let shown = pending(s, KnobId::Filter, t);
            if lp {
                super::editor::filter_text(shown)
            } else {
                format_hz(from_log01(shown, CUTOFF_MIN, CUTOFF_MAX))
            }
        },
        color,
        |s, t| {
            let p = s.selected_pad();
            let hz = if t >= 0.999 {
                CUTOFF_MAX
            } else {
                from_log01(t, CUTOFF_MIN, CUTOFF_MAX).round()
            };
            s.edit_pad(p, json!({ "filterCutoff": hz }))
        },
        Some(|s: &mut App| s.end_pad_edit()),
        s,
    )
}

// --- envelope display ----------------------------------------------------------------------

pub const ENV_W: f32 = 196.0;
pub const ENV_H: f32 = 62.0;
const INSET: f32 = 7.0;
/// Width shares of the attack, hold, decay, sustain (fixed) and release stages at full length.
const SEG: [f32; 5] = [0.17, 0.13, 0.25, 0.15, 0.30];

/// A draggable point of the amp envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvHandle {
    Attack,
    Hold,
    /// x = decay, y = sustain.
    Decay,
    Release,
}

const HANDLES: [EnvHandle; 4] = [
    EnvHandle::Attack,
    EnvHandle::Hold,
    EnvHandle::Decay,
    EnvHandle::Release,
];

/// The envelope drawing for a pad: polyline, handle points, stage origins (x) and the y range.
#[derive(Debug, Clone, PartialEq)]
pub struct EnvGeom {
    pub line: Vec<[f32; 2]>,
    pub handles: [[f32; 2]; 4],
    /// Stage start x of attack, hold, decay, release.
    pub origin: [f32; 4],
    pub top: f32,
    pub bottom: f32,
}

fn seg_w(i: usize) -> f32 {
    SEG[i] * (ENV_W - 2.0 * INSET)
}

/// Lay the AHDSR out on the display. Each stage's width is its knob position × its share, so a
/// handle moves exactly like its knob.
pub fn env_geom(shape: &PadShape, release: f64) -> EnvGeom {
    let (top, bottom) = (INSET, ENV_H - INSET);
    let y = |level: f64| bottom - level as f32 * (bottom - top);
    let x0 = INSET;
    let xa = x0 + seg_w(0) * time_to01(shape.amp_attack) as f32;
    let xh = xa + seg_w(1) * time_to01(shape.amp_hold) as f32;
    let sus = shape.amp_sustain.clamp(0.0, 1.0);
    let xd = xh + seg_w(2) * to_log01(shape.amp_decay, DECAY_MIN, DECAY_MAX) as f32;
    let xs = xd + seg_w(3);
    let xr = xs + seg_w(4) * to_log01(release, DECAY_MIN, DECAY_MAX) as f32;
    let mut line = vec![[x0, bottom], [xa, top], [xh, top]];
    for i in 1..=16 {
        let f = i as f64 / 16.0;
        let level = sus + (1.0 - sus) * decay_curve(f);
        line.push([xh + (xd - xh) * f as f32, y(level)]);
    }
    line.push([xs, y(sus)]);
    line.push([xr, bottom]);
    EnvGeom {
        line,
        handles: [[xa, top], [xh, top], [xd, y(sus)], [xr, bottom]],
        origin: [x0, xa, xh, xs],
        top,
        bottom,
    }
}

/// The handle nearest to a point of the display.
pub fn nearest_handle(g: &EnvGeom, x: f32, y: f32) -> EnvHandle {
    let mut best = (f32::MAX, EnvHandle::Attack);
    for (i, h) in HANDLES.iter().enumerate() {
        let [hx, hy] = g.handles[i];
        // Later handles win ties (stacked handles at zero length: the outermost moves first).
        let d = (hx - x).powi(2) + (hy - y).powi(2);
        if d <= best.0 {
            best = (d, *h);
        }
    }
    best.1
}

/// The pad patch for dragging `h` to `(x, y)` on the display.
pub fn env_drag_patch(g: &EnvGeom, h: EnvHandle, x: f32, y: f32) -> serde_json::Value {
    let t = |i: usize, w: usize| ((x - g.origin[i]) / seg_w(w)).clamp(0.0, 1.0) as f64;
    let r3 = |v: f64| (v * 1000.0).round() / 1000.0;
    match h {
        EnvHandle::Attack => json!({ "ampAttack": time_from01(t(0, 0)) }),
        EnvHandle::Hold => json!({ "ampHold": time_from01(t(1, 1)) }),
        EnvHandle::Decay => {
            let sus = ((g.bottom - y) / (g.bottom - g.top)).clamp(0.0, 1.0) as f64;
            json!({
                "ampDecay": r3(from_log01(t(2, 2), DECAY_MIN, DECAY_MAX)),
                "ampSustain": (sus * 100.0).round() / 100.0,
            })
        }
        EnvHandle::Release => json!({ "decay": from_log01(t(3, 4), DECAY_MIN, DECAY_MAX) }),
    }
}

fn amp_display(settings: &PadSettings, color: rune::Color) -> El {
    let th = t();
    let g = env_geom(&settings.shape, settings.decay);
    let mut fill = g.line.clone();
    fill.push([g.line.last().map_or(0.0, |p| p[0]), g.bottom]);
    let mut shape = Shape::new().fill(fill, color.alpha(0.16)).stroke_with(
        g.line.clone(),
        false,
        1.5,
        LineCap::Round,
        color,
    );
    for [hx, hy] in g.handles {
        shape = shape.fill(circle(hx, hy, 3.0), th.fg);
    }
    lcd()
        .key("amp-env")
        .role("slider")
        .label("Amp envelope")
        .a11y_value(format!(
            "attack {}, hold {}, decay {}, sustain {}%, release {}",
            format_seconds(settings.shape.amp_attack),
            format_seconds(settings.shape.amp_hold),
            format_seconds(settings.shape.amp_decay),
            (settings.shape.amp_sustain * 100.0).round(),
            format_seconds(settings.decay)
        ))
        .help("Drag the points: attack, hold, decay / sustain, release")
        .size(ENV_W, ENV_H)
        .clip_rounded(crate::theme::R_MD)
        .cursor(rune::Cursor::Pointer)
        .child(
            canvas(shape.view_box(ENV_W, ENV_H))
                .absolute(0.0, 0.0)
                .size(ENV_W, ENV_H),
        )
        .on_drag(|s: &mut App, d: &mut rune::Drag, _tx: &mut Tx| {
            s.env_drag(d.x, d.y, d.phase);
        })
}

fn circle(cx: f32, cy: f32, r: f32) -> Vec<[f32; 2]> {
    (0..12)
        .map(|i| {
            let a = i as f32 / 12.0 * std::f32::consts::TAU;
            [cx + r * a.cos(), cy + r * a.sin()]
        })
        .collect()
}

/// A display-only curve (filter / pitch envelope, drive transfer).
fn curve_display(key: &'static str, label: String, line: Vec<[f32; 2]>, color: rune::Color) -> El {
    let th = t();
    let mid = ENV_H / 2.0;
    let shape = Shape::new()
        .fill(
            vec![
                [0.0, mid],
                [ENV_W, mid],
                [ENV_W, mid + 1.0],
                [0.0, mid + 1.0],
            ],
            th.white.alpha(0.06),
        )
        .stroke_with(line, false, 1.5, LineCap::Round, color);
    lcd()
        .key(key)
        .role("img")
        .label(label)
        .size(ENV_W, ENV_H)
        .clip_rounded(crate::theme::R_MD)
        .child(
            canvas(shape.view_box(ENV_W, ENV_H))
                .absolute(0.0, 0.0)
                .size(ENV_W, ENV_H),
        )
}

/// A decaying modulation over time: `amount` (−1..1 of the display's half height) at the hit,
/// back to 0 after `decay_t` (0..1 of the width).
fn mod_line(amount: f64, decay_t: f64) -> Vec<[f32; 2]> {
    let (l, r) = (INSET, ENV_W - INSET);
    let mid = ENV_H / 2.0;
    let half = mid - INSET;
    let span = ((r - l) * (0.1 + 0.9 * decay_t as f32)).max(1.0);
    (0..=40)
        .map(|i| {
            let x = l + (r - l) * i as f32 / 40.0;
            let v = amount * decay_curve(((x - l) / span) as f64);
            [x, mid - v as f32 * half]
        })
        .collect()
}

fn drive_line(amount: f64) -> Vec<[f32; 2]> {
    let d = Drive::new(amount);
    let (l, r) = (INSET, ENV_W - INSET);
    let mid = ENV_H / 2.0;
    let half = mid - INSET;
    (0..=48)
        .map(|i| {
            let u = i as f32 / 48.0 * 2.0 - 1.0;
            [l + (r - l) * (u + 1.0) / 2.0, mid - d.tick(u) * half]
        })
        .collect()
}

/// The shaping row under the main knobs.
pub fn shaping_row(s: &App, pad: usize, settings: &PadSettings) -> El {
    let th = t();
    let color = pad_color(Some(settings), pad);
    let accent = css_color(&color, th.primary);
    let tab = s.beat.shape_tab;
    let sh = &settings.shape;
    let tabs = Segmented::new(
        "Sound shaping section",
        tab,
        vec![
            (ShapeTab::Amp, "Amp".into()),
            (ShapeTab::Filter, "Filter".into()),
            (ShapeTab::Pitch, "Pitch".into()),
            (ShapeTab::Drive, "Drive".into()),
        ],
    )
    .sans()
    .weight(600.0, 600.0)
    .item_px(8.0)
    .key("shape-tab")
    .build(|s: &mut App, t| s.beat.shape_tab = t);
    // A dot on each section that changes the sound (so a hidden tab's shaping is visible).
    let on = [
        !sh.amp_is_neutral(),
        !sh.filter_is_neutral() || settings.filter_cutoff < CUTOFF_MAX,
        sh.pitch_env != 0.0,
        sh.drive > 0.0,
    ];
    let mut dots = row().key("shape-active").gap(4.0).pl(2.0);
    for (i, a) in on.into_iter().enumerate() {
        dots = dots.child(
            el().key(format!("shape-active-{i}"))
                .size(5.0, 5.0)
                .radius(2.5)
                .fill(if a { accent } else { th.surface4 }),
        );
    }
    let k = |c| ctl_knob(s, c, settings, &color);
    let mut knobs = row().items_end().gap(10.0);
    let display;
    match tab {
        ShapeTab::Amp => {
            for c in [Ctl::Attack, Ctl::Hold, Ctl::AmpDecay, Ctl::Sustain] {
                knobs = knobs.child(k(c));
            }
            knobs = knobs.child(release_knob(s, settings, &color));
            display = amp_display(settings, accent);
        }
        ShapeTab::Filter => {
            let ty = Segmented::new(
                "Filter type",
                sh.filter_type,
                vec![
                    (PadFilterType::Lp, "LP".into()),
                    (PadFilterType::Hp, "HP".into()),
                    (PadFilterType::Bp, "BP".into()),
                ],
            )
            .item_px(6.0)
            .key("filter-type")
            .build(|s: &mut App, v| {
                let p = s.selected_pad();
                let v = match v {
                    PadFilterType::Lp => "lp",
                    PadFilterType::Hp => "hp",
                    PadFilterType::Bp => "bp",
                };
                s.edit_pad(p, json!({ "filterType": v }));
                s.end_pad_edit();
            });
            knobs = knobs
                .child(col().pb(22.0).child(ty))
                .child(cutoff_knob(s, settings, &color))
                .child(k(Ctl::Resonance))
                .child(k(Ctl::FilterEnv))
                .child(k(Ctl::FilterEnvDecay));
            display = curve_display(
                "filter-env-curve",
                "Filter envelope".into(),
                mod_line(
                    sh.filter_env / 6.0,
                    to_log01(sh.filter_env_decay, DECAY_MIN, DECAY_MAX),
                ),
                accent,
            );
        }
        ShapeTab::Pitch => {
            knobs = knobs.child(k(Ctl::PitchEnv)).child(k(Ctl::PitchEnvDecay));
            display = curve_display(
                "pitch-env-curve",
                "Pitch envelope".into(),
                mod_line(
                    (sh.pitch_env / 48.0).clamp(-1.0, 1.0),
                    to_log01(sh.pitch_env_decay, DECAY_MIN, PITCH_DECAY_MAX),
                ),
                accent,
            );
        }
        ShapeTab::Drive => {
            knobs = knobs.child(k(Ctl::Drive));
            display = curve_display(
                "drive-curve",
                "Drive curve".into(),
                drive_line(sh.drive),
                accent,
            );
        }
    }
    row()
        .key("shape-row")
        .role("group")
        .label("Sound shaping")
        .w_full()
        .flex_wrap()
        .items_center()
        .gaps(8.0, 14.0)
        .child(col().gap(6.0).child(tabs).child(dots))
        .child(knobs)
        .child(display)
}

impl App {
    /// A drag on the amp envelope display: the press picks the nearest handle, moves set it.
    pub fn env_drag(&mut self, x: f32, y: f32, phase: Phase) {
        let pad = self.selected_pad();
        let Some(settings) = self.pad_settings(pad) else {
            return;
        };
        let g = env_geom(&settings.shape, settings.decay);
        match phase {
            Phase::Down => self.beat.env_drag = Some(nearest_handle(&g, x, y)),
            Phase::Move => {
                if let Some(h) = self.beat.env_drag {
                    self.edit_pad(pad, env_drag_patch(&g, h, x, y));
                }
            }
            Phase::Up => {
                if self.beat.env_drag.take().is_some() {
                    self.end_pad_edit();
                }
            }
        }
    }
}
