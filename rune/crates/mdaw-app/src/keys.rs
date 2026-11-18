//! The window's keys: the start screen's "any key", the Beat view's computer-keyboard pads
//! (`BeatView.tsx`, which run before the app's shortcuts like its capture-phase listener) and
//! the global shortcuts of `App.tsx` that reach the Beat view.

use mdaw_model::beat_helpers::{KeyEventLike, pad_for_key_event};
use rune::{Key, KeyEvent, Tx};

use crate::app::{App, AppMode, DawView, StartPhase};

/// Focused elements that keep their keys (`isTypingTarget` / `isScopedKeyTarget`): text
/// fields, the browser (type-to-filter, Space = preview), menus.
pub fn scoped_target(focused: Option<&str>) -> bool {
    focused.is_some_and(|k| {
        let leaf = k.rsplit('/').next().unwrap_or(k);
        matches!(
            leaf,
            "bpm-input" | "kit-name" | "pad-rename" | "browser-filter" | "browser-list"
        ) || leaf.starts_with("browser-")
            || crate::song_keys::text_field(leaf)
    })
}

fn typing_target(focused: Option<&str>) -> bool {
    focused.is_some_and(|k| {
        let leaf = k.rsplit('/').next().unwrap_or(k);
        matches!(
            leaf,
            "bpm-input" | "kit-name" | "pad-rename" | "browser-filter"
        ) || crate::song_keys::text_field(leaf)
    })
}

/// The DOM-ish view of a key event for the pure helpers (`e.key`, `e.code`).
pub fn key_like(ev: &KeyEvent, tx: &Tx) -> KeyEventLike {
    let key = match &ev.key {
        Key::Text(t) => t.clone(),
        Key::Command(c) => c.to_string(),
        Key::Enter => "Enter".into(),
        Key::Tab => "Tab".into(),
        Key::Escape => "Escape".into(),
        Key::Backspace => "Backspace".into(),
        _ => String::new(),
    };
    KeyEventLike {
        key,
        code: tx
            .physical_key()
            .map(|c| format!("{c:?}"))
            .unwrap_or_default(),
        meta: ev.mods.logo,
        ctrl: ev.mods.ctrl,
        alt: ev.mods.alt,
        shift: ev.mods.shift,
    }
}

/// The identity of a held key (its physical key when known, else its character).
fn held_id(ev: &KeyEvent, tx: &Tx) -> String {
    tx.physical_key()
        .map(|c| format!("{c:?}"))
        .unwrap_or_else(|| match &ev.key {
            Key::Text(t) => t.to_lowercase(),
            other => format!("{other:?}"),
        })
}

/// BeatView's `e.code === 'KeyR'`: the physical R key whatever the layout (AZERTY, Dvorak:
/// the key under the left index finger's row, like the pad keys), else, for a key with no
/// physical origin (scripts, tests), the character r.
pub fn is_record_key(code: Option<&str>, ev: &KeyEvent) -> bool {
    match code {
        Some(c) => c == "KeyR",
        None => is_char(ev, "r"),
    }
}

pub fn is_char(ev: &KeyEvent, c: &str) -> bool {
    matches!(&ev.key, Key::Text(t) if t.eq_ignore_ascii_case(c))
}

