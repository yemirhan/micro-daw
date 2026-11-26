//! `TransportBar.tsx` (the top bar's centre in the Song view): play / stop / record | loop,
//! metronome, the position LCD (bars.beats.sixteenths, time from 1440 points, REC / PUNCH),
//! the BPM field and the marker jump select.

use mdaw_engine::engine::TransportState;
use rune::{
    Align, Color, El, Key, KeyEvent, Phase, Shape, Tx, canvas, el, icon, row, spacer, text,
    text_input,
};

use crate::app::{App, Menu};
use crate::icons;
use crate::icons_song;
use crate::song::SongMenu;
use crate::theme::{R_MD, mono, t, ui};
use crate::ui::beat::transport::clamp_bpm;
use crate::ui::widgets::{Button, Size, Variant, lcd, popover_frame, select_option, select_popup};

/// The metronome glyph (`MetronomeIcon`, 24-point view box, 2-point strokes).
fn metronome_icon(c: Color) -> El {
    let s = 16.0 / 24.0;
    let p = |x: f32, y: f32| [x * s, y * s];
    let sh = Shape::new()
        .stroke_with(
            vec![p(9.0, 3.0), p(15.0, 3.0), p(19.0, 21.0), p(5.0, 21.0)],
            true,
            2.0 * s,
            rune::LineCap::Round,
            c,
        )
        .line(
            p(12.0, 16.0),
            p(17.0, 7.0),
            2.0 * s,
            rune::LineCap::Round,
            c,
        )
        .line(
            p(7.0, 16.0),
            p(17.0, 16.0),
            2.0 * s,
            rune::LineCap::Round,
            c,
        );
    canvas(sh).size(16.0, 16.0)
}

fn tbtn(key: &str, a11y: &str) -> Button {
    Button::new(Variant::Ghost, Size::IconSm)
        .key(key)
        .a11y(a11y)
}

