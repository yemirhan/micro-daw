//! The live instrument rack: `EffectsPanel` (the Song view's Live FX strip), `SynthEditor`
//! (toggled from the top bar) and the top bar's `InstrumentSelector` popover.

use mdaw_engine::fx::live::LiveOp;
use mdaw_engine::synth::bank::synth_presets;
use mdaw_engine::synth::params::get_path;
use mdaw_engine::synth::presets::{PRESET_CATEGORIES, PRESET_VIBES, SynthPreset};
use mdaw_model::types::{EffectParams, FilterType, LfoWaveform};
use rune::{Align, Color, El, col, el, icon, row, spacer, text, text_input};
use serde_json::Value;

use crate::app::App;
use crate::icons;
use crate::icons_views;
use crate::theme::{R_LG, R_MD, R_SM, daw_label, mono, t, ui};
use crate::ui::knob::{KnobId, KnobSpec, knob_keyed};
use crate::ui::song::headers::matches_query;
use crate::ui::widgets::{popover, popover_frame, select_option, select_popup};
use crate::views::{FxSec, P9Menu};

// ------------------------------------------------------------------------------- Live FX

/// One Live FX knob: label, range, step, default, format, op, getter.
pub struct FxKnob {
    pub label: &'static str,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub default: f64,
    pub fmt: fn(f64) -> String,
    pub op: fn(f64) -> LiveOp,
    pub get: fn(&EffectParams) -> f64,
}

fn fmt_hz(v: f64) -> String {
    if v >= 1000.0 {
        format!("{:.1}k", v / 1000.0)
    } else {
        format!("{}", v.round())
    }
}
fn fmt_pct(v: f64) -> String {
    format!("{}%", (v * 100.0).round())
}
fn fmt_db(v: f64) -> String {
    format!("{}{:.1}dB", if v > 0.0 { "+" } else { "" }, v)
}
fn fmt_ms(v: f64) -> String {
    format!("{}ms", (v * 1000.0).round())
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
    format!("{}dB", mdaw_model::js::number_to_string(v))
}

/// Every Live FX knob, in `EffectsPanel` order (ids are the indexes).
pub fn fx_knobs() -> [FxKnob; 18] {
    let d = EffectParams::default();
    [
        FxKnob { label: "Cutoff", min: 60.0, max: 18000.0, step: 1.0, default: d.filter_cutoff, fmt: fmt_hz, op: LiveOp::FilterCutoff, get: |p| p.filter_cutoff },
        FxKnob { label: "Reso", min: 0.0, max: 20.0, step: 0.1, default: d.filter_resonance, fmt: fmt_q, op: LiveOp::FilterResonance, get: |p| p.filter_resonance },
        FxKnob { label: "Reverb", min: 0.0, max: 1.0, step: 0.01, default: d.reverb_wet, fmt: fmt_pct, op: LiveOp::ReverbWet, get: |p| p.reverb_wet },
        FxKnob { label: "Chorus", min: 0.0, max: 1.0, step: 0.01, default: d.chorus_depth, fmt: fmt_pct, op: LiveOp::ChorusDepth, get: |p| p.chorus_depth },
        FxKnob { label: "Rate", min: 0.1, max: 20.0, step: 0.1, default: d.filter_lfo_rate, fmt: fmt_rate, op: LiveOp::FilterLfoRate, get: |p| p.filter_lfo_rate },
        FxKnob { label: "Depth", min: 0.0, max: 1.0, step: 0.01, default: d.filter_lfo_depth, fmt: fmt_pct, op: LiveOp::FilterLfoDepth, get: |p| p.filter_lfo_depth },
        FxKnob { label: "Time", min: 0.01, max: 1.0, step: 0.01, default: d.delay_time, fmt: fmt_ms, op: LiveOp::DelayTime, get: |p| p.delay_time },
        FxKnob { label: "Feedback", min: 0.0, max: 0.95, step: 0.01, default: d.delay_feedback, fmt: fmt_pct, op: LiveOp::DelayFeedback, get: |p| p.delay_feedback },
        FxKnob { label: "Mix", min: 0.0, max: 1.0, step: 0.01, default: d.delay_wet, fmt: fmt_pct, op: LiveOp::DelayWet, get: |p| p.delay_wet },
        FxKnob { label: "Drive", min: 0.0, max: 1.0, step: 0.01, default: d.distortion_amount, fmt: fmt_pct, op: LiveOp::DistortionAmount, get: |p| p.distortion_amount },
        FxKnob { label: "Mix", min: 0.0, max: 1.0, step: 0.01, default: d.distortion_wet, fmt: fmt_pct, op: LiveOp::DistortionWet, get: |p| p.distortion_wet },
        FxKnob { label: "Low", min: -12.0, max: 12.0, step: 0.5, default: d.eq_low, fmt: fmt_db, op: LiveOp::EqLow, get: |p| p.eq_low },
        FxKnob { label: "Mid", min: -12.0, max: 12.0, step: 0.5, default: d.eq_mid, fmt: fmt_db, op: LiveOp::EqMid, get: |p| p.eq_mid },
        FxKnob { label: "High", min: -12.0, max: 12.0, step: 0.5, default: d.eq_high, fmt: fmt_db, op: LiveOp::EqHigh, get: |p| p.eq_high },
        FxKnob { label: "Thresh", min: -60.0, max: 0.0, step: 1.0, default: d.comp_threshold, fmt: fmt_thresh, op: LiveOp::CompThreshold, get: |p| p.comp_threshold },
        FxKnob { label: "Ratio", min: 1.0, max: 20.0, step: 0.5, default: d.comp_ratio, fmt: fmt_ratio, op: LiveOp::CompRatio, get: |p| p.comp_ratio },
        FxKnob { label: "Attack", min: 0.001, max: 1.0, step: 0.001, default: d.comp_attack, fmt: fmt_ms, op: LiveOp::CompAttack, get: |p| p.comp_attack },
        FxKnob { label: "Release", min: 0.01, max: 1.0, step: 0.01, default: d.comp_release, fmt: fmt_ms, op: LiveOp::CompRelease, get: |p| p.comp_release },
    ]
}

