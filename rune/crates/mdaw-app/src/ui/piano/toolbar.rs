//! `PianoRollToolbar.tsx`: Back, region / track name, tools, Grid and Length selects,
//! Quantize, status, note count, follow, zoom, row height, fit, shortcuts.

use mdaw_model::piano_roll::{
    self as pr, NOTE_LENGTH_OPTIONS, PR_MAX_PX_PER_BEAT, PR_MIN_PX_PER_BEAT, PianoRollTool,
    SNAP_OPTIONS,
};
use rune::{Align, Cx, El, Phase, Tx, col, el, icon, row, spacer, text};

use crate::app::{App, Menu};
use crate::icons;
use crate::icons_song;
use crate::piano::{Ctx, status_text};
use crate::song::SongMenu;
use crate::theme::{R_MD, mono, t, ui};
use crate::ui::song::track_color;
use crate::ui::widgets::{
    Button, Size, Variant, popover, popover_frame, select_option, select_popup,
};

fn divider() -> El {
    el().w(1.0).h(20.0).mx(4.0).fill(t().border)
}

/// `ToolButton`: ghost 28-point icon button; active = primary tint + ring.
fn tool_button(key: &str, title: &str, ic: &'static rune::Icon, active: bool) -> Button {
    let th = t();
    let b = Button::new(Variant::Ghost, Size::IconSm)
        .icon(ic)
        .icon_size(14.0)
        .a11y(title)
        .tip(title, None)
        .key(key)
        .pressed(active);
    if active {
        b.fill(th.primary.alpha(0.15))
            .ink(th.primary)
            .icon_ink(th.primary)
            .border(th.primary.alpha(0.4))
    } else {
        b.ink(th.muted_fg)
    }
}

fn select(key: &str, title: &str, value: String, m: SongMenu, open: bool) -> El {
    let th = t();
    row()
        .key(key.to_owned())
        .role("combobox")
        .focusable(true)
        .label(title.to_owned())
        .help(title.to_owned())
        .w(84.0)
        .h(28.0)
        .items_center()
        .justify_between()
        .px(8.0)
        .radius(R_MD)
        .fill(th.surface0)
        .border(1.0, if open { th.input } else { th.border })
        .hover(th.surface3)
        .child(text(value).font(mono(12.0, 400.0)).color(th.fg))
        .child(icon(&icons::CHEVRON_DOWN, 14.0).color(th.muted_fg.alpha(0.5)))
        .on_press(move |s: &mut App| s.toggle_menu(Menu::Song(m.clone())))
}

fn caption(s: &str) -> El {
    text(s.to_owned())
        .font(ui(10.0, 600.0).tracking(0.25))
        .text_case(rune::TextCase::Uppercase)
        .color(t().muted_fg)
}

