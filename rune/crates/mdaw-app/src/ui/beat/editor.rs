//! `PadEditor.tsx`: header (colour bar, label, name, missing-pack status, trim readout,
//! audition, chop), the trim waveform with handles and chop markers, the five knobs, play mode,
//! reverse and choke; plus the chop panel (`ChopPanel`).

use mdaw_model::beat_helpers::{
    fmt_db, fmt_pan, fmt_pitch, format_hz, format_seconds, from_log01, pad_color, pad_label,
    pad_name,
};
use mdaw_model::types::{PadPlayMode, PadSampleSource, PadSettings};
use rune::{
    Align, Cx, El, Key, KeyEvent, Phase, Shape, Tx, canvas, col, el, icon, row, spacer, text,
};
use serde_json::json;

use crate::actions::MissingPadFix;
use crate::app::{App, ChopUi, Menu};
use crate::buffers::BufferStatus;
use crate::icons;
use crate::theme::{R_MD, R_SM, css_color, mono, t, ui};
use crate::ui::beat::packs::{MissingPack, get_pack_button, unavailable_actions};
use crate::ui::beat::pads::{buffer_duration, peaks_shape};
use crate::ui::knob::{CUTOFF_MAX, CUTOFF_MIN, KnobId, KnobSpec, knob};
use crate::ui::widgets::{Button, Segmented, Size, Variant, lcd, popover, popover_frame};

/// `MIN_SLICE`: the trim handles keep at least 5 ms between them.
pub const MIN_SLICE: f64 = 0.005;

/// `CHOKE_GROUPS` of the editor (0–8; the context menu offers 0–4).
pub const CHOKE_GROUPS: [i64; 9] = [0, 1, 2, 3, 4, 5, 6, 7, 8];

/// The trim range `(start, end)` clamped to the buffer (`TrimWaveform`).
pub fn trim_range(settings: &PadSettings, duration: f64) -> (f64, f64) {
    if duration > 0.0 {
        (
            settings.start_sec.min(duration),
            settings.end_sec.unwrap_or(duration).min(duration),
        )
    } else {
        (0.0, 0.0)
    }
}

/// A handle drag / key nudge: the patch it writes (`onHandleMove` / `nudge`).
pub fn handle_patch(
    start_handle: bool,
    sec: f64,
    start: f64,
    end: f64,
    duration: f64,
) -> serde_json::Value {
    if start_handle {
        json!({ "startSec": sec.min(end - MIN_SLICE) })
    } else {
        let next = sec.max(start + MIN_SLICE);
        if next >= duration - 1e-4 {
            json!({ "endSec": null })
        } else {
            json!({ "endSec": next })
        }
    }
}

/// `nudge`: a handle's arrow-key step (`(shift ? 0.01 : 0.001) × max(1, duration)` seconds).
pub fn nudge_patch(
    start_handle: bool,
    dir: f64,
    shift: bool,
    start: f64,
    end: f64,
    duration: f64,
) -> serde_json::Value {
    let step = if shift { 0.01 } else { 0.001 } * duration.max(1.0);
    if start_handle {
        json!({ "startSec": (start + dir * step).min(end - MIN_SLICE).max(0.0) })
    } else {
        let next = (end + dir * step).max(start + MIN_SLICE).min(duration);
        if next >= duration - 1e-4 {
            json!({ "endSec": null })
        } else {
            json!({ "endSec": next })
        }
    }
}