/// The value of phase-9 knob `n` (`KnobId::View(n)`): 0..18 the Live FX strip, 200.. the synth
/// editor's controls.
pub fn knob_value(s: &App, n: u16) -> f64 {
    if n < 100 {
        let p = s.svc.engine.live_effect_params();
        return fx_knobs().get(n as usize).map_or(0.0, |k| (k.get)(&p));
    }
    let i = (n - 200) as usize;
    let controls = synth_controls(engine_of(&s.p9_preset()));
    match controls.get(i) {
        Some(Control::Knob(k)) => knob_shown(k, &s.p9_preset_json()),
        _ => 0.0,
    }
}

fn fx_knob(s: &App, id: usize) -> El {
    let k = &fx_knobs()[id];
    let v = (k.get)(&s.svc.engine.live_effect_params());
    let op = k.op;
    knob_keyed(
        KnobSpec {
            id: KnobId::View(id as u16),
            label: k.label,
            min: k.min,
            max: k.max,
            step: k.step,
            default: k.default,
            small: true,
            hide_text: false,
        },
        format!("livefx-{}-{id}", k.label.to_lowercase()),
        v,
        (k.fmt)(v),
        "var(--primary)",
        move |s: &mut App, v| s.live_fx_op(op(v)),
        None,
        s,
    )
}

const FILTER_TYPES: [(FilterType, &str); 5] = [
    (FilterType::Lowpass, "LP"),
    (FilterType::Highpass, "HP"),
    (FilterType::Bandpass, "BP"),
    (FilterType::Notch, "Notch"),
    (FilterType::Allpass, "AP"),
];
const LFO_WAVES: [(LfoWaveform, &str); 4] = [
    (LfoWaveform::Sine, "Sin"),
    (LfoWaveform::Triangle, "Tri"),
    (LfoWaveform::Square, "Sqr"),
    (LfoWaveform::Sawtooth, "Saw"),
];

fn small_select(key: &str, label: &str, value: &str, w: f32, m: P9Menu) -> El {
    let th = t();
    row()
        .key(key.to_owned())
        .role("combobox")
        .label(label.to_owned())
        .focusable(true)
        .w(w)
        .h(24.0)
        .px(6.0)
        .items_center()
        .justify_between()
        .radius(R_MD)
        .border(1.0, th.border)
        .fill(th.surface2)
        .hover(th.surface3)
        .child(text(value.to_owned()).font(ui(11.0, 600.0)).color(th.fg))
        .child(icon(&icons::CHEVRON_DOWN, 12.0).color(th.muted_fg.alpha(0.5)))
        .on_press(move |s: &mut App| s.toggle_p9_menu(m))
}

/// The Live FX filter-type list.
pub fn filter_type_menu(s: &App) -> El {
    let cur = s.svc.engine.live_effect_params().filter_type;
    let mut list = popover_frame(Some(96.0), 4.0);
    for (f, l) in FILTER_TYPES {
        list = list.child(
            select_option(
                format!("livefx-type-{}", l.to_lowercase()),
                row().child(text(l).font(ui(12.0, 400.0)).color(t().fg)),
                f == cur,
            )
            .on_press(move |s: &mut App| {
                s.live_fx_op(LiveOp::FilterType(f));
                s.menu = None;
            }),
        );
    }
    select_popup("livefx-filter-type", Align::Start, list)
}

