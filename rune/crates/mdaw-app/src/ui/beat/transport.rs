//! `BeatTransport.tsx`: Transport (play / record, status LCD, BPM) | Loop (bars, quantize,
//! swing) | Record (count-in, overdub / replace, metronome) | Note repeat, then Export.

use mdaw_model::beat_helpers::swing_grid_label;
use mdaw_model::doc::LoopSettingsPatch;
use mdaw_model::types::{NoteRepeatRate, QuantizeGrid, SwingGrid};
use rune::{Align, Color, Cx, El, Key, KeyEvent, Phase, Tx, el, icon, row, text, text_input};

use crate::app::{App, Menu};
use crate::icons;
use crate::theme::{R_MD, R_SM, mono, t, ui};
use crate::ui::knob::{KnobId, KnobSpec, knob};
use crate::ui::widgets::{
    Button, Segmented, Size, Variant, lcd, mod_key, popover_frame, select_option, select_popup,
};

pub const MIN_BPM: f64 = 40.0;
pub const MAX_BPM: f64 = 240.0;

/// `clamp` of the BPM field: rounded into 40..240.
pub fn clamp_bpm(v: f64) -> f64 {
    v.clamp(MIN_BPM, MAX_BPM).round()
}

fn section_caption(label: &str, show: bool) -> Option<El> {
    show.then(|| {
        text(label.to_owned())
            .font(ui(11.0, 700.0).tracking(11.0 * 0.08))
            .text_case(rune::TextCase::Uppercase)
            .color(t().muted_fg)
    })
}

fn group_label(label: &str, show: bool) -> Option<El> {
    show.then(|| {
        text(label.to_owned())
            .font(ui(11.0, 400.0))
            .color(t().muted_fg)
    })
}

fn section_rule() -> El {
    el().w(1.0).h(28.0).fill(t().fg.alpha(0.15))
}

/// A select trigger (`SelectTrigger size="sm"`) showing `value`.
fn select_trigger(
    key: &str,
    label: &str,
    title: Option<&str>,
    value: &str,
    w: f32,
    bare: bool,
    open: bool,
) -> El {
    let th = t();
    let mut tr = row()
        .key(key.to_owned())
        .role("combobox")
        .label(label.to_owned())
        .a11y_value(value.to_owned())
        .focusable(true)
        .w(w)
        .h(if bare { 24.0 } else { 28.0 })
        .items_center()
        .justify_between()
        .gap(2.0)
        .px(if bare { 4.0 } else { 6.0 })
        .radius(if bare { R_SM } else { R_MD })
        .child(text(value.to_owned()).font(mono(11.0, 400.0)).color(th.fg))
        .child(icon(&icons::CHEVRON_DOWN, 12.0).color(th.muted_fg.alpha(0.6)));
    if !bare {
        tr = tr
            .fill(th.surface0)
            .border(1.0, if open { th.input } else { th.border })
            .hover(th.surface3);
    }
    if let Some(t) = title {
        tr = tr.help(t.to_owned());
    }
    tr
}

/// The open select list (`SelectContent`), checked item in primary with a check mark; a
/// keyboard listbox ([`select_popup`]).
fn select_list<T: Copy + PartialEq + 'static>(
    anchor: &str,
    items: Vec<(T, String)>,
    current: T,
    on: fn(&mut App, T),
) -> El {
    let th = t();
    let mut list = popover_frame(None, 4.0).min_w(128.0);
    for (i, (v, label)) in items.into_iter().enumerate() {
        let sel = v == current;
        list = list.child(
            select_option(
                format!("{anchor}-opt-{i}"),
                row()
                    .label(label.clone())
                    .child(text(label).font(mono(11.0, 400.0)).color(if sel {
                        th.primary
                    } else {
                        th.fg
                    }))
                    .child(rune::spacer())
                    .child(if sel {
                        icon(&icons::CHECK, 14.0).color(th.primary)
                    } else {
                        el().size(14.0, 14.0)
                    }),
                sel,
            )
            .on_press(move |s: &mut App| {
                on(s, v);
                s.menu = None;
            }),
        );
    }
    select_popup(anchor, Align::Center, list)
}

/// The quantize list (open).
pub fn quantize_menu(s: &App) -> El {
    let cur = s.svc.engine.doc().get_loop_settings().quantize;
    select_list(
        "quantize",
        QuantizeGrid::ALL
            .iter()
            .map(|q| {
                (
                    *q,
                    if *q == QuantizeGrid::Off {
                        "Off".to_owned()
                    } else {
                        q.label().to_owned()
                    },
                )
            })
            .collect(),
        cur,
        |s, q| {
            s.set_loop_settings(LoopSettingsPatch {
                quantize: Some(q),
                ..Default::default()
            })
        },
    )
}

