//! The Samples page: the Sound Designer (state and actions: `crate::designer`).
//!
//! Left: the library (search, the templates to start from, your sounds and imported audio).
//! Right: the editor — name and status, the live waveform, the action bar (Play, Save, Send to
//! pad, Add to song), then the synth (three oscillator strips; envelope, filter, character and
//! output sections) or, for an imported file, one edit panel.

use mdaw_dsp::sound_design::envelope_curve;
use mdaw_model::beat_helpers::{from_log01, pad_name, to_log01};
use mdaw_model::fs_entries::AUDIO_EXTENSIONS;
use mdaw_model::sound_design::{
    DesignedSound, FilterMode, LAYERS, SoundRecipe, Template, Wave, note_name,
};
use rune::{
    Align, Cx, El, Icon, LineCap, Shape, Tx, canvas, col, el, icon, row, spacer, text, text_input,
};

use crate::app::App;
use crate::designer::{Kind, kind_of, listed};
use crate::icons;
use crate::icons_views;
use crate::samples::{column_peaks, list_duration};
use crate::theme::{R_LG, R_MD, daw_label, mono, t, ui};
use crate::ui::knob::{KnobId, KnobSpec, knob_keyed};
use crate::ui::p9::kit::{kbd, switch};
use crate::ui::widgets::{Button, Segmented, Size, Variant, popover, popover_frame};
use crate::views::P9Menu;

const LIB_W: f32 = 280.0;
const WAVE_H: f32 = 148.0;

// ---------------------------------------------------------------- parameters

/// How a knob maps to a parameter.
#[derive(Clone, Copy)]
enum Scale {
    Lin {
        min: f64,
        max: f64,
        step: f64,
    },
    /// The knob runs 0..1, the value `min · (max / min)^k`.
    Log {
        min: f64,
        max: f64,
    },
}

#[derive(Clone, Copy)]
enum Unit {
    Pct,
    Semis,
    Cents,
    Sec,
    Hz,
    Oct,
    Db,
    Note,
    Bits,
    Count,
    Plain,
}

/// Knob ids: 1000 + layer·10 + k (osc), 1100 + k (amp), 1200 + k (filter), 1300 + k
/// (character / output), 1400 + k (sample edits). See [`def`].
pub const BASE: u16 = 1000;

struct Def {
    label: &'static str,
    scale: Scale,
    unit: Unit,
    /// Default value (in parameter units).
    default: f64,
}

fn def(n: u16, file_secs: f64) -> Def {
    let lin = |min, max, step| Scale::Lin { min, max, step };
    let log = |min, max| Scale::Log { min, max };
    let d = |label, scale, unit, default| Def {
        label,
        scale,
        unit,
        default,
    };
    let dur = file_secs.max(0.001);
    match n {
        1000..=1029 => match (n - 1000) % 10 {
            0 => d("Level", lin(0.0, 1.0, 0.01), Unit::Pct, 0.8),
            1 => d("Coarse", lin(-48.0, 48.0, 1.0), Unit::Semis, 0.0),
            2 => d("Fine", lin(-100.0, 100.0, 1.0), Unit::Cents, 0.0),
            3 => d("Width", lin(0.05, 0.95, 0.01), Unit::Pct, 0.5),
            4 => d("P.Env", lin(-48.0, 48.0, 1.0), Unit::Semis, 0.0),
            _ => d("P.Dec", log(0.001, 2.0), Unit::Sec, 0.05),
        },
        1100 => d("Attack", log(0.0005, 2.0), Unit::Sec, 0.001),
        1101 => d("Hold", lin(0.0, 1.0, 0.001), Unit::Sec, 0.0),
        1102 => d("Decay", log(0.001, 4.0), Unit::Sec, 0.3),
        1103 => d("Sustain", lin(0.0, 1.0, 0.01), Unit::Pct, 0.0),
        1104 => d("Release", log(0.001, 4.0), Unit::Sec, 0.05),
        1105 => d("Curve", lin(-1.0, 1.0, 0.01), Unit::Plain, 0.5),
        1106 => d("Retrig", lin(0.0, 4.0, 1.0), Unit::Count, 0.0),
        1200 => d("Cutoff", log(20.0, 20000.0), Unit::Hz, 20000.0),
        1201 => d("Reso", lin(0.0, 1.0, 0.01), Unit::Pct, 0.0),
        1202 => d("Env", lin(-8.0, 8.0, 0.1), Unit::Oct, 0.0),
        1203 => d("Env Dec", log(0.001, 4.0), Unit::Sec, 0.2),
        1300 => d("Drive", lin(0.0, 1.0, 0.01), Unit::Pct, 0.0),
        1301 => d("Crush", lin(0.0, 14.0, 1.0), Unit::Bits, 0.0),
        1302 => d("Click", lin(0.0, 1.0, 0.01), Unit::Pct, 0.0),
        1303 => d("FM", lin(0.0, 1.0, 0.01), Unit::Pct, 0.0),
        1304 => d("Pitch", lin(24.0, 96.0, 1.0), Unit::Note, 60.0),
        1305 => d("Length", log(0.02, 4.0), Unit::Sec, 0.6),
        1306 => d("Gain", lin(-24.0, 12.0, 0.1), Unit::Db, 0.0),
        1400 => d("Start", lin(0.0, dur, 0.001), Unit::Sec, 0.0),
        1401 => d("End", lin(0.0, dur, 0.001), Unit::Sec, dur),
        1402 => d("Fade in", lin(0.0, dur, 0.001), Unit::Sec, 0.0),
        1403 => d("Fade out", lin(0.0, dur, 0.001), Unit::Sec, 0.0),
        _ => d("Gain", lin(-24.0, 24.0, 0.1), Unit::Db, 0.0),
    }
}