pub fn view(s: &App, c: &Ctx, cx: &mut Cx<App>) -> El {
    let th = t();
    let p = &s.song.piano;
    let track = s.find_track(&c.track);
    let region_name = track
        .and_then(|t| t.regions.iter().find(|r| r.id == c.region))
        .and_then(|r| r.name.clone())
        .unwrap_or_else(|| "Region".into());
    let track_name = track.map(|t| t.name.clone()).unwrap_or_default();
    let tcolor = track_color(&track.map(|t| t.color.clone()).unwrap_or_default());
    let snap_id = pr::find_snap_option(p.prefs.snap, &SNAP_OPTIONS).map_or("Off", |o| o.label);
    let len_opt = pr::find_snap_option(p.prefs.draw_length, &NOTE_LENGTH_OPTIONS);
    let len_label = len_opt.map_or_else(
        || pr::format_beats(p.prefs.draw_length),
        |o| o.label.to_owned(),
    );
    let notes_n = c.notes.len();
    let sel_n = s.pr_selected(notes_n).len();
    let win_w = cx.layout_size("root").map_or(1440.0, |s| s[0]);
    let menu = s.menu.clone();
    let mut r = row()
        .key("pr-toolbar")
        .h(44.0)
        .w_full()
        .items_center()
        .gap(4.0)
        .px(8.0)
        .fill(th.surface1)
        .border_sides([0.0, 0.0, 1.0, 0.0], th.border)
        .child(
            Button::new(Variant::Ghost, Size::Xs)
                .icon(&icons_song::ARROW_LEFT)
                .label("Back")
                .font_size(12.0)
                .key("pr-back")
                .tip("Close editor (Esc)", None)
                .build(|s: &mut App, _| s.close_piano_roll())
                .h(28.0),
        )
        .child(
            row()
                .min_w(0.0)
                .max_w(220.0)
                .items_center()
                .gap(8.0)
                .px(4.0)
                .child(el().size(10.0, 10.0).radius(5.0).fill(tcolor))
                .child(
                    col()
                        .min_w(0.0)
                        .child(
                            text(region_name)
                                .key("pr-region-name")
                                .font(ui(12.0, 600.0))
                                .color(th.fg)
                                .truncate(),
                        )
                        .child(
                            text(track_name)
                                .font(ui(10.0, 400.0))
                                .color(th.muted_fg)
                                .truncate(),
                        ),
                ),
        )
        .child(divider());
    let mut tools = row().role("group").label("Tools").items_center().gap(2.0);
    for (tool, ic, label, k) in [
        (
            PianoRollTool::Pointer,
            &icons_song::MOUSE_POINTER_2,
            "Select",
            "V",
        ),
        (PianoRollTool::Draw, &icons_song::PENCIL, "Draw", "D"),
        (PianoRollTool::Eraser, &icons::ERASER, "Erase", "E"),
    ] {
        tools = tools.child(
            tool_button(
                &format!("pr-tool-{}", label.to_lowercase()),
                &format!("{label} ({k})"),
                ic,
                p.tool == tool,
            )
            .build(move |s: &mut App, _| s.set_pr_tool(tool)),
        );
    }
    r = r.child(tools).child(divider()).child(
        row()
            .items_center()
            .gap(6.0)
            .child(caption("Grid"))
            .child(select(
                "pr-grid-select",
                "Snap / grid (hold Cmd/Ctrl or Alt while dragging to bypass)",
                snap_id.to_owned(),
                SongMenu::PrGrid,
                menu == Some(Menu::Song(SongMenu::PrGrid)),
            )),
    );
    if !c.is_drum {
        r = r.child(
            row()
                .ml(4.0)
                .items_center()
                .gap(6.0)
                .child(caption("Length"))
                .child(select(
                    "pr-length-select",
                    "Length of new notes (follows the last drawn/resized note)",
                    len_label,
                    SongMenu::PrLength,
                    menu == Some(Menu::Song(SongMenu::PrLength)),
                )),
        );
    }
    r = r.child(
        Button::new(Variant::Ghost, Size::Xs)
            .icon(&icons_song::GRID_3X3)
            .label("Quantize")
            .font_size(12.0)
            .key("pr-quantize")
            .tip(
                "Quantize selected notes to the grid, or all notes if none are selected (Q)",
                None,
            )
            .disabled(p.prefs.snap <= 0.0 || notes_n == 0)
            .build(|s: &mut App, _| s.pr_quantize())
            .ml(4.0)
            .h(28.0),
    );
    r = r.child(el().min_w(8.0).grow(1.0));
    if win_w >= 1280.0 {
        r = r.child(
            text(status_text(s, c))
                .key("pr-status")
                .font(mono(11.0, 400.0))
                .color(th.muted_fg),
        );
    }
    r = r
        .child(
            text(if sel_n > 0 {
                format!("{sel_n}/{notes_n} sel")
            } else {
                format!("{notes_n} notes")
            })
            .key("pr-count")
            .font(mono(11.0, 400.0))
            .color(th.muted_fg)
            .ml(8.0),
        )
        .child(divider())
        .child(
            tool_button(
                "pr-follow",
                "Follow playhead",
                &icons_song::LOCATE_FIXED,
                p.prefs.follow,
            )
            .build(|s: &mut App, _| {
                let f = s.song.piano.prefs.follow;
                s.set_pr_follow(!f);
            }),
        )
        .child(
            tool_button("pr-zoom-out", "Zoom out (-)", &icons_song::ZOOM_OUT, false).build(
                |s: &mut App, _| {
                    let px = s.song.piano.vp.ppb;
                    s.pr_zoom_x(px * 0.8, None);
                },
            ),
        )
        .child(zoom_slider(s))
        .child(
            tool_button("pr-zoom-in", "Zoom in (+)", &icons_song::ZOOM_IN, false).build(
                |s: &mut App, _| {
                    let px = s.song.piano.vp.ppb;
                    s.pr_zoom_x(px * 1.25, None);
                },
            ),
        )
        .child(
            tool_button(
                "pr-rows-shorter",
                "Shorter rows (Alt+wheel)",
                &icons_song::FOLD_VERTICAL,
                false,
            )
            .build(|s: &mut App, _| {
                let rh = s.song.piano.vp.row_h;
                s.pr_zoom_y(rh - 2.0, None);
            }),
        )
        .child(
            tool_button(
                "pr-rows-taller",
                "Taller rows (Alt+wheel)",
                &icons_song::UNFOLD_VERTICAL,
                false,
            )
            .build(|s: &mut App, _| {
                let rh = s.song.piano.vp.row_h;
                s.pr_zoom_y(rh + 2.0, None);
            }),
        )
        .child(
            tool_button("pr-fit", "Fit notes to view (F)", &icons::MAXIMIZE_2, false)
                .build(|s: &mut App, _| s.pr_fit()),
        )
        .child(divider())
        .child(
            Button::new(Variant::Ghost, Size::IconSm)
                .icon(&icons_song::CIRCLE_HELP)
                .icon_size(14.0)
                .a11y("Keyboard shortcuts")
                .tip("Keyboard shortcuts", None)
                .key("pr-shortcuts")
                .ink(th.muted_fg)
                .build(|s: &mut App, _| s.toggle_menu(Menu::Song(SongMenu::PrShortcuts))),
        );
    r
}

