//! `components/samples/*`: the Samples view (library list, Edit / Process / Generate tabs, the
//! trim waveform).

use mdaw_engine::sample_processor::SignalType;
use mdaw_model::types::FilterType;
use rune::{Align, Color, Cx, El, Phase, Shape, Tx, canvas, col, el, icon, row, spacer, text, text_input};

use crate::app::App;
use crate::icons;
use crate::icons_views;
use crate::samples::{column_peaks, info_duration, list_duration};
use crate::theme::{R_LG, R_MD, R_SM, daw_label, mono, t, ui};
use crate::ui::p9::kit::{
    Chip, SliderSpec, effect_section, empty_state, empty_text, filter_chips, kbd, param_slider,
    readout,
};
use crate::ui::widgets::{Button, Size, Variant, popover_frame, select_option, select_popup};
use crate::views::{P9Menu, SamplesTab, SliderDrag, samples_tab};

const THUMB_W: f32 = 72.0;
const THUMB_H: f32 = 28.0;
const WAVE_H: f32 = 192.0;

fn fmt_hz(v: f64) -> String {
    if v >= 1000.0 {
        format!("{:.1}k", v / 1000.0)
    } else {
        format!("{}Hz", v.round())
    }
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
        .child(text("Samples").font(ui(14.0, 600.0)).color(th.fg))
        .child(
            text("Import, trim, process or generate audio for your arrangement")
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
                .build(|s: &mut App, tx: &mut Tx| s.sample_import_pick(tx)),
        )
}

/// The thumbnail / list waveform: mirrored peaks around the middle.
fn thumb(peaks: &[f32], w: f32, h: f32, color: Color) -> El {
    let mid = h / 2.0;
    let mut pts = vec![[0.0, mid]];
    for (i, p) in peaks.iter().enumerate() {
        pts.push([i as f32, mid - p * mid]);
    }
    for (i, p) in peaks.iter().enumerate().rev() {
        pts.push([i as f32, mid + p * mid]);
    }
    canvas(Shape::new().fill(pts, color)).size(w, h)
}

fn list_item(s: &App, e: &crate::samples::LibEntry) -> El {
    let th = t();
    let sel = s.p9.samples.selected.as_deref() == Some(e.sample.id.as_str());
    let peaks = s.p9.samples.peaks(&e.sample.id, THUMB_W as usize).unwrap_or_default();
    let color = if sel {
        th.primary.alpha(0.95)
    } else {
        th.muted_fg.alpha(0.6)
    };
    let id = e.sample.id.clone();
    let mut r = row()
        .key(format!("sample-{}", e.sample.id))
        .role("button")
        .label(e.sample.name.clone())
        .checked(sel)
        .w_full()
        .items_center()
        .gap(12.0)
        .px(8.0)
        .py(6.0)
        .radius(R_MD)
        .child(
            el().size(THUMB_W + 2.0, THUMB_H + 2.0)
                .radius(R_SM - 2.0)
                .border(1.0, th.border)
                .fill(th.surface0)
                .child(thumb(&peaks, THUMB_W, THUMB_H, color).absolute(1.0, 1.0)),
        )
        .child(
            col()
                .min_w(0.0)
                .grow(1.0)
                .child(
                    text(e.sample.name.clone())
                        .font(ui(13.0, 500.0))
                        .color(if sel { th.fg } else { th.fg.alpha(0.85) })
                        .truncate(),
                )
                .child(
                    text(format!(
                        "{} · {}",
                        list_duration(e.sample.duration_seconds),
                        if e.sample.channels == 1 { "mono" } else { "stereo" }
                    ))
                    .font(mono(10.5, 400.0))
                    .color(th.muted_fg),
                ),
        )
        .on_press(move |s: &mut App| s.select_sample(&id));
    if sel {
        r = r.fill(th.surface3).child(
            el().absolute(0.0, 6.0)
                .w(2.0)
                .h(THUMB_H + 12.0 - 12.0)
                .radius(1.0)
                .fill(th.primary),
        );
    } else {
        r = r.hover(th.surface2);
    }
    r
}