fn trim_wave(
    s: &App,
    cx: &mut Cx<App>,
    pad: usize,
    settings: &PadSettings,
    missing: Option<&MissingPack>,
) -> El {
    let th = t();
    let color = css_color(&pad_color(Some(settings), pad), th.primary);
    let cache = s.svc.engine.cache().clone();
    let source = settings.sample.as_ref().map(|x| &x.source);
    let (buf, status) = s.buffers.borrow_mut().get(&cache, source);
    let size = cx.layout_size("trim-wave").unwrap_or([600.0, 200.0]);
    let (w, h) = (size[0].max(1.0), size[1].max(1.0));
    let mut shape = Shape::new().fill(
        vec![
            [0.0, (h / 2.0).round()],
            [w, (h / 2.0).round()],
            [w, (h / 2.0).round() + 1.0],
            [0.0, (h / 2.0).round() + 1.0],
        ],
        th.white.alpha(0.08),
    );
    let mut layers: Vec<El> = Vec::new();
    let duration = buf.as_ref().map_or(0.0, buffer_duration);
    let (start, end) = trim_range(settings, duration);
    if let Some(buf) = &buf {
        let cols = (w.round() as usize).max(32);
        let key = source.map(|src| cache.key(src)).unwrap_or_default();
        let peaks = s.buffers.borrow_mut().peaks(&key, buf, cols).clone();
        shape = peaks_shape(shape, &peaks, w, h - 8.0, 4.0, th.white.alpha(0.22), None);
        let a = (start / duration) as f32 * w;
        let b = ((end / duration) as f32 * w).max(a + 1.0);
        shape = peaks_shape(shape, &peaks, w, h - 8.0, 4.0, color, Some((a, b)));
        let pct = |sec: f64| {
            if duration > 0.0 {
                (sec / duration) as f32 * w
            } else {
                0.0
            }
        };
        // Dimmed outside the trim range.
        layers.push(
            el().absolute(0.0, 0.0)
                .size(pct(start), h)
                .fill(th.surface0.alpha(0.55)),
        );
        layers.push(
            el().absolute(pct(end), 0.0)
                .size((w - pct(end)).max(0.0), h)
                .fill(th.surface0.alpha(0.55)),
        );
        for (i, sl) in s.beat.slices.iter().enumerate() {
            layers.push(
                el().absolute(pct(sl.start_sec), 0.0)
                    .size(1.0, h)
                    .fill(th.warn.alpha(0.8))
                    .child(
                        text(format!("{}", i + 1))
                            .font(mono(11.0, 400.0).lh(11.0))
                            .color(th.warn)
                            .absolute(2.0, 2.0),
                    ),
            );
        }
        for start_handle in [true, false] {
            let sec = if start_handle { start } else { end };
            let x = pct(sec);
            let square = el()
                .absolute(1.0, if start_handle { 0.0 } else { h - 10.0 })
                .size(10.0, 10.0)
                .radius(2.0)
                .fill(th.fg);
            layers.push(
                el().key(if start_handle {
                    "trim-start"
                } else {
                    "trim-end"
                })
                .role("slider")
                .label(if start_handle {
                    "Trim start"
                } else {
                    "Trim end"
                })
                .range_value(
                    (sec * 1000.0).round() / 1000.0,
                    0.0,
                    (duration * 1000.0).round() / 1000.0,
                )
                .range_step(0.001 * duration.max(1.0))
                .a11y_value(format_seconds(sec))
                .absolute(x - 6.0, 0.0)
                .size(12.0, h)
                .focusable(true)
                .cursor(rune::Cursor::ResizeColumn)
                .child(el().absolute(5.0, 0.0).size(2.0, h).fill(th.fg.alpha(0.85)))
                .child(square)
                .on_drag(move |s: &mut App, d: &mut rune::Drag, _tx: &mut Tx| {
                    // The pointer in the wave's points: the handle's left at the press
                    // (Drag::data) + where the press landed on it + the travel since.
                    if d.phase == Phase::Down {
                        d.data = x - 6.0 + d.start_x;
                    }
                    s.trim_drag(start_handle, d.data + d.dx, d.phase, d.cancelled());
                })
                .on_key(move |s: &mut App, ev: &KeyEvent, _tx: &mut Tx| {
                    s.nudge_trim(start_handle, ev)
                })
                .on_step(move |s: &mut App, delta: i32, tx: &mut Tx| {
                    let shift = tx.mods().shift;
                    s.nudge_trim_by(
                        start_handle,
                        delta.signum() as f64,
                        shift || delta.abs() >= 10,
                    );
                }),
            );
        }
    }
    let wave = canvas(shape.view_box(w, h)).absolute(0.0, 0.0).size(w, h);
    let mut inner = el()
        .key("trim-wave")
        .pin(Some(8.0), Some(0.0), Some(8.0), Some(0.0))
        .child(wave);
    for l in layers {
        inner = inner.child(l);
    }
    if buf.is_none() {
        let msg = match status {
            BufferStatus::Loading => "Loading…".to_owned(),
            BufferStatus::Missing => missing.map_or_else(
                || "Sample file not found. Drop a new one on the pad.".to_owned(),
                |m| {
                    if m.available {
                        format!("This pad plays a sound from {}. Get the pack to hear it.", m.name)
                    } else {
                        format!(
                            "This pad plays a sound from {}, which is no longer available to download. Clear the pad or replace it with a built-in sound.",
                            m.name
                        )
                    }
                },
            ),
            _ => "Drop a sample on a pad, or press Enter in the browser.".to_owned(),
        };
        inner = inner.child(
            row()
                .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
                .center()
                .child(
                    text(msg)
                        .font(ui(11.0, 400.0))
                        .color(if status == BufferStatus::Missing {
                            th.warn
                        } else {
                            th.muted_fg
                        })
                        .px(8.0)
                        .py(2.0)
                        .radius(R_SM)
                        .fill(th.surface0),
                ),
        );
    } else {
        s.trim_width.set(w);
        inner = inner
            .help("Drag the handles to trim. Double-click to reset.")
            .on_double_press(|s: &mut App| {
                let pad = s.selected_pad();
                s.edit_pad(pad, json!({ "startSec": 0, "endSec": null }));
                s.end_pad_edit();
            });
    }
    lcd()
        .key("trim")
        .min_h(96.0)
        .grow(1.0)
        .clip_rounded(R_MD)
        .child(inner)
}

