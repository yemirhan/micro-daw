//! `InstrumentDock` in the Song view (pads available, expanded by default): header ("Play",
//! Pads / Keys toggles, MIDI route, chord + held notes, collapse), then the dock pads (one bank
//! of eight with the A / B switch, `micro-daw:dock-pad-bank`) and the compact on-screen keyboard
//! (`PianoRoll.tsx` / `PianoKey.tsx`, §7 of the views inventory: the Beat dock's
//! `crate::ui::dock::keyboard`, glissando and releases included).

use mdaw_midi::RoutingMode;
use mdaw_model::notes::midi_to_note_name;
use mdaw_model::settings::{PadBank, write_dock_pad_bank};
use rune::{Color, Cx, El, col, el, icon, row, spacer, text};

use crate::app::App;
use crate::icons;
use crate::theme::{R_MD, daw_label, mono, t, ui};
use crate::ui::beat::pads::{GridOpts, grid};
use crate::ui::widgets::{Button, Segmented, Size, Variant, vrule};

fn view_toggle(key: &str, on: bool, ic: &'static rune::Icon, label: &str) -> El {
    let th = t();
    row()
        .key(key.to_owned())
        .role("button")
        .label(label.to_owned())
        .checked(on)
        .h(24.0)
        .items_center()
        .gap(6.0)
        .px(8.0)
        .radius(4.0)
        .fill(if on { th.surface3 } else { Color::TRANSPARENT })
        .hover(th.surface3)
        .child(icon(ic, 14.0).color(if on { th.fg } else { th.muted_fg }))
        .child(text(label.to_owned()).font(ui(11.0, 600.0)).color(if on {
            th.fg
        } else {
            th.muted_fg
        }))
}

pub fn view(s: &App) -> El {
    view_cx(s, None)
}

/// The dock (`cx` measures the compact pads).
pub fn view_cx(s: &App, cx: Option<&mut Cx<App>>) -> El {
    let th = t();
    let d = &s.song.dock;
    let collapsed = !d.show_pads && !d.show_keys;
    let mode = s.svc.midi.host.shared().lock().router.routing_mode();
    let notes = s.active_notes();
    let chord =
        mdaw_model::chords::detect_chord(&notes.iter().map(|n| *n as i32).collect::<Vec<_>>());
    let route = Segmented::new(
        "MIDI note routing",
        mode,
        vec![
            (RoutingMode::Auto, "Auto".into()),
            (RoutingMode::Keys, "Keys".into()),
            (RoutingMode::Split, "Split".into()),
        ],
    )
    .sans()
    .weight(600.0, 600.0)
    .item_px(8.0)
    .key("song-routing")
    .build(|s: &mut App, m| s.svc.with_router(|r, _| r.set_routing_mode(m)));
    let mut chord_el = row()
        .min_w(52.0)
        .px(6.0)
        .py(2.0)
        .radius(3.0)
        .justify_center()
        .help("Detected chord")
        .child(
            text(chord.as_ref().map_or("—".to_owned(), |c| c.display.clone()))
                .font(mono(11.0, 600.0))
                .color(if chord.is_some() {
                    th.primary
                } else {
                    th.muted_fg
                }),
        );
    if chord.is_some() {
        chord_el = chord_el.fill(th.primary.alpha(0.15));
    }
    let mut held = row().min_w(140.0).items_center().gap(6.0);
    if notes.is_empty() {
        held = held.child(
            text("no notes held")
                .font(mono(11.0, 400.0))
                .color(th.muted_fg),
        );
    } else {
        for n in notes.iter().take(8) {
            held = held.child(
                text(midi_to_note_name(*n as f64))
                    .font(mono(11.0, 400.0))
                    .color(th.fg),
            );
        }
    }
    let header = row()
        .key("song-dock-header")
        .h(36.0)
        .w_full()
        .items_center()
        .gap(8.0)
        .px(12.0)
        .child(
            text("Play")
                .font(daw_label(11.0))
                .text_case(rune::TextCase::Uppercase)
                .color(th.muted_fg)
                .mr(4.0),
        )
        .child(
            row()
                .items_center()
                .gap(2.0)
                .child(
                    view_toggle("song-dock-pads", d.show_pads, &icons::DRUM, "Pads")
                        .on_press(|s: &mut App| s.song.dock.show_pads = !s.song.dock.show_pads),
                )
                .child(
                    view_toggle("song-dock-keys", d.show_keys, &icons::PIANO, "Keys")
                        .on_press(|s: &mut App| s.song.dock.show_keys = !s.song.dock.show_keys),
                ),
        )
        .child(vrule(16.0, th.border).mx(4.0))
        .child(
            text("MIDI route")
                .font(daw_label(11.0))
                .text_case(rune::TextCase::Uppercase)
                .color(th.muted_fg),
        )
        .child(route)
        .child(spacer())
        .child(row().items_center().gap(8.0).child(chord_el).child(held))
        .child(
            Button::new(Variant::Ghost, Size::IconXs)
                .icon(if collapsed {
                    &icons::CHEVRON_UP
                } else {
                    &icons::CHEVRON_DOWN
                })
                .a11y(if collapsed {
                    "Expand instrument dock"
                } else {
                    "Collapse instrument dock"
                })
                .tip(
                    if collapsed {
                        "Show instruments"
                    } else {
                        "Hide instruments"
                    },
                    None,
                )
                .key("song-dock-collapse")
                .build(|s: &mut App, _| {
                    let c = !s.song.dock.show_pads && !s.song.dock.show_keys;
                    s.song.dock.show_pads = c;
                    s.song.dock.show_keys = c;
                }),
        );
    let mut dock = col()
        .key("song-dock")
        .w_full()
        .fill(th.surface1)
        .border_sides([1.0, 0.0, 0.0, 0.0], th.border)
        .child(header);
    if !collapsed {
        let mut body = row()
            .h(156.0)
            .w_full()
            .items_stretch()
            .gap(12.0)
            .px(12.0)
            .py(10.0)
            .border_sides([1.0, 0.0, 0.0, 0.0], th.hairline);
        if d.show_pads {
            body = body.child(dock_pads(s, cx));
        }
        if d.show_keys {
            body = body.child(
                row()
                    .grow(1.0)
                    .min_w(0.0)
                    .items_end()
                    .child(crate::ui::dock::keyboard(s))
                    .clip_rounded(0.0),
            );
        }
        dock = dock.child(body);
    }
    dock
}