/// The zoom slider's width in points.
const ZOOM_SLIDER_W: f32 = 96.0;

/// The horizontal zoom slider (0..1000, log mapping), 96 points.
fn zoom_slider(s: &App) -> El {
    let th = t();
    let f = pr::zoom_to_slider(s.song.piano.vp.ppb, PR_MIN_PX_PER_BEAT, PR_MAX_PX_PER_BEAT) as f32;
    let f = (f * 1000.0).round() / 1000.0;
    // Track, fill, then the thumb last so it paints over both (as the master volume slider
    // does): flex siblings with negative margins painted the empty track over the thumb's
    // right half, leaving a light square inside its ring.
    let w = ZOOM_SLIDER_W;
    let thumb_x = f * (w - 12.0);
    el().key("pr-zoom-slider")
        .role("slider")
        .label("Horizontal zoom")
        .size(w, 16.0)
        .child(
            el().absolute(0.0, 6.0)
                .size(w, 4.0)
                .radius(2.0)
                .fill(th.surface4)
                .child(
                    el().absolute(0.0, 0.0)
                        .size(thumb_x + 6.0, 4.0)
                        .radius(2.0)
                        .fill(th.primary),
                ),
        )
        .child(
            el().key("pr-zoom-thumb")
                .absolute(thumb_x, 2.0)
                .size(12.0, 12.0)
                .radius(6.0)
                .fill(th.fg)
                .border(1.0, th.primary),
        )
        .on_drag(|s: &mut App, d: &mut rune::Drag, _tx: &mut Tx| {
            if d.phase == Phase::Up {
                return;
            }
            let v =
                (((d.x - 6.0) / (d.w - 12.0).max(1.0)).clamp(0.0, 1.0) * 1000.0).round() / 1000.0;
            s.pr_zoom_x(
                pr::slider_to_zoom(v as f64, PR_MIN_PX_PER_BEAT, PR_MAX_PX_PER_BEAT),
                None,
            );
        })
}

