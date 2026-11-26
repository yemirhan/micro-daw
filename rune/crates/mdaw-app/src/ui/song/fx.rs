//! `TrackEffectsPanel.tsx`: the 336-point drawer — header (Track FX, track name, close), then
//! one section per effect (switch, expand chevron, knobs; off sections at 45 %).

use mdaw_model::doc::EffectName;
use mdaw_model::{FilterType, LfoWaveform, Track};
use rune::{Align, Color, El, col, el, icon, row, spacer, text};
use serde_json::{Map, Value};

use crate::app::{App, Menu};
use crate::icons;
use crate::song::{FX_PARAMS, FxSection, SongMenu, fx_value};
use crate::theme::{R_LG, R_MD, daw_label, mono, t, ui};
use crate::ui::knob::{KnobId, KnobSpec, knob_keyed};
use crate::ui::song::track_color;
use crate::ui::widgets::{Button, Size, Variant, popover_frame, select_option, select_popup};

pub const FILTER_TYPES: [(FilterType, &str, &str); 5] = [
    (FilterType::Lowpass, "lowpass", "LP"),
    (FilterType::Highpass, "highpass", "HP"),
    (FilterType::Bandpass, "bandpass", "BP"),
    (FilterType::Notch, "notch", "Notch"),
    (FilterType::Allpass, "allpass", "AP"),
];

pub const LFO_WAVES: [(LfoWaveform, &str, &str); 4] = [
    (LfoWaveform::Sine, "sine", "Sin"),
    (LfoWaveform::Triangle, "triangle", "Tri"),
    (LfoWaveform::Square, "square", "Sqr"),
    (LfoWaveform::Sawtooth, "sawtooth", "Saw"),
];

/// The Song knobs' values (`KnobId::Song(n)`): FX parameter `n` of the open drawer, or the
/// mixer pan of track `n − 1000`.
pub fn knob_value(s: &App, n: u16) -> f64 {
    if n >= 1000 {
        return s.tracks().get((n - 1000) as usize).map_or(0.0, |t| t.pan);
    }
    let Some(t) = s.song.fx_track.as_ref().and_then(|id| s.find_track(id)) else {
        return 0.0;
    };
    FX_PARAMS.get(n as usize).map_or(0.0, |p| fx_value(t, p))
}

fn fx_knob(s: &App, track: &Track, i: usize) -> El {
    let p = FX_PARAMS[i];
    let v = fx_value(track, &p);
    let spec = KnobSpec {
        id: KnobId::Song(i as u16),
        label: p.label,
        min: p.min,
        max: p.max,
        step: p.step,
        default: p.default,
        small: true,
        hide_text: false,
    };
    knob_keyed(
        spec,
        format!("fx-knob-{}-{}", p.section.key(), p.key),
        v,
        (p.fmt)(v),
        &track.color,
        move |s: &mut App, v| s.set_fx_param(&FX_PARAMS[i], v),
        Some(App::end_fx_knob),
        s,
    )
}

fn select_trigger(key: &str, label: &str, value: &str, w: f32, m: SongMenu, open: bool) -> El {
    let th = t();
    row()
        .key(key.to_owned())
        .role("combobox")
        .focusable(true)
        .label(label.to_owned())
        .w(w)
        .h(24.0)
        .items_center()
        .justify_between()
        .gap(8.0)
        .px(6.0)
        .radius(R_MD)
        // `SelectTrigger`: `bg-surface-2 border-border`, `hover:bg-surface-3`, open
        // `border-input`; the chevron `size-3.5 opacity-60`.
        .fill(th.surface2)
        .border(1.0, if open { th.input } else { th.border })
        .hover(th.surface3)
        .child(text(value.to_owned()).font(ui(11.0, 600.0)).color(th.fg))
        .child(icon(&icons::CHEVRON_DOWN, 14.0).color(th.muted_fg.alpha(0.6)))
        .on_press(move |s: &mut App| s.toggle_menu(Menu::Song(m.clone())))
}