/// The parameter's value (parameter units).
fn get(sd: &DesignedSound, n: u16, file_secs: f64) -> f64 {
    let r = &sd.recipe;
    match n {
        1000..=1029 => {
            let l = &r.layers[((n - 1000) / 10) as usize];
            match (n - 1000) % 10 {
                0 => l.level,
                1 => l.coarse,
                2 => l.fine,
                3 => l.width,
                4 => l.pitch_amount,
                _ => l.pitch_decay,
            }
        }
        1100 => r.amp.attack,
        1101 => r.amp.hold,
        1102 => r.amp.decay,
        1103 => r.amp.sustain,
        1104 => r.amp.release,
        1105 => r.amp.curve,
        1106 => r.amp.retrig as f64,
        1200 => r.filter.cutoff,
        1201 => r.filter.resonance,
        1202 => r.filter.env_amount,
        1203 => r.filter.env_decay,
        1300 => r.drive,
        // Knob 0 = off, 1..14 = 16..3 bits... shown as the bit depth.
        1301 => {
            if r.crush >= 2.0 {
                16.0 - r.crush + 1.0
            } else {
                0.0
            }
        }
        1302 => r.click,
        1303 => r.fm.amount,
        1304 => r.note,
        1305 => r.length,
        1306 => r.gain_db,
        _ => {
            let Some(e) = &sd.sample else { return 0.0 };
            match n {
                1400 => e.start,
                1401 => e.end.unwrap_or(file_secs),
                1402 => e.fade_in,
                1403 => e.fade_out,
                _ => e.gain_db,
            }
        }
    }
}

fn set(sd: &mut DesignedSound, n: u16, v: f64) {
    let r: &mut SoundRecipe = &mut sd.recipe;
    match n {
        1000..=1029 => {
            let l = &mut r.layers[((n - 1000) / 10) as usize];
            match (n - 1000) % 10 {
                0 => l.level = v,
                1 => l.coarse = v,
                2 => l.fine = v,
                3 => l.width = v,
                4 => l.pitch_amount = v,
                _ => l.pitch_decay = v,
            }
        }
        1100 => r.amp.attack = v,
        1101 => r.amp.hold = v,
        1102 => r.amp.decay = v,
        1103 => r.amp.sustain = v,
        1104 => r.amp.release = v,
        1105 => r.amp.curve = v,
        1106 => r.amp.retrig = v.round().clamp(0.0, 4.0) as u8,
        1200 => r.filter.cutoff = v,
        1201 => r.filter.resonance = v,
        1202 => r.filter.env_amount = v,
        1203 => r.filter.env_decay = v,
        1300 => r.drive = v,
        1301 => r.crush = if v < 0.5 { 0.0 } else { 17.0 - v.round() },
        1302 => r.click = v,
        1303 => r.fm.amount = v,
        1304 => r.note = v,
        1305 => r.length = v,
        1306 => r.gain_db = v,
        _ => {
            let Some(e) = sd.sample.as_mut() else { return };
            match n {
                1400 => e.start = v,
                1401 => e.end = Some(v),
                1402 => e.fade_in = v,
                1403 => e.fade_out = v,
                _ => e.gain_db = v,
            }
        }
    }
}

fn to_knob(df: &Def, v: f64) -> f64 {
    match df.scale {
        Scale::Lin { .. } => v,
        Scale::Log { min, max } => to_log01(v, min, max),
    }
}

fn from_knob(df: &Def, k: f64) -> f64 {
    match df.scale {
        Scale::Lin { .. } => k,
        Scale::Log { min, max } => from_log01(k, min, max),
    }
}

fn fmt(df: &Def, v: f64) -> String {
    match df.unit {
        Unit::Pct => format!("{}%", (v * 100.0).round()),
        Unit::Semis => format!("{}{} st", if v > 0.0 { "+" } else { "" }, v.round()),
        Unit::Cents => format!("{}{}¢", if v > 0.0 { "+" } else { "" }, v.round()),
        Unit::Sec => {
            if v < 1.0 {
                format!("{}ms", (v * 1000.0).round())
            } else {
                format!("{v:.2}s")
            }
        }
        Unit::Hz => {
            if v >= 1000.0 {
                format!("{:.1}k", v / 1000.0)
            } else {
                format!("{}Hz", v.round())
            }
        }
        Unit::Oct => format!("{}{v:.1} oct", if v > 0.0 { "+" } else { "" }),
        Unit::Db => format!("{}{v:.1} dB", if v > 0.0 { "+" } else { "" }),
        Unit::Note => note_name(v),
        Unit::Bits => {
            if v < 0.5 {
                "Off".into()
            } else {
                format!("{} bit", 17.0 - v.round())
            }
        }
        Unit::Count => format!("{}", v.round()),
        Unit::Plain => format!("{v:.2}"),
    }
}