fn library(s: &App) -> El {
    let th = t();
    let entries = &s.p9.samples.entries;
    let mut list = col().w_full().padding(6.0).gap(2.0);
    if entries.is_empty() {
        list = list.child(empty_state(
            Some(&icons::AUDIO_LINES),
            "No samples yet",
            vec![
                empty_text(
                    "Import WAV, MP3 or other audio files, or create a tone in the Generate tab.",
                ),
                el().h(12.0),
                Button::new(Variant::Outline, Size::Sm)
                    .icon(&icons::PLUS)
                    .icon_size(14.0)
                    .label("Import audio")
                    .font_size(12.0)
                    .key("samples-import-empty")
                    .build(|s: &mut App, tx: &mut Tx| s.sample_import_pick(tx)),
            ],
        ));
    } else {
        for e in entries {
            list = list.child(list_item(s, e));
        }
    }
    col()
        .key("sample-library")
        .label("Sample library")
        .w(288.0)
        .h_full()
        .fill(th.surface1)
        .border_sides([0.0, 1.0, 0.0, 0.0], th.border)
        .child(
            row()
                .h(36.0)
                .w_full()
                .items_center()
                .justify_between()
                .px(12.0)
                .border_sides([0.0, 0.0, 1.0, 0.0], th.border)
                .child(
                    text("Library")
                        .font(daw_label(10.5))
                        .text_case(rune::TextCase::Uppercase)
                        .color(th.muted_fg),
                )
                .child(
                    text(entries.len().to_string())
                        .font(mono(10.5, 400.0))
                        .color(th.muted_fg),
                ),
        )
        .child(
            col()
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .child(list)
                .scroll(rune::ScrollSpec {
                    stick_bottom: false,
                }),
        )
}

fn tabs(s: &App) -> El {
    let th = t();
    let has = s.p9.samples.selected_entry().is_some();
    let active = samples_tab(s);
    let mut bar = row()
        .key("sample-tabs")
        .role("tablist")
        .label("Sample tools")
        .h(36.0)
        .w_full()
        .items_end()
        .gap(4.0)
        .px(16.0)
        .border_sides([0.0, 0.0, 1.0, 0.0], th.border);
    for (tab, label, ic, needs) in [
        (SamplesTab::Info, "Edit", &icons_views::INFO, true),
        (
            SamplesTab::Process,
            "Process",
            &icons::SLIDERS_HORIZONTAL,
            true,
        ),
        (SamplesTab::Generate, "Generate", &icons_views::WAND_SPARKLES, false),
    ] {
        let disabled = needs && !has;
        let on = active == tab;
        let mut b = row()
            .key(format!("sample-tab-{}", label.to_lowercase()))
            .role("tab")
            .label(label)
            .focus_selected(on)
            .h(36.0)
            .items_center()
            .gap(6.0)
            .px(12.0)
            .border_sides(
                [0.0, 0.0, 2.0, 0.0],
                if on { th.primary } else { Color::TRANSPARENT },
            )
            .child(icon(ic, 14.0).color(if on { th.fg } else { th.muted_fg }))
            .child(text(label).font(ui(12.0, 600.0)).color(if on {
                th.fg
            } else {
                th.muted_fg
            }));
        if disabled {
            b = b.opacity(0.4).disabled(true);
        } else {
            b = b.on_press(move |s: &mut App| s.p9.tab = Some(tab));
        }
        bar = bar.child(b);
    }
    bar
}

