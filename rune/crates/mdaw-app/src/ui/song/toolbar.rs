//! `ArrangementToolbar.tsx`: Add track (a menu: Synth, Drums, Audio, Import) | Marker, Group | Pointer / Cut |
//! snap select | zoom out, %, zoom in | Export. ("+ Audio" adds an empty audio track to arm and
//! record on: Rust-only; the Electron app's "Audio" button, importing a file, is "Import".)

use mdaw_model::InstrumentType;
use rune::{Align, El, el, icon, row, spacer, text};

use crate::app::{App, Menu};
use crate::icons;
use crate::icons_song;
use crate::song::{ArrTool, DEFAULT_PPB, MAX_PPB, MIN_PPB, SNAP_VALUES, SongMenu};
use crate::theme::{R_MD, mono, t, ui};
use crate::ui::widgets::{
    Button, Size, Variant, mod_key, popover_frame, select_option, select_popup,
};

fn divider() -> El {
    el().w(1.0).h(16.0).fill(t().border)
}

/// A group's caption ("ADD TRACK", "TOOL", "SNAP", "ZOOM"): what the controls after it do.
fn caption(label: &str) -> El {
    text(label.to_owned())
        .font(crate::theme::daw_label(10.0))
        .text_case(rune::TextCase::Uppercase)
        .color(t().muted_fg.alpha(0.8))
}

/// One row of the "Add track" menu: the kind's icon, its name and what it is for.
fn add_row(key: &str, ic: &'static rune::Icon, name: &str, what: &str) -> El {
    let th = t();
    row()
        .key(key.to_owned())
        .role("menuitem")
        .label(format!("{name}: {what}"))
        .w_full()
        .items_center()
        .gap(10.0)
        .px(8.0)
        .py(6.0)
        .radius(crate::theme::R_SM)
        .focusable(true)
        .focus_highlight(th.surface3)
        .child(
            el().size(28.0, 28.0)
                .center()
                .radius(R_MD)
                .fill(th.primary.alpha(0.12))
                .child(icon(ic, 16.0).color(th.primary)),
        )
        .child(
            rune::col()
                .gap(1.0)
                .child(text(name.to_owned()).font(ui(12.0, 600.0)).color(th.fg))
                .child(
                    text(what.to_owned())
                        .font(ui(11.0, 400.0))
                        .color(th.muted_fg),
                ),
        )
}

/// The "Add track" menu: one button in the toolbar instead of a button per kind, each kind
/// with a line saying what it is for; importing an audio file (a new audio track) is here too.
pub fn add_track_menu() -> El {
    let th = t();
    let add = |key: &str, ic: &'static rune::Icon, name: &str, what: &str, kind: InstrumentType| {
        add_row(key, ic, name, what).on_press(move |s: &mut App| {
            s.menu = None;
            s.add_song_track(kind);
        })
    };
    let list = popover_frame(Some(292.0), 4.0)
        .key("add-track-menu")
        .child(add(
            "add-synth",
            &icons::PIANO,
            "Synth",
            "Play and record notes with a built-in sound",
            InstrumentType::Synth,
        ))
        .child(add(
            "add-drums",
            &icons::DRUM,
            "Drums",
            "A kit of pads: make its beat in the Beat view",
            InstrumentType::Drums,
        ))
        .child(add(
            "add-audio",
            &crate::icons_io::MIC,
            "Audio",
            "Record a microphone, guitar or line input",
            InstrumentType::Audio,
        ))
        .child(el().h(1.0).w_full().my(4.0).fill(th.border))
        .child(
            add_row(
                "import-audio",
                &icons::FILE_AUDIO,
                "Import audio file…",
                "Put a WAV, MP3 or other file on a new track",
            )
            .on_press_tx(|s: &mut App, tx| {
                s.menu = None;
                s.import_audio_pick(tx);
            }),
        );
    select_popup("add-track", Align::Start, list)
}

fn ghost(key: &str, icon: &'static rune::Icon, label: &str, tip: &str, keys: &str) -> Button {
    Button::new(Variant::Ghost, Size::Sm)
        .icon(icon)
        .icon_size(14.0)
        .label(label)
        .font_size(12.0)
        .px(8.0)
        .key(key)
        .tip(tip, Some(keys))
}

fn tool_radio(s: &App, tool: ArrTool, ic: &'static rune::Icon, label: &str, keycap: &str) -> El {
    let th = t();
    let on = s.song.tool == tool;
    let mut b = row()
        .key(match tool {
            ArrTool::Pointer => "tool-pointer",
            ArrTool::Cut => "tool-cut",
        })
        .role("radio")
        .label(label.to_owned())
        .checked(on)
        .w(28.0)
        .h(22.0)
        .center()
        .radius(4.0)
        .help(label.to_owned())
        .help_keys([keycap.to_owned()])
        .child(icon(ic, 14.0).color(if on { th.primary_fg } else { th.muted_fg }))
        .on_press(move |s: &mut App| s.song.tool = tool);
    b = if on {
        b.fill(th.primary)
    } else {
        b.hover(th.surface3)
    };
    b
}