fn knob_row(s: &App, pad: usize, settings: &PadSettings) -> El {
    let color = pad_color(Some(settings), pad);
    let spec = |id, label, min, max, step, default| KnobSpec {
        id,
        label,
        min,
        max,
        step,
        default,
        small: false,
        hide_text: false,
    };
    let end: Option<fn(&mut App)> = Some(|s: &mut App| s.end_pad_edit());
    row()
        .items_end()
        .gap(12.0)
        .child(knob(
            spec(KnobId::Gain, "Gain", -60.0, 12.0, 0.5, 0.0),
            settings.gain_db,
            fmt_db(settings.gain_db),
            &color,
            |s, v| {
                let p = s.selected_pad();
                s.edit_pad(p, json!({ "gainDb": v }))
            },
            end,
            s,
        ))
        .child(knob(
            spec(KnobId::Pan, "Pan", -1.0, 1.0, 0.01, 0.0),
            settings.pan,
            fmt_pan(settings.pan),
            &color,
            |s, v| {
                let p = s.selected_pad();
                s.edit_pad(p, json!({ "pan": v }))
            },
            end,
            s,
        ))
        .child(knob(
            spec(KnobId::Pitch, "Pitch", -24.0, 24.0, 1.0, 0.0),
            settings.pitch,
            fmt_pitch(settings.pitch),
            &color,
            |s, v| {
                let p = s.selected_pad();
                s.edit_pad(p, json!({ "pitch": v }))
            },
            end,
            s,
        ))
}

/// The filter knob's text: "Open" at the top, else `formatHz`.
pub fn filter_text(t: f64) -> String {
    if t >= 0.999 {
        "Open".into()
    } else {
        format_hz(from_log01(t, CUTOFF_MIN, CUTOFF_MAX))
    }
}

fn controls(s: &App, pad: usize, settings: &PadSettings) -> El {
    let th = t();
    let mode = Segmented::new(
        "Play mode",
        settings.mode,
        vec![
            (PadPlayMode::Oneshot, "One-shot".into()),
            (PadPlayMode::Gate, "Gate".into()),
        ],
    )
    .sans()
    .weight(600.0, 600.0)
    .item_px(8.0)
    .key("play-mode")
    .build(|s: &mut App, m| {
        let p = s.selected_pad();
        let v = if m == PadPlayMode::Gate {
            "gate"
        } else {
            "oneshot"
        };
        s.edit_pad(p, json!({ "mode": v }));
        s.end_pad_edit();
    });
    let rev = settings.reverse;
    let reverse = row()
        .key("reverse")
        .role("button")
        .label("Reverse")
        .checked(rev)
        .h(28.0)
        .px(8.0)
        .items_center()
        .radius(R_MD)
        .border(
            1.0,
            if rev {
                rune::Color::TRANSPARENT
            } else {
                th.border
            },
        )
        .fill(if rev { th.fg } else { th.surface0 })
        .child(text("Reverse").font(ui(11.0, 600.0)).color(if rev {
            th.surface0
        } else {
            th.muted_fg
        }))
        .on_press(move |s: &mut App| {
            let p = s.selected_pad();
            s.edit_pad(p, json!({ "reverse": !rev }));
            s.end_pad_edit();
        });
    let choke_label = if settings.choke_group == 0 {
        "Off".to_owned()
    } else {
        settings.choke_group.to_string()
    };
    let choke = row()
        .h(28.0)
        .items_center()
        .gap(6.0)
        .pl(8.0)
        .pr(4.0)
        .radius(R_MD)
        .border(1.0, th.border)
        .fill(th.surface0)
        .child(text("Choke").font(ui(11.0, 400.0)).color(th.muted_fg))
        .child(
            row()
                .key("choke")
                .role("combobox")
                .label("Choke group")
                .focusable(true)
                .h(20.0)
                .px(4.0)
                .gap(4.0)
                .items_center()
                .radius(R_SM)
                .fill(th.surface2)
                .child(
                    text(choke_label)
                        .font(mono(11.0, 400.0))
                        .color(th.fg)
                        .truncate()
                        .shrink(),
                )
                .child(icon(&icons::CHEVRON_DOWN, 12.0).color(th.fg))
                .on_press(|s: &mut App| s.toggle_menu(Menu::Choke)),
        );
    col()
        .w_full()
        .gap(10.0)
        .child(
            row()
                .w_full()
                .flex_wrap()
                .items_end()
                .gaps(8.0, 12.0)
                .child(knob_row(s, pad, settings))
                .child(spacer())
                .child(
                    row()
                        .flex_wrap()
                        .items_center()
                        .gap(8.0)
                        .child(mode)
                        .child(reverse)
                        .child(choke),
                ),
        )
        .child(super::shaping::shaping_row(s, pad, settings))
}