fn waveform(s: &App, cx: &mut Cx<App>, e: &crate::samples::LibEntry) -> El {
    let th = t();
    let w = cx
        .layout_size("sample-waveform")
        .map_or(s.p9.waveform_w.get(), |z| z[0])
        .max(10.0);
    s.p9.waveform_w.set(w);
    let h = WAVE_H;
    let dur = e.sample.duration_seconds.max(1e-9);
    let (ts, te) = (e.trim_start(), e.trim_end());
    let sx = (ts / dur) as f32 * w;
    let ex = (te / dur) as f32 * w;
    let mid = h / 2.0;
    let mut shape = Shape::new();
    // centre line + quarter grid
    shape = shape.fill(
        vec![[0.0, mid.round()], [w, mid.round()], [w, mid.round() + 1.0], [0.0, mid.round() + 1.0]],
        th.border,
    );
    for q in 1..4 {
        let x = (w * q as f32 / 4.0).round();
        shape = shape.fill(vec![[x, 0.0], [x + 1.0, 0.0], [x + 1.0, h], [x, h]], th.border);
    }
    if let Some(b) = s.p9.samples.buffer(&e.sample.id) {
        let peaks = column_peaks(&b, w.ceil() as usize);
        let poly = |from: usize, to: usize| {
            let mut pts = vec![[from as f32, mid]];
            for i in from..to {
                pts.push([i as f32, mid - peaks[i] * mid * 0.92]);
            }
            for i in (from..to).rev() {
                pts.push([i as f32, mid + peaks[i] * mid * 0.92]);
            }
            pts
        };
        shape = shape.fill(poly(0, peaks.len()), th.fg.alpha(0.2));
        let a = (sx.max(0.0) as usize).min(peaks.len());
        let z = (ex.ceil().max(0.0) as usize).min(peaks.len());
        if z > a {
            shape = shape.fill(poly(a, z), th.primary.alpha(0.9));
        }
    }
    let shade = Color::BLACK.alpha(0.35);
    if sx > 0.0 {
        shape = shape.fill(vec![[0.0, 0.0], [sx, 0.0], [sx, h], [0.0, h]], shade);
    }
    if ex < w {
        shape = shape.fill(vec![[ex, 0.0], [w, 0.0], [w, h], [ex, h]], shade);
    }
    let rect = |x: f32, y: f32, ww: f32, hh: f32| vec![[x, y], [x + ww, y], [x + ww, y + hh], [x, y + hh]];
    shape = shape
        .fill(rect(sx, 0.0, 2.0, h), th.primary)
        .fill(rect(ex - 2.0, 0.0, 2.0, h), th.primary)
        .fill(rect(sx, 0.0, 8.0, 12.0), th.primary)
        .fill(rect(ex - 8.0, 0.0, 8.0, 12.0), th.primary);
    if s.p9.samples.audition.is_some() {
        let p = s.p9.samples.audition_progress(s.now);
        let cx_ = ((ts + p * (te - ts)) / dur) as f32 * w;
        shape = shape.fill(rect(cx_, 0.0, 1.0, h), th.fg);
    }
    el().key("sample-waveform")
        .role("group")
        .label("Waveform: drag the orange handles to trim")
        .w_full()
        .h(h)
        .radius(R_LG)
        .border(1.0, th.border)
        .fill(th.surface0)
        .clip_rounded(R_LG)
        .child(canvas(shape).absolute(0.0, 0.0).size(w, h))
        .on_drag(move |s: &mut App, d: &mut rune::Drag, _tx: &mut Tx| {
            let Some(e) = s.p9.samples.selected_entry().cloned() else {
                return;
            };
            let dur = e.sample.duration_seconds.max(1e-9);
            let w = d.w.max(1.0);
            match d.phase {
                Phase::Down => {
                    let x = d.x;
                    let sx = (e.trim_start() / dur) as f32 * w;
                    let ex = (e.trim_end() / dur) as f32 * w;
                    s.p9.drag = if (x - sx).abs() < 10.0 {
                        Some(SliderDrag::Trim { end: false })
                    } else if (x - ex).abs() < 10.0 {
                        Some(SliderDrag::Trim { end: true })
                    } else {
                        None
                    };
                }
                Phase::Move => {
                    if let Some(SliderDrag::Trim { end }) = s.p9.drag {
                        let sec = ((d.x / w) as f64 * dur).clamp(0.0, dur);
                        s.set_trim(end, sec);
                    }
                }
                Phase::Up => s.p9.drag = None,
            }
        })
}

