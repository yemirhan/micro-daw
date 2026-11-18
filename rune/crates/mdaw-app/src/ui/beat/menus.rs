//! `DrumTrackMenu.tsx`, `KitMenu.tsx` and `PadContextMenu.tsx`: triggers and their open
//! popovers (the popovers are drawn at the root, anchored to the trigger keys).

use mdaw_model::beat_helpers::{PAD_COLORS, pad_label, pad_name};
use mdaw_model::classic_meta::CLASSIC_MACHINES;
use mdaw_model::kit::create_classic_kit;
use mdaw_model::packs::service::{kit_from_installed, pack_kit_choices};
use mdaw_model::types::{InstrumentType, KitState};
use rune::{Align, El, KeyEvent, Tx, col, el, icon, row, text, text_input};

use crate::app::{App, Menu};
use crate::icons;
use crate::theme::{R_SM, css_color, mono, t, ui};
use crate::ui::widgets::{Button, Size, Variant, menu_row, popover, popover_frame};

/// `DrumTrackInfo` (`listDrumTracks`): drum tracks with repeated names numbered.
#[derive(Debug, Clone, PartialEq)]
pub struct DrumTrackInfo {
    pub id: String,
    pub label: String,
    pub color: String,
}

pub fn list_drum_tracks(s: &App) -> Vec<DrumTrackInfo> {
    let mut seen: Vec<(String, usize)> = Vec::new();
    s.svc
        .engine
        .doc()
        .get_tracks()
        .iter()
        .filter(|t| t.instrument.kind == InstrumentType::Drums)
        .map(|t| {
            let n = match seen.iter_mut().find(|(name, _)| *name == t.name) {
                Some((_, n)) => {
                    *n += 1;
                    *n
                }
                None => {
                    seen.push((t.name.clone(), 1));
                    1
                }
            };
            DrumTrackInfo {
                id: t.id.clone(),
                label: if n > 1 {
                    format!("{} {n}", t.name)
                } else {
                    t.name.clone()
                },
                color: t.color.clone(),
            }
        })
        .collect()
}

/// The drum track trigger: colour dot, label (or "No track"), chevron; max 112 points.
pub fn drum_track_trigger(s: &App) -> El {
    let th = t();
    let tracks = list_drum_tracks(s);
    let active_id = s.active_track();
    let active = tracks.iter().find(|t| Some(&t.id) == active_id.as_ref());
    let dot = css_color(active.map_or("", |a| a.color.as_str()), th.muted_fg);
    row()
        .key("drum-track")
        .role("button")
        .label(format!(
            "Drum track: {}",
            active.map_or("none", |a| a.label.as_str())
        ))
        .help("Drum track the pads and steps edit")
        .h(24.0)
        .max_w(112.0)
        .items_center()
        .gap(4.0)
        .px(6.0)
        .ml(-6.0)
        .radius(R_SM)
        .fill(if s.menu == Some(Menu::DrumTrack) {
            th.surface3
        } else {
            rune::Color::TRANSPARENT
        })
        .hover(th.surface3)
        .child(el().size(8.0, 8.0).radius(4.0).fill(dot))
        .child(
            text(active.map_or("No track".to_owned(), |a| a.label.clone()))
                .font(ui(12.0, 600.0))
                .color(th.fg)
                .truncate()
                .min_w(0.0),
        )
        .child(icon(&icons::CHEVRON_DOWN, 12.0).color(th.muted_fg))
        .on_press(|s: &mut App| s.toggle_menu(Menu::DrumTrack))
}

