//! The native menu bar (macOS): every command of the app with its shortcut, next to the
//! standard Edit and Window menus Rune installs.
//!
//! - ⌘ chords are real key equivalents with [`MenuItem::keys_first`]: the press goes to the
//!   window first (a focused field, the dialogs, [`crate::keys::on_key`]) and the item runs only
//!   when nothing took it, so a shortcut behaves the same with or without the menu.
//! - Unmodified keys (Space, R, M ...) are NOT key equivalents: AppKit offers every key-down to
//!   the menu bar before the key window, so a plain-key equivalent would take that letter from
//!   the native Open / Save panels' fields. Their items name the key in the title instead and the
//!   key itself keeps going through `on_key`.
//! - Undo, Redo and Select All are the standard Edit items: they reach `on_key` as ⌘Z / ⇧⌘Z / ⌘A.

use rune::{MenuItem, Tx};

use crate::app::{App, AppMode, DawView};

/// What the checkmarks show; the menu is set again when it changes ([`sync`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Checks {
    pub mode: AppMode,
    pub song: bool,
    pub browser: bool,
    pub mixer: bool,
    pub looping: bool,
    pub metronome: bool,
    pub repeat: bool,
    pub key_piano: bool,
}

impl Default for Checks {
    fn default() -> Self {
        Checks {
            mode: AppMode::Daw,
            song: false,
            browser: false,
            mixer: false,
            looping: false,
            metronome: false,
            repeat: false,
            key_piano: false,
        }
    }
}

impl Checks {
    pub fn of(s: &App) -> Self {
        let song = s.view == DawView::Song;
        Checks {
            mode: s.mode,
            song,
            browser: s.browser_open,
            mixer: s.song.show_mixer,
            looping: s.svc.engine.doc().is_loop_enabled(),
            metronome: if song {
                s.svc.engine.is_metronome_enabled()
            } else {
                s.svc.engine.doc().get_loop_settings().metronome
            },
            repeat: s.repeat.enabled,
            key_piano: s.key_piano,
        }
    }
}

/// Runs a menu command. Nothing before the session starts or under the unsaved prompt / the
/// load report (they own the app); an open dialog is closed first (`run_command`).
pub fn run(s: &mut App, id: &str, tx: &mut Tx) {
    if !s.started()
        || s.project.unsaved.is_some()
        || s.project.report.is_some()
        || s.project.write_error.is_some()
    {
        return;
    }
    match id {
        "record" => {
            let _ = s.leave_dialog();
            s.switch_mode(AppMode::Daw);
            if s.view == DawView::Beat {
                s.toggle_record();
            } else {
                s.song_r_key();
            }
        }
        "return-to-start" => s.song_home(),
        "toggle-loop" => s.toggle_song_loop(),
        "toggle-mixer" => {
            let _ = s.leave_dialog();
            s.switch_mode(AppMode::Daw);
            s.view = DawView::Song;
            s.song.show_mixer = !s.song.show_mixer;
        }
        "toggle-key-piano" => s.toggle_key_piano(),
        "command-palette" => s.toggle_dialog(crate::views::Dialog::Palette),
        other => s.run_command(other, tx),
    }
}

macro_rules! actions {
    ($($name:ident => $id:literal),* $(,)?) => {
        $(fn $name(s: &mut App, tx: &mut Tx) { run(s, $id, tx); })*
    };
}

actions! {
    a_settings => "open-settings",
    a_new => "new-project",
    a_template => "new-from-template",
    a_open => "open-project",
    a_save => "save",
    a_save_as => "save-as",
    a_import => "import-audio",
    a_export => "export-wav",
    a_clear => "clear-pattern",
    a_view => "toggle-view",
    a_browser => "toggle-browser",
    a_mixer => "toggle-mixer",
    a_daw => "switch-daw",
    a_samples => "switch-samples",
    a_packs => "browse-packs",
    a_dev => "switch-dev",
    a_palette => "command-palette",
    a_key_piano => "toggle-key-piano",
    a_play => "play-stop",
    a_record => "record",
    a_home => "return-to-start",
    a_loop => "toggle-loop",
    a_metronome => "toggle-metronome",
    a_repeat => "toggle-note-repeat",
    a_marker => "add-marker",
    a_next_marker => "next-marker",
    a_prev_marker => "prev-marker",
    a_synth => "add-synth-track",
    a_drum => "add-drum-track",
    a_audio => "add-audio-track",
    a_group => "create-group",
    a_controller => "controller-setup",
}

type Action = fn(&mut App, &mut Tx);

/// An item with a ⌘ chord (`keys`: a `rune::Shortcut`, "" for none).
fn item(menu: &str, id: &str, title: &str, keys: &str, action: Action) -> MenuItem<App> {
    let mut it = MenuItem::new(title, action).id(id).menu(menu);
    if !keys.is_empty() {
        match keys.parse() {
            Ok(k) => it = it.key_equivalent(k).keys_first(),
            Err(e) => eprintln!("micro-daw: menu shortcut {keys:?}: {e:?}"),
        }
    }
    it
}