/// The choke `<select>` (open): Off, 1–8.
pub fn choke_menu(s: &App) -> El {
    let th = t();
    let cur = s
        .pad_settings(s.selected_pad())
        .map_or(0, |p| p.choke_group);
    let mut list = popover_frame(None, 4.0).min_w(72.0);
    for g in CHOKE_GROUPS {
        let sel = g == cur;
        let label = if g == 0 {
            "Off".to_owned()
        } else {
            g.to_string()
        };
        list = list.child(
            crate::ui::widgets::select_option(
                format!("choke-opt-{g}"),
                row().label(label.clone()).child(
                    text(label)
                        .font(mono(11.0, 400.0))
                        .color(if sel { th.primary } else { th.fg }),
                ),
                sel,
            )
            .on_press(move |s: &mut App| {
                let p = s.selected_pad();
                s.edit_pad(p, json!({ "chokeGroup": g }));
                s.end_pad_edit();
                s.menu = None;
            }),
        );
    }
    crate::ui::widgets::select_popup("choke", Align::End, list)
}

/// `ChopPanel` in its popover (aligned to the Chop button's end).
pub fn chop_menu(s: &App, cx: &mut Cx<App>) -> El {
    let th = t();
    let pad = s.selected_pad();
    let n = s.beat.slices.len();
    let last = (pad + n.max(1) - 1).min(15);
    let mode_value: i64 = match s.beat.chop {
        ChopUi::Transients { .. } => 0,
        ChopUi::Equal { count } => count as i64,
    };
    let modes = Segmented::new(
        "Chop mode",
        mode_value,
        vec![
            (0, "Hits".into()),
            (4, "4".into()),
            (8, "8".into()),
            (16, "16".into()),
        ],
    )
    .sans()
    .weight(600.0, 600.0)
    .item_px(8.0)
    .key("chop-mode")
    .build(|s: &mut App, v| {
        s.beat.chop = if v == 0 {
            ChopUi::Transients { sensitivity: 0.5 }
        } else {
            ChopUi::Equal { count: v as usize }
        };
        s.recompute_slices();
    });
    let mut panel = popover_frame(Some(256.0), 12.0)
        .key("chop-panel")
        .gap(12.0)
        .child(text("Chop to pads").font(ui(12.0, 600.0)).color(th.fg))
        .child(modes);
    if let ChopUi::Transients { sensitivity } = s.beat.chop {
        // `<input type="range" class="flex-1">`: the track takes the row's width (256 − 24
        // padding − the label − the gap).
        let w = cx
            .layout_size("chop-sensitivity")
            .map_or(150.0, |sz| sz[0])
            .max(14.0);
        let x = (sensitivity as f32) * w;
        let set = |s: &mut App, v: f64| {
            s.beat.chop = ChopUi::Transients {
                sensitivity: ((v.clamp(0.0, 1.0)) * 100.0).round() / 100.0,
            };
            s.recompute_slices();
        };
        let slider = el()
            .key("chop-sensitivity")
            .role("slider")
            .label("Sensitivity")
            .range_value(sensitivity, 0.0, 1.0)
            .range_step(0.01)
            .a11y_value(format!("{sensitivity}"))
            .h(16.0)
            .grow(1.0)
            .min_w(0.0)
            .child(
                el().absolute(0.0, 6.0)
                    .size(w, 4.0)
                    .radius(2.0)
                    .fill(th.surface4),
            )
            .child(
                el().absolute(0.0, 6.0)
                    .size(x, 4.0)
                    .radius(2.0)
                    .fill(th.primary),
            )
            .child(
                el().absolute((x - 7.0).clamp(0.0, (w - 14.0).max(0.0)), 1.0)
                    .size(14.0, 14.0)
                    .radius(7.0)
                    .fill(th.primary),
            )
            .on_drag(move |s: &mut App, d: &mut rune::Drag, _tx: &mut Tx| {
                set(s, (d.x / d.w.max(1.0)) as f64);
            })
            // Arrow keys ±0.01, Page Up / Down ±0.1, Home / End (a native range input's keys).
            .on_step(move |s: &mut App, delta: i32, _tx: &mut Tx| {
                if let ChopUi::Transients { sensitivity } = s.beat.chop {
                    set(s, sensitivity + delta as f64 * 0.01);
                }
            })
            .on_step_limit(move |s: &mut App, max: bool, _tx: &mut Tx| {
                set(s, if max { 1.0 } else { 0.0 });
            });
        panel = panel.child(
            row()
                .w_full()
                .items_center()
                .gap(8.0)
                .child(text("Sensitivity").font(ui(11.0, 400.0)).color(th.muted_fg))
                .child(slider),
        );
    }
    let msg = if n > 0 {
        format!(
            "{n} slice{} → pads {}–{}. Each pad keeps the same file with its own start and end.",
            if n == 1 { "" } else { "s" },
            pad_label(pad),
            pad_label(last)
        )
    } else {
        "No slices found in the trimmed part.".to_owned()
    };
    panel = panel
        .child(
            text(msg)
                .font(ui(11.0, 400.0).lh(14.0))
                .color(th.muted_fg)
                .wrap(),
        )
        .child(
            Button::new(Variant::Primary, Size::Sm)
                .label(format!("Chop to {n} pad{}", if n == 1 { "" } else { "s" }))
                .disabled(n == 0)
                .key("chop-apply")
                .build(|s: &mut App, _| s.apply_chop()),
        );
    popover("chop-button", Align::End, panel)
}

