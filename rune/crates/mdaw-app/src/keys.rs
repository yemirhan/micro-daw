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
            || crate::views::text_field(leaf)
    })
}

fn typing_target(focused: Option<&str>) -> bool {
    focused.is_some_and(|k| {
        let leaf = k.rsplit('/').next().unwrap_or(k);
        matches!(
            leaf,
            "bpm-input" | "kit-name" | "pad-rename" | "browser-filter"
        ) || crate::song_keys::text_field(leaf)
            || crate::views::text_field(leaf)
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

/// Musical typing's velocity (a computer key has none).
const KEY_PIANO_VELOCITY: f64 = 0.8;

/// What a key is while musical typing is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PianoKey {
    /// Semitones above the octave's C.
    Note(i64),
    /// Octave down / up.
    Octave(i32),
}

/// Ableton's layout by physical key (any keyboard layout): A W S E D F T G Y H U J K O L P ;
/// are C to E an octave and a third up, Z / X shift the octave. A key with no physical origin
/// (scripts, tests) goes by its character.
pub fn piano_key(code: Option<&str>, ev: &KeyEvent) -> Option<PianoKey> {
    const NOTES: [(&str, &str); 17] = [
        ("KeyA", "a"),
        ("KeyW", "w"),
        ("KeyS", "s"),
        ("KeyE", "e"),
        ("KeyD", "d"),
        ("KeyF", "f"),
        ("KeyT", "t"),
        ("KeyG", "g"),
        ("KeyY", "y"),
        ("KeyH", "h"),
        ("KeyU", "u"),
        ("KeyJ", "j"),
        ("KeyK", "k"),
        ("KeyO", "o"),
        ("KeyL", "l"),
        ("KeyP", "p"),
        ("Semicolon", ";"),
    ];
    let is = |c: &str, ch: &str| match code {
        Some(k) => k == c,
        None => is_char(ev, ch),
    };
    if let Some(i) = NOTES.iter().position(|(c, ch)| is(c, ch)) {
        return Some(PianoKey::Note(i as i64));
    }
    if is("KeyZ", "z") {
        return Some(PianoKey::Octave(-1));
    }
    if is("KeyX", "x") {
        return Some(PianoKey::Octave(1));
    }
    None
}

/// The root's `El::on_key`, which sees a key before the focused control's own Return / Space
/// press. A control the pointer clicked keeps focus (web focus, as an HTML button), so Space
/// pressed it again instead of playing: a toggle just clicked flipped back, a pad re-triggered.
/// Space now goes to the transport unless the keyboard is driving focus (the ring shows: Tab
/// then Space still presses the control), a text field / the browser has it, or a menu or
/// dialog is open.
pub fn pointer_focus_key(s: &mut App, ev: &KeyEvent, tx: &mut Tx) -> bool {
    let m = &ev.mods;
    if !is_char(ev, " ") || m.cmd || m.ctrl || m.alt || m.shift || m.logo {
        return false;
    }
    if tx.focus_visible()
        || !s.started()
        || s.menu.is_some()
        || s.p9.dialog.is_some()
        || s.project.unsaved.is_some()
        || s.project.report.is_some()
        || s.project.write_error.is_some()
        || !matches!(s.mode, AppMode::Daw | AppMode::Samples)
    {
        return false;
    }
    if scoped_target(tx.focused()) {
        return false;
    }
    on_key(s, ev, tx)
}

/// `AppSpec::on_key`.
pub fn on_key(s: &mut App, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if matches!(ev.key, Key::Modifier { .. }) {
        return false;
    }
    // Phase 10: the unsaved prompt and the load report own the keyboard, over the start
    // screen too (review finding 12).
    if crate::shell::dialog_keys(s, ev, tx) {
        return true;
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
    // ⇧⌘K: musical typing on / off (before ⌘K, the palette's).
    if ev.mods.cmd && ev.mods.shift && matches!(ev.key, Key::Command('k' | 'K')) {
        if !tx.key_repeat() {
            s.toggle_key_piano();
        }
        return true;
    }
    // Phase 9: dialogs, ⌘K / ⌘E / ⌘I, the Packs page's Escape.
    if crate::views::keys(s, ev, tx, focused) {
        return true;
    }

    // Musical typing: while it is on, the letter rows are a piano in both views (they take
    // those letters from the pads and the shortcuts, as Ableton's computer MIDI keyboard does).
    if s.key_piano
        && s.mode == AppMode::Daw
        && s.p9.dialog.is_none()
        && !menu_open
        && !scoped_target(focused)
        && !(ev.mods.cmd || ev.mods.ctrl || ev.mods.alt || ev.mods.shift || ev.mods.logo)
    {
        let code = tx.physical_key().map(|c| format!("{c:?}"));
        match piano_key(code.as_deref(), ev) {
            Some(PianoKey::Note(semi)) => {
                let id = held_id(ev, tx);
                if !tx.key_repeat() && !s.key_notes.contains_key(&id) {
                    let note = (60 + 12 * s.key_piano_octave as i64 + semi).clamp(0, 127);
                    s.key_notes.insert(id, note);
                    s.svc
                        .with_router(|r, out| r.note_on(note, KEY_PIANO_VELOCITY, out));
                }
                return true;
            }
            Some(PianoKey::Octave(d)) => {
                if !tx.key_repeat() {
                    s.release_key_notes();
                    s.key_piano_octave = (s.key_piano_octave + d).clamp(-4, 4);
                }
                return true;
            }
            None => {}
        }
    }

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
    // The Samples page (Sound Designer): Space plays, ↑ / ↓ browse the library.
    if s.mode == AppMode::Samples && !menu_open && crate::designer::keys(s, ev, tx, focused) {
        return true;
    }
    // The Song view's editor keys and the arrangement's window keys (before the app's).
    if crate::song_keys::before_app(s, ev, tx) {
        return true;
    }
    let m = ev.mods.cmd;
    // Global (all modes).
    if m {
        match &ev.key {
            // Project (`App.tsx`): ⌘S save, ⇧⌘S save as, ⌘O open, ⌘N new.
            Key::Command('s' | 'S') => {
                if ev.mods.shift {
                    s.save_project_as(tx);
                } else {
                    s.save_project(tx);
                }
                return true;
            }
            Key::Command('o') => {
                s.request_project_op(crate::project::ProjectOp::Open, tx);
                return true;
            }
            Key::Command('n') => {
                s.request_project_op(crate::project::ProjectOp::New, tx);
                return true;
            }
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

/// The root's `on_key_up`: a held computer-key pad or musical-typing note is released.
pub fn on_key_up(s: &mut App, ev: &KeyEvent, tx: &mut Tx) -> bool {
    let id = held_id(ev, tx);
    if let Some(note) = s.key_notes.remove(&id) {
        s.svc.with_router(|r, out| r.note_off(note, out));
        return true;
    }
    if let Some(pad) = s.held_keys.remove(&id) {
        s.svc.drum_release(pad);
        return true;
    }
    false
}

impl App {
    /// Window blur / hidden: every held computer-key pad is released.
    pub fn release_held_keys(&mut self) {
        self.release_key_notes();
        let pads: Vec<usize> = self.held_keys.drain().map(|(_, p)| p).collect();
        for p in pads {
            self.svc.drum_release(p);
        }
    }

    /// Every musical-typing note is released (blur, octave change, the toggle going off).
    pub fn release_key_notes(&mut self) {
        let notes: Vec<i64> = self.key_notes.drain().map(|(_, n)| n).collect();
        for n in notes {
            self.svc.with_router(|r, out| r.note_off(n, out));
        }
    }

    /// Musical typing on / off.
    pub fn toggle_key_piano(&mut self) {
        self.release_key_notes();
        self.key_piano = !self.key_piano;
        if self.key_piano {
            // The pads' computer keys are the piano's now.
            self.release_held_keys();
        }
    }
}