pub fn drum_track_menu(s: &App) -> El {
    let th = t();
    let tracks = list_drum_tracks(s);
    let active = s.active_track();
    let mut list = col().role("listbox").label("Drum tracks");
    for tr in &tracks {
        let sel = Some(&tr.id) == active.as_ref();
        let id = tr.id.clone();
        list = list.child(
            menu_row(
                row()
                    .key(format!("drum-track-{}", tr.id))
                    .role("option")
                    .checked(sel)
                    .child(
                        el().size(8.0, 8.0)
                            .radius(4.0)
                            .fill(css_color(&tr.color, th.muted_fg)),
                    )
                    .child(
                        text(tr.label.clone())
                            .font(ui(12.0, if sel { 600.0 } else { 400.0 }))
                            .color(th.fg)
                            .grow(1.0)
                            .truncate(),
                    )
                    .child(if sel {
                        icon(&icons::CHECK, 14.0).color(th.primary)
                    } else {
                        el()
                    }),
                false,
            )
            .on_press(move |s: &mut App| {
                s.set_active_drum_track(&id);
                s.menu = None;
            }),
        );
    }
    if tracks.is_empty() {
        list = list.child(
            text("No drum tracks yet.")
                .font(ui(11.0, 400.0))
                .color(th.muted_fg)
                .px(8.0)
                .py(4.0),
        );
    }
    let panel = popover_frame(Some(224.0), 4.0)
        .key("drum-track-menu")
        .child(
            text("Drum track")
                .font(ui(11.0, 400.0))
                .color(th.muted_fg)
                .px(8.0)
                .pt(6.0)
                .pb(4.0),
        )
        .child(list)
        .child(el().h(1.0).w_full().my(4.0).fill(th.border))
        .child(
            menu_row(
                row()
                    .key("new-drum-track")
                    .child(icon(&icons::PLUS, 14.0).color(th.muted_fg))
                    .child(text("New drum track").font(ui(12.0, 400.0)).color(th.fg)),
                false,
            )
            .on_press(|s: &mut App| {
                s.new_drum_track();
                s.menu = None;
            }),
        );
    popover("drum-track", Align::Start, panel)
}

/// The kit trigger: drum icon, kit name, chevron; max 150 points (96 with a pending chip).
pub fn kit_trigger(s: &App, narrow: bool) -> El {
    let th = t();
    let name = s.kit().name;
    row()
        .key("kit-menu")
        .role("button")
        .label(format!("Kit: {name}"))
        .h(24.0)
        .max_w(if narrow { 96.0 } else { 150.0 })
        .items_center()
        .gap(4.0)
        .px(6.0)
        .radius(R_SM)
        .fill(if s.menu == Some(Menu::Kit) {
            th.surface3
        } else {
            rune::Color::TRANSPARENT
        })
        .hover(th.surface3)
        .child(icon(&icons::DRUM, 14.0).color(th.muted_fg))
        .child(
            text(name)
                .font(ui(12.0, 600.0))
                .color(th.fg)
                .truncate()
                .min_w(0.0),
        )
        .child(icon(&icons::CHEVRON_DOWN, 12.0).color(th.muted_fg))
        .on_press(|s: &mut App| {
            if s.menu == Some(Menu::Kit) {
                s.close_menu();
            } else {
                // Opening prefills the save field with the kit name ("Factory" leaves it empty).
                let name = s.kit().name;
                s.beat.kit_name = rune::TextEdit::new(if name == "Factory" {
                    String::new()
                } else {
                    name
                });
                s.menu = Some(Menu::Kit);
            }
        })
}

fn group_label(label: &str) -> El {
    text(label.to_owned())
        .font(ui(11.0, 400.0))
        .color(t().muted_fg)
        .px(8.0)
        .pt(8.0)
        .pb(4.0)
}

fn choice_row(
    key: String,
    name: &str,
    detail: Option<&str>,
    color: rune::Color,
    pick: impl Fn(&mut App) + 'static,
) -> El {
    let th = t();
    let mut r = row()
        .key(key)
        .child(el().size(8.0, 8.0).radius(2.0).fill(color))
        .child(
            text(name.to_owned())
                .font(ui(12.0, 400.0))
                .color(th.fg)
                .grow(1.0)
                .min_w(0.0)
                .truncate(),
        );
    if let Some(d) = detail {
        r = r.child(
            text(d.to_owned())
                .font(ui(10.5, 400.0))
                .color(th.muted_fg)
                .truncate()
                .max_w(110.0),
        );
    }
    menu_row(r, false).on_press(pick)
}