/// The Live FX LFO-wave list.
pub fn lfo_wave_menu(s: &App) -> El {
    let cur = s.svc.engine.live_effect_params().filter_lfo_wave;
    let mut list = popover_frame(Some(96.0), 4.0);
    for (w, l) in LFO_WAVES {
        list = list.child(
            select_option(
                format!("livefx-wave-{}", l.to_lowercase()),
                row().child(text(l).font(ui(12.0, 400.0)).color(t().fg)),
                w == cur,
            )
            .on_press(move |s: &mut App| {
                s.live_fx_op(LiveOp::FilterLfoWave(w));
                s.menu = None;
            }),
        );
    }
    select_popup("livefx-lfo-wave", Align::Start, list)
}

/// `EffectsPanel`: 68 points tall.
pub fn live_fx(s: &App) -> El {
    let th = t();
    let p = s.svc.engine.live_effect_params();
    let ft = FILTER_TYPES
        .iter()
        .find(|f| f.0 == p.filter_type)
        .map_or("LP", |f| f.1);
    let sections = [
        (FxSec::Lfo, "LFO", p.filter_lfo_depth > 0.0),
        (FxSec::Delay, "Delay", p.delay_wet > 0.0),
        (FxSec::Drive, "Drive", p.distortion_wet > 0.0),
        (
            FxSec::Eq,
            "EQ",
            p.eq_low != 0.0 || p.eq_mid != 0.0 || p.eq_high != 0.0,
        ),
        (FxSec::Comp, "Comp", p.comp_ratio > 1.0),
    ];
    let mut strip = row()
        .key("live-fx")
        .h(68.0)
        .w_full()
        .items_center()
        .gap(16.0)
        .px(12.0)
        .fill(th.surface1)
        .border_sides([0.0, 0.0, 1.0, 0.0], th.hairline)
        .child(
            col()
                .w(72.0)
                .gap(4.0)
                .child(
                    text("Live FX")
                        .font(daw_label(10.5))
                        .text_case(rune::TextCase::Uppercase)
                        .color(th.muted_fg),
                )
                .child(small_select(
                    "livefx-filter-type",
                    "Filter type",
                    ft,
                    64.0,
                    P9Menu::LiveFilterType,
                )),
        )
        .child(
            row()
                .items_center()
                .gap(12.0)
                .child(fx_knob(s, 0))
                .child(fx_knob(s, 1))
                .child(el().w(1.0).h(36.0).fill(th.hairline_strong))
                .child(fx_knob(s, 2))
                .child(fx_knob(s, 3)),
        );
    if let Some(sec) = s.p9.fx_section {
        let label = sections.iter().find(|x| x.0 == sec).map_or("", |x| x.1);
        let mut inner = row()
            .key("live-fx-section")
            .h(60.0)
            .items_center()
            .gap(12.0)
            .px(12.0)
            .radius(R_MD)
            .border(1.0, th.border)
            .fill(th.surface0)
            .child(
                text(label)
                    .font(daw_label(10.5))
                    .text_case(rune::TextCase::Uppercase)
                    .color(th.primary)
                    .w(40.0),
            );
        let ids: &[usize] = match sec {
            FxSec::Lfo => &[4, 5],
            FxSec::Delay => &[6, 7, 8],
            FxSec::Drive => &[9, 10],
            FxSec::Eq => &[11, 12, 13],
            FxSec::Comp => &[14, 15, 16, 17],
        };
        for &i in ids {
            inner = inner.child(fx_knob(s, i));
        }
        if sec == FxSec::Lfo {
            let w = LFO_WAVES
                .iter()
                .find(|x| x.0 == p.filter_lfo_wave)
                .map_or("Sin", |x| x.1);
            inner = inner.child(small_select(
                "livefx-lfo-wave",
                "LFO waveform",
                w,
                60.0,
                P9Menu::LiveLfoWave,
            ));
        }
        strip = strip.child(inner);
    }
    let mut tabs = row()
        .role("tablist")
        .label("More effects")
        .h(28.0)
        .items_center()
        .gap(2.0)
        .padding(2.0)
        .radius(R_MD)
        .border(1.0, th.border)
        .fill(th.surface0);
    for (sec, label, has) in sections {
        let on = s.p9.fx_section == Some(sec);
        let mut b = row()
            .key(format!("livefx-tab-{}", label.to_lowercase()))
            .role("tab")
            .label(label)
            .focus_selected(on)
            .h(22.0)
            .px(8.0)
            .items_center()
            .gap(6.0)
            .radius(R_SM)
            .help(if has {
                format!("{label} (active)")
            } else {
                label.to_owned()
            })
            .child(el().size(6.0, 6.0).radius(3.0).fill(if has {
                th.primary
            } else {
                th.surface4
            }))
            .child(
                text(label)
                    .font(ui(10.5, 700.0).tracking(0.63))
                    .text_case(rune::TextCase::Uppercase)
                    .color(if on { th.fg } else { th.muted_fg }),
            )
            .on_press(move |s: &mut App| {
                s.p9.fx_section = if s.p9.fx_section == Some(sec) {
                    None
                } else {
                    Some(sec)
                };
            });
        b = if on { b.fill(th.surface3) } else { b.hover(th.surface2) };
        tabs = tabs.child(b);
    }
    strip.child(spacer()).child(tabs)
}

