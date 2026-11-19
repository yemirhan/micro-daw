//! `components/sidebar/Sidebar.tsx` on shadcn's collapsible sidebar: icons only by default
//! (3rem), 16rem with labels when opened from the top bar's trigger. Modes (DAW, Samples,
//! Packs, Developer), Project (New, Open), Recent (expanded only, phase 10), Settings in the
//! footer.

use rune::{El, Icon, Tx, col, el, icon, row, spacer, text};

use crate::app::{App, AppMode};
use crate::icons;
use crate::project::ProjectOp;
use crate::theme::{R_MD, daw_label, t, ui};
use crate::ui::widgets::mod_key;

fn item(
    s: &App,
    ic: &'static Icon,
    label: &'static str,
    shortcut: Option<String>,
    active: bool,
    key: &'static str,
    on: fn(&mut App, &mut Tx),
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
    b.on_press_tx(on)
}

/// `SidebarGroupLabel`: h-8, px-2, 10.5 px bold uppercase, 0.08 em tracking, muted. `h` is 28
/// inside a column whose 4-point gap stands in for the rest.
fn group_label(ic: Option<&'static Icon>, label: &'static str, h: f32) -> El {
    let th = t();
    let mut r = row().h(h).px(8.0).items_center();
    if let Some(ic) = ic {
        r = r.child(icon(ic, 14.0).color(th.muted_fg).mr(8.0));
    }
    r.child(
        text(label)
            .font(daw_label(10.5))
            .text_case(rune::TextCase::Uppercase)
            .color(th.muted_fg),
    )
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
            |s, _| s.switch_mode(AppMode::Daw),
        ))
        .child(item(
            s,
            &icons::AUDIO_LINES,
            "Samples",
            Some(format!("{m}2")),
            s.mode == AppMode::Samples,
            "mode-samples",
            |s, _| s.switch_mode(AppMode::Samples),
        ))
        .child(item(
            s,
            &icons::PACKAGE,
            "Packs",
            Some(format!("{m}3")),
            s.mode == AppMode::Packs,
            "mode-packs",
            |s, _| s.switch_mode(AppMode::Packs),
        ))
        .child(item(
            s,
            &icons::TERMINAL,
            "Developer",
            Some(format!("{m}4")),
            s.mode == AppMode::Dev,
            "mode-dev",
            |s, _| s.switch_mode(AppMode::Dev),
        ));
    let mut project = col().padding(8.0).gap(4.0);
    if s.sidebar_open {
        project = project.child(group_label(Some(&icons::FOLDER_OPEN), "Project", 28.0));
    }
    project = project
        .child(item(
            s,
            &icons::PLUS,
            "New",
            Some(format!("{m}N")),
            false,
            "project-new",
            |s, tx| s.request_project_op(ProjectOp::New, tx),
        ))
        .child(item(
            s,
            &icons::FILE_UP,
            "Open",
            Some(format!("{m}O")),
            false,
            "project-open",
            |s, tx| s.request_project_op(ProjectOp::Open, tx),
        ));
    // Recent projects (expanded sidebar only): `title={filePath}`, opens behind the prompt.
    // `SidebarGroup` (p-2, `gap-2` from the previous group) + `SidebarMenu` (`gap-1`) of
    // `size="sm"` buttons (h-7, text-xs).
    let mut recent = col().key("recent-projects").padding(8.0).mt(8.0);
    let show_recent = s.sidebar_open && !s.project.recent.is_empty();
    if show_recent {
        recent = recent.child(group_label(None, "Recent", 32.0));
        let mut rows = col().w_full().gap(4.0);
        for (i, p) in s.project.recent.iter().enumerate() {
            let path = p.file_path.clone();
            rows = rows.child(
                row()
                    .key(format!("recent-{i}"))
                    .role("button")
                    .label(p.name.clone())
                    .help(p.file_path.clone())
                    .w_full()
                    .h(28.0)
                    .items_center()
                    .px(8.0)
                    .radius(R_MD)
                    .hover(th.surface2)
                    .child(
                        text(p.name.clone())
                            .font(ui(12.0, 400.0))
                            .color(th.fg.alpha(0.65))
                            .truncate(),
                    )
                    .on_press_tx(move |s: &mut App, tx: &mut Tx| {
                        s.request_project_op(ProjectOp::OpenPath(path.clone()), tx)
                    }),
            );
        }
        recent = recent.child(rows);
    }
    let footer = col().padding(8.0).gap(8.0).child(item(
        s,
        &icons::SETTINGS,
        "Settings",
        Some(format!("{m},")),
        s.mode == AppMode::Settings,
        "mode-settings",
        |s, _| {
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
        // `SidebarSeparator` between the groups, `SidebarContent`'s `gap-2` around it.
        .child(el().h(1.0).mx(8.0).my(8.0).fill(th.sidebar_border))
        .child(project)
        .child(if show_recent { recent } else { el() })
        .child(spacer())
        .child(footer)
}