/// The note-repeat rate list (open).
pub fn rate_menu(s: &App) -> El {
    select_list(
        "repeat-rate",
        NoteRepeatRate::ALL
            .iter()
            .map(|r| (*r, r.label().to_owned()))
            .collect(),
        s.repeat.rate,
        |s, r| s.set_repeat_rate(r),
    )
}

/// `BpmField`: type (click without moving), drag vertically (2 px per BPM, 8 with Shift),
/// wheel ±1, arrows ±1 / ±10.
fn bpm_field(s: &App) -> El {
    let th = t();
    let bpm = s.svc.engine.doc().get_bpm();
    let value: El = match &s.beat.bpm_edit {
        Some(edit) => text_input(edit, ui(15.0, 600.0))
            .key("bpm-input")
            .single_line()
            .color(th.primary)
            .w(30.0)
            .align(Align::End)
            // After a commit / cancel the field may still be written to while it leaves
            // (blur bookkeeping): that goes to a scratch edit, so the draft is not re-created.
            .on_edit(|s: &mut App| match &mut s.beat.bpm_edit {
                Some(e) => e,
                None => &mut s.beat.bpm_scratch,
            })
            .on_input_change(|s: &mut App, _tx: &mut Tx| {
                if let Some(e) = &mut s.beat.bpm_edit {
                    let clean: String = e
                        .text
                        .chars()
                        .filter(|c| c.is_ascii_digit() || *c == '.')
                        .take(5)
                        .collect();
                    if clean != e.text {
                        *e = rune::TextEdit::new(&clean);
                    }
                }
            })
            .on_input_key(|s: &mut App, ev: &KeyEvent, _tx: &mut Tx| match &ev.key {
                Key::Escape => {
                    s.beat.bpm_edit = None;
                    true
                }
                Key::Up | Key::Down => {
                    let bpm = s.svc.engine.doc().get_bpm();
                    let d = if matches!(ev.key, Key::Up) { 1.0 } else { -1.0 }
                        * if ev.mods.shift { 10.0 } else { 1.0 };
                    s.beat.bpm_edit = None;
                    s.set_bpm(clamp_bpm(bpm + d));
                    true
                }
                _ => false,
            })
            .on_submit(|s: &mut App, _tx: &mut Tx| s.commit_bpm()),
        None => text(format!("{}", bpm.round() as i64))
            .key("bpm-value")
            .font(ui(15.0, 600.0))
            .color(th.fg)
            .w(30.0)
            .align(Align::End),
    };
    lcd()
        .key("bpm")
        .role("spinbutton")
        .label("Tempo (BPM)")
        .range_value(bpm.round(), MIN_BPM, MAX_BPM)
        .range_step(1.0)
        .a11y_value(format!("{}", bpm.round() as i64))
        // Assistive increment / decrement and, while the box itself has focus, the arrows
        // (±1, Page Up / Down ±10), Home / End: the input's own keys when typing.
        .on_step(|s: &mut App, delta: i32, _tx: &mut Tx| {
            let bpm = s.svc.engine.doc().get_bpm();
            s.set_bpm(clamp_bpm(bpm + delta as f64));
        })
        .on_step_limit(|s: &mut App, max: bool, _tx: &mut Tx| {
            s.set_bpm(if max { MAX_BPM } else { MIN_BPM });
        })
        .h(32.0)
        .items_center()
        .gap(6.0)
        .px(8.0)
        .cursor(rune::Cursor::ResizeRow)
        .child(value)
        .child(text("BPM").font(ui(11.0, 600.0)).color(th.muted_fg))
        .on_wheel(|s: &mut App, e: rune::WheelEvent, _tx: &mut Tx| {
            if e.dy == 0.0 {
                return false;
            }
            let bpm = s.svc.engine.doc().get_bpm();
            s.set_bpm(clamp_bpm(bpm + if e.dy > 0.0 { 1.0 } else { -1.0 }));
            true
        })
        .on_drag(|s: &mut App, d: &mut rune::Drag, tx: &mut Tx| {
            if s.beat.bpm_edit.is_some() {
                return;
            }
            match d.phase {
                Phase::Down => {
                    let bpm = s.svc.engine.doc().get_bpm();
                    s.beat.bpm_drag = Some((0.0, bpm, false));
                }
                Phase::Move => {
                    let Some((_, start, mut moved)) = s.beat.bpm_drag else {
                        return;
                    };
                    let dy = -d.dy as f64;
                    if dy.abs() > 2.0 {
                        moved = true;
                    }
                    s.beat.bpm_drag = Some((0.0, start, moved));
                    if moved {
                        let div = if tx.mods().shift { 8.0 } else { 2.0 };
                        s.set_bpm(clamp_bpm(start + dy / div));
                    }
                }
                Phase::Up => {
                    let drag = s.beat.bpm_drag.take();
                    if drag.is_some_and(|(_, _, moved)| !moved) && !d.cancelled() {
                        // A click without movement: type a value (focus selects all).
                        let bpm = s.svc.engine.doc().get_bpm();
                        s.beat.bpm_edit =
                            Some(rune::TextEdit::new(format!("{}", bpm.round() as i64)));
                        tx.focus("bpm-input", true);
                        tx.select_all_text("bpm-input");
                    }
                }
            }
        })
}