/// The knob position of designer knob `n` (`KnobId::View(n)`, `n ≥ 1000`).
pub fn knob_value(s: &App, n: u16) -> f64 {
    let secs = s.draft_file_seconds();
    s.p9.designer
        .draft_sound()
        .map_or(0.0, |d| to_knob(&def(n, secs), get(d, n, secs)))
}

/// A designer knob.
fn kn(s: &App, n: u16, enabled: bool) -> El {
    let secs = s.draft_file_seconds();
    let df = def(n, secs);
    let v =
        s.p9.designer
            .draft_sound()
            .map_or(df.default, |d| get(d, n, secs));
    let (min, max, step) = match df.scale {
        Scale::Lin { min, max, step } => (min, max, step),
        Scale::Log { .. } => (0.0, 1.0, 0.001),
    };
    let spec = KnobSpec {
        id: KnobId::View(n),
        label: df.label,
        min,
        max,
        step,
        default: to_knob(&df, df.default),
        small: true,
        hide_text: false,
    };
    let k = knob_keyed(
        spec,
        format!("sd-k-{n}"),
        to_knob(&df, v),
        fmt(&df, v),
        "",
        move |s: &mut App, kv| {
            let secs = s.draft_file_seconds();
            let df = def(n, secs);
            let v = from_knob(&df, kv);
            s.edit_sound(|d| set(d, n, v));
        },
        // Releasing a knob plays the result.
        Some(|s: &mut App| s.play_sound()),
        s,
    );
    let w = col().w(54.0).items_center().child(k);
    if enabled { w } else { w.opacity(0.35) }
}

// ---------------------------------------------------------------- pieces

fn section_label(s: &str) -> El {
    text(s.to_owned())
        .font(daw_label(10.0).tracking(0.9))
        .text_case(rune::TextCase::Uppercase)
        .color(t().muted_fg)
}

fn panel(key: &str, title: &str, extra: Option<El>, body: El) -> El {
    let th = t();
    let mut head = row()
        .w_full()
        .h(22.0)
        .items_center()
        .gap(8.0)
        .child(section_label(title));
    if let Some(e) = extra {
        head = head.child(spacer()).child(e);
    }
    col()
        .key(key.to_owned())
        .role("group")
        .label(title.to_owned())
        .gap(8.0)
        .padding(12.0)
        .radius(R_LG)
        .border(1.0, th.border)
        .fill(th.surface1)
        .child(head)
        .child(body)
}

fn knobs(items: Vec<El>) -> El {
    let mut r = row().items_start().gap(2.0);
    for i in items {
        r = r.child(i);
    }
    r
}

fn header() -> El {
    let th = t();
    row()
        .h(48.0)
        .w_full()
        .items_center()
        .gap(12.0)
        .px(16.0)
        .fill(th.surface1)
        .border_sides([0.0, 0.0, 1.0, 0.0], th.border)
        .child(icon(&icons::AUDIO_LINES, 16.0).color(th.muted_fg))
        .child(text("Sound Designer").font(ui(14.0, 600.0)).color(th.fg))
        .child(
            text(
                "Design drums and one-shots from waveforms, then put them on a pad or in the song",
            )
            .font(ui(12.0, 400.0))
            .color(th.muted_fg),
        )
        .child(spacer())
        .child(
            Button::new(Variant::Outline, Size::Sm)
                .icon(&icons::PLUS)
                .icon_size(14.0)
                .label("Import audio")
                .font_size(12.0)
                .px(10.0)
                .key("samples-import")
                .tip("Import audio files (or drop them anywhere here)", None)
                .build(|s: &mut App, tx: &mut Tx| s.sample_import_pick(tx)),
        )
}

// ---------------------------------------------------------------- library

fn templates(s: &App) -> El {
    let th = t();
    let mut g = row().w_full().flex_wrap().gaps(4.0, 4.0);
    for tp in Template::ALL {
        let on =
            s.p9.designer
                .draft
                .as_ref()
                .is_some_and(|d| !s.draft_is_stored() && d.sound.name.starts_with(tp.name()));
        let mut b = row()
            .key(format!(
                "sd-tpl-{}",
                tp.name().to_lowercase().replace(' ', "-")
            ))
            .role("button")
            .label(format!("New {}", tp.name()))
            .h(26.0)
            .px(8.0)
            .items_center()
            .radius(R_MD)
            .border(1.0, if on { th.primary.alpha(0.5) } else { th.border })
            .fill(if on {
                th.primary.alpha(0.12)
            } else {
                th.surface2
            })
            .child(text(tp.name()).font(ui(11.5, 500.0)).color(if on {
                th.fg
            } else {
                th.fg.alpha(0.85)
            }))
            .on_press(move |s: &mut App| s.pick_template(tp, true));
        if !on {
            b = b.hover(th.surface3);
        }
        g = g.child(b);
    }
    g
}

