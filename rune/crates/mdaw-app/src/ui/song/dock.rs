//! `InstrumentDock` in the Song view (pads available, expanded by default): header ("Play",
//! Pads / Keys toggles, MIDI route, chord + held notes, collapse), then the dock pads (one bank
//! of eight with the A / B switch, `micro-daw:dock-pad-bank`) and the compact on-screen keyboard
//! (`PianoRoll.tsx` / `PianoKey.tsx`, §7 of the views inventory).

use mdaw_midi::RoutingMode;
use mdaw_model::notes::midi_to_note_name;
use mdaw_model::settings::{PadBank, write_dock_pad_bank};
use rune::{Color, Cx, El, Phase, Shadow, Tx, col, el, icon, row, spacer, text};

use crate::app::App;
use crate::icons;
use crate::theme::{R_LG, R_MD, daw_label, mono, t, ui};
use crate::ui::beat::pads::{GridOpts, grid};
use crate::ui::widgets::{Button, Segmented, Size, Variant, vrule};

/// `PIANO_START` / `PIANO_END` (C2–C6).
pub const PIANO_START: i64 = 36;
pub const PIANO_END: i64 = 84;
/// `POINTER_VELOCITY`.
pub const POINTER_VELOCITY: f64 = 0.7;

fn is_black(n: i64) -> bool {
    matches!(n.rem_euclid(12), 1 | 3 | 6 | 8 | 10)
}

/// Key geometry: (white width, black offset) — compact 32 / 12, full 40 / 15; 1-point gap.
pub fn key_geometry(compact: bool) -> (f32, f32) {
    if compact { (32.0, 12.0) } else { (40.0, 15.0) }
}

/// The on-screen keyboard (`PianoRoll`): the pointer holds at most one key (velocity 0.7),
/// sliding retriggers (glissando), release on up / leave / blur.
pub fn keyboard(s: &App, compact: bool) -> El {
    let th = t();
    let held = s.active_notes();
    let (ww, boff) = key_geometry(compact);
    let gap = 1.0;
    let step = ww + gap;
    let (wh, bw, bh) = if compact {
        (100.0, 24.0, 60.0)
    } else {
        (160.0, 30.0, 100.0)
    };
    let mut whites = Vec::new();
    let mut blacks = Vec::new();
    for n in PIANO_START..=PIANO_END {
        if is_black(n) {
            blacks.push((n, whites.len()));
        } else {
            whites.push(n);
        }
    }
    let total_w = whites.len() as f32 * step - gap;
    // C labels above the keys.
    let mut labels = el().h(16.0).w(total_w).mb(4.0);
    for (i, n) in whites.iter().enumerate() {
        if n % 12 == 0 {
            labels = labels.child(
                row()
                    .absolute(i as f32 * step, 0.0)
                    .h(16.0)
                    .items_end()
                    .gap(4.0)
                    .child(el().w(1.0).h(10.0).fill(th.border))
                    .child(
                        text(midi_to_note_name(*n as f64))
                            .font(mono(10.0, 500.0))
                            .color(th.muted_fg),
                    ),
            );
        }
    }
    let ivory = rune::oklch(0.93, 0.006, 80.0);
    let ivory_held = rune::oklch(0.80, 0.13, 58.0);
    let graphite = rune::oklch(0.24, 0.005, 65.0);
    let mut kb = el()
        .key("song-keys")
        .role("group")
        .label("Playable piano keyboard")
        .size(total_w, wh)
        .on_pointer_move_local(|s: &mut App, p: rune::PointerEvent, _tx: &mut Tx| {
            // Glissando: a held pointer sliding onto another key retriggers.
            if s.song.key_pointer.is_some()
                && let Some(n) = key_at(p.x, p.y, true)
            {
                s.press_key_pointer(n);
            }
        })
        .on_hover_change(|s: &mut App, inside: bool, _tx: &mut Tx| {
            if !inside {
                s.release_key_pointer();
            }
        });
    for (i, n) in whites.iter().enumerate() {
        let on = held.contains(n);
        let mut k = col()
            .key(format!("song-key-{n}"))
            .role("button")
            .label(midi_to_note_name(*n as f64))
            .checked(on)
            .absolute(i as f32 * step, if on { 1.0 } else { 0.0 })
            .size(ww, wh)
            .corners([0.0, 0.0, 6.0, 6.0])
            .fill(if on { ivory_held } else { ivory })
            .border(1.0, Color::BLACK.alpha(0.4))
            .box_shadow(Shadow {
                x: 0.0,
                y: if on { -2.0 } else { -5.0 },
                blur: 0.0,
                spread: 0.0,
                color: Color::BLACK.alpha(if on { 0.18 } else { 0.14 }),
                inset: true,
            })
            .justify_end()
            .items_center();
        if !compact {
            k = k.child(
                text(midi_to_note_name(*n as f64))
                    .font(mono(10.0, 600.0))
                    .color(Color::BLACK.alpha(if on { 0.7 } else { 0.45 }))
                    .mb(10.0),
            );
        }
        let n = *n;
        kb = kb.child(k.on_drag(move |s: &mut App, d: &mut rune::Drag, _tx: &mut Tx| key_drag(s, n, d)));
    }
    for (n, wi) in blacks {
        let on = held.contains(&n);
        kb = kb.child(
            el().key(format!("song-key-{n}"))
                .role("button")
                .label(midi_to_note_name(n as f64))
                .checked(on)
                .absolute(wi as f32 * step - boff, if on { 1.0 } else { 0.0 })
                .size(bw, bh)
                .corners([0.0, 0.0, 4.0, 4.0])
                .fill(if on { th.primary } else { graphite })
                .border(1.0, Color::BLACK.alpha(0.7))
                .box_shadow(Shadow {
                    x: 0.0,
                    y: if on { -2.0 } else { -5.0 },
                    blur: 0.0,
                    spread: 0.0,
                    color: Color::BLACK.alpha(if on { 0.35 } else { 0.45 }),
                    inset: true,
                })
                .on_drag(move |s: &mut App, d: &mut rune::Drag, _tx: &mut Tx| key_drag(s, n, d)),
        );
    }
    let mut wrap = col()
        .key("song-keyboard")
        .radius(if compact { R_LG } else { 12.0 })
        .border(1.0, th.border)
        .fill(if compact { th.surface0 } else { th.surface1 })
        .clip_rounded(if compact { R_LG } else { 12.0 })
        .px(if compact { 8.0 } else { 16.0 })
        .pt(if compact { 4.0 } else { 12.0 })
        .pb(if compact { 8.0 } else { 16.0 });
    if !compact {
        wrap = wrap.child(
            row()
                .mb(12.0)
                .items_center()
                .justify_between()
                .child(
                    text("Keyboard")
                        .font(daw_label(11.0))
                        .text_case(rune::TextCase::Uppercase)
                        .color(th.muted_fg),
                )
                .child(
                    row()
                        .gap(6.0)
                        .px(8.0)
                        .py(2.0)
                        .radius(2.0)
                        .fill(th.surface3)
                        .items_center()
                        .child(el().size(6.0, 6.0).radius(3.0).fill(if held.is_empty() {
                            th.surface4
                        } else {
                            th.primary
                        }))
                        .child(
                            text(format!("held {}", held.len()))
                                .font(mono(11.0, 400.0))
                                .color(th.muted_fg),
                        ),
                ),
        );
    }
    wrap.child(labels).child(kb)
}

