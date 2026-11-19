//! The header column: `GroupHeader`, `TrackHeader` (+ `TrackToggle`, `PanDial`, the volume
//! slider, the automation menu, the instrument popover) and the automation-lane header cells.

use mdaw_engine::synth::bank::synth_presets;
use mdaw_engine::synth::presets::PRESET_VIBES;
use mdaw_model::automation::get_automation_parameter_label;
use mdaw_model::{InstrumentType, Track, TrackGroup};
use rune::{
    Align, Color, El, Key, KeyEvent, LineCap, Phase, Shape, Tx, WheelEvent, canvas, col, el, icon,
    row, text, text_input,
};

use crate::app::{App, Menu};
use crate::icons;
use crate::icons_song;
use crate::song::{
    ALL_AUTOMATION_PARAMS, AUTO_H, GROUP_H, MAX_VOLUME, MIN_VOLUME, Row, SongMenu, TRACK_H,
    pan_label, visible_lanes,
};
use crate::theme::{R_LG, R_MD, R_SM, daw_label, mono, t, ui};
use crate::ui::song::{cmix, track_color};
use crate::ui::widgets::{Button, Size, Variant, popover, popover_frame};

/// `TrackToggle`'s kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToggleKind {
    Mute,
    Solo,
    Arm,
}

/// `TrackToggle` (sm 18 / md 24): M, S or ●; mute amber, solo blue, arm red.
pub fn toggle(
    key: String,
    kind: ToggleKind,
    active: bool,
    title: &str,
    md: bool,
    on: impl Fn(&mut App) + 'static,
) -> El {
    let th = t();
    let (bg, fg) = match kind {
        ToggleKind::Mute => (th.warn, Color::BLACK),
        ToggleKind::Solo => (th.select, Color::BLACK),
        ToggleKind::Arm => (th.rec, Color::WHITE),
    };
    let h = if md { 24.0 } else { 18.0 };
    let content: El = match kind {
        ToggleKind::Mute | ToggleKind::Solo => {
            text(if kind == ToggleKind::Mute { "M" } else { "S" })
                .font(mono(if md { 11.0 } else { 10.0 }, 700.0))
                .color(if active { fg } else { th.muted_fg })
        }
        ToggleKind::Arm => {
            let d = if md { 10.0 } else { 8.0 };
            el().size(d, d).radius(d / 2.0).fill(if active {
                Color::WHITE
            } else {
                th.rec.alpha(0.8)
            })
        }
    };
    let mut b = row()
        .key(key)
        .role("button")
        .label(title.to_owned())
        .help(title.to_owned())
        .checked(active)
        .h(h)
        .min_w(h)
        .px(if md { 6.0 } else { 4.0 })
        .center()
        .radius(4.0)
        .child(content)
        .on_press(on);
    b = if active {
        b.fill(bg).border(1.0, bg)
    } else {
        b.fill(th.surface2)
            .border(1.0, th.border)
            .hover(th.surface3)
    };
    b
}

/// The header column (rows in render order, scrolled with the lanes).
pub fn column(s: &App) -> El {
    let th = t();
    let tracks = s.tracks();
    let rows = s.song_rows();
    let sy = s.song.scroll[1];
    // Only the rows that show are built, as the lanes (48 tracks: every header on every
    // rebuild made a drag frame 18 ms, ⌘A 77 ms).
    let vh = s.song.viewport.get()[1];
    let shows = |top: f32, h: f32| top - sy < vh && top - sy + h > 0.0;
    let mut c = el()
        .key("song-header-rows")
        .grow(1.0)
        .w_full()
        .min_h(0.0)
        .clip_rounded(0.0)
        .on_wheel(|s: &mut App, e: WheelEvent, _tx: &mut Tx| {
            s.song.scroll[1] -= e.dy;
            s.clamp_song_scroll();
            true
        });
    for r in rows.iter() {
        match r {
            Row::Group { id, top } => {
                if !shows(*top, GROUP_H) {
                    continue;
                }
                if let Some(g) = s.svc.engine.doc().get_groups().iter().find(|g| &g.id == id) {
                    c = c.child(group_header(s, g).absolute(0.0, top - sy));
                }
            }
            Row::Track { id, top, height } => {
                if !shows(*top, *height) {
                    continue;
                }
                let Some(track) = s.find_track(id) else {
                    continue;
                };
                if shows(*top, TRACK_H) {
                    c = c.child(track_header(s, track).absolute(0.0, top - sy));
                }
                for (i, lane) in visible_lanes(track).into_iter().enumerate() {
                    let ly = top + TRACK_H + i as f32 * AUTO_H;
                    if shows(ly, AUTO_H) {
                        c = c.child(lane_header(track, lane.parameter).absolute(0.0, ly - sy));
                    }
                }
            }
        }
    }
    if tracks.is_empty() {
        c = c.child(
            col()
                .key("song-empty")
                .absolute(0.0, 0.0)
                .w_full()
                .items_start()
                .gap(8.0)
                .px(12.0)
                .py(16.0)
                .child(text("No tracks yet").font(ui(12.0, 600.0)).color(th.fg))
                .child(
                    text("Add a synth or drum track, arm it, and press R to record from your keyboard.")
                        .key("song-empty-hint")
                        // `leading-relaxed`: 1.625 × 11.
                        .font(ui(11.0, 400.0).lh(17.875))
                        .color(th.muted_fg)
                        .wrap(),
                )
                .child(
                    row()
                        .gap(6.0)
                        .pt(4.0)
                        .child(
                            Button::new(Variant::Primary, Size::Sm)
                                .icon(&icons::PLUS)
                                .label("Synth track")
                                .key("empty-add-synth")
                                .build(|s: &mut App, _| s.add_song_track(InstrumentType::Synth)),
                        )
                        .child(
                            Button::new(Variant::Outline, Size::Sm)
                                .icon(&icons::PLUS)
                                .label("Drums")
                                .key("empty-add-drums")
                                .build(|s: &mut App, _| s.add_song_track(InstrumentType::Drums)),
                        ),
                ),
        );
    }
    c
}