pub fn kit_menu(s: &App) -> El {
    let th = t();
    let mut scroll = col().child(group_label("Built-in")).child(choice_row(
        "kit-factory".into(),
        "Factory",
        None,
        th.primary,
        |s: &mut App| {
            s.load_factory_kit();
            s.menu = None;
        },
    ));
    for m in CLASSIC_MACHINES.iter() {
        let id = m.id;
        scroll = scroll.child(choice_row(
            format!(
                "kit-classic-{}",
                mdaw_model::classic_meta::classic_kit_id(id)
            ),
            m.name,
            None,
            css_color(m.color, th.surface4),
            move |s: &mut App| {
                let kit = create_classic_kit(id);
                s.load_kit(&kit);
                s.menu = None;
            },
        ));
    }
    let installed = s.svc.packs.snapshot().store.installed.clone();
    let choices = pack_kit_choices(&installed);
    if !choices.is_empty() {
        scroll = scroll.child(group_label("Installed packs"));
    }
    for c in choices {
        let (pack_id, kit_id) = (c.pack_id.clone(), c.kit_id.clone());
        let inst = installed.clone();
        scroll = scroll.child(choice_row(
            format!("kit-pack-{}", c.id),
            &c.name,
            c.detail.as_deref(),
            c.color
                .as_deref()
                .map_or(th.surface4, |x| css_color(x, th.surface4)),
            move |s: &mut App| {
                if let Some(kit) = inst
                    .iter()
                    .find(|p| p.id == pack_id)
                    .and_then(|p| kit_from_installed(p, &kit_id))
                {
                    s.load_kit(&kit);
                }
                s.menu = None;
            },
        ));
    }
    scroll = scroll.child(group_label("Your kits"));
    for k in &s.user_kits {
        let kit: KitState = k.clone();
        let del_id = k.id.clone();
        scroll = scroll.child(
            row()
                .key(format!("user-kit-{}", k.id))
                .w_full()
                .items_center()
                .radius(R_SM)
                .hover(th.surface3)
                .hover_group()
                .child(
                    text(k.name.clone())
                        .font(ui(12.0, 400.0))
                        .color(th.fg)
                        .px(8.0)
                        .py(6.0)
                        .grow(1.0)
                        .truncate()
                        .on_press(move |s: &mut App| {
                            s.load_kit(&kit);
                            s.menu = None;
                        }),
                )
                .child(
                    row()
                        .key(format!("delete-kit-{}", k.id))
                        .role("button")
                        .label(format!("Delete kit {}", k.name))
                        .mr(4.0)
                        .padding(4.0)
                        .radius(R_SM)
                        .group_opacity(0.0, 1.0)
                        .child(icon(&icons::TRASH_2, 14.0).color(th.muted_fg))
                        .on_press(move |s: &mut App| s.delete_kit(&del_id)),
                ),
        );
    }
    if s.user_kits.is_empty() {
        scroll = scroll.child(
            text("Saved kits appear here.")
                .font(ui(11.0, 400.0))
                .color(th.muted_fg)
                .px(8.0)
                .py(4.0),
        );
    }
    let scroll = scroll
        .scroll(rune::ScrollSpec {
            stick_bottom: false,
        })
        .max_h(540.0)
        .min_h(0.0);
    let save = row()
        .gap(4.0)
        .padding(4.0)
        .child(
            text_input(&s.beat.kit_name, ui(12.0, 400.0))
                .key("kit-name")
                .label("Kit name")
                .placeholder("Kit name", t().muted_fg)
                .single_line()
                .color(th.fg)
                .h(28.0)
                .grow(1.0)
                .min_w(0.0)
                .px(8.0)
                .radius(R_SM)
                .border(1.0, th.input)
                .fill(th.surface0)
                .on_edit(|s: &mut App| &mut s.beat.kit_name)
                .on_submit(|s: &mut App, _tx: &mut Tx| s.submit_kit_name()),
        )
        .child(
            Button::new(Variant::Primary, Size::Sm)
                .label("Save kit")
                .key("save-kit")
                .build(|s: &mut App, _| s.submit_kit_name()),
        );
    let panel = popover_frame(Some(288.0), 4.0)
        .key("kit-menu-panel")
        .child(scroll)
        .child(el().h(1.0).w_full().my(4.0).fill(th.border))
        .child(
            menu_row(
                row()
                    .key("browse-packs")
                    .child(icon(&icons::PACKAGE, 14.0).color(th.muted_fg))
                    .child(text("Browse packs…").font(ui(12.0, 400.0)).color(th.fg)),
                false,
            )
            .on_press(|s: &mut App| {
                s.menu = None;
                s.switch_mode(crate::app::AppMode::Packs);
            }),
        )
        .child(el().h(1.0).w_full().my(4.0).fill(th.border))
        .child(save);
    popover("kit-menu", Align::Start, panel)
}