/// `DockPads`: the bank switch (B over A) and one bank of 8 compact pads.
fn dock_pads(s: &App, cx: Option<&mut Cx<App>>) -> El {
    let th = t();
    let bank = if s.song.dock.bank_b { 'B' } else { 'A' };
    let mut sw = col()
        .key("dock-bank")
        .role("radiogroup")
        .label("Pad bank")
        .w(28.0)
        .gap(2.0)
        .padding(2.0)
        .radius(R_MD)
        .border(1.0, th.border)
        .fill(th.surface0);
    for b in ['B', 'A'] {
        let on = bank == b;
        sw = sw.child(
            row()
                .key(format!("dock-bank-{b}"))
                .role("radio")
                .label(format!("Bank {b} (pads {b}1–{b}8)"))
                .help(format!("Bank {b}: pads {b}1–{b}8"))
                .checked(on)
                .grow(1.0)
                .center()
                .radius(4.0)
                .fill(if on { th.surface3 } else { Color::TRANSPARENT })
                .child(text(b.to_string()).font(mono(11.0, 600.0)).color(if on {
                    th.fg
                } else {
                    th.muted_fg
                }))
                .on_press(move |s: &mut App| {
                    s.song.dock.bank_b = b == 'B';
                    let pb = if b == 'B' { PadBank::B } else { PadBank::A };
                    s.with_settings_mut(|st| {
                        let _ = write_dock_pad_bank(st, pb);
                    });
                }),
        );
    }
    let pads = s.kit().pads;
    let missing = crate::ui::beat::packs::missing_packs(s, &pads);
    let titles = crate::ui::beat::packs::missing_pack_titles(&missing, pads.len());
    let opts = GridOpts {
        layout: mdaw_model::beat_helpers::PadLayout::FourByFour,
        bank: Some(bank),
        compact: true,
        key_hints: false,
        full: false,
        prefix: "dock-pad-",
    };
    let g = match cx {
        Some(cx) => grid(s, cx, opts, &titles),
        None => el(),
    };
    row()
        .key("dock-pads")
        .w(340.0)
        .items_stretch()
        .gap(6.0)
        .child(sw)
        .child(g.grow(1.0).min_w(0.0))
}