/// An item whose shortcut is an unmodified key: named in the title (see the module docs).
fn plain(menu: &str, id: &str, title: &str, key: &str, action: Action) -> MenuItem<App> {
    MenuItem::new(format!("{title}\u{2003}\u{2003}{key}"), action)
        .id(id)
        .menu(menu)
}

/// The whole menu bar's custom items for `c`.
pub fn items(c: &Checks) -> Vec<MenuItem<App>> {
    let sep = |menu: &str| MenuItem::separator().menu(menu);
    let daw = c.mode == AppMode::Daw;
    vec![
        // The app menu (after About).
        MenuItem::new("Settings…", a_settings)
            .id("settings")
            .key_equivalent("cmd+,".parse().expect("shortcut"))
            .keys_first()
            .checked(c.mode == AppMode::Settings),
        // File
        item("File", "new", "New Project", "cmd+n", a_new),
        item("File", "template", "New from Template…", "", a_template),
        item("File", "open", "Open Project…", "cmd+o", a_open),
        sep("File"),
        item("File", "save", "Save", "cmd+s", a_save),
        item("File", "save-as", "Save As…", "cmd+shift+s", a_save_as),
        sep("File"),
        item("File", "import", "Import Audio…", "cmd+i", a_import),
        item("File", "export", "Export…", "cmd+e", a_export),
        // Edit (the standard menu: Undo, Redo, Cut, Copy, Paste, Select All).
        MenuItem::separator().menu("Edit").after("selectAll:"),
        plain("Edit", "clear-pattern", "Clear Pattern", "⇧⌫", a_clear).after("selectAll:"),
        // View
        plain("View", "view", "Switch Beat / Song View", "Tab", a_view),
        plain("View", "browser", "Sample Browser", "B", a_browser).checked(daw && c.browser),
        plain("View", "mixer", "Mixer", "X", a_mixer).checked(daw && c.song && c.mixer),
        sep("View"),
        item("View", "daw", "DAW", "cmd+1", a_daw).checked(daw),
        item("View", "samples", "Samples", "cmd+2", a_samples).checked(c.mode == AppMode::Samples),
        item("View", "packs", "Packs", "cmd+3", a_packs).checked(c.mode == AppMode::Packs),
        item("View", "dev", "Developer", "cmd+4", a_dev).checked(c.mode == AppMode::Dev),
        sep("View"),
        item("View", "palette", "Command Palette…", "cmd+k", a_palette),
        // (macOS adds its own Enter Full Screen item to a menu named View.)
        // Transport
        plain("Transport", "play", "Play / Stop", "Space", a_play),
        plain("Transport", "record", "Record", "R", a_record),
        plain("Transport", "home", "Return to Start", "Home", a_home),
        sep("Transport"),
        plain("Transport", "loop", "Loop", "L", a_loop).checked(c.looping),
        plain("Transport", "metronome", "Metronome", "M", a_metronome).checked(c.metronome),
        plain("Transport", "repeat", "Note Repeat", "N", a_repeat).checked(c.repeat),
        item(
            "Transport",
            "key-piano",
            "Computer Keyboard Plays Notes",
            "cmd+shift+k",
            a_key_piano,
        )
        .checked(c.key_piano),
        sep("Transport"),
        plain(
            "Transport",
            "marker",
            "Add Marker at Playhead",
            "⇧M",
            a_marker,
        ),
        plain(
            "Transport",
            "next-marker",
            "Next Marker",
            "]",
            a_next_marker,
        ),
        plain(
            "Transport",
            "prev-marker",
            "Previous Marker",
            "[",
            a_prev_marker,
        ),
        // Track
        item("Track", "add-synth", "Add Synth Track", "", a_synth),
        item("Track", "add-drum", "Add Drum Track", "", a_drum),
        item("Track", "add-audio", "Add Audio Track", "", a_audio),
        sep("Track"),
        item("Track", "group", "Group All Tracks", "cmd+g", a_group),
        sep("Track"),
        item("Track", "controller", "Controller Setup…", "", a_controller),
    ]
}

/// Each frame of a window run: the menu follows the state its checkmarks show.
pub fn sync(s: &mut App) {
    let Some(handle) = s.project.handle.clone() else {
        return;
    };
    let now = Checks::of(s);
    if s.menu_checks == Some(now) {
        return;
    }
    match handle.set_menu(items(&now)) {
        Ok(()) => s.menu_checks = Some(now),
        // Not running yet: tried again next frame. Anything else is final (no native menus).
        Err(rune::MenuError::NotRunning) => {}
        Err(e) => {
            eprintln!("micro-daw: menu bar: {e}");
            s.menu_checks = Some(now);
        }
    }
    for e in handle.take_menu_errors() {
        eprintln!("micro-daw: menu bar: {e}");
    }
}