fn info_tab(s: &App, cx: &mut Cx<App>) -> El {
    let th = t();
    let Some(e) = s.p9.samples.selected_entry().cloned() else {
        return el();
    };
    let name: El = match &s.p9.rename {
        Some(edit) => row()
            .items_center()
            .gap(4.0)
            .child(
                row()
                    .h(32.0)
                    .w(288.0)
                    .px(8.0)
                    .items_center()
                    .radius(R_MD)
                    .border(1.0, th.input)
                    .fill(th.surface0)
                    .child(
                        text_input(edit, ui(14.0, 400.0))
                            .key("sample-rename")
                            .label("Sample name")
                            .single_line()
                            .autofocus()
                            .color(th.fg)
                            .grow(1.0)
                            .on_edit(|s: &mut App| {
                                s.p9.rename.get_or_insert_with(|| rune::TextEdit::new(""))
                            })
                            .on_submit(|s: &mut App, _tx: &mut Tx| s.commit_sample_rename())
                            .on_input_key(|s: &mut App, ev: &rune::KeyEvent, _tx: &mut Tx| {
                                if matches!(ev.key, rune::Key::Escape) {
                                    s.p9.rename = None;
                                    return true;
                                }
                                false
                            }),
                    ),
            )
            .child(
                Button::new(Variant::Ghost, Size::IconSm)
                    .icon(&icons::CHECK)
                    .a11y("Save name")
                    .build(|s: &mut App, _| s.commit_sample_rename()),
            ),
        None => row()
            .min_w(0.0)
            .items_center()
            .gap(6.0)
            .child(
                text(e.sample.name.clone())
                    .font(ui(16.0, 600.0))
                    .color(th.fg)
                    .truncate(),
            )
            .child({
                let n = e.sample.name.clone();
                Button::new(Variant::Ghost, Size::IconXs)
                    .icon(&crate::icons_song::PENCIL)
                    .a11y("Rename sample")
                    .tip("Rename", None)
                    .key("sample-rename-button")
                    .build(move |s: &mut App, _| {
                        // `autoFocus`: focused through its model (autofocus is a modal's).
                        let mut e = rune::TextEdit::new(&n);
                        e.focused = true;
                        s.p9.rename = Some(e);
                    })
            }),
    };
    let auditioning = s.p9.samples.audition.is_some();
    let top = row()
        .w_full()
        .items_center()
        .gap(12.0)
        .child(row().min_w(0.0).grow(1.0).child(name))
        .child(
            Button::new(Variant::Outline, Size::Md)
                .icon(if auditioning {
                    &icons::SQUARE_FILLED
                } else {
                    &icons::PLAY_FILLED
                })
                .icon_size(14.0)
                .label(if auditioning { "Stop" } else { "Audition" })
                .pressed(auditioning)
                .key("sample-audition")
                .build(|s: &mut App, _| s.toggle_audition()),
        )
        .child(
            Button::new(Variant::Primary, Size::Md)
                .icon(&icons_views::SEND_HORIZONTAL)
                .label("Send to arrangement")
                .key("sample-send")
                .build(|s: &mut App, _| s.send_to_arrangement()),
        );
    let labels = row()
        .w_full()
        .mt(4.0)
        .justify_between()
        .child(text("0.00s").font(mono(10.0, 400.0)).color(th.muted_fg))
        .child(
            text("Drag the orange handles to trim")
                .font(mono(10.0, 400.0))
                .color(th.muted_fg),
        )
        .child(
            text(format!("{:.2}s", e.sample.duration_seconds))
                .font(mono(10.0, 400.0))
                .color(th.muted_fg),
        );
    let time_input = |key: &'static str, label: &'static str, end: bool| {
        let edit = if end {
            &s.p9.trim_end_edit
        } else {
            &s.p9.trim_start_edit
        };
        col()
            .gap(4.0)
            .child(
                text(label)
                    .font(daw_label(9.5).tracking(0.95))
                    .text_case(rune::TextCase::Uppercase)
                    .color(th.muted_fg),
            )
            .child(
                crate::ui::widgets::lcd()
                    .h(28.0)
                    .w(80.0)
                    .px(8.0)
                    .items_center()
                    .child(
                        text_input(edit, mono(12.0, 400.0))
                            .key(key)
                            .label(label)
                            .single_line()
                            .color(th.fg)
                            .align(Align::End)
                            .grow(1.0)
                            .on_edit(move |s: &mut App| {
                                if end {
                                    &mut s.p9.trim_end_edit
                                } else {
                                    &mut s.p9.trim_start_edit
                                }
                            })
                            .on_submit(move |s: &mut App, _tx: &mut Tx| s.commit_trim_input(end)),
                    ),
            )
    };
    let trimmed = e.trim_start() > 0.0 || e.trim_end() < e.sample.duration_seconds;
    let reset = Button::new(Variant::Ghost, Size::Sm)
        .icon(&icons_views::ROTATE_CCW)
        .icon_size(14.0)
        .label("Reset trim")
        .font_size(12.0)
        .px(8.0)
        .disabled(!trimmed)
        .key("sample-reset-trim")
        .build(|s: &mut App, _| s.reset_trim());
    let rate = row()
        .items_end()
        .child(
            text(format!("{:.1}", e.sample.sample_rate / 1000.0))
                .font(mono(16.0, 600.0).lh(20.0))
                .color(th.fg),
        )
        .child(
            text("kHz")
                .font(mono(10.0, 600.0).lh(18.0))
                .color(th.muted_fg)
                .ml(2.0),
        );
    let value = |s: String| text(s).font(mono(16.0, 600.0).lh(20.0)).color(th.fg);
    let trims = row()
        .w_full()
        .items_end()
        .flex_wrap()
        .gaps(12.0, 12.0)
        .child(time_input("trim-start", "Start (s)", false))
        .child(time_input("trim-end", "End (s)", true))
        .child(reset)
        .child(spacer())
        .child(
            row()
                .items_end()
                .gap(8.0)
                .child(readout(
                    "Length",
                    value(info_duration(e.trim_end() - e.trim_start())),
                ))
                .child(readout("Rate", rate))
                .child(readout(
                    "Channels",
                    value(if e.sample.channels == 1 { "Mono" } else { "Stereo" }.into()),
                )),
        );
    let mut c = col()
        .w_full()
        .gap(16.0)
        .child(top)
        .child(col().w_full().child(waveform(s, cx, &e)).child(labels))
        .child(trims);
    if !e.sample.path.is_empty() {
        c = c.child(
            text(e.sample.path.clone())
                .font(mono(11.0, 400.0))
                .color(th.muted_fg)
                .truncate()
                .help(e.sample.path.clone()),
        );
    }
    c.child(
        row()
            .w_full()
            .justify_end()
            .pt(12.0)
            .border_sides([1.0, 0.0, 0.0, 0.0], th.border)
            .child(
                Button::new(Variant::Ghost, Size::Sm)
                    .icon(&icons::TRASH_2)
                    .icon_size(14.0)
                    .label("Remove from library")
                    .ink(th.destructive)
                    .key("sample-remove")
                    .build(|s: &mut App, _| s.remove_sample()),
            ),
    )
}

