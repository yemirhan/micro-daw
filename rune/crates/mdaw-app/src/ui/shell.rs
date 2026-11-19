//! Phase 10's dialogs: the unsaved-changes prompt (`project:show-unsaved-dialog`), the load
//! report (a failed open, or the warnings of a migrated / coerced one; Electron only logs them),
//! a failed save, and the one-time Electron import note.
//!
//! The prompt mirrors the native alert Electron shows (`dialog.showMessageBox` as a sheet:
//! macOS's compact alert with the app icon, the bold message, the detail line and the three
//! buttons stacked, Save the default): Rune has no message boxes, so the app draws it.

use rune::{El, Tx, col, el, icon, row, text};

use crate::app::App;
use crate::icons;
use crate::theme::{R_LG, mono, t, ui};
use crate::ui::p9::kit::{dialog_layer, dialog_panel};
use crate::ui::start::mark_glyph;
use crate::ui::widgets::{Button, Size, Variant};

fn alert_icon() -> El {
    let th = t();
    // The app icon (the top bar's mark, large) with the caution badge NSAlert adds to warnings.
    el().size(64.0, 64.0)
        .child(
            row()
                .size(56.0, 56.0)
                .absolute(4.0, 4.0)
                .center()
                .radius(13.0)
                .fill(th.primary)
                .child(mark_glyph(44.0, 12.0, th.primary_fg)),
        )
        .child(
            row()
                .size(28.0, 28.0)
                .absolute(38.0, 38.0)
                .center()
                .radius(14.0)
                .fill(th.surface1)
                .child(icon(&icons::TRIANGLE_ALERT, 22.0).color(th.warn)),
        )
}

fn stacked(b: El) -> El {
    b.w_full()
}

/// `"<name>" has unsaved changes.` / `Do you want to save before continuing?` with Save /
/// Don't Save / Cancel.
pub fn unsaved(s: &App) -> El {
    let th = t();
    let name = s.project_name();
    let panel = dialog_panel(260.0, 16.0)
        .items_center()
        .gap(0.0)
        .label("Unsaved Changes")
        .child(alert_icon().mt(4.0))
        .child(
            text(format!("\u{201c}{name}\u{201d} has unsaved changes."))
                .key("unsaved-message")
                .font(ui(13.0, 700.0))
                .color(th.fg)
                .wrap()
                .align(rune::Align::Center)
                .mt(12.0),
        )
        .child(
            text("Do you want to save before continuing?")
                .font(ui(11.0, 400.0))
                .color(th.fg.alpha(0.85))
                .wrap()
                .align(rune::Align::Center)
                .mt(6.0),
        )
        .child(
            col()
                .w_full()
                .gap(8.0)
                .mt(16.0)
                .child(stacked(
                    Button::new(Variant::Primary, Size::Md)
                        .label("Save")
                        .key("unsaved-save")
                        .build(|s: &mut App, tx: &mut Tx| s.answer_unsaved(Some(true), tx)),
                ))
                .child(stacked(
                    Button::new(Variant::Secondary, Size::Md)
                        .label("Don\u{2019}t Save")
                        .key("unsaved-dont-save")
                        .build(|s: &mut App, tx: &mut Tx| s.answer_unsaved(Some(false), tx)),
                ))
                .child(stacked(
                    Button::new(Variant::Secondary, Size::Md)
                        .label("Cancel")
                        .key("unsaved-cancel")
                        .build(|s: &mut App, tx: &mut Tx| s.answer_unsaved(None, tx)),
                )),
        );
    dialog_layer("unsaved", panel, None, |s: &mut App| {
        // A click outside is Cancel (the sheet has no outside; Escape is Cancel too).
        s.project.force_close = false;
        s.project.unsaved = None;
    })
}