/// The automation-lane arrow's offset from the row's top padding (Chromium's glyph top).
const ARROW_TOP: f32 = 2.0;

fn lane_header(track: &Track, p: mdaw_model::AutomationParameter) -> El {
    let th = t();
    row()
        .w_full()
        .h(AUTO_H)
        .items_start()
        .gap(6.0)
        .pt(6.0)
        .pr(8.0)
        .pl(20.0)
        .fill(th.surface1)
        .border_sides([0.0, 0.0, 1.0, 0.0], th.border)
        .child(
            el().absolute(0.0, 0.0)
                .w(4.0)
                .h(AUTO_H)
                .fill(track_color(&track.color).alpha(0.4)),
        )
        // `↳` at `text-[10px]`: Manrope has no such glyph and Chromium's fallback draws an
        // 8 × 6 arrow (Rune's fallback is half that), so it is drawn as a shape of that size.
        .child(
            canvas(
                Shape::new()
                    .line([0.5, 0.0], [0.5, 5.0], 1.0, LineCap::Butt, th.muted_fg)
                    .line([0.5, 4.5], [7.5, 4.5], 1.0, LineCap::Butt, th.muted_fg)
                    .line([5.5, 2.5], [7.5, 4.5], 1.0, LineCap::Butt, th.muted_fg)
                    .line([5.5, 6.5], [7.5, 4.5], 1.0, LineCap::Butt, th.muted_fg),
            )
            .key(format!(
                "auto-arrow-{}-{}",
                track.id,
                crate::ui::song::timeline::param_key(p)
            ))
            .size(9.0, 7.0)
            .mt(ARROW_TOP)
            .ml(1.0),
        )
        .child(
            text(get_automation_parameter_label(p))
                .font(ui(11.0, 500.0))
                .color(th.muted_fg)
                .truncate(),
        )
}

fn group_header(s: &App, g: &TrackGroup) -> El {
    let th = t();
    let gc = track_color(&g.color);
    let gt = s.group_tracks(&g.id);
    let all_muted = !gt.is_empty() && gt.iter().all(|t| t.muted);
    let any_solo = gt.iter().any(|t| t.solo);
    let id = g.id.clone();
    let editing = s
        .song
        .group_rename
        .as_ref()
        .filter(|(gid, _)| *gid == g.id)
        .map(|(_, e)| e);
    let id2 = id.clone();
    let id3 = id.clone();
    let id4 = id.clone();
    let id5 = id.clone();
    let name_el: El = match editing {
        Some(edit) => text_input(edit, ui(11.0, 400.0))
            .key("group-rename")
            .single_line()
            .autofocus()
            .color(th.fg)
            .h(20.0)
            .grow(1.0)
            .min_w(0.0)
            .px(4.0)
            .radius(3.0)
            .border(1.0, th.primary)
            .fill(th.surface0)
            .on_edit(|s: &mut App| {
                &mut s
                    .song
                    .group_rename
                    .get_or_insert_with(|| (String::new(), rune::TextEdit::new("")))
                    .1
            })
            .on_input_key(|s: &mut App, ev: &KeyEvent, _tx: &mut Tx| {
                if matches!(ev.key, Key::Escape) {
                    s.song.group_rename = None;
                    return true;
                }
                false
            })
            .on_submit(|s: &mut App, _tx: &mut Tx| s.commit_group_rename()),
        None => row()
            .key(format!("group-name-{}", g.id))
            .grow(1.0)
            .min_w(0.0)
            .items_center()
            .cursor(rune::Cursor::Text)
            .help("Double-click to rename")
            .child(
                text(g.name.clone())
                    .font(ui(11.0, 600.0))
                    .color(th.fg)
                    .truncate(),
            )
            .child(
                text(g.track_ids.len().to_string())
                    .font(mono(10.0, 400.0))
                    .color(th.muted_fg)
                    .ml(6.0),
            )
            .on_double_press_tx(move |s: &mut App, tx: &mut Tx| {
                let name = s
                    .svc
                    .engine
                    .doc()
                    .get_groups()
                    .iter()
                    .find(|g| g.id == id2)
                    .map(|g| g.name.clone())
                    .unwrap_or_default();
                s.song.group_rename = Some((id2.clone(), crate::song::selected_edit(name)));
                tx.focus("group-rename", true);
            }),
    };
    row()
        .key(format!("group-{}", g.id))
        .w_full()
        .h(GROUP_H)
        .items_center()
        .gap(4.0)
        .pr(8.0)
        .pl(10.0)
        .fill(cmix(gc, th.surface2, 0.09))
        .border_sides([0.0, 0.0, 1.0, 0.0], th.border)
        .hover_group()
        .child(el().absolute(0.0, 0.0).w(4.0).h(GROUP_H).fill(gc))
        .child(
            Button::new(Variant::Ghost, Size::IconXs)
                .icon(if g.collapsed {
                    &icons::CHEVRON_RIGHT
                } else {
                    &icons::CHEVRON_DOWN
                })
                .icon_size(14.0)
                .a11y(if g.collapsed {
                    "Expand group"
                } else {
                    "Collapse group"
                })
                .key(format!("group-collapse-{}", g.id))
                .build(move |s: &mut App, _| s.toggle_group_collapsed(&id))
                .size(20.0, 20.0),
        )
        .child(icon(&icons::FOLDER_OPEN, 14.0).color(th.muted_fg))
        .child(name_el)
        .child(toggle(
            format!("group-mute-{}", g.id),
            ToggleKind::Mute,
            all_muted,
            "Mute group",
            false,
            move |s: &mut App| s.toggle_group_mute(&id3),
        ))
        .child(toggle(
            format!("group-solo-{}", g.id),
            ToggleKind::Solo,
            any_solo,
            "Solo group",
            false,
            move |s: &mut App| s.toggle_group_solo(&id4),
        ))
        .child(
            Button::new(Variant::Ghost, Size::IconXs)
                .icon(&icons::X)
                .icon_size(12.0)
                .a11y("Ungroup")
                .tip("Ungroup", None)
                .key(format!("group-ungroup-{}", g.id))
                .build(move |s: &mut App, _| s.remove_group(&id5))
                .size(20.0, 20.0)
                .group_opacity(0.0, 1.0),
        )
}