fn filter_label(k: FilterType) -> &'static str {
    match k {
        FilterType::Highpass => "Highpass",
        FilterType::Bandpass => "Bandpass",
        _ => "Lowpass",
    }
}

fn select_trigger(key: &str, label: &str, value: &str, w: f32, h: f32, m: P9Menu) -> El {
    let th = t();
    row()
        .key(key.to_owned())
        .role("combobox")
        .label(label.to_owned())
        .focusable(true)
        .w(w)
        .h(h)
        .items_center()
        .justify_between()
        .px(10.0)
        .radius(R_MD)
        .border(1.0, th.border)
        .fill(th.surface2)
        .hover(th.surface3)
        .child(text(value.to_owned()).font(ui(12.0, 400.0)).color(th.fg))
        .child(icon(&icons::CHEVRON_DOWN, 14.0).color(th.muted_fg.alpha(0.5)))
        .on_press(move |s: &mut App| s.toggle_p9_menu(m))
}

/// The Process tab's filter type list.
pub fn filter_type_menu(s: &App) -> El {
    let cur = s.p9.process.filter.kind;
    let mut list = popover_frame(Some(128.0), 4.0);
    for k in [FilterType::Lowpass, FilterType::Highpass, FilterType::Bandpass] {
        list = list.child(
            select_option(
                format!("process-filter-{}", filter_label(k).to_lowercase()),
                row().child(text(filter_label(k)).font(ui(12.0, 400.0)).color(t().fg)),
                k == cur,
            )
            .on_press(move |s: &mut App| {
                s.p9.process.filter.kind = k;
                s.menu = None;
            }),
        );
    }
    select_popup("process-filter-type", Align::Start, list)
}