// ------------------------------------------------------------------------------- synth editor

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    Subtractive,
    Fm,
    Pluck,
    Sampler,
}

pub fn engine_of(p: &SynthPreset) -> Engine {
    match p {
        SynthPreset::Subtractive(_) => Engine::Subtractive,
        SynthPreset::Fm(_) => Engine::Fm,
        SynthPreset::Pluck(_) => Engine::Pluck,
        SynthPreset::Sampler(_) => Engine::Sampler,
    }
}

#[derive(Debug, Clone, Copy)]
pub struct KnobDef {
    pub path: &'static str,
    pub label: &'static str,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub fmt: fn(f64) -> String,
    pub log: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct ChoiceDef {
    pub path: &'static str,
    pub label: &'static str,
    pub options: &'static [(&'static str, &'static str)],
}

#[derive(Debug, Clone, Copy)]
pub enum Control {
    Knob(KnobDef),
    Choice(ChoiceDef),
    /// A section break: the title of the next section.
    Section(&'static str),
}

fn f_sec(v: f64) -> String {
    if v >= 1.0 {
        format!("{v:.2}s")
    } else {
        format!("{}ms", (v * 1000.0).round())
    }
}
fn f_hz(v: f64) -> String {
    if v >= 1000.0 {
        format!("{:.1}k", v / 1000.0)
    } else {
        format!("{}Hz", v.round())
    }
}
fn f_f2(v: f64) -> String {
    format!("{v:.2}")
}
fn f_f1(v: f64) -> String {
    format!("{v:.1}")
}
fn f_int(v: f64) -> String {
    format!("{}", v.round())
}
fn f_semi(v: f64) -> String {
    format!("{}{}st", if v > 0.0 { "+" } else { "" }, v.round())
}
fn f_cents(v: f64) -> String {
    format!("{}{}c", if v > 0.0 { "+" } else { "" }, v.round())
}
fn f_oct(v: f64) -> String {
    format!("{}{v:.1}oct", if v > 0.0 { "+" } else { "" })
}
fn f_f3(v: f64) -> String {
    format!("{v:.3}")
}

const WAVES: &[(&str, &str)] = &[
    ("sine", "Sin"),
    ("triangle", "Tri"),
    ("sawtooth", "Saw"),
    ("square", "Sqr"),
    ("pulse", "Pls"),
];
const FM_WAVES: &[(&str, &str)] = &[
    ("sine", "Sin"),
    ("triangle", "Tri"),
    ("sawtooth", "Saw"),
    ("square", "Sqr"),
];
const FILTERS: &[(&str, &str)] = &[("lowpass", "LP"), ("highpass", "HP"), ("bandpass", "BP")];

const fn k(path: &'static str, label: &'static str, min: f64, max: f64, step: f64, fmt: fn(f64) -> String) -> Control {
    Control::Knob(KnobDef { path, label, min, max, step, fmt, log: false })
}
const fn klog(path: &'static str, label: &'static str, min: f64, max: f64, fmt: fn(f64) -> String) -> Control {
    Control::Knob(KnobDef { path, label, min, max, step: 0.01, fmt, log: true })
}
const fn ch(path: &'static str, label: &'static str, options: &'static [(&'static str, &'static str)]) -> Control {
    Control::Choice(ChoiceDef { path, label, options })
}

fn adsr(prefix: &'static str) -> [Control; 4] {
    let p = |s: &'static str| -> &'static str { Box::leak(format!("{prefix}.{s}").into_boxed_str()) };
    [
        k(p("attack"), "Attack", 0.001, 4.0, 0.001, f_sec),
        k(p("decay"), "Decay", 0.01, 6.0, 0.01, f_sec),
        k(p("sustain"), "Sustain", 0.0, 1.0, 0.01, fmt_pct),
        k(p("release"), "Release", 0.01, 6.0, 0.01, f_sec),
    ]
}

fn character(extra: &[Control]) -> Vec<Control> {
    let mut v = vec![
        Control::Section("Character"),
        k("character.drive", "Drive", 0.0, 1.0, 0.01, fmt_pct),
        k("character.chorus", "Chorus", 0.0, 1.0, 0.01, fmt_pct),
        k("character.presence", "Presence", -12.0, 12.0, 0.5, fmt_db),
    ];
    v.extend_from_slice(extra);
    v.push(k("gain", "Level", -18.0, 12.0, 0.5, fmt_db));
    v
}

/// The sections of `SynthEditor` per engine, flattened (`Control::Section` starts one).
pub fn synth_controls(e: Engine) -> Vec<Control> {
    static CACHE: std::sync::OnceLock<[Vec<Control>; 4]> = std::sync::OnceLock::new();
    let all = CACHE.get_or_init(|| {
        let mut sub = vec![
            Control::Section("Osc A"),
            ch("oscA.wave", "Wave", WAVES),
            k("oscA.level", "Level", 0.0, 1.0, 0.01, fmt_pct),
            k("oscA.unison", "Unison", 1.0, 7.0, 1.0, f_int),
            k("oscA.spread", "Detune", 0.0, 60.0, 1.0, f_cents),
            Control::Section("Osc B"),
            ch("oscB.wave", "Wave", WAVES),
            k("oscB.level", "Level", 0.0, 1.0, 0.01, fmt_pct),
            k("oscB.semi", "Semi", -24.0, 24.0, 1.0, f_semi),
            k("oscB.fine", "Fine", -50.0, 50.0, 1.0, f_cents),
            k("sub.level", "Sub", 0.0, 1.0, 0.01, fmt_pct),
            k("noise.level", "Noise", 0.0, 1.0, 0.01, fmt_pct),
            Control::Section("Filter"),
            ch("filter.type", "Type", FILTERS),
            klog("filter.cutoff", "Cutoff", 40.0, 18000.0, f_hz),
            k("filter.resonance", "Reso", 0.0, 20.0, 0.1, f_f1),
            k("filter.envAmount", "Env Amt", -4.0, 8.0, 0.1, f_oct),
            k("filter.keyTrack", "Key Trk", 0.0, 1.0, 0.01, fmt_pct),
            k("filter.velocity", "Vel", 0.0, 1.0, 0.01, fmt_pct),
            Control::Section("Filter Env"),
        ];
        sub.extend(adsr("filterEnv"));
        sub.push(Control::Section("Amp"));
        sub.extend(adsr("amp"));
        sub.push(k("amp.velocity", "Vel", 0.0, 1.0, 0.01, fmt_pct));
        sub.extend([
            Control::Section("Mod"),
            k("glide", "Glide", 0.0, 1.0, 0.005, f_sec),
            k("lfo.rate", "LFO Rate", 0.05, 20.0, 0.05, fmt_rate),
            k("lfo.vibrato", "Vibrato", 0.0, 100.0, 1.0, f_cents),
            k("lfo.filter", "LFO>Filt", 0.0, 4800.0, 10.0, f_cents),
        ]);
        sub.extend(character(&[]));
        let mut fm = vec![
            Control::Section("FM"),
            ch("carrier.wave", "Carrier", FM_WAVES),
            k("ratio", "Ratio", 0.25, 14.0, 0.01, f_f2),
            k("index", "Index", 0.0, 12.0, 0.05, f_f2),
            k("velocityToIndex", "Vel>Idx", 0.0, 1.0, 0.01, fmt_pct),
            k("keyScale", "Key Scl", 0.0, 1.0, 0.01, fmt_pct),
            k("detune", "Detune", -30.0, 30.0, 1.0, f_cents),
            Control::Section("Mod Env"),
        ];
        fm.extend(adsr("modEnv"));
        fm.extend([
            Control::Section("Tine"),
            k("ping.level", "Level", 0.0, 1.0, 0.01, fmt_pct),
            k("ping.ratio", "Ratio", 0.5, 16.0, 0.01, f_f2),
            k("ping.decay", "Decay", 0.005, 2.0, 0.005, f_sec),
            Control::Section("Amp"),
        ]);
        fm.extend(adsr("amp"));
        fm.push(k("amp.velocity", "Vel", 0.0, 1.0, 0.01, fmt_pct));
        fm.extend([
            Control::Section("Vibrato"),
            k("lfo.rate", "Rate", 0.05, 20.0, 0.05, fmt_rate),
            k("lfo.vibrato", "Depth", 0.0, 100.0, 1.0, f_cents),
        ]);
        fm.extend(character(&[
            k("character.tremolo", "Tremolo", 0.0, 1.0, 0.01, fmt_pct),
            k("character.tremoloRate", "Trem Hz", 0.5, 12.0, 0.1, f_f1),
        ]));
        let mut pluck = vec![
            Control::Section("String"),
            k("attackNoise", "Pick", 0.1, 8.0, 0.1, f_f1),
            klog("dampening", "Bright", 300.0, 12000.0, f_hz),
            k("resonance", "Sustain", 0.8, 0.999, 0.001, f_f3),
            k("release", "Release", 0.05, 5.0, 0.01, f_sec),
            k("velocity", "Vel", 0.0, 1.0, 0.01, fmt_pct),
        ];
        pluck.extend(character(&[klog("character.tone", "Tone", 500.0, 20000.0, f_hz)]));
        let mut sampler = vec![
            Control::Section("Piano"),
            k("release", "Release", 0.05, 3.0, 0.01, f_sec),
            k("velocityCurve", "Touch", 0.6, 2.5, 0.05, f_f2),
        ];
        sampler.extend(character(&[klog("character.tone", "Tone", 500.0, 20000.0, f_hz)]));
        [sub, fm, pluck, sampler]
    });
    all[match e {
        Engine::Subtractive => 0,
        Engine::Fm => 1,
        Engine::Pluck => 2,
        Engine::Sampler => 3,
    }]
    .clone()
}

/// `numberAt(preset, path, min)`.
fn number_at(json: &Value, path: &str, fallback: f64) -> f64 {
    get_path(json, path)
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite())
        .unwrap_or(fallback)
}

/// A knob's position value (log knobs move in log2 space).
fn knob_shown(k: &KnobDef, json: &Value) -> f64 {
    let v = number_at(json, k.path, k.min);
    if k.log {
        v.clamp(k.min, k.max).log2()
    } else {
        v
    }
}

impl App {
    /// The live preset with the edits (`resolvePreset(index, overrides)`).
    pub fn p9_preset(&self) -> SynthPreset {
        self.p9.preset_cache.borrow().0.clone()
    }