pub fn view(s: &App, cx: &mut Cx<App>, missing: Option<&MissingPack>) -> El {
    let th = t();
    let pad = s.selected_pad();
    let Some(settings) = s.pad_settings(pad) else {
        return el();
    };
    let color = css_color(&pad_color(Some(&settings), pad), th.primary);
    let name = pad_name(Some(&settings));
    let cache = s.svc.engine.cache().clone();
    let source = settings.sample.as_ref().map(|x| &x.source);
    let (buf, status) = s.buffers.borrow_mut().get(&cache, source);
    let missing = if buf.is_some() { None } else { missing };
    let mut header = row()
        .key("editor-header")
        .w_full()
        .min_w(0.0)
        .items_center()
        .gap(8.0)
        .child(el().size(4.0, 20.0).radius(2.0).fill(color))
        .child(
            text(pad_label(pad))
                .font(mono(11.0, 600.0))
                .color(th.muted_fg),
        );
    let mut name_el = text(if name.is_empty() {
        "Empty pad".to_owned()
    } else {
        name
    })
    .key("editor-name")
    .font(ui(13.0, 600.0))
    .color(th.fg)
    .truncate()
    .min_w(0.0);
    if let Some(PadSampleSource::File { path, .. }) = source {
        name_el = name_el.help(path.clone());
    }
    header = header.child(name_el);
    if let Some(m) = missing {
        let mut st = row().items_center().gap(6.0).child(
            text(if m.available {
                "Pack not installed"
            } else {
                "No longer available"
            })
            .font(ui(11.0, 400.0))
            .color(th.warn),
        );
        st = if m.available {
            st.child(get_pack_button(s, m))
        } else {
            st.child(unavailable_actions(m))
        };
        header = header.child(st);
    } else if status == BufferStatus::Missing {
        header = header.child(text("File not found").font(ui(11.0, 400.0)).color(th.warn));
    }
    let mut right = row().ml(0.0).items_center().gap(4.0);
    if let Some(b) = &buf {
        let d = buffer_duration(b);
        let (st, en) = trim_range(&settings, d);
        right = right.child(
            row()
                .key("trim-readout")
                .mr(6.0)
                .help("Trim start – end / sample length")
                .child(
                    text(format!("{} – {}", format_seconds(st), format_seconds(en)))
                        .font(mono(11.0, 400.0))
                        .color(th.muted_fg),
                )
                .child(
                    text(format!(" / {}", format_seconds(d)))
                        .font(mono(11.0, 400.0))
                        .color(th.muted_fg),
                ),
        );
    }
    let has_sample = settings.sample.is_some();
    right = right
        .child(
            Button::new(Variant::Ghost, Size::IconSm)
                .icon(&icons::PLAY)
                .a11y("Audition pad")
                .tip("Audition pad", None)
                .disabled(!has_sample)
                .key("audition")
                .build(|s: &mut App, _| {
                    let p = s.selected_pad();
                    s.svc.drum_hit(p, 0.85);
                }),
        )
        .child(
            Button::new(Variant::Outline, Size::Sm)
                .icon(&icons::SCISSORS)
                .label("Chop")
                .a11y("Chop to pads")
                .pressed(s.menu == Some(Menu::Chop))
                .disabled(!has_sample || buf.is_none())
                .key("chop-button")
                .build(|s: &mut App, _| {
                    if s.menu == Some(Menu::Chop) {
                        s.close_menu();
                    } else {
                        s.menu = Some(Menu::Chop);
                        s.recompute_slices();
                    }
                }),
        );
    header = header.child(spacer()).child(right);
    col()
        .key("pad-editor")
        .role("region")
        .label(format!("Pad {} editor", pad_label(pad)))
        .w_full()
        .h_full()
        .min_w(0.0)
        .min_h(0.0)
        .gap(8.0)
        .child(header)
        .child(trim_wave(s, cx, pad, &settings, missing))
        .child(controls(s, pad, &settings))
}