fn lib_row(s: &App, d: &DesignedSound) -> El {
    let th = t();
    let sel = s.p9.designer.selected_id() == Some(d.id.as_str());
    let dirty = sel && s.p9.designer.draft.as_ref().is_some_and(|x| x.dirty);
    let users = s.svc.engine.doc().sound_users(&d.id).len();
    let dur =
        s.p9.samples
            .buffer(&d.id)
            .map(|b| b.frames() as f64 / b.sample_rate.max(1) as f64);
    let (ic, kind): (&'static Icon, &str) = match kind_of(d) {
        Kind::Synth => (&icons::ZAP, "Synth"),
        Kind::Sample => (&icons::FILE_AUDIO, "Audio"),
    };
    let mut meta = kind.to_owned();
    if let Some(x) = dur {
        meta.push_str(&format!(" · {}", list_duration(x)));
    }
    if users > 0 {
        meta.push_str(&format!(
            " · {users} pad{}",
            if users == 1 { "" } else { "s" }
        ));
    }
    let id = d.id.clone();
    let mut r = row()
        .key(format!("sd-sound-{}", d.id))
        .role("button")
        .label(d.name.clone())
        .checked(sel)
        .w_full()
        .items_center()
        .gap(10.0)
        .px(10.0)
        .py(7.0)
        .radius(R_MD)
        .child(
            row()
                .size(26.0, 26.0)
                .center()
                .radius(R_MD)
                .fill(if sel {
                    th.primary.alpha(0.18)
                } else {
                    th.surface2
                })
                .child(icon(ic, 13.0).color(if sel { th.primary } else { th.muted_fg })),
        )
        .child(
            col()
                .min_w(0.0)
                .grow(1.0)
                .child(
                    text(d.name.clone())
                        .font(ui(13.0, 500.0))
                        .color(if sel { th.fg } else { th.fg.alpha(0.85) })
                        .truncate(),
                )
                .child(text(meta).font(mono(10.0, 400.0)).color(th.muted_fg)),
        )
        .on_press(move |s: &mut App| s.open_sound(&id, true));
    if dirty {
        r = r.child(
            el().size(6.0, 6.0)
                .radius(3.0)
                .fill(th.primary)
                .help("Unsaved edits"),
        );
    }
    if sel {
        r.fill(th.surface3)
    } else {
        r.hover(th.surface2)
    }
}

fn library(s: &App) -> El {
    let th = t();
    let list = listed(s);
    let mut rows = col().w_full().gap(1.0);
    if list.is_empty() {
        let msg = if s.p9.designer.search.text.trim().is_empty() {
            "Your sounds appear here. Start from a template above, or import audio files."
        } else {
            "No sound matches the search."
        };
        rows = rows.child(
            text(msg)
                .font(ui(12.0, 400.0).lh(18.0))
                .color(th.muted_fg)
                .wrap()
                .px(10.0)
                .py(12.0),
        );
    }
    for d in &list {
        rows = rows.child(lib_row(s, d));
    }
    let search = row()
        .h(30.0)
        .w_full()
        .px(8.0)
        .gap(6.0)
        .items_center()
        .radius(R_MD)
        .border(1.0, th.input)
        .fill(th.surface0)
        .child(icon(&icons::SEARCH, 13.0).color(th.muted_fg))
        .child(
            text_input(&s.p9.designer.search, ui(12.0, 400.0))
                .key("designer-search")
                .label("Search sounds")
                .placeholder("Search sounds", th.muted_fg)
                .single_line()
                .color(th.fg)
                .grow(1.0)
                .on_edit(|s: &mut App| &mut s.p9.designer.search),
        );
    col()
        .key("sample-library")
        .label("Sound library")
        .w(LIB_W)
        .h_full()
        .fill(th.surface1)
        .border_sides([0.0, 1.0, 0.0, 0.0], th.border)
        .child(
            col()
                .w_full()
                .gap(8.0)
                .padding(12.0)
                .border_sides([0.0, 0.0, 1.0, 0.0], th.border)
                .child(section_label("Start from"))
                .child(templates(s)),
        )
        .child(
            row()
                .w_full()
                .h(34.0)
                .px(12.0)
                .items_center()
                .child(section_label("My sounds"))
                .child(spacer())
                .child(
                    text(all_count(s).to_string())
                        .font(mono(10.5, 400.0))
                        .color(th.muted_fg),
                ),
        )
        .child(row().w_full().px(12.0).pb(8.0).child(search))
        .child(
            col()
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .child(col().w_full().px(6.0).pb(8.0).child(rows))
                .scroll(rune::ScrollSpec {
                    stick_bottom: false,
                }),
        )
}

fn all_count(s: &App) -> usize {
    crate::designer::all_sounds(s).len()
}

// ---------------------------------------------------------------- editor

