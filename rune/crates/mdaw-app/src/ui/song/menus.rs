//! `RegionContextMenu` and `MarkerContextMenu`: fixed at the pointer, closed by a click
//! outside or Escape.

use mdaw_model::doc::{MARKER_COLORS, MarkerUpdate};
use rune::{El, Key, KeyEvent, Tx, col, el, row, text, text_input};

use crate::app::{App, Menu};
use crate::song::SongMenu;
use crate::theme::{R_SM, ui};
use crate::ui::song::track_color;
use crate::ui::widgets::popover_frame;

/// Keeps a menu of `w × h` inside the window (4-point margin).
fn place(x: f32, y: f32, w: f32, h: f32, win: [f32; 2]) -> (f32, f32) {
    (
        x.min(win[0] - w - 4.0).max(4.0),
        y.min(win[1] - h - 4.0).max(4.0),
    )
}

fn item(key: &str, label: &str, destructive: bool) -> El {
    let th = crate::theme::t();
    row()
        .key(key.to_owned())
        .role("menuitem")
        .label(label.to_owned())
        .w_full()
        .px(12.0)
        .py(6.0)
        .hover(th.surface3)
        .child(
            text(label.to_owned())
                .font(ui(12.0, 400.0))
                .color(if destructive { th.destructive } else { th.fg }),
        )
}

/// The region context menu: Split at playhead, Duplicate, Copy, Paste (with a clipboard),
/// Quantize (MIDI regions), Delete.
pub fn region_menu(s: &App, x: f32, y: f32, track: &str, region: &str, win: [f32; 2]) -> El {
    let is_audio = s
        .find_track(track)
        .and_then(|t| t.regions.iter().find(|r| r.id == region))
        .is_some_and(|r| r.audio.is_some());
    let can_paste = s.has_region_clipboard();
    let n = 4 + usize::from(can_paste) + usize::from(!is_audio);
    let (px, py) = place(x, y, 140.0, n as f32 * 28.0 + 8.0, win);
    let (t, r) = (track.to_owned(), region.to_owned());
    let mut m = popover_frame(None, 0.0)
        .key("region-menu")
        .role("menu")
        .min_w(140.0)
        .radius(6.0)
        .py(4.0)
        .absolute(px, py)
        .on_secondary_press(|_s: &mut App, _p, _tx: &mut Tx| {});
    let (t1, r1) = (t.clone(), r.clone());
    m = m.child(item("region-split", "Split at playhead", false).on_press(move |s: &mut App| {
        s.split_region_at_playhead(&t1, &r1);
        s.menu = None;
    }));
    let (t1, r1) = (t.clone(), r.clone());
    m = m.child(item("region-duplicate", "Duplicate", false).on_press(move |s: &mut App| {
        s.duplicate_region(&t1, &r1);
        s.menu = None;
    }));
    let (t1, r1) = (t.clone(), r.clone());
    m = m.child(item("region-copy", "Copy", false).on_press(move |s: &mut App| {
        s.copy_region(&t1, &r1);
        s.menu = None;
    }));
    if can_paste {
        let t1 = t.clone();
        m = m.child(item("region-paste", "Paste", false).on_press(move |s: &mut App| {
            s.paste_region(&t1);
            s.menu = None;
        }));
    }
    if !is_audio {
        let (t1, r1) = (t.clone(), r.clone());
        m = m.child(item("region-quantize", "Quantize", false).on_press(move |s: &mut App| {
            s.quantize_region(&t1, &r1);
            s.menu = None;
        }));
    }
    let (t1, r1) = (t, r);
    m = m.child(item("region-delete", "Delete", true).on_press(move |s: &mut App| {
        s.remove_region(&t1, &r1);
        s.menu = None;
    }));
    m.modal(true, |s: &mut App| s.close_menu())
}

/// The marker context menu: Rename (inline input), Color swatches, Delete.
pub fn marker_menu(
    s: &App,
    x: f32,
    y: f32,
    marker: &str,
    renaming: bool,
    win: [f32; 2],
) -> Option<El> {
    let th = crate::theme::t();
    let mk = s
        .svc
        .engine
        .doc()
        .get_markers()
        .iter()
        .find(|m| m.id == marker)?
        .clone();
    let (px, py) = place(x, y, 160.0, 110.0, win);
    let mut m = popover_frame(None, 0.0)
        .key("marker-menu")
        .role("menu")
        .min_w(160.0)
        .radius(6.0)
        .py(4.0)
        .absolute(px, py)
        .on_secondary_press(|_s: &mut App, _p, _tx: &mut Tx| {});
    if renaming {
        m = m.child(
            row().px(8.0).py(4.0).child(
                text_input(&s.song.marker_rename, ui(12.0, 400.0))
                    .key("marker-rename")
                    .label("Marker name")
                    .single_line()
                    .autofocus()
                    .color(th.fg)
                    .w_full()
                    .h(22.0)
                    .px(6.0)
                    .radius(R_SM)
                    .border(1.0, th.border)
                    .fill(th.surface0)
                    .on_edit(|s: &mut App| &mut s.song.marker_rename)
                    .on_input_key(|s: &mut App, ev: &KeyEvent, _tx: &mut Tx| {
                        if matches!(ev.key, Key::Escape) {
                            if let Some(Menu::Song(SongMenu::Marker { renaming, .. })) = &mut s.menu {
                                *renaming = false;
                            }
                            return true;
                        }
                        false
                    })
                    .on_submit(|s: &mut App, _tx: &mut Tx| s.commit_marker_rename()),
            ),
        );
    } else {
        let name = mk.name.clone();
        m = m.child(item("marker-rename-item", "Rename", false).on_press_tx(
            move |s: &mut App, tx: &mut Tx| {
                s.song.marker_rename = crate::song::selected_edit(name.clone());
                if let Some(Menu::Song(SongMenu::Marker { renaming, .. })) = &mut s.menu {
                    *renaming = true;
                }
                tx.focus("marker-rename", true);
            },
        ));
    }
    let mut sw = row().gap(4.0);
    for (i, c) in MARKER_COLORS.into_iter().enumerate() {
        let id = mk.id.clone();
        let mut b = el()
            .key(format!("marker-color-{i}"))
            .role("button")
            .label(format!("Color {c}"))
            .size(16.0, 16.0)
            .radius(8.0)
            .fill(track_color(c))
            .border(1.0, th.border.alpha(0.5))
            .on_press(move |s: &mut App| {
                s.update_marker(
                    &id,
                    MarkerUpdate {
                        color: Some(c.to_owned()),
                        ..Default::default()
                    },
                );
                s.menu = None;
            });
        if c == mk.color {
            b = b.border(2.0, crate::theme::t().fg.alpha(0.85));
        }
        sw = sw.child(b);
    }
    m = m
        .child(
            col()
                .px(12.0)
                .py(6.0)
                .child(text("Color").font(ui(10.0, 600.0)).color(th.muted_fg).mb(4.0))
                .child(sw),
        )
        .child(el().h(1.0).w_full().my(2.0).fill(th.border));
    let id = mk.id;
    m = m.child(item("marker-delete", "Delete", true).on_press(move |s: &mut App| {
        s.remove_marker(&id);
        s.menu = None;
    }));
    Some(m.modal(true, |s: &mut App| {
        // A click outside: a pending rename commits (the input's blur), then the menu closes.
        s.commit_marker_rename();
        s.close_menu();
    }))
}