pub fn view(s: &App) -> El {
    let th = t();
    let snap_label = SNAP_VALUES
        .iter()
        .find(|(v, _)| *v == s.song.snap)
        .map_or_else(
            || crate::song::js_num(s.song.snap),
            |(_, l)| (*l).to_owned(),
        );
    let snap_open = s.menu == Some(Menu::Song(SongMenu::Snap));
    let snap = row()
        .key("snap-trigger")
        .role("combobox")
        .focusable(true)
        .label("Snap to grid")
        .help("Snap to grid")
        .w(64.0)
        .h(28.0)
        .items_center()
        .gap(6.0)
        .px(8.0)
        .radius(R_MD)
        .fill(th.surface0)
        .border(1.0, if snap_open { th.input } else { th.border })
        .hover(th.surface3)
        .child(text(snap_label).font(ui(12.0, 400.0)).color(th.fg))
        .child(spacer())
        .child(icon(&icons::CHEVRON_DOWN, 14.0).color(th.muted_fg.alpha(0.5)))
        .on_press(|s: &mut App| s.toggle_menu(Menu::Song(SongMenu::Snap)));
    let ppb = s.song.ppb;
    let pct = (ppb / DEFAULT_PPB * 100.0).round() as i64;
    let zoom = row()
        .items_center()
        .child(
            Button::new(Variant::Ghost, Size::IconSm)
                .icon(&icons_song::ZOOM_OUT)
                .a11y("Zoom out")
                .tip(
                    "Zoom out",
                    Some(&format!("{} scroll", mod_key().trim_end_matches('+'))),
                )
                .key("zoom-out")
                .disabled(ppb <= MIN_PPB)
                .build(|s: &mut App, _| {
                    let p = s.song.ppb;
                    s.song_zoom((p - 10.0).max(MIN_PPB), None);
                }),
        )
        .child(
            row()
                .key("zoom-reset")
                .role("button")
                .label("Reset zoom")
                .help("Reset zoom")
                .h(28.0)
                .w(44.0)
                .center()
                .radius(R_MD)
                .hover(th.surface3)
                .child(
                    text(format!("{pct}%"))
                        .key("zoom-pct")
                        .font(mono(11.0, 400.0))
                        .color(th.muted_fg),
                )
                .on_press(|s: &mut App| s.song_zoom(DEFAULT_PPB, None)),
        )
        .child(
            Button::new(Variant::Ghost, Size::IconSm)
                .icon(&icons_song::ZOOM_IN)
                .a11y("Zoom in")
                .tip(
                    "Zoom in",
                    Some(&format!("{} scroll", mod_key().trim_end_matches('+'))),
                )
                .key("zoom-in")
                .disabled(ppb >= MAX_PPB)
                .build(|s: &mut App, _| {
                    let p = s.song.ppb;
                    s.song_zoom((p + 10.0).min(MAX_PPB), None);
                }),
        );
    row()
        .key("song-toolbar")
        .h(36.0)
        .w_full()
        .items_center()
        .gap(8.0)
        .px(8.0)
        .fill(th.surface1)
        .border_sides([0.0, 0.0, 1.0, 0.0], th.border)
        .child(
            Button::new(Variant::Outline, Size::Sm)
                .icon(&icons::PLUS)
                .icon_size(14.0)
                .icon_ink(th.primary)
                .label("Add track")
                .weight(600.0)
                .font_size(12.0)
                .px(10.0)
                .pressed(s.menu == Some(Menu::Song(SongMenu::AddTrack)))
                .key("add-track")
                .tip("Add a synth, drum or audio track, or import audio", None)
                .build(|s: &mut App, _| s.toggle_menu(Menu::Song(SongMenu::AddTrack))),
        )
        .child(divider())
        .child(
            row()
                .items_center()
                .gap(2.0)
                .child(
                    ghost(
                        "add-marker",
                        &icons_song::FLAG,
                        "Add marker",
                        "Add marker at playhead",
                        "⇧M",
                    )
                    .build(|s: &mut App, _| s.add_marker_at_playhead()),
                )
                .child(
                    ghost(
                        "add-group",
                        &icons::FOLDER_PLUS,
                        "Group tracks",
                        "Group tracks (selected regions' tracks, or all)",
                        &format!("{}G", mod_key()),
                    )
                    .build(|s: &mut App, _| s.create_group_from_selection()),
                ),
        )
        .child(divider())
        .child(caption("Tool"))
        .child(
            row()
                .key("edit-tool")
                .role("radiogroup")
                .label("Edit tool")
                .h(28.0)
                .items_center()
                .gap(2.0)
                .padding(2.0)
                .radius(R_MD)
                .border(1.0, th.border)
                .fill(th.surface0)
                .child(tool_radio(
                    s,
                    ArrTool::Pointer,
                    &icons_song::MOUSE_POINTER_2,
                    "Pointer: select, move, resize",
                    "V",
                ))
                .child(tool_radio(
                    s,
                    ArrTool::Cut,
                    &icons::SCISSORS,
                    "Cut: click a region to split it",
                    "C",
                )),
        )
        .child(caption("Snap"))
        .child(snap)
        .child(caption("Zoom"))
        .child(zoom)
        .child(spacer())
        .child(
            Button::new(Variant::Outline, Size::Sm)
                .icon(&icons::DOWNLOAD)
                .icon_size(14.0)
                .label("Export")
                .weight(600.0)
                .font_size(12.0)
                .px(10.0)
                .key("song-export")
                .tip("Export WAV or MIDI", Some(&format!("{}E", mod_key())))
                .build(|s: &mut App, _| s.open_dialog(crate::views::Dialog::Export)),
        )
}

/// The snap select's list.
pub fn snap_menu(s: &App) -> El {
    let th = t();
    let mut list = popover_frame(None, 4.0).key("snap-menu").min_w(86.0);
    for (v, label) in SNAP_VALUES {
        let sel = v == s.song.snap;
        list = list.child(
            select_option(
                format!("snap-{label}"),
                row()
                    .child(text(label).font(ui(12.0, 400.0)).color(if sel {
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
                s.song.snap = v;
                s.menu = None;
            }),
        );
    }
    select_popup("snap-trigger", Align::Start, list)
}