impl App {
    /// A trim handle's drag (`onHandleDown` / `onHandleMove` / `onHandleUp`): seconds =
    /// pointer x / width × duration, clamped; release closes the edit (`onChangeEnd`).
    pub fn trim_drag(&mut self, start_handle: bool, wave_x: f32, phase: Phase, cancelled: bool) {
        let pad = self.selected_pad();
        let Some(settings) = self.pad_settings(pad) else {
            return;
        };
        let cache = self.svc.engine.cache().clone();
        let (buf, _) = self
            .buffers
            .borrow_mut()
            .get(&cache, settings.sample.as_ref().map(|x| &x.source));
        let Some(buf) = buf else { return };
        let duration = buffer_duration(&buf);
        if duration <= 0.0 {
            return;
        }
        match phase {
            Phase::Down => self.beat.trim_drag = Some(start_handle),
            Phase::Move => {
                if self.beat.trim_drag == Some(start_handle) {
                    let (start, end) = trim_range(&settings, duration);
                    let w = self.trim_width.get().max(1.0) as f64;
                    let sec = ((wave_x as f64 / w) * duration).clamp(0.0, duration);
                    self.edit_pad(pad, handle_patch(start_handle, sec, start, end, duration));
                }
            }
            Phase::Up => {
                let _ = cancelled;
                if self.beat.trim_drag.take().is_some() {
                    self.end_pad_edit();
                }
            }
        }
    }

    /// A trim handle's arrow keys: one step, then `onChangeEnd`.
    pub fn nudge_trim(&mut self, start_handle: bool, ev: &KeyEvent) -> bool {
        let dir = match ev.key {
            Key::Right | Key::Up => 1.0,
            Key::Left | Key::Down => -1.0,
            _ => return false,
        };
        self.nudge_trim_by(start_handle, dir, ev.mods.shift)
    }

    /// One trim nudge (`nudge`): `dir` ±1, `coarse` = the Shift step.
    pub fn nudge_trim_by(&mut self, start_handle: bool, dir: f64, coarse: bool) -> bool {
        let pad = self.selected_pad();
        let Some(settings) = self.pad_settings(pad) else {
            return false;
        };
        let cache = self.svc.engine.cache().clone();
        let (buf, _) = self
            .buffers
            .borrow_mut()
            .get(&cache, settings.sample.as_ref().map(|x| &x.source));
        let Some(buf) = buf else { return false };
        let duration = buffer_duration(&buf);
        let (start, end) = trim_range(&settings, duration);
        self.edit_pad(
            pad,
            nudge_patch(start_handle, dir, coarse, start, end, duration),
        );
        self.end_pad_edit();
        true
    }

    /// `MissingPadFix` from the editor / strip.
    pub fn fix_missing(&mut self, pads: Vec<usize>, fix: MissingPadFix) {
        self.fix_missing_pads(&pads, fix);
    }
}
