//! `BeatView.tsx`: transport strip; pads (header: drum track / kit / pending chip / layout /
//! key hints / controller) with the missing-packs strip and the count-in overlay; the pad
//! editor; the step sequencer.

pub mod editor;
pub mod menus;
pub mod packs;
pub mod pads;
pub mod steps;
pub mod transport;

use mdaw_model::beat_helpers::PadLayout;
use rune::{Cx, El, Shadow, col, el, row, spacer, text};

use crate::app::App;
use crate::icons;
use crate::theme::{t, ui};
use crate::ui::widgets::{Button, Segmented, Size, Variant};

/// The pads header (`group/padhead`).
fn pads_header(s: &App, chip: Option<El>) -> El {
    let th = t();
    let layout = s.beat.layout;
    let pending = chip.is_some();
    let mut h = row()
        .key("pads-header")
        .h(24.0)
        .mb(8.0)
        .w_full()
        .min_w(0.0)
        .items_center()
        .gap(4.0)
        .child(menus::drum_track_trigger(s))
        .child(text("/").font(ui(13.0, 400.0)).color(th.muted_fg))
        .child(menus::kit_trigger(s, pending));
    if let Some(c) = chip {
        h = h.child(c);
    }
    // 4×4 is too narrow for both: the layout toggle and key hints step aside for the chip.
    let hide = layout == PadLayout::FourByFour && pending;
    if !hide {
        h = h.child(
            Segmented::new(
                "Pad layout",
                layout,
                vec![
                    (PadLayout::FourByFour, "4×4".into()),
                    (PadLayout::TwoByEight, "2×8".into()),
                ],
            )
            .height(24.0)
            .key("pad-layout")
            .build(|s: &mut App, l| s.set_layout(l)),
        );
    }
    let mut right = row().items_center().gap(2.0);
    if !hide {
        right = right.child(
            Button::new(Variant::Ghost, Size::IconXs)
                .icon(&icons::KEYBOARD)
                .a11y("Computer-key hints")
                .tip(
                    if s.beat.show_keys {
                        "Hide computer-key hints"
                    } else {
                        "Show computer-key hints"
                    },
                    None,
                )
                .pressed(s.beat.show_keys)
                .key("key-hints")
                .build(|s: &mut App, _| s.toggle_key_hints()),
        );
    }
    let profile_name = s
        .svc
        .midi
        .host
        .shared()
        .lock()
        .router
        .mapping
        .get_profile()
        .name
        .clone();
    let mut ctl = Button::new(
        Variant::Ghost,
        if layout == PadLayout::TwoByEight {
            Size::Xs
        } else {
            Size::IconXs
        },
    )
    .icon(&icons::SETTINGS_2)
    .a11y("Controller setup")
    .tip(
        format!("Controller setup ({profile_name}): map pads and knobs"),
        None,
    )
    .key("pads-controller");
    if layout == PadLayout::TwoByEight {
        ctl = ctl.label(if profile_name.chars().count() > 18 {
            "Controller".to_owned()
        } else {
            profile_name
        });
    }
    right =
        right.child(ctl.build(|s: &mut App, _| s.open_dialog(crate::views::Dialog::Controller)));
    h.child(spacer()).child(right)
}

pub fn view(s: &App, cx: &mut Cx<App>, win_h: f32) -> El {
    let th = t();
    let kit = s.kit();
    let missing = packs::missing_packs(s, &kit.pads);
    let titles = packs::missing_pack_titles(&missing, kit.pads.len());
    let selected = s.selected_pad();
    let selected_missing = missing.iter().find(|m| m.pads.contains(&selected)).cloned();
    let layout = s.beat.layout;
    let chip_w = cx.layout_size("pending-load").map_or(200.0, |w| w[0]);
    let chip = packs::pending_chip(s, chip_w);

    let transport = row()
        .w_full()
        .px(12.0)
        .py(8.0)
        .fill(th.surface1)
        .border_sides([0.0, 0.0, 1.0, 0.0], th.border)
        .child(transport::view(s, cx));

    let grid_opts = pads::GridOpts {
        layout,
        bank: None,
        compact: false,
        key_hints: s.beat.show_keys,
        full: true,
        prefix: "pad-",
    };
    let mut grid = pads::grid(s, cx, grid_opts, &titles).min_h(0.0);
    grid = if layout == PadLayout::TwoByEight {
        grid.h(220.0)
    } else {
        grid.grow(1.0)
    };
    let mut pads_sec = col()
        .key("pads")
        .role("region")
        .label("Pads")
        .w(if layout == PadLayout::FourByFour {
            392.0
        } else {
            600.0
        })
        .h_full()
        .padding(12.0)
        .border_sides([0.0, 1.0, 0.0, 0.0], th.border)
        .child(pads_header(s, chip));
    if let Some(strip) = packs::strip(s, &missing, s.active_track().is_some()) {
        pads_sec = pads_sec.child(strip);
    }
    if layout == PadLayout::TwoByEight {
        pads_sec = pads_sec.child(spacer()).child(grid).child(spacer());
    } else {
        pads_sec = pads_sec.child(grid);
    }
    let count = s.count_in();
    if count > 0 {
        pads_sec = pads_sec.child(
            row()
                .key("count-in-overlay")
                .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
                .center()
                .fill(th.surface0.alpha(0.45))
                .inert()
                .child(
                    text(count.to_string())
                        .font(ui(132.0, 700.0).lh(132.0))
                        .color(th.rec)
                        .box_shadow(Shadow {
                            x: 0.0,
                            y: 4.0,
                            blur: 24.0,
                            spread: 0.0,
                            color: th.black.alpha(0.6),
                            inset: false,
                        }),
                ),
        );
    }
    let editor = col()
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .padding(12.0)
        .child(editor::view(s, cx, selected_missing.as_ref()));
    let top_h = (win_h * 0.46).min(372.0);
    let pads_row = row()
        .w_full()
        .h(top_h)
        .border_sides([0.0, 0.0, 1.0, 0.0], th.border)
        .child(pads_sec)
        .child(editor);
    let main = col()
        .grow(1.0)
        .min_w(0.0)
        .min_h(0.0)
        .child(pads_row)
        .child(steps::view(s, cx));
    col()
        .key("beat-view")
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .fill(th.surface0)
        .child(transport)
        .child(row().grow(1.0).min_h(0.0).child(main))
        .child(el().h(0.0))
}