fn section(s: &App, track: &Track, sec: FxSection, body: Vec<El>) -> El {
    let th = t();
    let fx = track.effects.clone().unwrap_or_default();
    let enabled = sec.enabled(&fx);
    let expanded = s.song.fx_expanded.contains(&sec);
    // A chain slot's power button: off takes the effect out of the chain (its settings are
    // kept for when it is added again).
    let sw = row()
        .key(format!("fx-switch-{}", sec.key()))
        .role("button")
        .label(format!("Remove {} from the chain", sec.label()))
        .help(format!("Remove {} from the chain", sec.label()))
        .size(22.0, 22.0)
        .center()
        .radius(R_MD)
        .hover(th.surface3)
        .child(icon(&crate::icons_io::POWER, 13.0).color(if enabled {
            th.primary
        } else {
            th.muted_fg
        }))
        .on_press(move |s: &mut App| s.toggle_fx_enabled(sec));
    let header = row()
        .h(32.0)
        .w_full()
        .items_center()
        .gap(8.0)
        .pr(4.0)
        .pl(8.0)
        .child(sw)
        .child(
            row()
                .key(format!("fx-expand-{}", sec.key()))
                .role("button")
                .label(sec.label())
                .expanded(expanded)
                .h(28.0)
                .grow(1.0)
                .items_center()
                .justify_between()
                .px(6.0)
                .radius(R_MD)
                .child(
                    text(sec.label())
                        .font(daw_label(11.0))
                        .text_case(rune::TextCase::Uppercase)
                        .color(if enabled { th.fg } else { th.muted_fg }),
                )
                .child(
                    icon(&icons::CHEVRON_DOWN, 14.0)
                        .color(th.muted_fg)
                        .icon_rotation(if expanded {
                            0.0
                        } else {
                            -std::f32::consts::FRAC_PI_2
                        }),
                )
                .on_press(move |s: &mut App| s.toggle_fx_section(sec)),
        );
    let mut box_ = col()
        .key(format!("fx-section-{}", sec.key()))
        .w_full()
        .radius(R_LG)
        .padding(1.0)
        .fill(th.surface2)
        .border(1.0, if enabled { th.input } else { th.border })
        .child(header);
    if expanded {
        let mut content = row()
            .key(format!("fx-body-{}", sec.key()))
            .w_full()
            .flex_wrap()
            .items_center()
            .gaps(12.0, 8.0)
            .px(12.0)
            .py(10.0)
            .border_sides([1.0, 0.0, 0.0, 0.0], th.hairline)
            .opacity(if enabled { 1.0 } else { 0.45 });
        for b in body {
            content = content.child(b);
        }
        box_ = box_.child(content);
    }
    box_
}

/// The connector between two slots of the chain.
fn link() -> El {
    el().w(1.0).h(8.0).fill(t().input)
}

/// The chain's ends: what feeds it and where it goes.
fn endpoint(key: &str, tag: &str, what: &str) -> El {
    let th = t();
    row()
        .key(key.to_owned())
        .h(22.0)
        .items_center()
        .gap(6.0)
        .px(8.0)
        .radius(11.0)
        .fill(th.surface2)
        .child(
            text(tag.to_owned())
                .font(daw_label(10.0))
                .text_case(rune::TextCase::Uppercase)
                .color(th.muted_fg),
        )
        .child(text(what.to_owned()).font(ui(11.0, 400.0)).color(th.fg))
}

/// What feeds the track's chain.
fn chain_source(track: &Track) -> &'static str {
    match track.instrument.kind {
        mdaw_model::InstrumentType::Audio => "Audio input and clips",
        mdaw_model::InstrumentType::Drums => "Drum kit",
        _ => "Instrument",
    }
}

/// The "Add effect" menu: the built-in effects not in the chain yet. Each lands at its place in
/// the signal order.
pub fn add_effect_menu(s: &App) -> El {
    let th = t();
    let fx = s
        .song
        .fx_track
        .as_ref()
        .and_then(|id| s.find_track(id))
        .and_then(|t| t.effects.clone())
        .unwrap_or_default();
    let mut list = popover_frame(Some(300.0), 4.0).key("fx-add-menu");
    for sec in FxSection::SIGNAL_ORDER {
        if sec.enabled(&fx) {
            continue;
        }
        list = list.child(
            select_option(
                format!("fx-add-{}", sec.key()),
                row()
                    .child(text(sec.label()).font(ui(12.0, 600.0)).color(th.fg).w(88.0))
                    .child(text(sec.about()).font(ui(11.0, 400.0)).color(th.muted_fg)),
                false,
            )
            .on_press(move |s: &mut App| {
                s.menu = None;
                s.add_fx(sec);
            }),
        );
    }
    select_popup("fx-add", Align::Start, list)
}

