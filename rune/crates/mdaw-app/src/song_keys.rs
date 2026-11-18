//! The Song view's keys (phase 8): the piano roll editor's keys (`PianoRollEditor`
//! `handleKeyDown`, NoteGrid's capture-phase Escape), the arrangement's window keys
//! (`ArrangementView`: ⌘A, Delete / Backspace, Escape, V / C) and the Song shortcuts of
//! `App.tsx` (Escape closes the editor, ⇧M, ] / [, Space, R, M, L, X; ⌘G is in `keys.rs`).

use rune::{Key, KeyEvent, Tx};

use crate::app::{App, AppMode, DawView, Menu};
use crate::keys::is_char;
use crate::song::{ArrTool, SongMenu};

/// The Song view's text fields (typing targets).
pub fn text_field(leaf: &str) -> bool {
    matches!(
        leaf,
        "group-rename" | "marker-rename" | "instrument-search"
    )
}

fn song_open(s: &App) -> bool {
    s.mode == AppMode::Daw && s.view == DawView::Song
}

/// Keys handled before the app's shortcuts: the piano roll (focused root) and the
/// arrangement's window listener.
pub fn before_app(s: &mut App, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if !song_open(s) {
        return false;
    }
    if s.song.editing.is_some() {
        if matches!(ev.key, Key::Escape) && s.pr_cancel_gesture() {
            return true;
        }
        if s.menu.is_some() {
            return false;
        }
        if tx.key_repeat() && matches!(&ev.key, Key::Text(t) if t == " ") {
            return true;
        }
        return s.pr_key(ev);
    }
    // ArrangementView's window keys.
    let m = &ev.mods;
    if m.cmd && matches!(&ev.key, Key::Command(c) if c.eq_ignore_ascii_case(&'a')) {
        s.select_all_regions();
        return true;
    }
    if m.cmd || m.ctrl || m.alt {
        return false;
    }
    match &ev.key {
        Key::Delete | Key::Backspace => {
            // Shift+Backspace is the Beat view's; here any Delete / Backspace deletes.
            let ctx_menu = matches!(s.menu, Some(Menu::Song(SongMenu::Region { .. })));
            if !s.song.selection.is_empty() || ctx_menu {
                s.delete_selected_regions();
            }
            true
        }
        Key::Escape if s.menu.is_none() => {
            s.clear_selection();
            // The App's Escape (editor closed) does nothing more here.
            true
        }
        _ if s.menu.is_some() => false,
        _ if is_char(ev, "v") => {
            s.song.tool = ArrTool::Pointer;
            true
        }
        _ if is_char(ev, "c") => {
            s.song.tool = ArrTool::Cut;
            true
        }
        _ => false,
    }
}

/// `App.tsx`'s DAW-mode shortcuts of the Song view (after Tab / B / N).
pub fn daw_keys(s: &mut App, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if s.mode != AppMode::Daw {
        return false;
    }
    let m = &ev.mods;
    if m.cmd || m.ctrl || m.alt {
        return false;
    }
    if matches!(ev.key, Key::Escape) && s.song.editing.is_some() {
        s.close_piano_roll();
        return true;
    }
    if m.shift && is_char(ev, "m") {
        s.add_marker_at_playhead();
        return true;
    }
    if is_char(ev, "]") {
        s.seek_next_marker();
        return true;
    }
    if is_char(ev, "[") {
        s.seek_previous_marker();
        return true;
    }
    if s.view != DawView::Song || m.shift {
        return false;
    }
    if is_char(ev, " ") {
        if !tx.key_repeat() {
            s.song_toggle_play();
        }
        return true;
    }
    if is_char(ev, "r") {
        s.song_r_key();
        return true;
    }
    if is_char(ev, "m") {
        s.toggle_song_metronome();
        return true;
    }
    if is_char(ev, "l") {
        s.toggle_song_loop();
        return true;
    }
    if is_char(ev, "x") {
        s.song.show_mixer = !s.song.show_mixer;
        return true;
    }
    false
}