/// The status LCD: "COUNT-IN n", else the loop position (+ a pulsing dot while recording).
fn status_lcd(s: &App, recording: bool, armed: bool) -> El {
    let th = t();
    let counting = s.count_in();
    let bars = s.svc.engine.doc().get_loop_settings().bars;
    let mut l = lcd()
        .key("status-lcd")
        .w(92.0)
        .h(32.0)
        .items_center()
        .justify_between()
        .gap(8.0)
        .px(8.0);
    if armed {
        l = l.border(1.0, th.rec.alpha(0.6));
    }
    if counting > 0 {
        l.child(text("COUNT-IN").font(mono(11.0, 600.0)).color(th.rec))
            .child(
                text(format!("{counting}"))
                    .key("count-in")
                    .font(mono(18.0, 700.0).lh(18.0))
                    .color(th.rec),
            )
    } else {
        let pos = row()
            .items_end()
            .gap(4.0)
            .child(
                text("1.1")
                    .key("loop-position")
                    .live_text(&s.live.loop_pos)
                    .font(mono(15.0, 600.0))
                    .color(th.fg)
                    .w(30.0),
            )
            .child(
                text(format!("/ {bars}"))
                    .font(mono(11.0, 400.0))
                    .color(th.muted_fg)
                    .mb(2.0),
            );
        let mut l = l.child(pos);
        if recording {
            l = l.child(
                el().key("rec-dot")
                    .size(8.0, 8.0)
                    .radius(4.0)
                    .fill(th.rec)
                    .label("Recording"),
            );
        }
        l
    }
}

fn metronome_icon(c: Color) -> El {
    use rune::{LineCap, Path, Shape, canvas};
    let body = Path::new()
        .move_to([9.0, 3.0])
        .line_to([15.0, 3.0])
        .line_to([19.0, 21.0])
        .line_to([5.0, 21.0])
        .close();
    let shape = Shape::new()
        .stroke_path(&body, 2.0 * 16.0 / 24.0, LineCap::Round, c)
        .line(
            [12.0, 16.0],
            [17.0, 7.0],
            2.0 * 16.0 / 24.0,
            LineCap::Round,
            c,
        )
        .line(
            [7.0, 16.0],
            [17.0, 16.0],
            2.0 * 16.0 / 24.0,
            LineCap::Round,
            c,
        )
        .view_box(24.0, 24.0);
    canvas(shape).size(16.0, 16.0)
}