/// The small `PanDial` (18 points): drag vertically 80 px per unit, double-click centres,
/// arrows ±0.05.
fn pan_dial(track: &Track) -> El {
    let th = t();
    let v = track.pan;
    let label = pan_label(v);
    let angle = (v as f32 * 135.0 - 90.0).to_radians();
    let tip = [9.0 + 6.5 * angle.cos(), 9.0 + 6.5 * angle.sin()];
    let circle: Vec<[f32; 2]> = (0..32)
        .map(|i| {
            let a = i as f32 / 32.0 * std::f32::consts::TAU;
            [9.0 + 7.5 * a.cos(), 9.0 + 7.5 * a.sin()]
        })
        .collect();
    let sh = Shape::new()
        .fill(circle.clone(), th.surface3)
        .stroke_with(circle, true, 1.0, LineCap::Butt, th.border)
        .line(
            [9.0, 9.0],
            tip,
            1.75,
            LineCap::Round,
            if v.abs() > 0.005 { th.primary } else { th.fg },
        );
    let id = track.id.clone();
    let id2 = id.clone();
    let id3 = id.clone();
    let id4 = id.clone();
    let id5 = id.clone();
    el().key(format!("pan-{}", track.id))
        .role("slider")
        .label("Pan")
        .description(label.clone())
        .help(format!("Pan {label}"))
        // aria-valuemin / -max / -now / -valuetext and the AT actions (the arrows are `on_key`).
        .range_value(v, -1.0, 1.0)
        .range_step(PAN_STEP)
        .a11y_value(label.clone())
        .focusable(true)
        .size(18.0, 18.0)
        .radius(9.0)
        .cursor(rune::Cursor::ResizeRow)
        .child(canvas(sh).absolute(0.0, 0.0).size(18.0, 18.0))
        .on_drag(
            move |s: &mut App, d: &mut rune::Drag, _tx: &mut Tx| match d.phase {
                Phase::Down => {
                    let v = s.find_track(&id).map_or(0.0, |t| t.pan);
                    s.song.pan_drag = Some((id.clone(), v));
                }
                Phase::Move => {
                    if let Some((tid, start)) = s.song.pan_drag.clone() {
                        let v = clamp_pan(start + (-d.dy as f64) / 80.0);
                        s.set_track_pan(&tid, v);
                    }
                }
                Phase::Up => s.song.pan_drag = None,
            },
        )
        .on_double_press(move |s: &mut App| s.set_track_pan(&id2, 0.0))
        .on_key(move |s: &mut App, ev: &KeyEvent, _tx: &mut Tx| {
            let v = s.find_track(&id3).map_or(0.0, |t| t.pan);
            let next = match ev.key {
                Key::Up | Key::Right => clamp_pan(v + PAN_STEP),
                Key::Down | Key::Left => clamp_pan(v - PAN_STEP),
                Key::Home => -1.0,
                Key::End => 1.0,
                _ => return false,
            };
            s.set_track_pan(&id3, next);
            true
        })
        .on_step(move |s: &mut App, delta: i32, _tx: &mut Tx| {
            let v = s.find_track(&id4).map_or(0.0, |t| t.pan);
            s.set_track_pan(&id4, clamp_pan(v + delta as f64 * PAN_STEP));
        })
        .on_step_limit(move |s: &mut App, max: bool, _tx: &mut Tx| {
            s.set_track_pan(&id5, if max { 1.0 } else { -1.0 });
        })
}

