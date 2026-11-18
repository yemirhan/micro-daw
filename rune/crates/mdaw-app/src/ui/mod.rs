//! The root view (`App.tsx`'s layout): start screen until the session starts, then the top
//! bar, sidebar and the DAW (browser + Beat view + instrument dock). Popovers and menus are
//! drawn last at the root, anchored to their triggers.

pub mod beat;
pub mod browser;
pub mod dock;
pub mod knob;
pub mod piano;
pub mod sidebar;
pub mod song;
pub mod start;
pub mod topbar;
pub mod widgets;

use rune::{Cx, El, KeyEvent, Tx, col, el, row, text};

use crate::app::{App, AppMode, DawView, Menu, StartPhase};
use crate::theme::{t, ui};

/// The modes other than DAW come in phase 9: a placeholder panel says so.
fn later(title: &str) -> El {
    let th = t();
    col()
        .grow(1.0)
        .h_full()
        .center()
        .fill(th.surface0)
        .gap(8.0)
        .child(text(title.to_owned()).font(ui(16.0, 600.0)).color(th.fg))
        .child(
            text("This page comes with phase 9 of the Rune port.")
                .font(ui(12.0, 400.0))
                .color(th.muted_fg),
        )
}

fn daw(s: &App, cx: &mut Cx<App>, win_h: f32) -> El {
    let mut r = row().grow(1.0).min_h(0.0);
    if s.browser_open {
        r = r.child(browser::view(s));
    }
    let main = if s.view == DawView::Beat {
        col()
            .grow(1.0)
            .min_w(0.0)
            .min_h(0.0)
            .child(beat::view(s, cx, win_h))
            .child(dock::view(s))
    } else {
        song::view(s, cx)
    };
    r.child(main)
}

fn overlays(s: &App, win: [f32; 2]) -> Vec<El> {
    let mut out = Vec::new();
    match &s.menu {
        Some(Menu::DrumTrack) => out.push(beat::menus::drum_track_menu(s)),
        Some(Menu::Kit) => out.push(beat::menus::kit_menu(s)),
        Some(Menu::Quantize) => out.push(beat::transport::quantize_menu(s)),
        Some(Menu::RepeatRate) => out.push(beat::transport::rate_menu(s)),
        Some(Menu::Choke) => out.push(beat::editor::choke_menu(s)),
        Some(Menu::Chop) => out.push(beat::editor::chop_menu(s)),
        Some(Menu::Pad {
            pad,
            x,
            y,
            renaming,
        }) => out.push(beat::menus::pad_menu(s, *pad, *x, *y, *renaming, win)),
        Some(Menu::MidiDevice) => out.push(topbar::midi_menu(s)),
        Some(Menu::Song(m)) => out.extend(song::overlay(s, m, win)),
        Some(Menu::Routing) | None => {}
    }
    if let Some(d) = &s.sample_drag {
        let th = t();
        out.push(
            row()
                .key("sample-drag-ghost")
                .absolute(d.at[0] + 12.0, d.at[1] + 8.0)
                .px(8.0)
                .py(4.0)
                .radius(4.0)
                .fill(th.popover)
                .border(1.0, th.border)
                .inert()
                .child(text(d.name.clone()).font(ui(12.0, 500.0)).color(th.fg)),
        );
    }
    out
}

/// The app's view.
pub fn view(s: &App, cx: &mut Cx<App>) -> El {
    let th = t();
    let win = cx.layout_size("root").unwrap_or([1440.0, 900.0]);
    if matches!(s.start, StartPhase::Idle | StartPhase::Starting) {
        return start::view(s).key("root");
    }
    let main: El = match s.mode {
        AppMode::Daw => daw(s, cx, win[1]),
        AppMode::Samples => later("Samples"),
        AppMode::Packs => later("Packs"),
        AppMode::Settings => later("Settings"),
        AppMode::Dev => later("Developer"),
    };
    let inset = cx.titlebar_inset();
    let mut root = col()
        .key("root")
        .w_full()
        .h_full()
        .fill(th.surface0)
        .child(topbar::view(
            s,
            win[0],
            if inset > 0.0 { 76.0 } else { 0.0 },
        ))
        .child(
            row()
                .grow(1.0)
                .min_h(0.0)
                .child(sidebar::view(s))
                .child(col().grow(1.0).min_w(0.0).child(main)),
        )
        .on_key_up(|s: &mut App, ev: &KeyEvent, tx: &mut Tx| crate::keys::on_key_up(s, ev, tx))
        .on_window_focus(|s: &mut App, focused: bool, _tx: &mut Tx| {
            if !focused {
                s.release_held_keys();
                s.release_pad_hold();
            }
        });
    for o in overlays(s, win) {
        root = root.child(o);
    }
    if let StartPhase::Exiting { .. } = s.start {
        root = root.child(
            el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
                .inert()
                .child(start::view(s)),
        );
    }
    root
}