fn title_row(s: &App, d: &DesignedSound) -> El {
    let th = t();
    let stored = s.draft_is_stored();
    let dirty = s.p9.designer.draft.as_ref().is_some_and(|x| x.dirty);
    let (status, color) = if !stored {
        ("New — not saved", th.warn)
    } else if dirty {
        ("Edited", th.primary)
    } else {
        ("Saved", th.muted_fg)
    };
    let name: El = match &s.p9.designer.rename {
        Some(edit) => row()
            .h(32.0)
            .w(300.0)
            .px(8.0)
            .items_center()
            .radius(R_MD)
            .border(1.0, th.input)
            .fill(th.surface0)
            .child(
                text_input(edit, ui(15.0, 500.0))
                    .key("designer-rename")
                    .label("Sound name")
                    .single_line()
                    .color(th.fg)
                    .grow(1.0)
                    .on_edit(|s: &mut App| {
                        s.p9.designer
                            .rename
                            .get_or_insert_with(|| rune::TextEdit::new(""))
                    })
                    .on_submit(|s: &mut App, _tx: &mut Tx| s.commit_sound_rename()),
            ),
        None => row()
            .min_w(0.0)
            .items_center()
            .gap(6.0)
            .child(
                text(d.name.clone())
                    .font(ui(18.0, 600.0).tracking(-0.3))
                    .color(th.fg)
                    .truncate(),
            )
            .child(
                Button::new(Variant::Ghost, Size::IconXs)
                    .icon(&crate::icons_song::PENCIL)
                    .a11y("Rename sound")
                    .tip("Rename", None)
                    .key("sd-rename")
                    .build(|s: &mut App, _| s.begin_sound_rename()),
            ),
    };
    row()
        .w_full()
        .items_center()
        .gap(12.0)
        .child(name)
        .child(
            row()
                .h(20.0)
                .px(8.0)
                .items_center()
                .radius(10.0)
                .border(1.0, color.alpha(0.4))
                .child(
                    text(status)
                        .key("sd-status")
                        .font(ui(11.0, 500.0))
                        .color(color),
                ),
        )
        .child(spacer())
        .child(
            Button::new(Variant::Ghost, Size::Sm)
                .icon(&icons_views::FILE_PLUS)
                .icon_size(14.0)
                .label("Duplicate")
                .font_size(12.0)
                .key("sd-duplicate")
                .build(|s: &mut App, _| s.duplicate_sound()),
        )
        .child(
            Button::new(Variant::Ghost, Size::Sm)
                .icon(&icons::TRASH_2)
                .icon_size(14.0)
                .label("Delete")
                .font_size(12.0)
                .ink(th.destructive)
                .disabled(!stored)
                .key("sd-delete")
                .build(|s: &mut App, _| s.delete_sound()),
        )
}

fn waveform(s: &App, cx: &mut Cx<App>) -> El {
    let th = t();
    let w = cx
        .layout_size("sd-waveform")
        .map_or(s.p9.designer.wave_w.get(), |z| z[0])
        .max(10.0);
    s.p9.designer.wave_w.set(w);
    let h = WAVE_H;
    let mid = (h / 2.0).round();
    let rect =
        |x: f32, y: f32, ww: f32, hh: f32| vec![[x, y], [x + ww, y], [x + ww, y + hh], [x, y + hh]];
    let mut shape = Shape::new().fill(rect(0.0, mid, w, 1.0), th.border);
    let mut caption = String::new();
    if let Some(b) = &s.p9.designer.preview {
        let cols = w.ceil() as usize;
        let peaks = column_peaks(b, cols);
        let top = peaks.iter().fold(0f32, |m, p| m.max(*p));
        // Over full scale: drawn to the edge, the clip shown.
        let amp = mid * 0.92;
        let mut pts = vec![[0.0, mid]];
        for (i, p) in peaks.iter().enumerate() {
            pts.push([i as f32, mid - p.min(1.0) * amp]);
        }
        for (i, p) in peaks.iter().enumerate().rev() {
            pts.push([i as f32, mid + p.min(1.0) * amp]);
        }
        shape = shape.fill(pts, th.primary.alpha(0.9));
        let secs = b.frames() as f64 / b.sample_rate.max(1) as f64;
        let db = if top > 0.0 { 20.0 * top.log10() } else { -96.0 };
        caption = format!("{} · peak {db:+.1} dB", list_duration(secs));
        if top > 1.0 {
            caption.push_str(" · clipping");
        }
    }
    let cursor = s.p9.designer.audition.is_some().then(|| {
        el().key("sd-cursor")
            .absolute(0.0, 0.0)
            .size(1.0, h)
            .fill(th.fg)
            .inert()
            .live_offset(&s.p9.designer.audition_x)
    });
    col()
        .w_full()
        .gap(4.0)
        .child(
            el().key("sd-waveform")
                .role("button")
                .label("Waveform: click to play")
                .w_full()
                .h(h)
                .radius(R_LG)
                .border(1.0, th.border)
                .fill(th.surface0)
                .clip_rounded(R_LG)
                .child(canvas(shape).absolute(0.0, 0.0).size(w, h))
                .children(cursor)
                .on_press(|s: &mut App| s.play_sound()),
        )
        .child(
            row()
                .w_full()
                .child(text(caption).font(mono(10.0, 400.0)).color(th.muted_fg))
                .child(spacer())
                .child(
                    row()
                        .items_center()
                        .gap(6.0)
                        .child(kbd("Space"))
                        .child(text("plays").font(ui(11.0, 400.0)).color(th.muted_fg))
                        .child(el().w(6.0))
                        .child(kbd("MIDI"))
                        .child(
                            text("keys play it at pitch (C4 = as designed)")
                                .font(ui(11.0, 400.0))
                                .color(th.muted_fg),
                        ),
                ),
        )
}