/// The pan dial's keyboard step.
pub const PAN_STEP: f64 = 0.05;

/// `clamp` of the pan dial: −1..1, rounded to 0.01.
pub fn clamp_pan(v: f64) -> f64 {
    ((v * 100.0).round() / 100.0).clamp(-1.0, 1.0)
}

/// The header volume slider's thumb (`size-3`).
pub const VOL_THUMB: f32 = 12.0;

/// One geometry for the header volume slider's paint and input: Radix keeps the thumb inside
/// the track, its centre at `6 + f × (w − 12)`; a press maps back through the same line, so
/// pressing the thumb keeps its value (the paint used `f × w` and the input this, so pressing
/// the −6 dB thumb set −3 dB). Radix itself maps a press over `[0, w]` (a press on the thumb
/// jumps a little near the ends); the shared line is the deliberate choice here.
pub fn vol_thumb_center(f: f32, w: f32) -> f32 {
    VOL_THUMB / 2.0 + f.clamp(0.0, 1.0) * (w - VOL_THUMB).max(0.0)
}

/// The value under `x` of a `w`-point slider (whole dB, −40..0).
pub fn vol_from_x(x: f32, w: f32) -> f64 {
    let f = ((x - VOL_THUMB / 2.0) / (w - VOL_THUMB).max(1.0)).clamp(0.0, 1.0) as f64;
    (MIN_VOLUME + f * (MAX_VOLUME - MIN_VOLUME)).round()
}

/// The fraction of the track volume `v`.
pub fn vol_fraction(v: f64) -> f32 {
    ((v - MIN_VOLUME) / (MAX_VOLUME - MIN_VOLUME)).clamp(0.0, 1.0) as f32
}

/// The track volume `Slider` (−40..0 dB, step 1). Painted in order: the rail, the range up to
/// the thumb, the thumb last (on top); the thumb's centre is [`vol_thumb_center`]
/// (a grown left part of `f × (w − 12)`, then the 12-point thumb).
fn volume_slider(track: &Track) -> El {
    let th = t();
    let f = vol_fraction(track.volume);
    let id = track.id.clone();
    let (i2, i3) = (id.clone(), id.clone());
    let db = crate::song::js_num(track.volume);
    // `border border-black/40 bg-foreground shadow-[0_1px_2px_oklch(0_0_0/0.5)]`: Chromium
    // shows the range colour through the translucent border (a dark orange ring), so the ring
    // is black/40 over the primary and the face is inset by the border.
    let thumb = el()
        .key(format!("vol-{}-thumb", track.id))
        .size(VOL_THUMB, VOL_THUMB)
        .radius(VOL_THUMB / 2.0)
        .fill(th.primary)
        .border(1.0, Color::BLACK.alpha(0.4))
        .box_shadow(rune::Shadow {
            x: 0.0,
            y: 1.0,
            blur: 2.0,
            spread: 0.0,
            color: Color::BLACK.alpha(0.5),
            inset: false,
        })
        .child(
            el().absolute(1.0, 1.0)
                .size(VOL_THUMB - 2.0, VOL_THUMB - 2.0)
                .radius(VOL_THUMB / 2.0 - 1.0)
                .fill(th.fg),
        );
    el().key(format!("vol-{}", track.id))
        .role("slider")
        .label("Track volume")
        .help(format!("Volume {db} dB"))
        .range_value(track.volume, MIN_VOLUME, MAX_VOLUME)
        .range_step(1.0)
        .a11y_value(format!("{db} dB"))
        .focusable(true)
        .grow(1.0)
        .min_w(0.0)
        .h(14.0)
        .ml(4.0)
        .child(
            el().absolute(0.0, 5.0)
                .w_full()
                .h(4.0)
                .radius(2.0)
                .fill(th.surface4),
        )
        .child(
            row()
                .absolute(0.0, 0.0)
                .w_full()
                .h(14.0)
                .items_center()
                .child(
                    el().grow(f)
                        .h(4.0)
                        .corners([2.0, 0.0, 0.0, 2.0])
                        .fill(th.primary),
                )
                .child(thumb)
                .child(el().grow(1.0 - f)),
        )
        .on_drag(move |s: &mut App, d: &mut rune::Drag, _tx: &mut Tx| {
            if d.phase == Phase::Up {
                return;
            }
            s.set_track_volume(&id, vol_from_x(d.x, d.w));
        })
        // Radix's keys: arrows ±1 dB (Page Up / Down ±10), Home / End the ends.
        .on_step(move |s: &mut App, delta: i32, _tx: &mut Tx| {
            let v = s.find_track(&i2).map_or(0.0, |t| t.volume);
            s.set_track_volume(
                &i2,
                (v + delta as f64).clamp(MIN_VOLUME, MAX_VOLUME).round(),
            );
        })
        .on_step_limit(move |s: &mut App, max: bool, _tx: &mut Tx| {
            s.set_track_volume(&i3, if max { MAX_VOLUME } else { MIN_VOLUME });
        })
}