    pub fn p9_preset_json(&self) -> Value {
        self.p9.preset_cache.borrow().1.clone()
    }

    /// Refreshes the preset mirror (after a preset change / a synth param).
    pub fn refresh_preset_cache(&mut self) {
        let live = self.svc.engine.live();
        let p = live.current_preset();
        let idx = live.preset_index();
        let edits = !live.synth_overrides().is_empty();
        let json = p.to_json();
        *self.p9.preset_cache.borrow_mut() = (p, json, idx, edits);
    }
}

fn choice(def: ChoiceDef, value: Option<&str>) -> El {
    let th = t();
    let mut stack = col()
        .gap(2.0)
        .padding(2.0)
        .radius(R_MD)
        .border(1.0, th.border)
        .fill(th.surface0);
    for (v, l) in def.options {
        let on = value == Some(*v);
        let path = def.path;
        let mut b = row()
            .key(format!("synth-{}-{v}", def.path))
            .role("radio")
            .label(*l)
            .checked(on)
            .px(6.0)
            .radius(3.0)
            .child(text(*l).font(mono(9.5, 600.0).lh(16.0)).color(if on {
                th.primary_fg
            } else {
                th.muted_fg
            }))
            .on_press(move |s: &mut App| s.set_synth_param(path, Value::String((*v).to_owned())));
        b = if on { b.fill(th.primary) } else { b.hover(th.surface3) };
        stack = stack.child(b);
    }
    col()
        .items_center()
        .gap(4.0)
        .child(stack)
        .child(
            text(def.label)
                .font(daw_label(9.5).lh(9.5))
                .text_case(rune::TextCase::Uppercase)
                .color(th.muted_fg),
        )
}

/// `SynthEditor`.
pub fn synth_editor(s: &App) -> El {
    let th = t();
    let (preset, json, index, edited) = s.p9.preset_cache.borrow().clone();
    let presets = synth_presets();
    let base = presets.get(index).unwrap_or(&presets[0]);
    let base_json = base.to_json();
    let eng = engine_of(base);
    let is_mono = matches!(&preset, SynthPreset::Subtractive(p) if p.mono);
    let engine_label = match eng {
        Engine::Subtractive => "Analog",
        Engine::Fm => "FM",
        Engine::Pluck => "String",
        Engine::Sampler => "Sampled",
    };
    let cat = base.meta().category;
    let mut cats = row().flex_wrap().items_center().gaps(4.0, 4.0);
    for (c, label) in PRESET_CATEGORIES {
        let Some(first) = presets.iter().position(|p| p.meta().category == c) else {
            continue;
        };
        let on = c == cat;
        let mut b = row()
            .key(format!("synth-cat-{}", label.to_lowercase()))
            .role("button")
            .label(label)
            .h(24.0)
            .px(8.0)
            .items_center()
            .radius(R_SM - 2.0)
            .child(text(label).font(ui(11.0, 600.0)).color(if on {
                th.primary_fg
            } else {
                th.muted_fg
            }))
            .on_press(move |s: &mut App| s.change_preset(first));
        b = if on {
            b.fill(th.primary)
        } else {
            b.fill(th.surface3).hover(th.surface4)
        };
        cats = cats.child(b);
    }
    let reset = {
        let mut r = row()
            .key("synth-reset")
            .role("button")
            .label(if edited { "Reset edits" } else { "Reset" })
            .help("Reset to the preset's original sound")
            .h(28.0)
            .px(8.0)
            .items_center()
            .gap(6.0)
            .radius(R_MD)
            .child(icon(&icons_views::ROTATE_CCW, 14.0).color(th.muted_fg))
            .child(
                text(if edited { "Reset edits" } else { "Reset" })
                    .font(ui(12.0, 500.0))
                    .color(th.muted_fg),
            );
        if edited {
            r = r
                .hover(th.surface3)
                .on_press(move |s: &mut App| s.change_preset(index));
        } else {
            r = r.opacity(0.4).disabled(true);
        }
        r
    };
    let header = row()
        .w_full()
        .mb(8.0)
        .items_center()
        .gap(8.0)
        .child(
            text("Sound")
                .font(daw_label(10.5))
                .text_case(rune::TextCase::Uppercase)
                .color(th.muted_fg),
        )
        .child(
            row()
                .mr(4.0)
                .px(6.0)
                .py(2.0)
                .radius(R_SM - 2.0)
                .fill(th.surface3)
                .child(
                    text(format!("{engine_label}{}", if is_mono { " · Mono" } else { "" }))
                        .font(mono(10.0, 600.0).tracking(0.6))
                        .text_case(rune::TextCase::Uppercase)
                        .color(th.fg.alpha(0.8)),
                ),
        )
        .child(cats)
        .child(spacer())
        .child(reset);
    let mut in_cat = row().w_full().mb(10.0).flex_wrap().items_center().gaps(2.0, 2.0);
    for (i, p) in presets.iter().enumerate() {
        if p.meta().category != cat {
            continue;
        }
        let on = i == index;
        let mut b = row()
            .key(format!("synth-preset-{i}"))
            .role("button")
            .label(p.meta().name.clone())
            .h(24.0)
            .px(8.0)
            .items_center()
            .radius(R_MD)
            .child(
                text(p.meta().name.clone())
                    .font(ui(12.0, if on { 600.0 } else { 400.0 }))
                    .color(if on { th.fg } else { th.muted_fg }),
            )
            .on_press(move |s: &mut App| s.change_preset(i));
        b = if on {
            b.fill(th.surface3).border(1.0, th.primary)
        } else {
            b.hover(th.surface2)
        };
        in_cat = in_cat.child(b);
    }
    if let Some(d) = &base.meta().description {
        in_cat = in_cat.child(
            text(d.clone())
                .font(ui(11.0, 400.0))
                .color(th.muted_fg)
                .truncate()
                .ml(8.0),
        );
    }
    let mut sections = row().w_full().items_stretch().gap(8.0);
    let mut cur: Option<El> = None;
    let mut controls_row = row().items_start().gap(10.0);
    let flush = |cur: &mut Option<El>, controls_row: &mut El, sections: &mut El| {
        if let Some(c) = cur.take() {
            let ctl = std::mem::replace(controls_row, row().items_start().gap(10.0));
            *sections = std::mem::replace(sections, row()).child(c.child(ctl));
        }
    };
    for (i, c) in synth_controls(eng).into_iter().enumerate() {
        match c {
            Control::Section(title) => {
                flush(&mut cur, &mut controls_row, &mut sections);
                cur = Some(
                    col()
                        .gap(8.0)
                        .px(10.0)
                        .pt(6.0)
                        .pb(8.0)
                        .radius(R_LG)
                        .border(1.0, th.border)
                        .fill(th.surface2)
                        .child(
                            text(title)
                                .font(daw_label(10.5))
                                .text_case(rune::TextCase::Uppercase)
                                .color(th.muted_fg),
                        ),
                );
            }
            Control::Choice(d) => {
                let v = get_path(&json, d.path).and_then(Value::as_str).map(str::to_owned);
                controls_row = controls_row.child(choice(d, v.as_deref()));
            }
            Control::Knob(d) => {
                let value = knob_shown(&d, &json);
                let default = knob_shown(&d, &base_json);
                let (min, max, step) = if d.log {
                    (d.min.log2(), d.max.log2(), 0.01)
                } else {
                    (d.min, d.max, d.step)
                };
                let shown = if d.log { 2f64.powf(value) } else { value };
                let path = d.path;
                let log = d.log;
                controls_row = controls_row.child(knob_keyed(
                    KnobSpec {
                        id: KnobId::View(200 + i as u16),
                        label: d.label,
                        min,
                        max,
                        step,
                        default,
                        small: true,
                        hide_text: false,
                    },
                    format!("synth-{path}"),
                    value,
                    (d.fmt)(shown),
                    "var(--primary)",
                    move |s: &mut App, v| {
                        let out = if log { (2f64.powf(v)).round() } else { v };
                        s.set_synth_param(path, serde_json::json!(out));
                    },
                    None,
                    s,
                ));
            }
        }
    }
    flush(&mut cur, &mut controls_row, &mut sections);
    col()
        .key("synth-editor")
        .w_full()
        .px(12.0)
        .pt(10.0)
        .pb(12.0)
        .fill(th.surface1)
        .border_sides([0.0, 0.0, 1.0, 0.0], th.hairline)
        .child(header)
        .child(in_cat)
        .child(
            col()
                .w_full()
                .child(sections)
                .scroll_x(),
        )
}

// ------------------------------------------------------------------------------- selector

/// The top bar's `InstrumentSelector` popover: search, vibe chips, groups by category.
pub fn instrument_menu(s: &App) -> El {
    let th = t();
    let q = s.p9.instrument_search.text.clone();
    let presets = synth_presets();
    let index = s.p9.preset_cache.borrow().2;
    let search = row()
        .h(40.0)
        .w_full()
        .items_center()
        .gap(8.0)
        .px(12.0)
        .border_sides([0.0, 0.0, 1.0, 0.0], th.border)
        .child(icon(&icons::SEARCH, 16.0).color(th.muted_fg.alpha(0.5)))
        .child(
            text_input(&s.p9.instrument_search, ui(13.0, 400.0))
                .key("sound-search")
                .label("Search sounds…")
                .placeholder("Search sounds…", th.muted_fg)
                .single_line()
                .autofocus()
                .color(th.fg)
                .grow(1.0)
                .on_edit(|s: &mut App| &mut s.p9.instrument_search),
        );
    let chip = |key: String, label: &str, on: bool| {
        let c = row()
            .key(key)
            .role("button")
            .label(label.to_owned())
            .h(24.0)
            .px(8.0)
            .center()
            .radius(R_SM - 2.0)
            .child(text(label.to_owned()).font(ui(11.0, 600.0)).color(if on {
                th.primary_fg
            } else {
                th.muted_fg
            }));
        if on {
            c.fill(th.primary)
        } else {
            c.fill(th.surface3).hover(th.surface4)
        }
    };
    let mut chips = row()
        .w_full()
        .gap(4.0)
        .px(8.0)
        .py(8.0)
        .border_sides([0.0, 0.0, 1.0, 0.0], th.border)
        .clip_rounded(0.0)
        .child(chip("sound-vibe-all".into(), "All", s.p9.vibe.is_none()).on_press(|s: &mut App| s.p9.vibe = None));
    for (v, label, _) in PRESET_VIBES {
        if !presets.iter().any(|p| p.meta().vibe == v) {
            continue;
        }
        chips = chips.child(
            chip(format!("sound-vibe-{label}"), label, s.p9.vibe == Some(v)).on_press(
                move |s: &mut App| {
                    s.p9.vibe = if s.p9.vibe == Some(v) { None } else { Some(v) };
                },
            ),
        );
    }
    let mut list = col().w_full().padding(4.0);
    let mut any = false;
    for (c, label) in PRESET_CATEGORIES {
        let mut group = Vec::new();
        for (i, p) in presets.iter().enumerate() {
            let m = p.meta();
            if m.category != c || s.p9.vibe.is_some_and(|v| v != m.vibe) {
                continue;
            }
            let vibe = PRESET_VIBES
                .iter()
                .find(|x| x.0 == m.vibe)
                .map_or("", |x| x.1);
            let hay = format!(
                "{} {label} {:?} {}",
                m.name,
                m.vibe,
                m.description.clone().unwrap_or_default()
            );
            if !matches_query(&hay, &q) {
                continue;
            }
            let on = i == index;
            let mut r = row()
                .key(format!("sound-{i}"))
                .role("option")
                .label(m.name.clone())
                .w_full()
                .items_center()
                .justify_between()
                .gap(8.0)
                .px(8.0)
                .py(6.0)
                .radius(R_MD)
                .hover(th.surface3)
                .child(
                    row()
                        .min_w(0.0)
                        .items_center()
                        .gap(8.0)
                        .child(el().size(6.0, 6.0).radius(3.0).fill(if on {
                            th.primary
                        } else {
                            Color::TRANSPARENT
                        }))
                        .child(
                            text(m.name.clone())
                                .font(ui(13.0, if on { 600.0 } else { 400.0 }))
                                .color(if on { th.primary } else { th.fg })
                                .truncate(),
                        ),
                )
                .child(text(vibe.to_owned()).font(ui(10.5, 400.0)).color(th.muted_fg))
                .on_press(move |s: &mut App| {
                    s.change_preset(i);
                    s.menu = None;
                });
            if let Some(d) = &m.description {
                r = r.help(d.clone());
            }
            group.push(r);
        }
        if group.is_empty() {
            continue;
        }
        let mut g = col().w_full().padding(4.0);
        if any {
            g = g.border_sides([1.0, 0.0, 0.0, 0.0], th.border.alpha(0.6));
        }
        any = true;
        g = g.child(
            text(label)
                .font(daw_label(10.5))
                .text_case(rune::TextCase::Uppercase)
                .color(th.muted_fg)
                .px(8.0)
                .pt(8.0)
                .pb(4.0),
        );
        for r in group {
            g = g.child(r);
        }
        list = list.child(g);
    }
    if !any {
        list = list.child(
            text("No sounds found.")
                .key("sound-empty")
                .font(ui(14.0, 400.0))
                .color(th.muted_fg)
                .py(24.0)
                .self_center(),
        );
    }
    let content = popover_frame(Some(340.0), 0.0)
        .key("instrument-popover")
        .child(search)
        .child(chips)
        .child(
            col()
                .w_full()
                .max_h(360.0)
                .child(list)
                .scroll(rune::ScrollSpec {
                    stick_bottom: false,
                }),
        );
    popover("instrument", Align::End, content)
}