fn action_bar(s: &App) -> El {
    let th = t();
    let playing = s.p9.designer.audition.is_some();
    let dirty = s.p9.designer.draft.as_ref().is_some_and(|d| d.dirty);
    let stored = s.draft_is_stored();
    let target = s.send_target();
    let send_label = match &target {
        Some((_, p)) => format!("Send to pad {}", p + 1),
        None => "Send to pad".to_owned(),
    };
    let play = Button::new(Variant::Outline, Size::Md)
        .icon(if playing {
            &icons::SQUARE_FILLED
        } else {
            &icons::PLAY_FILLED
        })
        .icon_size(16.0)
        .label(if playing { "Stop" } else { "Play" })
        .weight(600.0)
        .px(18.0)
        .pressed(playing)
        .key("sd-play")
        .tip("Play the sound", Some("Space"))
        .build(|s: &mut App, _| s.toggle_sound_play());
    let save = Button::new(Variant::Outline, Size::Md)
        .icon(&icons_views::SAVE)
        .label("Save")
        .disabled(stored && !dirty)
        .key("sd-save")
        .tip("Save to the project and your library", None)
        .build(|s: &mut App, _| s.save_sound());
    let send = row()
        .items_center()
        .child(
            Button::new(Variant::Primary, Size::Md)
                .icon(&icons::DRUM)
                .label(send_label)
                .key("sd-send")
                .tip(
                    "Put this sound on the pad (re-editing it later updates the pad)",
                    None,
                )
                .build(|s: &mut App, _| s.send_sound_to_pad()),
        )
        .child(el().w(1.0).h(36.0).fill(th.primary_fg.alpha(0.25)))
        .child(
            Button::new(Variant::Primary, Size::Icon)
                .icon(&icons::CHEVRON_DOWN)
                .a11y("Choose the pad")
                .key("sd-send-pick")
                .build(|s: &mut App, _| s.toggle_p9_menu(P9Menu::SendToPad)),
        );
    let song = Button::new(Variant::Outline, Size::Md)
        .icon(&icons::MUSIC)
        .label("Add to song")
        .key("sd-song")
        .tip("A new audio track with this sound at the start", None)
        .build(|s: &mut App, _| s.add_sound_to_song());
    let mut bar = row()
        .key("sd-actions")
        .w_full()
        .items_center()
        .gap(8.0)
        .child(play)
        .child(save)
        .child(spacer())
        .child(send)
        .child(song);
    if let Some((msg, err, _)) = &s.p9.designer.notice {
        bar = bar.child(
            text(msg.clone())
                .key("sd-notice")
                .font(ui(12.0, 500.0))
                .color(if *err { th.destructive } else { th.play }),
        );
    }
    bar
}

/// The Send-to-pad picker: the drum track (when there are several) and its 16 pads.
pub fn send_menu(s: &App) -> El {
    let th = t();
    let doc = s.svc.engine.doc();
    let (track, pad) = s.send_target().unwrap_or_default();
    let drums: Vec<(String, String)> = doc
        .get_tracks()
        .iter()
        .filter(|t| t.instrument.kit.is_some())
        .map(|t| (t.id.clone(), t.name.clone()))
        .collect();
    let mut body = popover_frame(Some(296.0), 10.0)
        .gap(8.0)
        .child(section_label("Send to pad"));
    if drums.len() > 1 {
        let mut tr = row().w_full().flex_wrap().gaps(4.0, 4.0);
        for (id, name) in drums {
            let on = id == track;
            let tid = id.clone();
            tr = tr.child(
                row()
                    .key(format!("sd-send-track-{id}"))
                    .role("radio")
                    .label(name.clone())
                    .checked(on)
                    .h(24.0)
                    .px(8.0)
                    .items_center()
                    .radius(R_MD)
                    .fill(if on { th.surface3 } else { th.surface2 })
                    .child(text(name).font(ui(11.0, 500.0)).color(th.fg))
                    .on_press(move |s: &mut App| s.p9.designer.send_track = Some(tid.clone())),
            );
        }
        body = body.child(tr);
    }
    let kit = doc.kit(&track).cloned();
    let mut grid = col().w_full().gap(4.0);
    // Pad 13–16 on top, like the hardware.
    for r in (0..4).rev() {
        let mut line = row().w_full().gap(4.0);
        for c in 0..4 {
            let p = r * 4 + c;
            let name = pad_name(kit.as_ref().and_then(|k| k.pads.get(p)));
            let on = p == pad;
            let tid = track.clone();
            line = line.child(
                col()
                    .key(format!("sd-send-pad-{p}"))
                    .role("button")
                    .label(format!("Pad {}", p + 1))
                    .grow(1.0)
                    .h(48.0)
                    .padding(5.0)
                    .radius(R_MD)
                    .border(1.0, if on { th.primary } else { th.border })
                    .fill(if on {
                        th.primary.alpha(0.12)
                    } else {
                        th.surface2
                    })
                    .hover(th.surface3)
                    .child(
                        text(format!("{}", p + 1))
                            .font(mono(10.0, 600.0))
                            .color(if on { th.primary } else { th.muted_fg }),
                    )
                    .child(
                        text(if name.is_empty() { "—".into() } else { name })
                            .font(ui(10.0, 400.0))
                            .color(th.fg.alpha(0.8))
                            .truncate(),
                    )
                    .on_press(move |s: &mut App| {
                        s.p9.designer.send_track = Some(tid.clone());
                        s.p9.designer.send_pad = Some(p);
                        s.send_sound_to_pad();
                    }),
            );
        }
        grid = grid.child(line);
    }
    popover(
        "sd-send-pick",
        Align::End,
        body.child(grid)
            .key("sd-send-menu")
            .role("dialog")
            .label("Send to pad"),
    )
}