pub fn track_header(s: &App, track: &Track) -> El {
    let th = t();
    let doc = s.svc.engine.doc();
    let fx_open = s.song.fx_track.as_deref() == Some(track.id.as_str());
    let armed = doc.get_armed_track_id() == Some(track.id.as_str());
    let recording = s.svc.engine.get_recording_track_id() == Some(track.id.as_str());
    let is_drums = track.instrument.kind == InstrumentType::Drums;
    let is_audio = track.instrument.kind == InstrumentType::Audio;
    let active_drums =
        is_drums && doc.get_active_drum_track_id().as_deref() == Some(track.id.as_str());
    let tc = track_color(&track.color);
    let lanes = track.automation.clone().unwrap_or_default();
    let has_visible = lanes.iter().any(|l| l.visible);
    let auto_open = s.menu
        == Some(Menu::Song(SongMenu::Automation {
            track: track.id.clone(),
        }));
    let id = track.id.clone();

    let name_color = if track.muted { th.muted_fg } else { th.fg };
    let mut r1 = row().h(20.0).w_full().items_center().gap(4.0);
    if is_drums {
        let i1 = id.clone();
        let i2 = id.clone();
        let mut chip = row()
            .key(format!("beat-chip-{}", track.id))
            .role("button")
            .label(if active_drums {
                format!("{}: active drum track", track.name)
            } else {
                format!("Make {} the active drum track", track.name)
            })
            .checked(active_drums)
            .help(if active_drums {
                "Active drum track: the Beat view edits this one"
            } else {
                "Edit this drum track in the Beat view"
            })
            .h(16.0)
            .px(4.0)
            .center()
            .radius(3.0)
            .child(text("Beat").font(mono(11.0, 600.0)).color(if active_drums {
                th.primary
            } else {
                th.muted_fg
            }))
            .on_press(move |s: &mut App| s.set_active_drum_track(&i1));
        chip = if active_drums {
            chip.fill(th.primary.alpha(0.2))
        } else {
            chip.border(1.0, th.border)
        };
        r1 = r1.child(chip).child(
            row()
                .key(format!("track-name-{}", track.id))
                .grow(1.0)
                .min_w(0.0)
                .help(if active_drums {
                    track.name.clone()
                } else {
                    format!("{} (click to edit in the Beat view)", track.name)
                })
                .child(
                    text(track.name.clone())
                        .font(ui(12.0, 600.0))
                        .color(name_color)
                        .truncate(),
                )
                .on_press(move |s: &mut App| s.set_active_drum_track(&i2)),
        );
    } else {
        r1 = r1.child(
            row()
                .key(format!("track-name-{}", track.id))
                .grow(1.0)
                .min_w(0.0)
                .help(track.name.clone())
                .child(
                    text(track.name.clone())
                        .font(ui(12.0, 600.0))
                        .color(name_color)
                        .truncate(),
                ),
        );
    }
    let ifx = id.clone();
    let mut fx_btn = Button::new(Variant::Ghost, Size::IconXs)
        .icon(&icons::SLIDERS_HORIZONTAL)
        .icon_size(12.0)
        .a11y("Track effects")
        .tip("Track effects", None)
        .pressed(fx_open)
        .key(format!("fx-{}", track.id));
    if fx_open {
        // The TS asks for `bg-primary/15 text-primary`, but the ghost Button's
        // `aria-pressed:bg-surface-3 aria-pressed:text-foreground` wins in Chromium (a variant
        // tailwind-merge doesn't drop): the open drawer's button is grey with a light icon.
        fx_btn = fx_btn.fill(th.surface3).ink(th.fg).icon_ink(th.fg);
    }
    let iauto = id.clone();
    let mut auto_btn = Button::new(Variant::Ghost, Size::IconXs)
        .icon(&icons_song::ACTIVITY)
        .icon_size(12.0)
        .a11y("Automation lanes")
        .key(format!("automation-{}", track.id));
    if !auto_open {
        auto_btn = auto_btn.tip("Automation lanes", None);
    }
    if has_visible || auto_open {
        auto_btn = auto_btn.ink(th.primary).icon_ink(th.primary);
    }
    let idel = id.clone();
    r1 = r1
        .child(
            fx_btn
                .build(move |s: &mut App, _| s.toggle_fx(&ifx))
                .size(20.0, 20.0),
        )
        .child(
            auto_btn
                .build(move |s: &mut App, _| {
                    s.toggle_menu(Menu::Song(SongMenu::Automation {
                        track: iauto.clone(),
                    }))
                })
                .size(20.0, 20.0)
                .expanded(auto_open),
        )
        .child(
            Button::new(Variant::Ghost, Size::IconXs)
                .icon(&icons::TRASH_2)
                .icon_size(12.0)
                .a11y("Delete track")
                .tip("Delete track", None)
                .key(format!("delete-{}", track.id))
                .build(move |s: &mut App, _| s.remove_song_track(&idel))
                .size(20.0, 20.0)
                .group_opacity(0.0, 1.0),
        );

    // Row 2: instrument.
    let r2: El = if is_audio {
        row()
            .h(16.0)
            .items_center()
            .gap(4.0)
            .child(icon(&icons::FILE_AUDIO, 12.0).color(th.muted_fg))
            .child(text("Audio").font(ui(11.0, 400.0)).color(th.muted_fg))
    } else {
        let name = if is_drums {
            "Drums".to_owned()
        } else {
            synth_presets()
                .get(track.instrument.preset_index as usize)
                .map_or("Unknown".to_owned(), |p| p.meta().name.clone())
        };
        let open = s.menu
            == Some(Menu::Song(SongMenu::Instrument {
                track: track.id.clone(),
            }));
        let iinst = id.clone();
        let mut tr = row()
            .key(format!("instrument-{}", track.id))
            .role("button")
            .label("Change instrument")
            .help("Change instrument")
            .h(16.0)
            .ml(-4.0)
            .px(4.0)
            .items_center()
            .gap(4.0)
            .radius(3.0)
            .hover(th.surface3)
            .focusable(true)
            .on_press(move |s: &mut App| {
                s.song.instrument_search = rune::TextEdit::new("");
                s.song.vibe = None;
                s.song.instrument_active = 0;
                s.toggle_menu(Menu::Song(SongMenu::Instrument {
                    track: iinst.clone(),
                }));
            });
        if open {
            tr = tr.fill(th.surface3);
        }
        if is_drums {
            tr = tr.child(icon(&icons::DRUM, 12.0).color(th.muted_fg));
        }
        row().h(16.0).items_center().child(
            tr.child(text(name).font(ui(11.0, 400.0)).color(if open {
                th.fg
            } else {
                th.muted_fg
            }))
            .child(icon(&icons::CHEVRON_DOWN, 12.0).color(th.muted_fg.alpha(0.6))),
        )
    };

    // Row 3: arm / mute / solo, volume, pan.
    let mut r3 = row().h(18.0).w_full().items_center().gap(4.0);
    if !is_audio {
        let ia = id.clone();
        r3 = r3.child(toggle(
            format!("arm-{}", track.id),
            ToggleKind::Arm,
            armed || recording,
            if armed {
                "Disarm track"
            } else {
                "Arm track for recording"
            },
            false,
            move |s: &mut App| s.toggle_arm(&ia),
        ));
    }
    let im = id.clone();
    let isolo = id.clone();
    r3 = r3
        .child(toggle(
            format!("mute-{}", track.id),
            ToggleKind::Mute,
            track.muted,
            if track.muted {
                "Unmute track"
            } else {
                "Mute track"
            },
            false,
            move |s: &mut App| s.toggle_track_mute(&im),
        ))
        .child(toggle(
            format!("solo-{}", track.id),
            ToggleKind::Solo,
            track.solo,
            if track.solo {
                "Unsolo track"
            } else {
                "Solo track"
            },
            false,
            move |s: &mut App| s.toggle_track_solo(&isolo),
        ))
        .child(volume_slider(track))
        .child(
            text(if track.volume > 0.0 {
                format!("+{}", crate::song::js_num(track.volume))
            } else {
                crate::song::js_num(track.volume)
            })
            .key(format!("vol-readout-{}", track.id))
            .font(mono(11.0, 400.0))
            .color(th.muted_fg)
            .w(30.0)
            .align(Align::End)
            .help("Volume (dB)"),
        )
        .child(pan_dial(track));

    let hover_bg = cmix(th.surface1, th.surface2, 0.8);
    col()
        .key(format!("track-{}", track.id))
        .w_full()
        .h(TRACK_H)
        .justify_center()
        .gap(4.0)
        .pr(8.0)
        .pl(14.0)
        .fill(if fx_open { th.surface2 } else { th.surface1 })
        .hover(hover_bg)
        .hover_group()
        .border_sides([0.0, 0.0, 1.0, 0.0], th.border)
        .child(
            el().absolute(0.0, 0.0)
                .w(4.0)
                .h(TRACK_H)
                .fill(tc.alpha(if track.muted { 0.35 } else { 1.0 })),
        )
        .child(r1)
        .child(r2)
        .child(r3)
}