fn knobs(s: &App, track: &Track, sec: FxSection) -> Vec<El> {
    FX_PARAMS
        .iter()
        .enumerate()
        .filter(|(_, p)| p.section == sec)
        .map(|(i, _)| fx_knob(s, track, i))
        .collect()
}

pub fn view(s: &App, track: &Track) -> El {
    let th = t();
    let fx = track.effects.clone().unwrap_or_default();
    let ft = FILTER_TYPES
        .iter()
        .find(|f| f.0 == fx.filter.kind)
        .map_or("LP", |f| f.2);
    let lw = LFO_WAVES
        .iter()
        .find(|w| w.0 == fx.filter.lfo_wave)
        .map_or("Sin", |w| w.2);
    let ks = knobs(s, track, FxSection::Filter);
    // The TS wraps [select, cutoff, reso] / `basis-full` / [rate, depth, wave] in one flex-wrap
    // box; Chromium puts the zero-height break on its own line between two row gaps. Two
    // explicit rows 16 points apart draw the same (a zero-height wrapped item cost Rune one
    // more gap: the second row sat ~8 points low).
    let mut it = ks.into_iter();
    let line = |a: El, b: El, c: El| row().items_center().gap(12.0).child(a).child(b).child(c);
    let first = line(
        select_trigger(
            "fx-filter-type",
            "Filter type",
            ft,
            64.0,
            SongMenu::FilterType,
            s.menu == Some(Menu::Song(SongMenu::FilterType)),
        ),
        it.next().unwrap_or_else(el),
        it.next().unwrap_or_else(el),
    );
    let (rate, depth) = (it.next().unwrap_or_else(el), it.next().unwrap_or_else(el));
    let second = line(
        rate,
        depth,
        select_trigger(
            "fx-lfo-wave",
            "LFO waveform",
            lw,
            60.0,
            SongMenu::LfoWave,
            s.menu == Some(Menu::Song(SongMenu::LfoWave)),
        ),
    );
    let filter_body = vec![col().gap(16.0).child(first).child(second)];
    // The chain, top to bottom in the order the signal runs: the instrument, the hosted
    // plugins, then the built-in effects that are on (always in this order: the strip's is
    // fixed), an "Add effect" slot, and where it goes next.
    let active: Vec<FxSection> = FxSection::SIGNAL_ORDER
        .into_iter()
        .filter(|sec| sec.enabled(&fx))
        .collect();
    let mut list = col()
        .key("fx-sections")
        .w_full()
        .items_center()
        .gap(2.0)
        .padding(8.0)
        .child(endpoint("fx-chain-in", "In", chain_source(track)))
        .child(link())
        .child(crate::ui::song::io::plugins_section(s, track));
    let mut filter_body = Some(filter_body);
    for sec in &active {
        let body = if *sec == FxSection::Filter {
            filter_body.take().unwrap_or_default()
        } else {
            knobs(s, track, *sec)
        };
        list = list.child(link()).child(section(s, track, *sec, body));
    }
    let all_on = active.len() == FxSection::SIGNAL_ORDER.len();
    let add_open = s.menu == Some(Menu::Song(SongMenu::AddEffect));
    let mut add = row()
        .key("fx-add")
        .role("button")
        .label("Add effect")
        .w_full()
        .h(32.0)
        .center()
        .gap(6.0)
        .radius(R_LG)
        .border(1.0, if add_open { th.input } else { th.border })
        .child(icon(&icons::PLUS, 14.0).color(th.primary))
        .child(
            text(if all_on {
                "Every effect is in the chain"
            } else {
                "Add effect"
            })
            .font(ui(12.0, 600.0))
            .color(th.fg),
        );
    add = if all_on {
        add.opacity(0.45).disabled(true)
    } else {
        add.focusable(true)
            .hover(th.surface3)
            .on_press(|s: &mut App| s.toggle_menu(Menu::Song(SongMenu::AddEffect)))
    };
    list = list.child(link()).child(add).child(link()).child(endpoint(
        "fx-chain-out",
        "Out",
        "Pan, volume, mixer",
    ));
    col()
        .key("track-fx")
        .w(336.0)
        .h_full()
        .fill(th.surface1)
        .border_sides([0.0, 0.0, 0.0, 1.0], th.border)
        .box_shadow(rune::Shadow {
            x: -12.0,
            y: 0.0,
            blur: 32.0,
            spread: -12.0,
            color: Color::BLACK.alpha(0.5),
            inset: false,
        })
        .child(
            row()
                .h(40.0)
                .w_full()
                .items_center()
                .gap(8.0)
                .px(12.0)
                .border_sides([0.0, 0.0, 1.0, 0.0], th.border)
                .child(
                    el().w(4.0)
                        .h(16.0)
                        .radius(2.0)
                        .fill(track_color(&track.color)),
                )
                .child(
                    col()
                        .min_w(0.0)
                        .child(
                            // `daw-label leading-none`, the name `leading-tight`.
                            text("Track FX")
                                .font(daw_label(11.0).lh(11.0))
                                .text_case(rune::TextCase::Uppercase)
                                .color(th.muted_fg),
                        )
                        .child(
                            text(track.name.clone())
                                .key("fx-track-name")
                                .font(ui(13.0, 600.0).lh(16.25))
                                .color(th.fg)
                                .truncate(),
                        ),
                )
                .child(spacer())
                .child(
                    Button::new(Variant::Ghost, Size::IconSm)
                        .icon(&icons::X)
                        .a11y("Close track effects")
                        .key("fx-close")
                        .build(|s: &mut App, _| s.song.fx_track = None),
                ),
        )
        .child(
            col()
                .grow(1.0)
                .min_h(0.0)
                .w_full()
                .child(list)
                .scroll(rune::ScrollSpec {
                    stick_bottom: false,
                }),
        )
}