fn layer_strip(s: &App, i: usize) -> El {
    let th = t();
    let d =
        s.p9.designer
            .draft_sound()
            .cloned()
            .unwrap_or_else(|| DesignedSound {
                id: String::new(),
                name: String::new(),
                recipe: SoundRecipe::default(),
                sample: None,
            });
    let l = d.recipe.layers[i];
    let base = BASE + i as u16 * 10;
    let waves = Segmented::new(
        format!("Oscillator {} waveform", i + 1),
        l.wave,
        Wave::ALL
            .iter()
            .map(|w| (*w, w.label().to_owned()))
            .collect(),
    )
    .sans()
    .size(11.0)
    .item_px(7.0)
    .key(format!("sd-seg-wave-{i}"))
    .build(move |s: &mut App, w| s.edit_recipe(|r| r.layers[i].wave = w));
    let tonal = !l.wave.is_noise();
    let fm = d.recipe.fm;
    let fm_tag = (fm.amount > 0.0 && l.on).then_some(if fm.from as usize == i {
        "FM →"
    } else if fm.to as usize == i {
        "FM ←"
    } else {
        ""
    });
    let mut head = col()
        .w(244.0)
        .gap(8.0)
        .child(
            row()
                .items_center()
                .gap(8.0)
                .child(switch(
                    &format!("sd-osc-on-{i}"),
                    &format!("Oscillator {}", i + 1),
                    l.on,
                    move |s: &mut App, v| s.edit_recipe(|r| r.layers[i].on = v),
                ))
                .child(
                    text(format!("OSC {}", i + 1))
                        .font(daw_label(11.0).tracking(0.9))
                        .color(if l.on { th.fg } else { th.muted_fg }),
                )
                .children(
                    fm_tag
                        .filter(|t| !t.is_empty())
                        .map(|t| text(t).font(mono(10.0, 600.0)).color(th.primary)),
                ),
        )
        .child(waves);
    if !l.on {
        head = head.opacity(0.75);
    }
    let body = knobs(vec![
        kn(s, base, l.on),
        kn(s, base + 1, l.on && tonal),
        kn(s, base + 2, l.on && tonal),
        kn(s, base + 3, l.on && l.wave == Wave::Pulse),
        kn(s, base + 4, l.on && tonal),
        kn(s, base + 5, l.on && tonal && l.pitch_amount != 0.0),
    ]);
    row()
        .key(format!("sd-osc-{i}"))
        .role("group")
        .label(format!("Oscillator {}", i + 1))
        .w_full()
        .items_center()
        .gap(16.0)
        .px(12.0)
        .py(8.0)
        .radius(R_LG)
        .border(1.0, th.border)
        .fill(if l.on { th.surface1 } else { th.surface0 })
        .child(head)
        .child(body)
}

fn envelope_graph(r: &SoundRecipe) -> El {
    let th = t();
    let (w, h) = (150.0f32, 56.0f32);
    let pts: Vec<[f32; 2]> = envelope_curve(r, 96)
        .iter()
        .enumerate()
        .map(|(i, v)| [i as f32 / 95.0 * w, h - 2.0 - *v as f32 * (h - 4.0)])
        .collect();
    let mut fill = pts.clone();
    fill.push([w, h]);
    fill.push([0.0, h]);
    let shape = Shape::new().fill(fill, th.primary.alpha(0.14)).stroke_with(
        pts,
        false,
        1.5,
        LineCap::Round,
        th.primary,
    );
    el().size(w, h)
        .radius(R_MD)
        .fill(th.surface0)
        .border(1.0, th.border)
        .clip_rounded(R_MD)
        .child(canvas(shape).absolute(0.0, 0.0).size(w, h))
}

fn synth_sections(s: &App, d: &DesignedSound) -> El {
    let r = d.recipe;
    let mut c = col().w_full().gap(8.0);
    for i in 0..LAYERS {
        c = c.child(layer_strip(s, i));
    }
    let amp = panel(
        "sd-amp",
        "Amp envelope",
        None,
        row()
            .items_center()
            .gap(10.0)
            .child(envelope_graph(&r))
            .child(knobs((0..7).map(|k| kn(s, 1100 + k, true)).collect())),
    );
    let modes = Segmented::new(
        "Filter type",
        r.filter.mode,
        FilterMode::ALL
            .iter()
            .map(|m| (*m, m.label().to_owned()))
            .collect(),
    )
    .size(11.0)
    .item_px(8.0)
    .key("sd-seg-filter")
    .build(|s: &mut App, m| s.edit_recipe(|r| r.filter.mode = m));
    let filter = panel(
        "sd-filter",
        "Filter",
        Some(modes),
        knobs((0..4).map(|k| kn(s, 1200 + k, true)).collect()),
    );
    let route = |label: &str, key: &str, v: u8, to: bool| {
        Segmented::new(
            label.to_owned(),
            v,
            (0..LAYERS as u8)
                .map(|x| (x, format!("{}", x + 1)))
                .collect(),
        )
        .height(24.0)
        .size(10.5)
        .item_px(6.0)
        .key(key.to_owned())
        .build(move |s: &mut App, x| {
            s.edit_recipe(|r| if to { r.fm.to = x } else { r.fm.from = x })
        })
    };
    let th = t();
    let fm_route = row()
        .items_center()
        .gap(4.0)
        .child(text("FM").font(daw_label(10.0)).color(th.muted_fg).mr(2.0))
        .child(route("FM source", "sd-seg-fm-from", r.fm.from, false))
        .child(text("→").font(mono(11.0, 400.0)).color(th.muted_fg))
        .child(route("FM target", "sd-seg-fm-to", r.fm.to, true));
    let character = panel(
        "sd-character",
        "Character",
        Some(fm_route),
        knobs(vec![
            kn(s, 1300, true),
            kn(s, 1301, true),
            kn(s, 1302, true),
            kn(s, 1303, r.fm.from != r.fm.to),
        ]),
    );
    let trim = row()
        .items_center()
        .gap(6.0)
        .child(text("Trim tail").font(ui(11.0, 400.0)).color(th.muted_fg))
        .child(switch(
            "sd-trim-tail",
            "Trim silent tail",
            r.trim_tail,
            |s: &mut App, v| s.edit_recipe(|r| r.trim_tail = v),
        ));
    let output = panel(
        "sd-output",
        "Output",
        Some(trim),
        knobs(vec![
            kn(s, 1304, true),
            kn(s, 1305, true),
            kn(s, 1306, true),
        ]),
    );
    c.child(
        row()
            .w_full()
            .flex_wrap()
            .gaps(8.0, 8.0)
            .child(amp)
            .child(filter)
            .child(character)
            .child(output),
    )
}