/// The "Automate" menu of a track (fixed under its button).
pub fn automation_menu(s: &App, track_id: &str) -> El {
    let th = t();
    let lanes = s
        .find_track(track_id)
        .and_then(|t| t.automation.clone())
        .unwrap_or_default();
    let mut m = popover_frame(None, 4.0)
        .key("automation-menu")
        .role("menu")
        .min_w(168.0)
        .radius(R_LG)
        .child(
            text("Automate")
                .font(daw_label(11.0))
                .text_case(rune::TextCase::Uppercase)
                .color(th.muted_fg)
                .px(8.0)
                .pt(4.0)
                .pb(6.0),
        );
    for p in ALL_AUTOMATION_PARAMS {
        let visible = lanes
            .iter()
            .find(|l| l.parameter == p)
            .is_some_and(|l| l.visible);
        let tid = track_id.to_owned();
        let mut sq = el().size(8.0, 8.0).radius(2.0);
        sq = if visible {
            sq.fill(th.primary).border(1.0, th.primary)
        } else {
            sq.border(1.0, th.muted_fg.alpha(0.5))
        };
        m = m.child(
            row()
                .key(format!(
                    "automate-{}",
                    crate::ui::song::timeline::param_key(p)
                ))
                .role("menuitemcheckbox")
                .checked(visible)
                .label(get_automation_parameter_label(p))
                .h(28.0)
                .w_full()
                .items_center()
                .gap(8.0)
                .px(8.0)
                .radius(6.0)
                .hover(th.surface3)
                .child(sq)
                .child(
                    text(get_automation_parameter_label(p))
                        .font(ui(12.0, if visible { 600.0 } else { 400.0 }))
                        .color(if visible { th.fg } else { th.muted_fg }),
                )
                .on_press(move |s: &mut App| s.toggle_automation_param(&tid, p)),
        );
    }
    if !lanes.is_empty() {
        let tid = track_id.to_owned();
        m = m.child(el().h(1.0).w_full().my(4.0).fill(th.border)).child(
            row()
                .key("automate-clear")
                .role("menuitem")
                .label("Clear all")
                .h(28.0)
                .w_full()
                .items_center()
                .px(8.0)
                .radius(6.0)
                .hover(th.surface3)
                .child(text("Clear all").font(ui(12.0, 400.0)).color(th.muted_fg))
                .on_press(move |s: &mut App| {
                    s.clear_automation(&tid);
                    s.menu = None;
                }),
        );
    }
    popover(&format!("automation-{track_id}"), Align::Start, m)
}