fn process_tab(s: &App) -> El {
    let th = t();
    let p = s.p9.process;
    let sl = |id: u16, min: f64, max: f64, step: f64| SliderSpec { id, min, max, step };
    let filter_body = vec![
        row()
            .h(32.0)
            .items_center()
            .gap(12.0)
            .child(
                text("Type")
                    .font(daw_label(10.5).tracking(0.84))
                    .text_case(rune::TextCase::Uppercase)
                    .color(th.muted_fg)
                    .w(80.0),
            )
            .child(select_trigger(
                "process-filter-type",
                "Filter type",
                filter_label(p.filter.kind),
                128.0,
                32.0,
                P9Menu::ProcessFilterType,
            )),
        param_slider(
            "process-cutoff",
            "Cutoff",
            sl(1, 20.0, 20000.0, 1.0),
            p.filter.cutoff,
            fmt_hz(p.filter.cutoff),
            false,
            None,
            |s: &mut App, v| s.p9.process.filter.cutoff = v,
        ),
        param_slider(
            "process-resonance",
            "Resonance",
            sl(2, 0.1, 20.0, 0.1),
            p.filter.resonance,
            format!("{:.1}", p.filter.resonance),
            false,
            None,
            |s: &mut App, v| s.p9.process.filter.resonance = v,
        ),
    ];
    let pitch_body = vec![param_slider(
        "process-semitones",
        "Semitones",
        sl(3, -24.0, 24.0, 1.0),
        p.pitch_shift.semitones,
        format!(
            "{}{}",
            if p.pitch_shift.semitones > 0.0 { "+" } else { "" },
            p.pitch_shift.semitones
        ),
        false,
        None,
        |s: &mut App, v| s.p9.process.pitch_shift.semitones = v,
    )];
    let reverb_body = vec![
        param_slider(
            "process-decay",
            "Decay",
            sl(4, 0.1, 10.0, 0.1),
            p.reverb.decay,
            format!("{:.1}s", p.reverb.decay),
            false,
            None,
            |s: &mut App, v| s.p9.process.reverb.decay = v,
        ),
        param_slider(
            "process-reverb-wet",
            "Wet",
            sl(5, 0.0, 1.0, 0.01),
            p.reverb.wet,
            format!("{}%", (p.reverb.wet * 100.0).round()),
            false,
            None,
            |s: &mut App, v| s.p9.process.reverb.wet = v,
        ),
    ];
    let delay_body = vec![
        param_slider(
            "process-time",
            "Time",
            sl(6, 0.01, 1.0, 0.01),
            p.delay.time,
            format!("{}ms", (p.delay.time * 1000.0).round()),
            false,
            None,
            |s: &mut App, v| s.p9.process.delay.time = v,
        ),
        param_slider(
            "process-feedback",
            "Feedback",
            sl(7, 0.0, 0.95, 0.01),
            p.delay.feedback,
            format!("{}%", (p.delay.feedback * 100.0).round()),
            false,
            None,
            |s: &mut App, v| s.p9.process.delay.feedback = v,
        ),
        param_slider(
            "process-delay-wet",
            "Wet",
            sl(8, 0.0, 1.0, 0.01),
            p.delay.wet,
            format!("{}%", (p.delay.wet * 100.0).round()),
            false,
            None,
            |s: &mut App, v| s.p9.process.delay.wet = v,
        ),
    ];
    let any = p.any_enabled();
    let processing = s.p9.samples.processing;
    let mut apply = row()
        .w_full()
        .items_center()
        .gap(12.0)
        .pt(12.0)
        .child(
            {
                let b = Button::new(Variant::Primary, Size::Md);
                if processing { b.icon(&icons::LOADER_CIRCLE) } else { b }
            }
                .label(if processing {
                    "Processing…"
                } else {
                    "Apply to new sample"
                })
                .disabled(processing || !any)
                .key("process-apply")
                .build(|s: &mut App, _| s.process_sample()),
        );
    if !any {
        apply = apply.child(
            text("Switch on at least one stage")
                .font(ui(12.0, 400.0))
                .color(th.muted_fg),
        );
    }
    col()
        .w_full()
        .gap(8.0)
        .child(
            text("Switch on the stages you want; they run top to bottom and create a new sample, leaving the original untouched.")
                .font(ui(12.0, 400.0))
                .color(th.muted_fg)
                .wrap()
                .mb(8.0),
        )
        .child(effect_section(
            "filter",
            "Filter",
            "Cut lows or highs",
            p.filter.enabled,
            |s: &mut App, v| s.p9.process.filter.enabled = v,
            filter_body,
        ))
        .child(effect_section(
            "pitch-shift",
            "Pitch shift",
            "Up or down in semitones",
            p.pitch_shift.enabled,
            |s: &mut App, v| s.p9.process.pitch_shift.enabled = v,
            pitch_body,
        ))
        .child(effect_section(
            "reverse",
            "Reverse",
            "Play the sample backwards",
            p.reverse,
            |s: &mut App, v| s.p9.process.reverse = v,
            vec![],
        ))
        .child(effect_section(
            "normalize",
            "Normalize",
            "Raise the peak to full scale",
            p.normalize,
            |s: &mut App, v| s.p9.process.normalize = v,
            vec![],
        ))
        .child(effect_section(
            "reverb",
            "Reverb",
            "Add room and tail",
            p.reverb.enabled,
            |s: &mut App, v| s.p9.process.reverb.enabled = v,
            reverb_body,
        ))
        .child(effect_section(
            "delay",
            "Delay",
            "Echoes",
            p.delay.enabled,
            |s: &mut App, v| s.p9.process.delay.enabled = v,
            delay_body,
        ))
        .child(apply)
}

