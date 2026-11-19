//! The root view (`App.tsx`'s layout): start screen until the session starts, then the top
//! bar, sidebar and the DAW (browser + Beat view + instrument dock). Popovers and menus are
//! drawn last at the root, anchored to their triggers.

pub mod beat;
pub mod browser;
pub mod dock;
pub mod knob;
pub mod p9;
pub mod piano;
pub mod shell;
pub mod sidebar;
pub mod song;
pub mod start;
pub mod topbar;
pub mod widgets;

use rune::{Cx, El, KeyEvent, Tx, col, el, row, text};

use crate::app::{App, AppMode, DawView, Menu, StartPhase};
use crate::theme::{t, ui};

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
            .child(if s.p9.synth_editor {
                p9::rack::synth_editor(s)
            } else {
                el()
            })
            .child(dock::view(s))
    } else {
        song::view(s, cx)
    };
    r.child(main)
}

fn overlays(s: &App, cx: &mut Cx<App>, win: [f32; 2]) -> Vec<El> {
    let mut out = Vec::new();
    match &s.menu {
        Some(Menu::DrumTrack) => out.push(beat::menus::drum_track_menu(s)),
        Some(Menu::Kit) => out.push(beat::menus::kit_menu(s)),
        Some(Menu::Quantize) => out.push(beat::transport::quantize_menu(s)),
        Some(Menu::RepeatRate) => out.push(beat::transport::rate_menu(s)),
        Some(Menu::Choke) => out.push(beat::editor::choke_menu(s)),
        Some(Menu::Chop) => out.push(beat::editor::chop_menu(s, cx)),
        Some(Menu::Pad {
            pad,
            x,
            y,
            renaming,
        }) => out.push(beat::menus::pad_menu(s, *pad, *x, *y, *renaming, win)),

        Some(Menu::Song(m)) => out.extend(song::overlay(s, m, win)),
        Some(Menu::P9(m)) => out.push(p9::menu(s, *m)),
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
    if let Some(d) = p9::dialog(s, win) {
        out.push(d);
    }
    // Phase 10: the notes, the load report and the unsaved prompt (topmost).
    out.extend(shell::layers(s));
    out
}

/// The app's view.
pub fn view(s: &App, cx: &mut Cx<App>) -> El {
    let th = t();
    let win = cx.layout_size("root").unwrap_or([1440.0, 900.0]);
    if let Some(bg) = s.shell.icon_scene {
        return shell::app_icon(win).fill(bg).key("root");
    }
    if matches!(s.start, StartPhase::Idle | StartPhase::Starting) {
        // The shell's dialogs show over the start screen too (a restored unsaved project's
        // close prompt, a load report, the notes: review finding 12).
        let mut root = el()
            .key("root")
            .w_full()
            .h_full()
            .child(start::view(s).w_full().h_full());
        for l in shell::layers(s) {
            root = root.child(l);
        }
        return root;
    }
    let main: El = match s.mode {
        AppMode::Daw => daw(s, cx, win[1]),
        _ => p9::page(s, cx),
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
        // Phase 10: a `.mdaw` dropped anywhere opens it (behind the unsaved prompt); audio
        // files keep going to the pads (their own targets).
        .on_drop_files(|s: &mut App, files: &[std::path::PathBuf], tx: &mut Tx| {
            if let Some(f) = files.first() {
                let p = f.to_string_lossy().into_owned();
                s.request_project_op(crate::project::ProjectOp::OpenPath(p), tx);
            }
        })
        .accepts_extensions(&["mdaw"])
        .on_key_up(|s: &mut App, ev: &KeyEvent, tx: &mut Tx| crate::keys::on_key_up(s, ev, tx))
        .on_window_focus(|s: &mut App, focused: bool, _tx: &mut Tx| {
            if !focused {
                s.release_held_keys();
                s.release_pad_hold();
                s.release_dock_note();
            }
        });
    for o in overlays(s, cx, win) {
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