fn choice_menu<T: Copy + PartialEq + 'static>(
    anchor: &str,
    key: &str,
    items: &[(T, &'static str, &'static str)],
    current: T,
    effect: EffectName,
    field: &'static str,
) -> El {
    let th = t();
    let mut list = popover_frame(None, 4.0).key(key.to_owned()).min_w(96.0);
    for (v, id, label) in items {
        let sel = *v == current;
        let id = *id;
        list = list.child(
            select_option(
                format!("{key}-{id}"),
                row()
                    .child(text(*label).font(ui(12.0, 400.0)).color(if sel {
                        th.primary
                    } else {
                        th.fg
                    }))
                    .child(spacer())
                    .child(if sel {
                        icon(&icons::CHECK, 14.0).color(th.primary)
                    } else {
                        el().size(14.0, 14.0)
                    }),
                sel,
            )
            .on_press(move |s: &mut App| {
                if let Some(tid) = s.song.fx_track.clone() {
                    let mut m = Map::new();
                    m.insert(field.into(), Value::String(id.into()));
                    s.set_track_effect(&tid, effect, m);
                }
                s.menu = None;
            }),
        );
    }
    select_popup(anchor, Align::Start, list)
}

pub fn filter_type_menu(s: &App) -> El {
    let cur = s
        .song
        .fx_track
        .as_ref()
        .and_then(|id| s.find_track(id))
        .and_then(|t| t.effects.clone())
        .unwrap_or_default()
        .filter
        .kind;
    choice_menu(
        "fx-filter-type",
        "filter-type-menu",
        &FILTER_TYPES,
        cur,
        EffectName::Filter,
        "type",
    )
}

pub fn lfo_wave_menu(s: &App) -> El {
    let cur = s
        .song
        .fx_track
        .as_ref()
        .and_then(|id| s.find_track(id))
        .and_then(|t| t.effects.clone())
        .unwrap_or_default()
        .filter
        .lfo_wave;
    choice_menu(
        "fx-lfo-wave",
        "lfo-wave-menu",
        &LFO_WAVES,
        cur,
        EffectName::Filter,
        "lfoWave",
    )
}

/// Mono helper re-export for the mixer.
pub fn mono_text(s: String, size: f32) -> El {
    text(s).font(mono(size, 400.0))
}