fn generate_tab(s: &App) -> El {
    let th = t();
    let g = s.p9.signal;
    let noise = g.kind == SignalType::WhiteNoise;
    let waves = [
        (SignalType::Sine, "Sine"),
        (SignalType::Square, "Square"),
        (SignalType::Sawtooth, "Saw"),
        (SignalType::Triangle, "Triangle"),
        (SignalType::WhiteNoise, "Noise"),
    ];
    let chips = waves
        .iter()
        .map(|(k, l)| Chip {
            key: format!("wave-{}", l.to_lowercase()),
            label: (*l).to_owned(),
            count: None,
            active: *k == g.kind,
        })
        .collect();
    let sl = |id: u16, min: f64, max: f64, step: f64| SliderSpec { id, min, max, step };
    let w10 = || el().w(40.0);
    let note = s
        .p9
        .detected_note
        .clone()
        .filter(|_| !noise)
        .unwrap_or_default();
    let card = col()
        .w_full()
        .gap(4.0)
        .px(14.0)
        .py(12.0)
        .radius(R_LG)
        .border(1.0, th.border)
        .fill(th.surface1)
        .child(
            row()
                .h(36.0)
                .w_full()
                .items_center()
                .gap(12.0)
                .child(
                    text("Waveform")
                        .font(daw_label(10.5).tracking(0.84))
                        .text_case(rune::TextCase::Uppercase)
                        .color(th.muted_fg)
                        .w(80.0),
                )
                .child(filter_chips("Waveform", chips, move |s: &mut App, i| {
                    s.p9.signal.kind = waves[i].0;
                    s.p9.detected_note = None;
                })),
        )
        .child(param_slider(
            "gen-freq",
            "Freq",
            sl(10, 20.0, 20000.0, 1.0),
            g.frequency,
            fmt_hz(g.frequency),
            noise,
            Some(
                text(note)
                    .font(mono(11.0, 600.0))
                    .color(th.primary)
                    .w(40.0),
            ),
            |s: &mut App, v| {
                s.p9.signal.frequency = v;
                s.p9.detected_note = None;
            },
        ))
        .child(param_slider(
            "gen-duration",
            "Duration",
            sl(11, 0.1, 30.0, 0.1),
            g.duration,
            format!("{:.1}s", g.duration),
            false,
            Some(w10()),
            |s: &mut App, v| s.p9.signal.duration = v,
        ))
        .child(param_slider(
            "gen-level",
            "Level",
            sl(12, 0.0, 1.0, 0.01),
            g.amplitude,
            format!("{}%", (g.amplitude * 100.0).round()),
            false,
            Some(w10()),
            |s: &mut App, v| s.p9.signal.amplitude = v,
        ));
    let processing = s.p9.samples.processing;
    let hold = Button::new(Variant::Outline, Size::Md)
        .icon(&icons::VOLUME_2)
        .label("Hold to preview")
        .key("gen-preview")
        .build(|_s: &mut App, _| {})
        .on_drag(|s: &mut App, d: &mut rune::Drag, _tx: &mut Tx| match d.phase {
            Phase::Down => crate::signal_preview::start_hold(s),
            Phase::Move => {
                if d.x < 0.0 || d.y < 0.0 || d.x > d.w || d.y > d.h {
                    crate::signal_preview::stop_hold(s);
                }
            }
            Phase::Up => crate::signal_preview::stop_hold(s),
        });
    let actions = row()
        .w_full()
        .items_center()
        .gap(8.0)
        .child(hold)
        .child(
            Button::new(Variant::Primary, Size::Md)
                .icon(if processing {
                    &icons::LOADER_CIRCLE
                } else {
                    &icons_views::WAND_SPARKLES
                })
                .label(if processing { "Generating…" } else { "Generate" })
                .disabled(processing)
                .key("gen-generate")
                .build(|s: &mut App, _| s.generate_sample()),
        )
        .child(
            row()
                .ml(8.0)
                .items_center()
                .gap(6.0)
                .child(kbd("MIDI"))
                .child(
                    text("keys preview at their pitch")
                        .font(ui(11.0, 400.0))
                        .color(th.muted_fg),
                ),
        );
    col()
        .w_full()
        .gap(16.0)
        .child(
            row()
                .w_full()
                .flex_wrap()
                .child(
                    text("Render a test tone or noise into a new sample. Hold ")
                        .font(ui(12.0, 400.0))
                        .color(th.muted_fg),
                )
                .child(text("Preview").font(ui(12.0, 600.0)).color(th.fg))
                .child(
                    text(" or play your MIDI keyboard to hear it; a played key also sets the frequency.")
                        .font(ui(12.0, 400.0))
                        .color(th.muted_fg),
                ),
        )
        .child(card)
        .child(actions)
}