/// `AppSpec::on_key`.
pub fn on_key(s: &mut App, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if matches!(ev.key, Key::Modifier { .. }) {
        return false;
    }
    // Start screen: any key but Tab, Escape and modifiers; no repeat, no modifier held.
    if !s.started() {
        if s.start != StartPhase::Idle || tx.key_repeat() {
            return false;
        }
        if ev.mods.cmd || ev.mods.ctrl || ev.mods.alt || ev.mods.logo {
            return false;
        }
        if matches!(ev.key, Key::Tab | Key::Escape) {
            return false;
        }
        s.start_session();
        return true;
    }
    let focused = tx.focused().map(str::to_owned);
    let focused = focused.as_deref();
    let menu_open = s.menu.is_some();

    // Beat view pads (capture phase: before the app's shortcuts).
    if s.mode == AppMode::Daw && s.view == DawView::Beat && !scoped_target(focused) && !menu_open {
        let code = tx.physical_key().map(|c| format!("{c:?}"));
        let plain_r =
            is_record_key(code.as_deref(), ev) && !ev.mods.cmd && !ev.mods.ctrl && !ev.mods.alt;
        if plain_r {
            if !tx.key_repeat() {
                s.toggle_record();
            }
            return true;
        }
        if !ev.mods.shift {
            let like = key_like(ev, tx);
            if let Some(pad) = pad_for_key_event(&like) {
                let id = held_id(ev, tx);
                if tx.key_repeat() || s.held_keys.contains_key(&id) {
                    return true;
                }
                s.held_keys.insert(id, pad);
                s.svc.drum_press(pad, 0.85);
                return true;
            }
        }
    }

    if typing_target(focused) {
        return false;
    }
    // The Song view's editor keys and the arrangement's window keys (before the app's).
    if crate::song_keys::before_app(s, ev, tx) {
        return true;
    }
    let m = ev.mods.cmd;
    // Global (all modes).
    if m {
        match &ev.key {
            Key::Command('z') => {
                if ev.mods.shift {
                    s.redo();
                } else {
                    s.undo();
                }
                return true;
            }
            Key::Command('1') => {
                s.switch_mode(AppMode::Daw);
                return true;
            }
            Key::Command('2') => {
                s.switch_mode(AppMode::Samples);
                return true;
            }
            Key::Command('3') => {
                s.switch_mode(AppMode::Packs);
                return true;
            }
            Key::Command('4') => {
                s.switch_mode(AppMode::Dev);
                return true;
            }
            Key::Command('g') if s.mode == AppMode::Daw => {
                s.group_all_tracks();
                return true;
            }
            Key::Command(',') => {
                let next = if s.mode == AppMode::Settings {
                    AppMode::Daw
                } else {
                    AppMode::Settings
                };
                s.switch_mode(next);
                return true;
            }
            _ => return false,
        }
    }
    if s.mode != AppMode::Daw || menu_open {
        return false;
    }
    let plain = !ev.mods.alt && !ev.mods.shift && !ev.mods.ctrl;
    if plain
        && matches!(ev.key, Key::Tab)
        && !scoped_target(focused)
        && focused.is_none_or(|f| !f.ends_with("step-grid"))
    {
        s.view = if s.view == DawView::Beat {
            DawView::Song
        } else {
            DawView::Beat
        };
        return true;
    }
    if plain && is_char(ev, "b") {
        s.toggle_browser();
        return true;
    }
    if plain && is_char(ev, "n") {
        s.toggle_note_repeat();
        return true;
    }
    if crate::song_keys::daw_keys(s, ev, tx) {
        return true;
    }
    if s.view == DawView::Beat {
        if plain && is_char(ev, " ") {
            if !tx.key_repeat() {
                s.toggle_play();
            }
            return true;
        }
        if plain && is_char(ev, "m") {
            s.toggle_loop_metronome();
            return true;
        }
        if ev.mods.shift && matches!(ev.key, Key::Backspace) {
            s.clear_pattern();
            return true;
        }
    }
    false
}

/// The root's `on_key_up`: a held computer-key pad is released.
pub fn on_key_up(s: &mut App, ev: &KeyEvent, tx: &mut Tx) -> bool {
    let id = held_id(ev, tx);
    if let Some(pad) = s.held_keys.remove(&id) {
        s.svc.drum_release(pad);
        return true;
    }
    false
}

impl App {
    /// Window blur / hidden: every held computer-key pad is released.
    pub fn release_held_keys(&mut self) {
        let pads: Vec<usize> = self.held_keys.drain().map(|(_, p)| p).collect();
        for p in pads {
            self.svc.drum_release(p);
        }
    }
}