/// The key under keyboard point (x, y): black keys first (they sit on top).
pub fn key_at(x: f32, y: f32, compact: bool) -> Option<i64> {
    let (ww, boff) = key_geometry(compact);
    let step = ww + 1.0;
    let (bw, bh) = if compact { (24.0, 60.0) } else { (30.0, 100.0) };
    let mut wi = 0usize;
    let mut whites = Vec::new();
    for n in PIANO_START..=PIANO_END {
        if is_black(n) {
            let bx = wi as f32 * step - boff;
            if y >= 0.0 && y < bh && x >= bx && x < bx + bw {
                return Some(n);
            }
        } else {
            whites.push(n);
            wi += 1;
        }
    }
    if x < 0.0 {
        return None;
    }
    whites.get((x / step) as usize).copied()
}

fn key_drag(s: &mut App, n: i64, d: &rune::Drag) {
    match d.phase {
        Phase::Down => s.press_key_pointer(n),
        Phase::Move => {}
        Phase::Up => s.release_key_pointer(),
    }
}

impl App {
    /// `pressPointer`: the pointer moves to `n` (the previous key released first).
    pub fn press_key_pointer(&mut self, n: i64) {
        if self.song.key_pointer == Some(n) {
            return;
        }
        self.release_key_pointer();
        self.song.key_pointer = Some(n);
        self.svc
            .with_router(|r, out| r.note_on(n, POINTER_VELOCITY, out));
    }

    /// `releasePointer`: the held key is released exactly once.
    pub fn release_key_pointer(&mut self) {
        if let Some(n) = self.song.key_pointer.take() {
            self.svc.with_router(|r, out| r.note_off(n, out));
        }
    }
}

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
                .color(if chord.is_some() { th.primary } else { th.muted_fg }),
        );
    if chord.is_some() {
        chord_el = chord_el.fill(th.primary.alpha(0.15));
    }
    let mut held = row().min_w(140.0).items_center().gap(6.0);
    if notes.is_empty() {
        held = held.child(text("no notes held").font(mono(11.0, 400.0)).color(th.muted_fg));
    } else {
        for n in notes.iter().take(8) {
            held = held.child(text(midi_to_note_name(*n as f64)).font(mono(11.0, 400.0)).color(th.fg));
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
                    .child(keyboard(s, true))
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
