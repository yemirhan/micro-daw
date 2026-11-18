//! `InstrumentDock.tsx` in the Beat view (`padsAvailable={false}`, `defaultCollapsed`): the
//! header ("Play", Keys toggle, MIDI route Auto / Keys / Split, chord + held notes readout,
//! collapse) and, expanded, the compact on-screen keyboard (C2–C6, 32-point white keys) playing
//! the live synth through the MIDI router (`noteOn` / `noteOff`).

use mdaw_midi::RoutingMode;
use mdaw_model::notes::midi_to_note_name;
use rune::{El, Phase, Tx, col, el, icon, row, spacer, text};

use crate::app::App;
use crate::icons;
use crate::theme::{R_LG, daw_label, mono, t, ui};
use crate::ui::widgets::{Button, Segmented, Size, Variant, vrule};

pub const PIANO_START: i64 = 36;
pub const PIANO_END: i64 = 84;
/// `POINTER_VELOCITY` of the on-screen keyboard.
pub const POINTER_VELOCITY: f64 = 0.7;

fn is_black(n: i64) -> bool {
    matches!(n.rem_euclid(12), 1 | 3 | 6 | 8 | 10)
}

/// `PianoRoll compact` + `PianoKey compact`: white keys 32 × 100 with a 1-point gap, black keys
/// 24 × 60 centred on the gap (`whiteIndex × step − 12`).
const WW: f32 = 32.0;
const WH: f32 = 100.0;
const BW: f32 = 24.0;
const BH: f32 = 60.0;
const STEP: f32 = 33.0;

/// Every key's rectangle `[x, y, w, h]` in the keys row's points, white keys first (the
/// drawing order: black keys on top).
pub fn key_rects() -> Vec<(i64, [f32; 4])> {
    let mut whites = Vec::new();
    let mut blacks = Vec::new();
    let mut wi = 0;
    for n in PIANO_START..=PIANO_END {
        if is_black(n) {
            blacks.push((n, [wi as f32 * STEP - 12.0, 0.0, BW, BH]));
        } else {
            whites.push((n, [wi as f32 * STEP, 0.0, WW, WH]));
            wi += 1;
        }
    }
    whites.extend(blacks);
    whites
}

/// The key under a keyboard point (black keys first; the 1-point gap after a white key belongs
/// to it), None off the keyboard.
pub fn key_at(x: f32, y: f32) -> Option<i64> {
    let rects = key_rects();
    let black = rects.iter().rev().find(|(n, r)| {
        is_black(*n) && x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3]
    });
    black
        .or_else(|| {
            rects.iter().find(|(n, r)| {
                !is_black(*n) && x >= r[0] && x < r[0] + STEP && y >= r[1] && y < r[1] + r[3]
            })
        })
        .map(|(n, _)| *n)
}

fn inset(y: f32, color: rune::Color) -> rune::Shadow {
    rune::Shadow {
        x: 0.0,
        y,
        blur: 0.0,
        spread: 0.0,
        color,
        inset: true,
    }
}

fn keyboard(s: &App) -> El {
    let th = t();
    let held = s.active_notes();
    let rects = key_rects();
    let whites = rects.iter().filter(|(n, _)| !is_black(*n)).count();
    let width = whites as f32 * STEP - 1.0;
    // The octave labels' strip (`h-4 mb-1`): a 1×10 rule and the C's name at its bottom left.
    let mut labels = el().h(16.0).w(width).mb(4.0);
    for (n, r) in &rects {
        if *n % 12 == 0 {
            labels = labels.child(
                row()
                    .absolute(r[0], 6.0)
                    .h(10.0)
                    .items_end()
                    .gap(4.0)
                    .child(el().w(1.0).h(10.0).fill(th.border))
                    .child(
                        text(midi_to_note_name(*n as f64))
                            .font(mono(10.0, 500.0).lh(10.0))
                            .color(th.muted_fg),
                    ),
            );
        }
    }
    let mut kb = el().key("dock-keys").h(WH).w(width);
    for (n, r) in rects {
        let on = held.contains(&n);
        let key = el()
            .key(format!("key-{n}"))
            .role("button")
            .label(midi_to_note_name(n as f64))
            .checked(on)
            .absolute(r[0], r[1])
            .size(r[2], r[3]);
        kb = kb.child(if is_black(n) {
            let k = key
                .corners([0.0, 0.0, 4.0, 4.0])
                .border(1.0, th.black.alpha(0.7))
                .fill(if on {
                    th.primary
                } else {
                    rune::oklch(0.24, 0.005, 65.0)
                });
            if on {
                k.box_shadow(inset(-2.0, th.black.alpha(0.35)))
            } else {
                k.box_shadow(inset(-5.0, th.black.alpha(0.45)))
                    .box_shadow(inset(1.0, th.white.alpha(0.10)))
                    .shadow(2.0, 3.0, th.black.alpha(0.4))
            }
        } else {
            let k = key
                .corners([0.0, 0.0, 6.0, 6.0])
                .border(1.0, th.black.alpha(0.4))
                .fill(if on {
                    rune::oklch(0.80, 0.13, 58.0)
                } else {
                    rune::oklch(0.93, 0.006, 80.0)
                });
            if on {
                k.box_shadow(inset(-2.0, th.black.alpha(0.18)))
            } else {
                k.box_shadow(inset(-5.0, th.black.alpha(0.14)))
                    .box_shadow(inset(1.0, th.white.alpha(0.6)))
            }
        });
    }
    // One gesture for the whole keyboard (`PianoRoll`'s pointer map): the note under the
    // captured pointer plays; sliding to another key releases the old note and presses the new
    // one (glissando); leaving the keys or releasing ends it.
    let kb = kb.on_drag(|s: &mut App, d: &mut rune::Drag, _tx: &mut Tx| key_drag(s, d));
    // The compact card: `rounded-lg border bg-surface-0 px-2 pt-1 pb-2` + inset shadow.
    col()
        .role("group")
        .label("Playable piano keyboard")
        .radius(R_LG)
        .clip_rounded(R_LG)
        .border(1.0, th.border)
        .fill(th.surface0)
        .px(8.0)
        .pt(4.0)
        .pb(8.0)
        .box_shadow(rune::Shadow {
            x: 0.0,
            y: 1.0,
            blur: 3.0,
            spread: 0.0,
            color: th.black.alpha(0.35),
            inset: true,
        })
        .child(labels)
        .child(kb)
}

/// `d.data` = 1 once the pointer left the keys: the gesture is over until the next press
/// (`handleKeyboardLeave` deletes the pointer, `handleKeyEnter` ignores unknown pointers).
fn key_drag(s: &mut App, d: &mut rune::Drag) {
    match d.phase {
        Phase::Down | Phase::Move => {
            if d.phase == Phase::Down {
                d.data = 0.0;
            } else if d.data > 0.0 {
                return;
            }
            let n = key_at(d.x, d.y);
            if n.is_none() && d.phase == Phase::Move {
                d.data = 1.0;
            }
            if n == s.dock_note {
                return;
            }
            s.release_dock_note();
            if let Some(n) = n {
                s.dock_note = Some(n);
                s.svc
                    .with_router(|r, out| r.note_on(n, POINTER_VELOCITY, out));
            }
        }
        Phase::Up => s.release_dock_note(),
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
