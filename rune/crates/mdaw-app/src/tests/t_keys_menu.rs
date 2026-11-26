//! Space after a click (a clicked control keeps focus but Space is the transport's), Beat Play
//! leaving the Song view, and the native menu bar's commands.

use mdaw_engine::engine::TransportState;
use rune::{Key, Mods};

use super::harness::*;
use crate::app::{AppMode, DawView};
use crate::menu;

#[test]
fn space_after_clicking_a_pad_plays_and_does_not_hit_the_pad() {
    let mut r = rt();
    click(&mut r, "pad-6");
    press(&mut r, text_key(" "));
    assert_eq!(
        r.state.beat.hold, None,
        "Space did not play the clicked pad"
    );
    run(&mut r, 0.2);
    assert_eq!(r.state.transport(), TransportState::Playing);
    release(&mut r, text_key(" "));
    press(&mut r, text_key(" "));
    run(&mut r, 0.1);
    assert_eq!(r.state.transport(), TransportState::Stopped);
}

#[test]
fn space_after_picking_from_a_select_plays_and_does_not_reopen_it() {
    let mut r = rt();
    click(&mut r, "quantize");
    assert!(r.state.menu.is_some());
    click(&mut r, "quantize");
    frames(&mut r, 2);
    assert!(r.state.menu.is_none());
    press(&mut r, text_key(" "));
    run(&mut r, 0.2);
    assert!(
        r.state.menu.is_none(),
        "Space did not open the clicked select"
    );
    assert_eq!(r.state.transport(), TransportState::Playing);
}

#[test]
fn space_on_a_keyboard_focused_pad_still_plays_the_pad() {
    let mut r = rt();
    r.update(|_, tx| tx.focus("pad-6", true));
    frames(&mut r, 2);
    press(&mut r, text_key(" "));
    assert_eq!(r.state.beat.hold, Some(crate::app::PadHold::Key(6)));
    assert_eq!(r.state.transport(), TransportState::Stopped);
    release(&mut r, text_key(" "));
    assert_eq!(r.state.beat.hold, None);
}

#[test]
fn space_after_clicking_a_song_mute_plays_the_song() {
    let mut r = rt();
    r.state.view = DawView::Song;
    frames(&mut r, 3);
    let id = r.state.tracks()[0].id.clone();
    click(&mut r, &format!("mute-{id}"));
    assert!(r.state.tracks()[0].muted);
    press(&mut r, text_key(" "));
    run(&mut r, 0.2);
    assert_eq!(r.state.transport(), TransportState::Playing);
    assert!(!r.state.svc.engine.is_pattern_playback());
    assert!(r.state.tracks()[0].muted, "the mute was not toggled back");
}

#[test]
fn beat_play_stops_when_the_song_view_shows() {
    let mut r = rt();
    press(&mut r, text_key(" "));
    run(&mut r, 0.2);
    assert!(r.state.svc.engine.is_pattern_playback());
    press(&mut r, ev(Key::Tab, Mods::default()));
    run(&mut r, 0.1);
    assert_eq!(r.state.view, DawView::Song);
    assert_eq!(r.state.transport(), TransportState::Stopped);
}

#[test]
fn menu_items_are_valid_and_cover_the_shortcuts() {
    let items = menu::items(&menu::Checks::default());
    // `App::menu_item` validates when the window starts: unique ids, no empty titles.
    let app = items
        .into_iter()
        .fold(rune::App::new(crate::spec(true)), |a, i| a.menu_item(i));
    let _ = app;
    for keys in [
        "cmd+,",
        "cmd+n",
        "cmd+o",
        "cmd+s",
        "cmd+shift+s",
        "cmd+i",
        "cmd+e",
        "cmd+1",
        "cmd+2",
        "cmd+3",
        "cmd+4",
        "cmd+k",
        "cmd+g",
    ] {
        assert!(keys.parse::<rune::Shortcut>().is_ok(), "{keys}");
    }
}

#[test]
fn menu_commands_run_in_the_view_they_show() {
    let mut r = rt();
    r.update(|s, tx| menu::run(s, "play-stop", tx));
    run(&mut r, 0.2);
    assert!(r.state.svc.engine.is_pattern_playback());
    r.update(|s, tx| menu::run(s, "play-stop", tx));
    run(&mut r, 0.1);
    assert_eq!(r.state.transport(), TransportState::Stopped);
    r.update(|s, tx| menu::run(s, "toggle-mixer", tx));
    frames(&mut r, 2);
    assert!(r.state.view == DawView::Song && r.state.song.show_mixer);
    assert!(menu::Checks::of(&r.state).mixer);
    r.update(|s, tx| menu::run(s, "toggle-loop", tx));
    assert!(menu::Checks::of(&r.state).looping);
    r.update(|s, tx| menu::run(s, "open-settings", tx));
    assert_eq!(r.state.mode, AppMode::Settings);
    r.state.view = DawView::Beat;
    r.update(|s, tx| menu::run(s, "record", tx));
    run(&mut r, 0.2);
    assert_eq!(r.state.mode, AppMode::Daw);
    assert!(r.state.loop_recording());
    r.update(|s, tx| menu::run(s, "play-stop", tx));
    run(&mut r, 0.1);
    assert_eq!(r.state.transport(), TransportState::Stopped);
}

#[test]
fn musical_typing_plays_notes_only_while_it_is_on() {
    let mut r = rt();
    // Off: A is not a note.
    press(&mut r, text_key("a"));
    assert!(r.state.key_notes.is_empty());
    release(&mut r, text_key("a"));
    click(&mut r, "key-piano");
    assert!(r.state.key_piano);
    press(&mut r, text_key("a"));
    press(&mut r, text_key("w"));
    let mut held: Vec<i64> = r.state.key_notes.values().copied().collect();
    held.sort();
    assert_eq!(held, [60, 61], "A = C, W = C#");
    assert_eq!(r.state.active_notes(), [60, 61]);
    release(&mut r, text_key("a"));
    release(&mut r, text_key("w"));
    assert!(r.state.key_notes.is_empty());
    assert!(r.state.active_notes().is_empty());
    // X: an octave up; the pads' letters are the piano's while it is on.
    press(&mut r, text_key("x"));
    release(&mut r, text_key("x"));
    press(&mut r, text_key("s"));
    assert_eq!(
        r.state.key_notes.values().copied().collect::<Vec<_>>(),
        [74]
    );
    assert!(r.state.held_keys.is_empty());
    // ⇧⌘K turns it off and releases what is held.
    let mut m = cmd();
    m.shift = true;
    press(&mut r, ev(Key::Command('k'), m));
    assert!(!r.state.key_piano && r.state.key_notes.is_empty());
    assert!(r.state.p9.dialog.is_none(), "not the palette's ⌘K");
}