fn sample_sections(s: &App, d: &DesignedSound) -> El {
    let th = t();
    let Some(e) = d.sample.clone() else {
        return el();
    };
    let readable = s.draft_file_seconds() > 0.0;
    let toggles = row()
        .items_center()
        .gap(16.0)
        .child(
            row()
                .items_center()
                .gap(6.0)
                .child(switch(
                    "sd-normalize",
                    "Normalize",
                    e.normalize,
                    |s: &mut App, v| {
                        s.edit_sound(|d| {
                            if let Some(e) = d.sample.as_mut() {
                                e.normalize = v
                            }
                        })
                    },
                ))
                .child(text("Normalize").font(ui(12.0, 400.0)).color(th.fg)),
        )
        .child(
            row()
                .items_center()
                .gap(6.0)
                .child(switch(
                    "sd-reverse",
                    "Reverse",
                    e.reverse,
                    |s: &mut App, v| {
                        s.edit_sound(|d| {
                            if let Some(e) = d.sample.as_mut() {
                                e.reverse = v
                            }
                        })
                    },
                ))
                .child(text("Reverse").font(ui(12.0, 400.0)).color(th.fg)),
        )
        .child(
            Button::new(Variant::Ghost, Size::Sm)
                .icon(&icons_views::ROTATE_CCW)
                .icon_size(14.0)
                .label("Reset edits")
                .font_size(12.0)
                .disabled(e.is_identity())
                .key("sd-reset-edits")
                .build(|s: &mut App, _| {
                    s.edit_sound(|d| {
                        if let Some(e) = d.sample.as_mut() {
                            *e = mdaw_model::sound_design::SampleEdit::new(&e.path)
                        }
                    })
                }),
        );
    let mut c = col().w_full().gap(10.0).child(panel(
        "sd-edit",
        "Edit",
        Some(toggles),
        knobs((0..5).map(|k| kn(s, 1400 + k, readable)).collect()),
    ));
    c = c.child(
        text(e.path.clone())
            .font(mono(11.0, 400.0))
            .color(if readable {
                th.muted_fg
            } else {
                th.destructive
            })
            .truncate()
            .help(e.path.clone()),
    );
    if !readable {
        c = c.child(
            text("This file can't be read any more (moved or deleted).")
                .font(ui(12.0, 400.0))
                .color(th.destructive),
        );
    }
    c
}

fn editor(s: &App, cx: &mut Cx<App>) -> El {
    let Some(d) = s.p9.designer.draft_sound().cloned() else {
        return el();
    };
    let body = if d.is_sample() {
        sample_sections(s, &d)
    } else {
        synth_sections(s, &d)
    };
    col()
        .key("sd-editor")
        .w_full()
        .max_w(1120.0)
        .gap(14.0)
        .px(20.0)
        .py(16.0)
        .child(title_row(s, &d))
        .child(waveform(s, cx))
        .child(action_bar(s))
        .child(body)
}

/// `SamplesView`: the Sound Designer.
pub fn view(s: &App, cx: &mut Cx<App>) -> El {
    let th = t();
    let detail = col().grow(1.0).min_w(0.0).min_h(0.0).child(
        col()
            .key("sd-scroll")
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .child(editor(s, cx))
            .scroll(rune::ScrollSpec {
                stick_bottom: false,
            }),
    );
    col()
        .key("samples-view")
        .w_full()
        .h_full()
        .fill(th.surface0)
        .child(header())
        .child(row().grow(1.0).min_h(0.0).child(library(s)).child(detail))
        .on_drop_files(|s: &mut App, files: &[std::path::PathBuf], _tx: &mut Tx| {
            let paths: Vec<String> = files
                .iter()
                .map(|f| f.to_string_lossy().into_owned())
                .collect();
            s.import_sound_files(&paths);
        })
        .accepts_extensions(&AUDIO_EXTENSIONS)
}