/// cmdk-like filter: every query character appears in order (case-insensitive).
pub fn matches_query(value: &str, q: &str) -> bool {
    let q = q.trim().to_lowercase();
    if q.is_empty() {
        return true;
    }
    let v = value.to_lowercase();
    let mut it = v.chars();
    q.chars()
        .filter(|c| !c.is_whitespace())
        .all(|c| it.any(|x| x == c))
}

/// One result of the instrument popover, in display order.
#[derive(Debug, Clone, PartialEq)]
pub struct InstrumentChoice {
    pub key: String,
    /// The group heading ("Drums" or the vibe's label).
    pub group: &'static str,
    pub name: String,
    pub category: String,
    pub kind: InstrumentType,
    pub preset: i64,
    pub current: bool,
}

/// The popover's results for the current search and vibe filter (what is shown, in order):
/// render and keys share it, so Down / Up / Return walk exactly the rows on screen.
pub fn instrument_results(s: &App, track_id: &str) -> Vec<InstrumentChoice> {
    let Some(track) = s.find_track(track_id) else {
        return Vec::new();
    };
    let q = s.song.instrument_search.text.clone();
    let mut out = Vec::new();
    if matches_query("drums drum kit", &q) {
        out.push(InstrumentChoice {
            key: "instrument-drums".into(),
            group: "Drums",
            name: "Drum kit".into(),
            category: "percussion".into(),
            kind: InstrumentType::Drums,
            preset: 0,
            current: track.instrument.kind == InstrumentType::Drums,
        });
    }
    for (v, label, _) in PRESET_VIBES {
        if s.song.vibe.is_some_and(|f| f != v) {
            continue;
        }
        let vibe_key = format!("{v:?}").to_lowercase();
        for (i, p) in synth_presets().iter().enumerate() {
            let m = p.meta();
            if m.vibe != v {
                continue;
            }
            let cat = format!("{:?}", m.category);
            if !matches_query(&format!("{} {} {}", m.name, cat, vibe_key), &q) {
                continue;
            }
            out.push(InstrumentChoice {
                key: format!("instrument-preset-{i}"),
                group: label,
                name: m.name.clone(),
                category: cat,
                kind: InstrumentType::Synth,
                preset: i as i64,
                current: track.instrument.kind == InstrumentType::Synth
                    && track.instrument.preset_index == i as i64,
            });
        }
    }
    out
}

/// Choose result `i` of the popover: set the instrument, close, focus back on the trigger.
pub fn choose_instrument(s: &mut App, track_id: &str, i: usize, tx: &mut Tx) {
    let Some(c) = instrument_results(s, track_id).into_iter().nth(i) else {
        return;
    };
    s.set_track_instrument(track_id, c.kind, c.preset);
    s.menu = None;
    tx.focus(format!("instrument-{track_id}"), true);
}