fn list_menu(
    anchor: &str,
    key: &str,
    items: Vec<(String, String, bool)>,
    on: fn(&mut App, &str),
) -> El {
    let th = t();
    let mut list = popover_frame(None, 4.0).key(key.to_owned()).min_w(84.0);
    for (id, label, sel) in items {
        list = list.child(
            select_option(
                format!("{key}-{id}"),
                row()
                    .child(text(label).font(mono(12.0, 400.0)).color(if sel {
                        th.primary
                    } else {
                        th.fg
                    }))
                    .child(spacer())
                    .child(if sel {
                        icon(&icons::CHECK, 14.0).color(th.primary)
                    } else {
                        el().size(14.0, 14.0)
                    }),
                sel,
            )
            .on_press(move |s: &mut App| {
                on(s, &id);
                s.menu = None;
            }),
        );
    }
    select_popup(anchor, Align::Start, list)
}

pub fn grid_menu(s: &App) -> El {
    let cur = s.song.piano.prefs.snap;
    let items = SNAP_OPTIONS
        .iter()
        .map(|o| {
            (
                o.id.to_owned(),
                o.label.to_owned(),
                (o.beats - cur).abs() < 1e-6,
            )
        })
        .collect();
    list_menu("pr-grid-select", "pr-grid-menu", items, |s, id| {
        let beats = SNAP_OPTIONS
            .iter()
            .find(|o| o.id == id)
            .map_or(0.0, |o| o.beats);
        s.set_pr_snap(beats);
    })
}

pub fn length_menu(s: &App) -> El {
    let cur = s.song.piano.prefs.draw_length;
    let mut items: Vec<(String, String, bool)> = Vec::new();
    if pr::find_snap_option(cur, &NOTE_LENGTH_OPTIONS).is_none() {
        items.push(("custom".into(), pr::format_beats(cur), true));
    }
    items.extend(NOTE_LENGTH_OPTIONS.iter().map(|o| {
        (
            o.id.to_owned(),
            o.label.to_owned(),
            (o.beats - cur).abs() < 1e-6,
        )
    }));
    list_menu("pr-length-select", "pr-length-menu", items, |s, id| {
        if let Some(o) = NOTE_LENGTH_OPTIONS.iter().find(|o| o.id == id) {
            s.set_pr_draw_length(o.beats);
        }
    })
}

/// `SHORTCUTS` of the piano roll.
pub const SHORTCUTS: [(&str, &str); 23] = [
    ("V / D / E", "Select / Draw / Erase tool"),
    ("Click (Draw)", "Add note · drag to set length"),
    ("Double-click", "Add note (Select) · delete note"),
    ("Right-click / drag", "Delete notes"),
    ("Drag note", "Move (time + pitch)"),
    ("Alt + drag note", "Duplicate"),
    ("Drag note edges", "Resize start / end"),
    ("Cmd/Ctrl or Alt held", "Bypass snap while dragging"),
    ("Shift + click / drag", "Add to selection"),
    ("Cmd/Ctrl + A", "Select all"),
    ("Cmd/Ctrl + C / X / V", "Copy / cut / paste at playhead"),
    ("Cmd/Ctrl + D", "Duplicate selection"),
    ("Delete / Backspace", "Delete selection"),
    ("↑ / ↓", "Transpose a semitone"),
    ("Shift + ↑ / ↓", "Transpose an octave"),
    ("← / →", "Nudge by grid step"),
    ("Alt + ← / →", "Shorten / lengthen"),
    ("Q", "Quantize"),
    ("Space", "Play / stop from the cue"),
    ("Cmd/Ctrl + wheel", "Zoom horizontally"),
    ("Alt + wheel", "Zoom vertically"),
    ("+ / −  ·  F", "Zoom · fit"),
    ("Esc", "Cancel drag · close editor"),
];

pub fn shortcuts_popover() -> El {
    let th = t();
    let mut keys = col().gap(4.0);
    let mut descs = col().gap(4.0);
    for (k, d) in SHORTCUTS {
        keys = keys.child(text(k).font(mono(11.0, 400.0)).color(th.muted_fg));
        descs = descs.child(text(d).font(ui(11.0, 400.0)).color(th.fg));
    }
    let content = popover_frame(Some(320.0), 12.0)
        .key("pr-shortcuts-popover")
        .child(
            text("Piano roll shortcuts")
                .font(ui(12.0, 600.0))
                .color(th.fg)
                .mb(8.0),
        )
        .child(row().gap(12.0).child(keys).child(descs));
    popover("pr-shortcuts", Align::End, content)
}