pub fn view(s: &App, cx: &mut Cx<App>) -> El {
    let th = t();
    let doc = s.svc.engine.doc();
    let loop_s = doc.get_loop_settings();
    let counting = s.count_in() > 0;
    let recording = s.loop_recording() && !counting;
    let armed = recording || counting;
    let playing = s.playing();
    let width = cx.layout_size("beat-transport").map_or(1400.0, |w| w[0]);
    let captions = width >= 1160.0;
    let labels = width >= 1280.0;

    // Transport
    let play = {
        let stop = playing || armed;
        let b = Button::new(Variant::Outline, Size::Icon)
            .icon(if stop {
                &icons::SQUARE_FILLED
            } else {
                &icons::PLAY_FILLED
            })
            .icon_size(if stop { 14.0 } else { 16.0 })
            .a11y(if stop { "Stop" } else { "Play" })
            .tip(if stop { "Stop" } else { "Play" }, Some("Space"))
            .pressed(playing)
            .key("play");
        let b = if playing {
            b.border(th.play.alpha(0.5))
                .fill(th.play.alpha(0.15))
                .ink(th.play)
        } else {
            b
        };
        b.build(|s: &mut App, _| s.toggle_play())
    };
    let rec = {
        let b = Button::new(Variant::Outline, Size::Icon)
            .icon(&icons::CIRCLE_FILLED)
            .icon_size(14.0)
            .a11y(if armed {
                "Stop recording"
            } else {
                "Record loop"
            })
            .tip(
                if armed {
                    "Stop recording"
                } else {
                    "Record loop"
                },
                Some("R"),
            )
            .pressed(armed)
            .key("record");
        let b = if armed {
            b.fill(th.rec).border(Color::TRANSPARENT).ink(th.white)
        } else {
            b.ink(th.rec)
        };
        b.build(|s: &mut App, _| s.toggle_record())
    };
    let transport = row()
        .key("section-transport")
        .role("group")
        .label("Transport")
        .items_center()
        .gap(6.0)
        .child(play)
        .child(rec)
        .child(status_lcd(s, recording, armed))
        .child(bpm_field(s));

    // Loop
    let mut lp = row()
        .key("section-loop")
        .role("group")
        .label("Loop")
        .items_center()
        .gap(6.0);
    if let Some(c) = section_caption("Loop", captions) {
        lp = lp.child(c);
    }
    let bars = Segmented::new(
        "Loop length in bars",
        loop_s.bars,
        [1u32, 2, 4, 8]
            .iter()
            .map(|b| (*b, b.to_string()))
            .collect(),
    )
    .weight(400.0, 600.0)
    .min_item_w(22.0)
    .key("bars")
    .build(|s: &mut App, b| {
        s.set_loop_settings(LoopSettingsPatch {
            bars: Some(b),
            ..Default::default()
        })
    })
    .help("Loop length in bars");
    let mut g = row().items_center().gap(6.0);
    if let Some(l) = group_label("Bars", labels) {
        g = g.child(l);
    }
    lp = lp.child(g.child(bars));
    let q = loop_s.quantize;
    let qlabel = if q == QuantizeGrid::Off {
        "Off".to_owned()
    } else {
        q.label().to_owned()
    };
    let quant = select_trigger(
        "quantize",
        "Quantize",
        Some("Quantize recorded hits"),
        &qlabel,
        64.0,
        false,
        s.menu == Some(Menu::Quantize),
    )
    .on_press(|s: &mut App| s.toggle_menu(Menu::Quantize));
    let mut g = row().items_center().gap(6.0);
    if let Some(l) = group_label("Quant", labels) {
        g = g.child(l);
    }
    lp = lp.child(g.child(quant));
    let pct = format!("{}%", (50.0 + loop_s.swing * 50.0).round() as i64);
    let swing_knob = knob(
        KnobSpec {
            id: KnobId::Swing,
            label: "Swing",
            min: 0.0,
            max: 0.75,
            step: 0.01,
            default: 0.0,
            small: true,
            hide_text: true,
        },
        loop_s.swing,
        pct.clone(),
        "",
        |s, v| {
            s.set_loop_settings(LoopSettingsPatch {
                swing: Some(v),
                ..Default::default()
            })
        },
        None,
        s,
    );
    let next = if loop_s.swing_grid == SwingGrid::S8 {
        SwingGrid::S16
    } else {
        SwingGrid::S8
    };
    let grid_btn = row()
        .key("swing-grid")
        .role("button")
        .label(format!(
            "Swing grid: {}",
            swing_grid_label(loop_s.swing_grid)
        ))
        .help(format!(
            "Swing grid: {} swing. Click for {}. Independent of quantize.",
            if loop_s.swing_grid == SwingGrid::S8 {
                "off-beat 8th notes"
            } else {
                "every other 16th note"
            },
            swing_grid_label(next)
        ))
        .h(24.0)
        .px(6.0)
        .items_center()
        .radius(R_SM)
        .hover(th.surface3)
        .child(
            text(swing_grid_label(loop_s.swing_grid))
                .font(mono(11.0, 400.0))
                .color(th.fg),
        )
        .on_press(move |s: &mut App| {
            s.set_loop_settings(LoopSettingsPatch {
                swing_grid: Some(next),
                ..Default::default()
            })
        });
    let swing = row()
        .key("swing")
        .role("group")
        .label("Swing")
        .h(32.0)
        .items_center()
        .gap(4.0)
        .pl(4.0)
        .pr(2.0)
        .radius(R_MD)
        .border(1.0, th.border)
        .fill(th.surface0)
        .child(
            row()
                .items_center()
                .gap(4.0)
                .help("Swing: delays every other grid step (50% = straight). Double-click resets.")
                .child(
                    text("Swing")
                        .font(ui(11.0, 400.0))
                        .color(th.muted_fg)
                        .pl(2.0),
                )
                .child(swing_knob)
                .child(
                    text(pct)
                        .key("swing-pct")
                        .font(mono(11.0, 400.0))
                        .color(th.fg)
                        .w(28.0),
                ),
        )
        .child(grid_btn);
    lp = lp.child(swing);

    // Record
    let mut rc = row()
        .key("section-record")
        .role("group")
        .label("Record")
        .items_center()
        .gap(6.0);
    if let Some(c) = section_caption("Record", captions) {
        rc = rc.child(c);
    }
    let count_in = Segmented::new(
        "Count-in bars",
        loop_s.count_in_bars,
        vec![(0u32, "Off".into()), (1, "1".into()), (2, "2".into())],
    )
    .weight(400.0, 600.0)
    .min_item_w(22.0)
    .key("count-in-bars")
    .build(|s: &mut App, v| {
        s.set_loop_settings(LoopSettingsPatch {
            count_in_bars: Some(v),
            ..Default::default()
        })
    });
    let mut g = row().items_center().gap(6.0);
    if let Some(l) = group_label("Count-in", labels) {
        g = g.child(l);
    }
    rc = rc.child(g.child(count_in));
    rc = rc.child(
        Segmented::new(
            "Record mode",
            loop_s.overdub,
            vec![(true, "Overdub".into()), (false, "Replace".into())],
        )
        .sans()
        .weight(400.0, 600.0)
        .min_item_w(22.0)
        .key("record-mode")
        .build(|s: &mut App, v| {
            s.set_loop_settings(LoopSettingsPatch {
                overdub: Some(v),
                ..Default::default()
            })
        }),
    );
    let on = loop_s.metronome;
    let metro = row()
        .key("metronome")
        .role("button")
        .label("Metronome")
        .checked(on)
        .help(if on {
            "Metronome on while recording"
        } else {
            "Metronome off"
        })
        .help_keys(["M"])
        .size(28.0, 28.0)
        .center()
        .radius(R_MD)
        .border(1.0, if on { th.primary.alpha(0.5) } else { th.border })
        .fill(if on {
            th.primary.alpha(0.15)
        } else {
            th.surface0
        })
        .child(metronome_icon(if on { th.primary } else { th.muted_fg }))
        .on_press(|s: &mut App| s.toggle_loop_metronome());
    rc = rc.child(metro);

    // Note repeat
    let rp = s.repeat;
    let repeat_btn = row()
        .key("note-repeat")
        .role("button")
        .label("Note repeat")
        .checked(rp.enabled)
        .help(if rp.enabled {
            format!(
                "Note repeat on: held pads retrigger every {} while playing",
                rp.rate.label()
            )
        } else {
            "Note repeat: hold a pad to retrigger it on the grid while playing".into()
        })
        .help_keys(["N"])
        .h_full()
        .px(6.0)
        .items_center()
        .radius(R_SM)
        .child(text("Repeat").font(ui(11.0, 600.0)).color(if rp.enabled {
            th.primary
        } else {
            th.muted_fg
        }))
        .on_press(|s: &mut App| s.toggle_note_repeat());
    let repeat_btn = if rp.enabled {
        repeat_btn.fill(th.primary.alpha(0.2))
    } else {
        repeat_btn
    };
    let rate = select_trigger(
        "repeat-rate",
        "Note repeat rate",
        None,
        rp.rate.label(),
        54.0,
        true,
        s.menu == Some(Menu::RepeatRate),
    )
    .on_press(|s: &mut App| s.toggle_menu(Menu::RepeatRate));
    let repeat = row()
        .key("section-repeat")
        .role("group")
        .label("Note repeat")
        .h(28.0)
        .items_stretch()
        .padding(2.0)
        .radius(R_MD)
        .border(
            1.0,
            if rp.enabled {
                th.primary.alpha(0.5)
            } else {
                th.border
            },
        )
        .fill(th.surface0)
        .child(repeat_btn)
        .child(rate);

    let export = Button::new(Variant::Outline, Size::IconSm)
        .icon(&icons::DOWNLOAD)
        .a11y("Export")
        .tip("Export WAV or MIDI", Some(&format!("{}E", mod_key())))
        .key("export")
        .build(|s: &mut App, _| s.open_dialog(crate::views::Dialog::Export));

    row()
        .key("beat-transport")
        .w_full()
        .min_w(0.0)
        .items_center()
        .flex_wrap()
        .gaps(8.0, 10.0)
        .child(transport)
        .child(section_rule())
        .child(lp)
        .child(section_rule())
        .child(rc)
        .child(section_rule())
        .child(repeat)
        .child(rune::spacer())
        .child(export)
}
