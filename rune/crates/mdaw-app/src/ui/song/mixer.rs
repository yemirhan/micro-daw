//! `MixerView` (X): a 92-point `ChannelStrip` per track (name, pan knob, `Fader`, `LevelMeter`
//! with its scale, dB readout, mute / solo) and the `MasterStrip`. Meters are live values the frame
//! loop moves (`App::song_poll` reads `getTrackLevel` / `getMasterLevel` every frame).

use mdaw_model::Track;
use rune::{Color, El, Key, KeyEvent, Phase, Tx, col, el, row, spacer, text};

use crate::app::App;
use crate::song::{MAX_VOLUME, MIN_VOLUME, pan_label};
use crate::theme::{daw_label, mono, t, ui};
use crate::ui::knob::{KnobId, KnobSpec, knob_keyed};
use crate::ui::song::headers::{ToggleKind, toggle};
use crate::ui::song::track_color;
use crate::ui::widgets::lcd;

/// `STRIP_FADER_HEIGHT`.
pub const FADER_H: f32 = 104.0;
const CAP: f32 = 22.0;
/// `FLOOR_DB` of the meters.
pub const FLOOR_DB: f64 = -60.0;

/// The meter's normalised level (`(level − floor) / −floor`, 0 for −∞).
pub fn meter_norm(db: f64) -> f32 {
    if db.is_finite() {
        ((db - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0) as f32
    } else {
        0.0
    }
}

/// The colour ramp's offset: the full-height ramp is pushed down by `(1 − n) × h`.
pub fn meter_offset(db: f64, _strip: bool) -> f32 {
    (1.0 - meter_norm(db)) * FADER_H
}

/// `LevelMeter`: green to −12 dB, amber to −3 dB, red above; LED gaps every 4 points.
fn meter(key: String, w: f32, s: &App) -> El {
    let th = t();
    let yellow = ((-12.0 - FLOOR_DB) / -FLOOR_DB) as f32;
    let red = ((-3.0 - FLOOR_DB) / -FLOOR_DB) as f32;
    let live = {
        let mut m = s.song.live.meters.borrow_mut();
        let id = key.trim_start_matches("meter-").to_owned();
        let id = if id == "master" { String::new() } else { id };
        let lv = m
            .entry(id.clone())
            .or_insert_with(|| rune::Live::new([0.0, FADER_H]));
        let db = s.song.live.levels.borrow().get(&id).copied();
        if let Some(db) = db {
            lv.snap([0.0, meter_offset(db, true)]);
        }
        lv.clone()
    };
    let ramp = col()
        .absolute(0.0, 0.0)
        .size(w, FADER_H)
        .live_offset(&live)
        .child(el().w(w).h(FADER_H * (1.0 - red)).fill(th.rec))
        .child(el().w(w).h(FADER_H * (red - yellow)).fill(th.warn))
        .child(el().w(w).grow(1.0).fill(th.play));
    let mut gaps = rune::Shape::new();
    let mut y = FADER_H - 4.0;
    while y > 0.0 {
        gaps = gaps.fill(
            vec![[0.0, y], [w, y], [w, y + 1.0], [0.0, y + 1.0]],
            th.surface0,
        );
        y -= 4.0;
    }
    el().key(key)
        .size(w, FADER_H)
        .radius(2.0)
        .clip_rounded(2.0)
        .fill(th.surface0)
        .box_shadow(rune::Shadow {
            x: 0.0,
            y: 1.0,
            blur: 2.0,
            spread: 0.0,
            color: Color::BLACK.alpha(0.6),
            inset: true,
        })
        .inert()
        .child(ramp)
        .child(rune::canvas(gaps).absolute(0.0, 0.0).size(w, FADER_H))
}

/// `MeterScale`: 0, −6, −12, −24, −48.
fn scale() -> El {
    let th = t();
    let mut c = el().w(20.0).h(FADER_H).inert();
    for db in [0i64, -6, -12, -24, -48] {
        let y = (db as f64 / FLOOR_DB) as f32 * FADER_H;
        c = c.child(
            text(if db == 0 {
                "0".to_owned()
            } else {
                db.to_string()
            })
            .font(mono(8.5, 400.0))
            .color(th.muted_fg.alpha(0.7))
            .absolute(0.0, y - 5.0),
        );
    }
    c
}

/// `Fader`: click off the cap jumps there, the cap keeps its offset; Shift is 4× finer; arrows
/// ±1 (±6 with Shift); double-click resets to 0 dB; integer values.
fn fader(track: &Track) -> El {
    let th = t();
    let (min, max) = (MIN_VOLUME, MAX_VOLUME);
    let travel = FADER_H - CAP;
    let norm = ((track.volume.clamp(min, max) - min) / (max - min)) as f32;
    let cap_top = (1.0 - norm) * travel;
    let zero_top = ((max - 0.0) / (max - min)) as f32 * travel + CAP / 2.0;
    let id = track.id.clone();
    let id2 = id.clone();
    let id3 = id.clone();
    let (id4, id5) = (id.clone(), id.clone());
    let db = crate::song::js_num(track.volume);
    el().key(format!("fader-{}", track.id))
        .role("slider")
        .label(format!("{} volume", track.name))
        .description(format!("{db} dB"))
        // aria-valuemin / -max / -now / -valuetext, increment / decrement (phase-7 pattern).
        .range_value(track.volume, min, max)
        .range_step(1.0)
        .a11y_value(format!("{db} dB"))
        .focusable(true)
        .w(28.0)
        .h(FADER_H)
        .cursor(rune::Cursor::ResizeRow)
        .child(
            el().absolute(12.5, 11.0)
                .size(3.0, FADER_H - 22.0)
                .radius(1.5)
                .fill(th.surface0),
        )
        .child(
            el().absolute(2.0, zero_top)
                .size(24.0, 1.0)
                .fill(th.muted_fg.alpha(0.4)),
        )
        .child(
            el().key(format!("fader-{}-cap", track.id))
                .absolute(0.0, cap_top)
                .size(28.0, CAP)
                .radius(3.0)
                .border(1.0, Color::BLACK.alpha(0.5))
                .gradient(
                    rune::oklch(0.46, 0.006, 65.0),
                    rune::oklch(0.34, 0.006, 65.0),
                )
                .box_shadow(rune::Shadow {
                    x: 0.0,
                    y: 2.0,
                    blur: 4.0,
                    spread: 0.0,
                    color: Color::BLACK.alpha(0.5),
                    inset: false,
                })
                .child(
                    el().absolute(4.0, 10.0)
                        .size(20.0, 2.0)
                        .radius(1.0)
                        .fill(th.fg.alpha(0.85)),
                ),
        )
        .on_drag(move |s: &mut App, d: &mut rune::Drag, tx: &mut Tx| {
            let clamp = |v: f64| v.clamp(min, max).round();
            match d.phase {
                Phase::Down => {
                    let v = s.find_track(&id).map_or(0.0, |t| t.volume);
                    let norm = ((v.clamp(min, max) - min) / (max - min)) as f32;
                    let cap_top = (1.0 - norm) * travel;
                    let mut start = v;
                    if d.y < cap_top || d.y > cap_top + CAP {
                        let n = 1.0 - ((d.y - CAP / 2.0) / travel).clamp(0.0, 1.0);
                        start = clamp(min + n as f64 * (max - min));
                        s.set_track_volume(&id, start);
                    }
                    s.song.fader_drag = Some((id.clone(), start));
                }
                Phase::Move => {
                    if let Some((tid, start)) = s.song.fader_drag.clone() {
                        let per_px =
                            (max - min) / travel as f64 / if tx.mods().shift { 4.0 } else { 1.0 };
                        s.set_track_volume(&tid, clamp(start + (-d.dy) as f64 * per_px));
                    }
                }
                Phase::Up => s.song.fader_drag = None,
            }
        })
        .on_double_press(move |s: &mut App| s.set_track_volume(&id2, 0.0))
        .on_key(move |s: &mut App, ev: &KeyEvent, _tx: &mut Tx| {
            let step = if ev.mods.shift { 6.0 } else { 1.0 };
            let v = s.find_track(&id3).map_or(0.0, |t| t.volume);
            let next = match ev.key {
                Key::Up | Key::Right => v + step,
                Key::Down | Key::Left => v - step,
                Key::Home => min,
                Key::End => max,
                _ => return false,
            };
            s.set_track_volume(&id3, next.clamp(min, max).round());
            true
        })
        .on_step(move |s: &mut App, delta: i32, _tx: &mut Tx| {
            let v = s.find_track(&id4).map_or(0.0, |t| t.volume);
            s.set_track_volume(&id4, (v + delta as f64).clamp(min, max).round());
        })
        .on_step_limit(move |s: &mut App, top: bool, _tx: &mut Tx| {
            s.set_track_volume(&id5, if top { max } else { min });
        })
}

fn strip(s: &App, track: &Track, index: usize) -> El {
    let th = t();
    let spec = KnobSpec {
        id: KnobId::Song(1000 + index as u16),
        label: "Pan",
        min: -1.0,
        max: 1.0,
        step: 0.01,
        default: 0.0,
        small: true,
        hide_text: false,
    };
    let tid = track.id.clone();
    let im = track.id.clone();
    let is = track.id.clone();
    let pan = knob_keyed(
        spec,
        format!("mixer-pan-{}", track.id),
        track.pan,
        pan_label(track.pan),
        "var(--primary)",
        move |s: &mut App, v| s.set_track_pan(&tid, v),
        None,
        s,
    );
    col()
        .key(format!("strip-{}", track.id))
        .w(92.0)
        .items_center()
        .gap(8.0)
        .px(8.0)
        .pt(10.0)
        .pb(8.0)
        .fill(th.surface1)
        .border_sides([0.0, 1.0, 0.0, 0.0], th.border)
        .opacity(if track.muted { 0.7 } else { 1.0 })
        .child(
            el().absolute(0.0, 0.0)
                .w(92.0)
                .h(3.0)
                .fill(track_color(&track.color)),
        )
        .child(
            text(track.name.clone())
                .font(ui(11.0, 600.0))
                .color(th.fg)
                .truncate()
                .align(rune::Align::Center)
                .w_full()
                .help(track.name.clone()),
        )
        .child(pan)
        .child(
            row()
                .items_start()
                .gap(4.0)
                .child(fader(track))
                .child(meter(format!("meter-{}", track.id), 6.0, s))
                .child(scale()),
        )
        .child(
            lcd().w_full().py(2.0).justify_center().child(
                text(format!(
                    "{} dB",
                    if track.volume > 0.0 {
                        format!("+{}", crate::song::js_num(track.volume))
                    } else {
                        crate::song::js_num(track.volume)
                    }
                ))
                .key(format!("strip-db-{}", track.id))
                .font(mono(10.5, 400.0))
                .color(th.fg.alpha(0.85)),
            ),
        )
        .child(
            row()
                .items_center()
                .gap(4.0)
                .child(toggle(
                    format!("strip-mute-{}", track.id),
                    ToggleKind::Mute,
                    track.muted,
                    if track.muted { "Unmute" } else { "Mute" },
                    true,
                    move |s: &mut App| s.toggle_track_mute(&im),
                ))
                .child(toggle(
                    format!("strip-solo-{}", track.id),
                    ToggleKind::Solo,
                    track.solo,
                    if track.solo { "Unsolo" } else { "Solo" },
                    true,
                    move |s: &mut App| s.toggle_track_solo(&is),
                )),
        )
}

fn master(s: &App) -> El {
    let th = t();
    let level = s
        .song
        .live
        .levels
        .borrow()
        .get("")
        .copied()
        .unwrap_or(f64::NEG_INFINITY);
    let live = &s.song.live;
    live.master_db.set(crate::song::master_db_text(level));
    col()
        .key("master-strip")
        .w(104.0)
        .items_center()
        .gap(8.0)
        .px(8.0)
        .pt(10.0)
        .pb(8.0)
        .fill(th.surface2)
        .border_sides([0.0, 0.0, 0.0, 1.0], th.border)
        .child(el().absolute(0.0, 0.0).w(104.0).h(3.0).fill(th.primary))
        .child(
            text("Master")
                .font(daw_label(11.0))
                .text_case(rune::TextCase::Uppercase)
                .color(th.fg),
        )
        .child(
            // The lamp is lit by a live opacity (`live.clip`, set each frame from the master
            // level): the red layer over the idle one.
            row().h(62.0).items_center().child(
                row()
                    .key("clip-lamp")
                    .px(6.0)
                    .py(2.0)
                    .radius(3.0)
                    .fill(th.surface3)
                    .help("Lights up when the output is near 0 dBFS")
                    .child(
                        text("CLIP")
                            .font(mono(9.5, 700.0).tracking(0.57))
                            .color(th.muted_fg),
                    )
                    .child(
                        row()
                            .key("clip-lamp-lit")
                            .absolute(0.0, 0.0)
                            .w_full()
                            .h_full()
                            .center()
                            .radius(3.0)
                            .fill(th.rec)
                            .live_opacity(&live.clip)
                            .inert()
                            .child(
                                text("CLIP")
                                    .font(mono(9.5, 700.0).tracking(0.57))
                                    .color(Color::WHITE),
                            ),
                    ),
            ),
        )
        .child(
            row()
                .items_start()
                .gap(4.0)
                .child(meter("meter-master".into(), 9.0, s))
                .child(meter("meter-master".into(), 9.0, s).key("meter-master-r"))
                .child(scale()),
        )
        .child(
            lcd().w_full().py(2.0).justify_center().child(
                text(crate::song::master_db_text(level))
                    .live_text(&live.master_db)
                    .key("master-db")
                    .font(mono(10.5, 400.0))
                    .color(th.fg.alpha(0.85)),
            ),
        )
        .child(el().h(24.0))
}

pub fn view(s: &App) -> El {
    let th = t();
    let tracks = s.tracks();
    let mut strips = row().w_full().items_stretch();
    for (i, tr) in tracks.iter().enumerate() {
        strips = strips.child(strip(s, tr, i));
    }
    if tracks.is_empty() {
        strips = strips.child(
            row().grow(1.0).center().py(40.0).child(
                text("Add tracks to see mixer channels")
                    .key("mixer-empty")
                    .font(ui(12.0, 400.0))
                    .color(th.muted_fg),
            ),
        );
    }
    strips = strips.child(spacer());
    if !tracks.is_empty() {
        strips = strips.child(master(s));
    }
    col()
        .key("mixer")
        .role("region")
        .label("Mixer")
        .w_full()
        .fill(th.surface0)
        .border_sides([1.0, 0.0, 0.0, 0.0], th.border)
        .child(
            row()
                .h(28.0)
                .w_full()
                .items_center()
                .gap(8.0)
                .px(12.0)
                .fill(th.surface1)
                .border_sides([0.0, 0.0, 1.0, 0.0], th.border)
                .child(
                    text("Mixer")
                        .font(daw_label(11.0))
                        .text_case(rune::TextCase::Uppercase)
                        .color(th.muted_fg),
                )
                .child(
                    text("Press X to hide")
                        .font(ui(10.5, 400.0))
                        .color(th.muted_fg),
                ),
        )
        .child(row().w_full().child(strips).scroll_x())
}
