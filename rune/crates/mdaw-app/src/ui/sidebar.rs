//! `components/sidebar/Sidebar.tsx` on shadcn's collapsible sidebar: icons only by default
//! (3rem), 16rem with labels when opened from the top bar's trigger. Modes (DAW, Samples,
//! Packs, Developer), Project (New, Open), Recent (expanded only), Settings in the footer.

use rune::{El, Icon, col, el, icon, row, spacer, text};

use crate::app::{App, AppMode};
use crate::icons;
use crate::theme::{R_MD, t, ui};
use crate::ui::widgets::mod_key;

fn item(
    s: &App,
    ic: &'static Icon,
    label: &'static str,
    shortcut: Option<String>,
    active: bool,
    key: &'static str,
    on: fn(&mut App),
) -> El {
    let th = t();
    let open = s.sidebar_open;
    let mut b = row()
        .key(key)
        .role("button")
        .label(label)
        .h(32.0)
        .items_center()
        .gap(10.0)
        .padding(8.0)
        .radius(R_MD)
        .hover(th.surface2)
        .child(icon(ic, 16.0).color(if active {
            th.primary
        } else {
            th.fg.alpha(0.65)
        }));
    b = if open {
        b.w_full().child(
            text(label)
                .font(ui(13.0, if active { 600.0 } else { 400.0 }))
                .color(if active { th.fg } else { th.fg.alpha(0.65) }),
        )
    } else {
        b.w(32.0)
    };
    if active {
        b = b.fill(th.surface2);
    }
    b = b.help(label);
    if let Some(k) = shortcut {
        b = b.help_keys([k]);
    }
    b.on_press(on)
}

pub fn view(s: &App) -> El {
    let th = t();
    let m = mod_key();
    let w = if s.sidebar_open { 256.0 } else { 48.0 };
    let modes = col()
        .padding(8.0)
        .gap(4.0)
        .child(item(
            s,
            &icons::MUSIC,
            "DAW",
            Some(format!("{m}1")),
            s.mode == AppMode::Daw,
            "mode-daw",
            |s| s.switch_mode(AppMode::Daw),
        ))
        .child(item(
            s,
            &icons::AUDIO_LINES,
            "Samples",
            Some(format!("{m}2")),
            s.mode == AppMode::Samples,
            "mode-samples",
            |s| s.switch_mode(AppMode::Samples),
        ))
        .child(item(
            s,
            &icons::PACKAGE,
            "Packs",
            Some(format!("{m}3")),
            s.mode == AppMode::Packs,
            "mode-packs",
            |s| s.switch_mode(AppMode::Packs),
        ))
        .child(item(
            s,
            &icons::TERMINAL,
            "Developer",
            Some(format!("{m}4")),
            s.mode == AppMode::Dev,
            "mode-dev",
            |s| s.switch_mode(AppMode::Dev),
        ));
    let mut project = col().padding(8.0).gap(4.0);
    if s.sidebar_open {
        project = project.child(
            row()
                .h(32.0)
                .px(8.0)
                .items_center()
                .gap(8.0)
                .child(icon(&icons::FOLDER_OPEN, 14.0).color(th.fg.alpha(0.7)))
                .child(
                    text("Project")
                        .font(ui(12.0, 500.0))
                        .color(th.fg.alpha(0.7)),
                ),
        );
    }
    // New / Open run the project service (phase 10); the buttons are here for the layout.
    project = project
        .child(item(
            s,
            &icons::PLUS,
            "New",
            Some(format!("{m}N")),
            false,
            "project-new",
            |_| {},
        ))
        .child(item(
            s,
            &icons::FILE_UP,
            "Open",
            Some(format!("{m}O")),
            false,
            "project-open",
            |_| {},
        ));
    let footer = col().padding(8.0).gap(8.0).child(item(
        s,
        &icons::SETTINGS,
        "Settings",
        Some(format!("{m},")),
        s.mode == AppMode::Settings,
        "mode-settings",
        |s| {
            let next = if s.mode == AppMode::Settings {
                AppMode::Daw
            } else {
                AppMode::Settings
            };
            s.switch_mode(next)
        },
    ));
    col()
        .key("sidebar")
        .w(w)
        .h_full()
        .fill(th.sidebar)
        .border_sides([0.0, 1.0, 0.0, 0.0], th.sidebar_border)
        .child(modes)
        .child(el().h(1.0).mx(8.0).fill(th.sidebar_border))
        .child(project)
        .child(spacer())
        .child(footer)
}