pub fn view(s: &App, win_w: f32) -> El {
    let th = t();
    let st = s.song_state();
    let recording = st == TransportState::Recording;
    let playing = st == TransportState::Playing;
    let doc = s.svc.engine.doc();
    let loop_on = doc.is_loop_enabled();
    let metro = s.svc.engine.is_metronome_enabled();
    // Any armed track can be recorded: an audio track records its input, a synth or drum
    // track its notes (`get_armed_track_id` names only the note track, so a project with just
    // an audio track armed had Record disabled).
    let record_disabled = doc.armed_track_ids().is_empty();
    let punch = recording && s.svc.engine.is_punch_recording_active();

    let mut play = tbtn("song-play", "Play")
        .icon(if playing {
            &icons::PLAY_FILLED
        } else {
            &icons::PLAY
        })
        .tip(if playing { "Playing" } else { "Play" }, Some("Space"))
        .pressed(playing)
        .disabled(recording);
    if playing {
        play = play
            .fill(th.play.alpha(0.15))
            .ink(th.play)
            .icon_ink(th.play);
    }
    let stop = tbtn("song-stop", "Stop")
        .icon(&icons::SQUARE_FILLED)
        .icon_size(14.0)
        .tip("Stop", Some("Space"))
        .disabled(st == TransportState::Stopped);
    let rec_tip = if recording {
        "Stop recording"
    } else if record_disabled {
        "Arm a track to record"
    } else if loop_on {
        "Punch record (loop)"
    } else {
        "Record"
    };
    let mut rec = tbtn(
        "song-record",
        if recording {
            "Stop recording"
        } else {
            "Record"
        },
    )
    .icon(&icons::CIRCLE_FILLED)
    .icon_size(14.0)
    .pressed(recording)
    .disabled(!recording && record_disabled);
    rec = if recording {
        rec.fill(th.rec).ink(Color::WHITE).icon_ink(Color::WHITE)
    } else {
        rec.icon_ink(th.rec.alpha(0.8))
    };
    let mut loop_b = tbtn("song-loop", "Loop")
        .icon(&icons::REPEAT)
        .tip(if loop_on { "Loop on" } else { "Loop off" }, Some("L"))
        .pressed(loop_on);
    if loop_on {
        loop_b = loop_b
            .fill(th.primary.alpha(0.15))
            .ink(th.primary)
            .icon_ink(th.primary);
    }
    let metro_c = if metro { th.primary } else { th.muted_fg };
    let mut metro_b = row()
        .key("song-metronome")
        .role("button")
        .label("Metronome")
        .checked(metro)
        .help(if metro {
            "Metronome on"
        } else {
            "Metronome off"
        })
        .help_keys(["M".to_owned()])
        .size(28.0, 26.0)
        .center()
        .radius(R_MD)
        .hover(th.surface3)
        .child(metronome_icon(metro_c))
        .on_press(|s: &mut App| s.toggle_song_metronome());
    if metro {
        metro_b = metro_b.fill(th.primary.alpha(0.15));
    }
    let cluster = row()
        .h(32.0)
        .items_center()
        .gap(2.0)
        .padding(2.0)
        .radius(R_MD)
        .border(1.0, th.border)
        .fill(th.surface0)
        .child(play.build(|s: &mut App, _| s.song_play()).h(26.0))
        .child(stop.build(|s: &mut App, _| s.song_stop()).h(26.0))
        .child(
            rec.build(|s: &mut App, _| {
                if s.song_state() == TransportState::Recording {
                    s.song_stop();
                } else {
                    s.song_record();
                }
            })
            .h(26.0)
            .help(rec_tip)
            .help_keys(["R".to_owned()]),
        )
        .child(el().w(1.0).h(16.0).mx(2.0).fill(th.border))
        .child(loop_b.build(|s: &mut App, _| s.toggle_song_loop()).h(26.0))
        .child(metro_b);

    let live = &s.song.live;
    let mut pos = row()
        .key("song-lcd-digits")
        .items_end()
        .child(
            text("  1")
                .live_text(&live.lcd_bar)
                .font(mono(15.0, 600.0).tracking(-0.4))
                .color(th.fg),
        )
        .child(
            text(".")
                .font(mono(15.0, 600.0))
                .color(th.muted_fg.alpha(0.6)),
        )
        .child(
            text("1")
                .live_text(&live.lcd_beat)
                .font(mono(15.0, 600.0))
                .color(th.fg),
        )
        .child(
            text(".")
                .font(mono(15.0, 600.0))
                .color(th.muted_fg.alpha(0.6)),
        )
        .child(
            text("1")
                .live_text(&live.lcd_six)
                .font(mono(15.0, 600.0))
                .color(th.fg.alpha(0.7)),
        );
    pos = pos.key("song-lcd-digits");
    let mut lcd_box = lcd()
        .key("song-position")
        .role("group")
        .label("Song position")
        .h(32.0)
        .items_center()
        .gap(12.0)
        .pr(10.0)
        .pl(8.0)
        .child(pos);
    if recording {
        lcd_box = lcd_box.border(1.0, th.rec.alpha(0.5));
    }
    if win_w >= 1440.0 {
        lcd_box = lcd_box.child(
            text("0:00.0")
                .key("song-lcd-time")
                .live_text(&live.lcd_time)
                .font(mono(11.0, 400.0))
                .color(th.muted_fg),
        );
    }
    if recording {
        lcd_box = lcd_box.child(
            row()
                .key("song-rec-badge")
                .px(4.0)
                .py(1.0)
                .radius(3.0)
                .fill(if punch { th.warn } else { th.rec })
                .child(
                    text(if punch { "PUNCH" } else { "REC" })
                        .font(mono(9.5, 700.0).tracking(0.76))
                        .color(if punch { Color::BLACK } else { Color::WHITE }),
                ),
        );
    }
    let mut r = row()
        .key("song-transport")
        .items_center()
        .gap(8.0)
        .child(cluster)
        .child(lcd_box)
        .child(bpm_field(s));
    let markers = doc.get_markers();
    if !markers.is_empty() {
        let open = s.menu == Some(Menu::Song(SongMenu::MarkerJump));
        let mut trig = row()
            .key("marker-jump")
            .role("combobox")
            .focusable(true)
            .label("Jump to marker")
            .help("Jump to marker ( [ / ] )")
            .h(28.0)
            .items_center()
            .gap(6.0)
            .px(8.0)
            .radius(R_MD)
            .hover(th.surface3)
            .child(icon(&icons_song::FLAG, 14.0).color(th.muted_fg))
            .child(
                text(markers.len().to_string())
                    .font(ui(11.0, 600.0))
                    .color(th.muted_fg),
            )
            .child(icon(&icons::CHEVRON_DOWN, 14.0).color(th.muted_fg.alpha(0.5)))
            .on_press(|s: &mut App| s.toggle_menu(Menu::Song(SongMenu::MarkerJump)));
        if open {
            trig = trig.fill(th.surface3);
        }
        r = r.child(trig);
    }
    r
}