/// `SamplesView`.
pub fn view(s: &App, cx: &mut Cx<App>) -> El {
    let th = t();
    let tab = samples_tab(s);
    let content = match tab {
        SamplesTab::Info => info_tab(s, cx),
        SamplesTab::Process => process_tab(s),
        SamplesTab::Generate => generate_tab(s),
    };
    let detail = col()
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .child(
            col()
                .w_full()
                .child(tabs(s))
                .child(col().w_full().max_w(896.0).px(24.0).py(20.0).child(content))
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
        .child(
            row()
                .grow(1.0)
                .min_h(0.0)
                .child(library(s))
                .child(detail),
        )
}

impl App {
    /// The Start / End inputs: `parseFloat(value) || 0` (start) / `|| duration` (end).
    pub fn commit_trim_input(&mut self, end: bool) {
        let Some(e) = self.p9.samples.selected_entry().cloned() else {
            return;
        };
        let raw = if end {
            self.p9.trim_end_edit.text.clone()
        } else {
            self.p9.trim_start_edit.text.clone()
        };
        let v = raw
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|v| *v != 0.0 && v.is_finite())
            .unwrap_or(if end { e.sample.duration_seconds } else { 0.0 });
        self.set_trim(end, v);
    }
}

/// `WaveformMinimap` of an audio region: `max(4, round(length·ppb))` columns of the visible part
/// of the sample (from the region's offset, `length / bpm · 60` seconds), peak over channels,
/// mirrored, in `color-mix(in oklab, color 75%, white)` at 0.9, inset 2 points (the canvas is
/// `width − 4` wide, so the columns are squeezed into it like the TS canvas).
pub fn region_waveform(
    s: &App,
    audio: &mdaw_model::types::AudioRegionData,
    length_beats: f64,
    ppb: f64,
    color: Color,
    h: f32,
) -> Option<El> {
    let b = s.p9.samples.buffer(&audio.sample_id)?;
    let bpm = s.svc.engine.doc().get_bpm();
    let w = (length_beats * ppb).round().max(4.0) as usize;
    let sr = b.sample_rate as f64;
    let dur = length_beats / bpm * 60.0;
    let start = (audio.offset_seconds * sr).floor() as usize;
    let end = (((audio.offset_seconds + dur) * sr).floor() as usize).min(b.frames());
    let visible = end.saturating_sub(start);
    if visible == 0 {
        return None;
    }
    let per = visible as f64 / w as f64;
    let ch = (h - 4.0).max(1.0);
    let mid = ch / 2.0;
    let sx = (w as f32 - 4.0).max(1.0) / w as f32;
    let peak = |i: usize| {
        let a = start + (i as f64 * per).floor() as usize;
        let z = start + (((i + 1) as f64 * per).floor() as usize).min(visible);
        let mut m = 0f32;
        for c in &b.channels {
            for &x in &c[a.min(z)..z] {
                m = m.max(x.abs());
            }
        }
        m
    };
    let peaks: Vec<f32> = (0..w).map(peak).collect();
    let mut pts = vec![[0.0, mid]];
    for (i, p) in peaks.iter().enumerate() {
        pts.push([i as f32 * sx, mid - p * mid]);
    }
    for (i, p) in peaks.iter().enumerate().rev() {
        pts.push([i as f32 * sx, mid + p * mid]);
    }
    let fill = crate::ui::song::cmix(color, t().white, 0.75).alpha(0.9);
    Some(
        canvas(Shape::new().fill(pts, fill))
            .absolute(2.0, 2.0)
            .size(w as f32 - 4.0, ch),
    )
}