/// The instrument popover (search, vibe chips, Drums + one group per vibe). The search field
/// keeps focus; Down / Up move the active result (cmdk's `data-selected`, the first one at
/// first and after each edit of the query), Return chooses it, focus returns to the trigger.
pub fn instrument_menu(s: &App, track_id: &str) -> El {
    let th = t();
    if s.find_track(track_id).is_none() {
        return el();
    }
    let presets = synth_presets();
    let results = instrument_results(s, track_id);
    let active = s
        .song
        .instrument_active
        .min(results.len().saturating_sub(1));
    let tid_keys = track_id.to_owned();
    // Rune draws borders inside the box; the DOM adds them: `h-10` + `border-b` is 41 points,
    // and the popover's own top border pushes the content down 1 (`pt(1)` on the frame).
    let search = row()
        .h(41.0)
        .w_full()
        .items_center()
        .gap(8.0)
        .px(12.0)
        .border_sides([0.0, 0.0, 1.0, 0.0], th.border)
        .child(icon(&icons::SEARCH, 16.0).color(th.muted_fg.alpha(0.5)))
        .child(
            text_input(&s.song.instrument_search, ui(13.0, 400.0))
                .key("instrument-search")
                .placeholder("Search sounds…", th.muted_fg)
                .single_line()
                .autofocus()
                .color(th.fg)
                .grow(1.0)
                .on_edit(|s: &mut App| &mut s.song.instrument_search)
                .on_input_key(move |s: &mut App, ev: &KeyEvent, tx: &mut Tx| {
                    let n = instrument_results(s, &tid_keys).len();
                    let a = s.song.instrument_active.min(n.saturating_sub(1));
                    match ev.key {
                        Key::Down => s.song.instrument_active = (a + 1).min(n.saturating_sub(1)),
                        Key::Up => s.song.instrument_active = a.saturating_sub(1),
                        Key::Enter if n > 0 => choose_instrument(s, &tid_keys, a, tx),
                        _ => {
                            // An edit of the query: cmdk selects the first match again.
                            s.song.instrument_active = 0;
                            return false;
                        }
                    }
                    true
                }),
        );
    let mut chips = row()
        .w_full()
        .gap(4.0)
        .px(8.0)
        .pt(8.0)
        .pb(9.0)
        .border_sides([0.0, 0.0, 1.0, 0.0], th.border)
        .clip_rounded(0.0);
    let chip = |key: String, label: &str, on: bool| {
        let mut c = row()
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
        c = if on {
            c.fill(th.primary)
        } else {
            c.fill(th.surface3).hover(th.surface4)
        };
        c
    };
    chips = chips.child(
        chip("vibe-all".into(), "All", s.song.vibe.is_none()).on_press(|s: &mut App| {
            s.song.vibe = None;
            s.song.instrument_active = 0;
        }),
    );
    for (v, label, _) in PRESET_VIBES {
        if !presets.iter().any(|p| p.meta().vibe == v) {
            continue;
        }
        chips = chips.child(
            chip(format!("vibe-{label}"), label, s.song.vibe == Some(v)).on_press(
                move |s: &mut App| {
                    s.song.vibe = if s.song.vibe == Some(v) {
                        None
                    } else {
                        Some(v)
                    };
                    s.song.instrument_active = 0;
                },
            ),
        );
    }
    // `[cmdk-group-heading]`: px-2 pt-2 pb-1, 10.5 px bold uppercase, 0.08 em.
    let heading = |l: &str| {
        text(l.to_owned())
            .font(daw_label(10.5))
            .text_case(rune::TextCase::Uppercase)
            .color(th.muted_fg)
            .px(8.0)
            .pt(8.0)
            .pb(4.0)
    };
    let mut list = col().w_full();
    let mut group: Option<El> = None;
    let mut group_name = "";
    let mut groups = 0;
    for (i, c) in results.iter().enumerate() {
        if group.is_none() || c.group != group_name {
            if let Some(g) = group.take() {
                list = list.child(g);
            }
            // `CommandGroup` (p-1); the groups after the first have a top border.
            let mut g = col().w_full().padding(4.0).child(heading(c.group));
            if groups > 0 {
                g = g
                    .pt(5.0)
                    .border_sides([1.0, 0.0, 0.0, 0.0], th.border.alpha(0.6));
            }
            groups += 1;
            group = Some(g);
            group_name = c.group;
        }
        let tid = track_id.to_owned();
        let selected = i == active;
        let mut item = row()
            .key(c.key.clone())
            .role("option")
            .label(c.name.clone())
            .focus_selected(selected)
            .w_full()
            .items_center()
            .justify_between()
            .px(8.0)
            .py(6.0)
            .radius(R_MD)
            .child(
                text(c.name.clone())
                    .font(ui(13.0, if c.current { 600.0 } else { 400.0 }))
                    .color(if c.current { th.primary } else { th.fg }),
            )
            .child(
                text(c.category.clone())
                    .font(ui(11.0, 400.0))
                    .color(th.muted_fg),
            )
            .on_hover(move |s: &mut App| s.song.instrument_active = i)
            .on_press_tx(move |s: &mut App, tx: &mut Tx| choose_instrument(s, &tid, i, tx));
        if selected {
            item = item.fill(th.surface3);
        }
        group = group.map(|g| g.child(item));
    }
    if let Some(g) = group {
        list = list.child(g);
    }
    if results.is_empty() {
        list = list.child(
            text("No sounds found.")
                .key("instrument-empty")
                .font(ui(13.0, 400.0))
                .color(th.muted_fg)
                .py(24.0)
                .self_center(),
        );
    }
    let content = popover_frame(Some(300.0), 0.0)
        .key("instrument-menu")
        .pt(1.0)
        .focus_return(format!("instrument-{track_id}"))
        .child(search)
        .child(chips)
        .child(
            col()
                .w_full()
                .max_h(300.0)
                .child(col().w_full().child(list))
                .scroll(rune::ScrollSpec {
                    stick_bottom: false,
                }),
        );
    popover(&format!("instrument-{track_id}"), Align::Start, content)
}