/// `BpmField` of the transport bar (13-point digits; same rules as the Beat view's).
fn bpm_field(s: &App) -> El {
    let th = t();
    let bpm = s.svc.engine.doc().get_bpm();
    let value: El = match &s.beat.bpm_edit {
        Some(edit) => text_input(edit, mono(13.0, 600.0))
            .key("bpm-input")
            .single_line()
            .color(th.primary)
            .w(30.0)
            .align(Align::End)
            .on_edit(|s: &mut App| {
                s.beat
                    .bpm_edit
                    .get_or_insert_with(|| rune::TextEdit::new(""))
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
            .key("song-bpm-value")
            .font(mono(13.0, 600.0))
            .color(th.fg)
            .w(30.0)
            .align(Align::End),
    };
    lcd()
        .key("song-bpm")
        .role("spinbutton")
        .label("Tempo (BPM)")
        .help("Tempo: drag, scroll or type")
        .h(28.0)
        .items_center()
        .gap(6.0)
        .px(8.0)
        .cursor(rune::Cursor::ResizeRow)
        .child(value)
        .child(
            text("BPM")
                .font(mono(9.5, 700.0).tracking(0.76))
                .color(th.muted_fg),
        )
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
                        let bpm = s.svc.engine.doc().get_bpm();
                        s.beat.bpm_edit = Some(crate::song::selected_edit(format!(
                            "{}",
                            bpm.round() as i64
                        )));
                        tx.focus("bpm-input", true);
                    }
                }
            }
        })
}

/// The marker jump list: colour bar, name, bar.beat; picking a marker seeks there.
pub fn marker_jump_menu(s: &App) -> El {
    let th = t();
    let mut list = popover_frame(None, 4.0)
        .key("marker-jump-menu")
        .min_w(200.0);
    for m in s.svc.engine.doc().get_markers() {
        let id = m.id.clone();
        list = list.child(
            select_option(
                format!("jump-{}", m.id),
                row()
                    .child(
                        el().w(4.0)
                            .h(12.0)
                            .radius(2.0)
                            .fill(crate::ui::song::track_color(&m.color)),
                    )
                    .child(text(m.name.clone()).font(ui(13.0, 400.0)).color(th.fg))
                    .child(spacer())
                    .child(
                        text(format!(
                            "{}.{}",
                            (m.beat / 4.0).floor() as i64 + 1,
                            (m.beat % 4.0).floor() as i64 + 1
                        ))
                        .font(mono(10.5, 400.0))
                        .color(th.muted_fg)
                        .pl(12.0),
                    ),
                false,
            )
            .on_press(move |s: &mut App| {
                s.seek_to_marker(&id);
                s.menu = None;
            }),
        );
    }
    select_popup("marker-jump", Align::Center, list)
}
