//! `InstrumentDock.tsx` in the Beat view (`padsAvailable={false}`, `defaultCollapsed`): the
//! header ("Play", Keys toggle, MIDI route Auto / Keys / Split, chord + held notes readout,
//! collapse) and, expanded, the compact on-screen keyboard (C2–C6, 32-point white keys) playing
//! the live synth through the MIDI router (`noteOn` / `noteOff`).

use mdaw_midi::RoutingMode;
use mdaw_model::notes::midi_to_note_name;
use rune::{El, Phase, Tx, col, el, icon, row, spacer, text};

use crate::app::App;
use crate::icons;
use crate::theme::{daw_label, mono, t, ui};
use crate::ui::widgets::{Button, Segmented, Size, Variant, vrule};

pub const PIANO_START: i64 = 36;
pub const PIANO_END: i64 = 84;
/// `POINTER_VELOCITY` of the on-screen keyboard.
pub const POINTER_VELOCITY: f64 = 0.7;

fn is_black(n: i64) -> bool {
    matches!(n.rem_euclid(12), 1 | 3 | 6 | 8 | 10)
}

fn keyboard(s: &App) -> El {
    let th = t();
    let held = s.active_notes();
    let (ww, wh, bw, bh, step) = (32.0f32, 112.0f32, 20.0f32, 76.0f32, 33.0f32);
    let mut kb = el().key("dock-keys").h(wh);
    let mut wi = 0;
    let mut blacks = Vec::new();
    for n in PIANO_START..=PIANO_END {
        if is_black(n) {
            blacks.push((n, wi));
            continue;
        }
        let on = held.contains(&n);
        if n % 12 == 0 {
            kb = kb.child(
                text(midi_to_note_name(n as f64))
                    .font(mono(9.0, 400.0))
                    .color(th.muted_fg)
                    .absolute(wi as f32 * step + 2.0, -14.0),
            );
        }
        kb = kb.child(
            el().key(format!("key-{n}"))
                .role("button")
                .label(midi_to_note_name(n as f64))
                .absolute(wi as f32 * step, 0.0)
                .size(ww, wh)
                .corners([0.0, 0.0, 4.0, 4.0])
                .fill(if on { th.primary } else { th.fg.alpha(0.92) })
                .on_drag(move |s: &mut App, d: &mut rune::Drag, _tx: &mut Tx| key_drag(s, n, d)),
        );
        wi += 1;
    }
    for (n, wi) in blacks {
        let on = held.contains(&n);
        kb = kb.child(
            el().key(format!("key-{n}"))
                .role("button")
                .label(midi_to_note_name(n as f64))
                .absolute(wi as f32 * step - 12.0 + (step - bw) / 2.0 - 4.0, 0.0)
                .size(bw, bh)
                .corners([0.0, 0.0, 3.0, 3.0])
                .fill(if on { th.primary } else { th.surface0 })
                .on_drag(move |s: &mut App, d: &mut rune::Drag, _tx: &mut Tx| key_drag(s, n, d)),
        );
    }
    kb.w(wi as f32 * step)
}

fn key_drag(s: &mut App, n: i64, d: &rune::Drag) {
    match d.phase {
        Phase::Down => s
            .svc
            .with_router(|r, out| r.note_on(n, POINTER_VELOCITY, out)),
        Phase::Move => {}
        Phase::Up => s.svc.with_router(|r, out| r.note_off(n, out)),
    }
}

pub fn view(s: &App) -> El {
    let th = t();
    let mode = s.svc.midi.host.shared().lock().router.routing_mode();
    let notes = s.active_notes();
    let chord =
        mdaw_model::chords::detect_chord(&notes.iter().map(|n| *n as i32).collect::<Vec<_>>());
    let collapsed = !s.dock_open;
    let keys_toggle = row()
        .key("dock-keys-toggle")
        .role("button")
        .label("Keys")
        .checked(s.dock_open)
        .h(24.0)
        .items_center()
        .gap(6.0)
        .px(8.0)
        .radius(4.0)
        .fill(if s.dock_open {
            th.surface3
        } else {
            rune::Color::TRANSPARENT
        })
        .hover(th.surface3)
        .child(icon(&icons::PIANO, 14.0).color(if s.dock_open { th.fg } else { th.muted_fg }))
        .child(text("Keys").font(ui(11.0, 600.0)).color(if s.dock_open {
            th.fg
        } else {
            th.muted_fg
        }))
        .on_press(|s: &mut App| s.dock_open = !s.dock_open);
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
    .key("routing")
    .build(|s: &mut App, m| s.svc.with_router(|r, _| r.set_routing_mode(m)));
    let chord_el = row()
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
    let chord_el = if chord.is_some() {
        chord_el.fill(th.primary.alpha(0.15))
    } else {
        chord_el
    };
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
        .key("dock-header")
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
        .child(keys_toggle)
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
                .key("dock-collapse")
                .build(|s: &mut App, _| s.dock_open = !s.dock_open),
        );
    let mut dock = col()
        .key("dock")
        .w_full()
        .fill(th.surface1)
        .pt(1.0)
        .border_sides([1.0, 0.0, 0.0, 0.0], th.border)
        .child(header);
    if !collapsed {
        dock = dock.child(
            row()
                .h(156.0)
                .w_full()
                .px(12.0)
                .py(10.0)
                .items_end()
                .border_sides([1.0, 0.0, 0.0, 0.0], th.hairline)
                .child(keyboard(s)),
        );
    }
    dock
}