/// The load report / save error.
pub fn report(s: &App) -> Option<El> {
    let th = t();
    let (title, path, error, warnings) = match (&s.project.report, &s.project.write_error) {
        (Some(r), _) => (
            if r.error.is_some() {
                "Couldn\u{2019}t open project"
            } else {
                "Project opened with changes"
            },
            r.path.clone(),
            r.error.clone(),
            r.warnings.clone(),
        ),
        (None, Some(e)) => (
            "Couldn\u{2019}t save project",
            String::new(),
            Some(e.clone()),
            vec![],
        ),
        (None, None) => return None,
    };
    let mut panel = dialog_panel(480.0, 20.0).label(title).child(
        row()
            .items_center()
            .gap(8.0)
            .child(
                icon(
                    if error.is_some() {
                        &crate::icons_views::CIRCLE_ALERT
                    } else {
                        &icons::TRIANGLE_ALERT
                    },
                    16.0,
                )
                .color(if error.is_some() { th.rec } else { th.warn }),
            )
            .child(
                text(title)
                    .font(ui(15.0, 600.0).tracking(-0.4))
                    .color(th.fg),
            ),
    );
    if !path.is_empty() {
        panel = panel.child(
            text(path)
                .font(mono(11.0, 400.0))
                .color(th.muted_fg)
                .truncate()
                .mt(4.0),
        );
    }
    if let Some(e) = error {
        panel = panel.child(
            text(e)
                .key("report-error")
                .role("alert")
                .font(ui(12.0, 400.0))
                .color(th.rec)
                .wrap()
                .mt(12.0)
                .px(10.0)
                .py(6.0)
                .radius(R_LG)
                .border(1.0, th.rec.alpha(0.4))
                .fill(th.rec.alpha(0.1)),
        );
    }
    if !warnings.is_empty() {
        panel = panel.child(
            text(format!(
                "The project was updated while opening ({} change{}). Save it to keep them.",
                warnings.len(),
                if warnings.len() == 1 { "" } else { "s" }
            ))
            .font(ui(12.0, 400.0))
            .color(th.muted_fg)
            .wrap()
            .mt(8.0),
        );
        let mut list = col().key("report-warnings").w_full().gap(4.0);
        for w in warnings.iter().take(200) {
            list = list.child(
                text(w.clone())
                    .font(mono(11.0, 400.0))
                    .color(th.fg.alpha(0.85))
                    .wrap(),
            );
        }
        panel = panel.child(
            col()
                .w_full()
                .max_h(240.0)
                .mt(10.0)
                .padding(10.0)
                .radius(R_LG)
                .fill(th.surface2)
                .border(1.0, th.border)
                .child(list)
                .scroll(rune::ScrollSpec {
                    stick_bottom: false,
                }),
        );
    }
    panel = panel.child(
        row().w_full().justify_end().mt(16.0).child(
            Button::new(Variant::Primary, Size::Sm)
                .label("OK")
                .key("report-ok")
                .build(|s: &mut App, _| {
                    s.project.report = None;
                    s.project.write_error = None;
                }),
        ),
    );
    Some(dialog_layer("report", panel, None, |s: &mut App| {
        s.project.report = None;
        s.project.write_error = None;
    }))
}

/// The Electron import note: a card at the bottom right until dismissed.
pub fn import_note(s: &App) -> Option<El> {
    let th = t();
    let note = s.shell.import_note.as_ref()?;
    Some(
        row()
            .key("import-note")
            .role("status")
            .label(note.message())
            .pin(None, None, Some(16.0), Some(16.0))
            .w(380.0)
            .items_start()
            .gap(10.0)
            .padding(12.0)
            .radius(R_LG)
            .border(1.0, th.border)
            .fill(th.popover)
            .child(
                icon(&crate::icons_views::INFO, 16.0)
                    .color(if note.error.is_some() {
                        th.rec
                    } else {
                        th.primary
                    })
                    .mt(1.0),
            )
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .child(
                        text("Settings from Micro DAW")
                            .font(ui(13.0, 600.0))
                            .color(th.fg),
                    )
                    .child(
                        text(note.message())
                            .font(ui(12.0, 400.0))
                            .color(th.muted_fg)
                            .wrap()
                            .mt(2.0),
                    ),
            )
            .child(
                Button::new(Variant::Ghost, Size::IconXs)
                    .icon(&icons::X)
                    .a11y("Dismiss")
                    .key("import-note-dismiss")
                    .build(|s: &mut App, _| s.shell.import_note = None),
            ),
    )
}

/// The app icon for the bundle (`app-icon` scene): the top bar's mark on Apple's icon grid
/// (an 824 / 1024 rounded tile, 185.4 corner radius, a soft drop shadow).
pub fn app_icon(win: [f32; 2]) -> El {
    let th = t();
    let side = win[0].min(win[1]);
    let tile = side * 824.0 / 1024.0;
    el().w_full().h_full().center().child(
        row()
            .size(tile, tile)
            .center()
            .radius(tile * 0.225)
            .fill(th.primary)
            .box_shadow(rune::Shadow {
                x: 0.0,
                y: side * 0.01,
                blur: side * 0.03,
                spread: 0.0,
                color: rune::Color::BLACK.alpha(0.35),
                inset: false,
            })
            .child(mark_glyph(tile * 0.78, 4.5, th.primary_fg)),
    )
}