/// `PadContextMenu` at the pointer: name + label, Rename…, Color swatches (+ Reset color),
/// Choke group 0–4, Copy / Paste / Chop to pads…, Clear pad.
pub fn pad_menu(s: &App, pad: usize, x: f32, y: f32, renaming: bool, win: [f32; 2]) -> El {
    let th = t();
    let settings = s.pad_settings(pad).unwrap_or_default();
    let has_sample = settings.sample.is_some();
    let name = pad_name(Some(&settings));
    let item = |key: &str, label: &str, disabled: bool, destructive: bool| {
        menu_row(
            row().key(key.to_owned()).child(
                text(label.to_owned())
                    .font(ui(12.0, 400.0))
                    .color(if destructive { th.destructive } else { th.fg }),
            ),
            disabled,
        )
    };
    let mut m = popover_frame(Some(208.0), 4.0)
        .key("pad-menu")
        .role("menu")
        .label(format!("Pad {}", pad_label(pad)))
        .on_secondary_press(|_s: &mut App, _p, _tx: &mut Tx| {})
        .child(
            row()
                .items_end()
                .justify_between()
                .px(8.0)
                .pt(4.0)
                .pb(6.0)
                .child(
                    text(if name.is_empty() {
                        "Empty pad".to_owned()
                    } else {
                        name.clone()
                    })
                    .font(ui(12.0, 600.0))
                    .color(th.fg)
                    .truncate(),
                )
                .child(
                    text(pad_label(pad))
                        .font(mono(11.0, 400.0))
                        .color(th.muted_fg),
                ),
        );
    if renaming {
        m = m.child(
            row().px(4.0).pb(4.0).child(
                text_input(&s.beat.rename, ui(12.0, 400.0))
                    .key("pad-rename")
                    .label("Pad name")
                    .single_line()
                    .autofocus()
                    .color(th.fg)
                    .h(28.0)
                    .w_full()
                    .px(8.0)
                    .radius(R_SM)
                    .border(1.0, th.input)
                    .fill(th.surface0)
                    .on_edit(|s: &mut App| &mut s.beat.rename)
                    .on_submit(move |s: &mut App, _tx: &mut Tx| {
                        let draft = s.beat.rename.text.clone();
                        s.rename_pad(pad, &draft);
                        s.menu = None;
                    }),
            ),
        );
    } else {
        m = m.child(
            item("pad-menu-rename", "Rename…", !has_sample, false).on_press_tx(
                move |s: &mut App, tx: &mut Tx| {
                    let name = s
                        .pad_settings(pad)
                        .map(|p| pad_name(Some(&p)))
                        .unwrap_or_default();
                    s.beat.rename = rune::TextEdit::new(name);
                    if let Some(Menu::Pad { renaming, .. }) = &mut s.menu {
                        *renaming = true;
                    }
                    tx.focus("pad-rename", true);
                    tx.select_all_text("pad-rename");
                },
            ),
        );
    }
    // Colour swatches (8 columns of 18 points).
    let mut grid = col().gap(4.0);
    for chunk in PAD_COLORS.chunks(8) {
        let mut r = row().gap(4.0);
        for c in chunk {
            let cs = (*c).to_owned();
            let on = settings.color.as_deref() == Some(*c);
            let mut sw = el()
                .key(format!("pad-color-{c}"))
                .role("button")
                .label(format!("Color {c}"))
                .size(18.0, 18.0)
                .radius(3.0)
                .fill(css_color(c, th.primary))
                .on_press(move |s: &mut App| {
                    s.update_pad(pad, serde_json::json!({ "color": cs }));
                    s.menu = None;
                });
            if on {
                sw = sw.border(2.0, th.fg);
            }
            r = r.child(sw);
        }
        grid = grid.child(r);
    }
    let mut color = col()
        .px(8.0)
        .py(6.0)
        .child(
            text("Color")
                .font(ui(11.0, 400.0))
                .color(th.muted_fg)
                .mb(4.0),
        )
        .child(grid);
    if settings.color.is_some() {
        color = color.child(
            text("Reset color")
                .key("pad-color-reset")
                .role("button")
                .font(ui(11.0, 400.0))
                .color(th.muted_fg)
                .mt(4.0)
                .on_press(move |s: &mut App| {
                    s.update_pad(pad, serde_json::json!({ "color": null }));
                    s.menu = None;
                }),
        );
    }
    m = m.child(color);
    let mut choke = row().gap(4.0).role("radiogroup").label("Choke group");
    for g in [0i64, 1, 2, 3, 4] {
        let sel = settings.choke_group == g;
        let mut b = row()
            .key(format!("pad-choke-{g}"))
            .role("radio")
            .checked(sel)
            .h(24.0)
            .grow(1.0)
            .center()
            .radius(R_SM)
            .border(
                1.0,
                if sel {
                    rune::Color::TRANSPARENT
                } else {
                    th.border
                },
            )
            .fill(if sel { th.fg } else { th.surface2 })
            .child(
                text(if g == 0 {
                    "Off".to_owned()
                } else {
                    g.to_string()
                })
                .font(mono(11.0, 400.0))
                .color(if sel { th.surface0 } else { th.muted_fg }),
            );
        b = b.on_press(move |s: &mut App| {
            s.update_pad(pad, serde_json::json!({ "chokeGroup": g }));
            s.menu = None;
        });
        choke = choke.child(b);
    }
    m = m.child(
        col()
            .px(8.0)
            .py(6.0)
            .child(
                text("Choke group")
                    .font(ui(11.0, 400.0))
                    .color(th.muted_fg)
                    .mb(4.0),
            )
            .child(choke),
    );
    m = m
        .child(el().h(1.0).w_full().my(4.0).fill(th.border))
        .child(
            item("pad-menu-copy", "Copy pad", false, false).on_press(move |s: &mut App| {
                s.copy_pad(pad);
                s.menu = None;
            }),
        )
        .child(
            item("pad-menu-paste", "Paste pad", s.clipboard.is_none(), false).on_press(
                move |s: &mut App| {
                    s.paste_pad(pad);
                    s.menu = None;
                },
            ),
        )
        .child(
            item("pad-menu-chop", "Chop to pads…", !has_sample, false).on_press(
                move |s: &mut App| {
                    s.menu = None;
                    s.request_chop(pad);
                },
            ),
        )
        .child(el().h(1.0).w_full().my(4.0).fill(th.border))
        .child(
            item("pad-menu-clear", "Clear pad", !has_sample, true).on_press(move |s: &mut App| {
                s.clear_pad(pad);
                s.menu = None;
            }),
        );
    // Keep it at least 4 points inside the window (measured height, else an estimate).
    let h = 330.0;
    let nx = x.min(win[0] - 208.0 - 4.0).max(4.0);
    let ny = y.min(win[1] - h - 4.0).max(4.0);
    m.absolute(nx, ny)
        .modal(true, |s: &mut App| s.close_menu())
        .on_key(|s: &mut App, ev: &KeyEvent, _tx: &mut Tx| {
            if matches!(ev.key, rune::Key::Escape) {
                s.close_menu();
                return true;
            }
            false
        })
}

impl App {
    /// The kit menu's save form (`onSaveKit(name)`, then close).
    pub fn submit_kit_name(&mut self) {
        let name = self.beat.kit_name.text.clone();
        self.save_kit(&name);
        self.menu = None;
    }
}
