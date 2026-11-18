//! `components/StartScreen.tsx`: the launch screen. Click anywhere (or any key but Tab,
//! Escape and modifiers, no repeat, no modifier held) starts the session.
//!
//! The intro animation (trace drawn by a playhead, collapsing into the mark) is not ported:
//! the screen shows its final state, which is also what the app shows with reduced motion
//! (listed in the phase-7 doc). The exit fade (380 ms) is kept as a timed phase.

use rune::{El, LineCap, Path, Shape, canvas, col, el, row, spacer, text};

use crate::app::{App, StartPhase};
use crate::theme::{R_MD, R_SM, daw_label, display, mono, t, ui};

/// The app mark's glyph (half sine, half square) in a 48-point box.
pub fn mark_glyph(size: f32, width: f32, color: rune::Color) -> El {
    let path = Path::new()
        .move_to([10.0, 25.0])
        .cubic_to([13.0, 12.0], [20.0, 12.0], [24.0, 25.0])
        .line_to([24.0, 33.0])
        .line_to([37.0, 33.0])
        .line_to([37.0, 25.0]);
    let scale = size / 48.0;
    canvas(
        Shape::new()
            .stroke_path(&path, width * scale, LineCap::Round, color)
            .view_box(48.0, 48.0),
    )
    .size(size, size)
}

pub fn view(s: &App) -> El {
    let th = t();
    let busy = s.start != StartPhase::Idle;
    let status = if !busy {
        if s.start_error.is_some() {
            "Error"
        } else {
            "Standby"
        }
    } else {
        "Starting"
    };
    let mark = row()
        .size(48.0, 48.0)
        .center()
        .radius(11.0)
        .fill(th.primary)
        .child(mark_glyph(48.0, 3.25, th.primary_fg));
    let word = row()
        .items_end()
        .gap(12.0)
        .child(
            text("micro")
                .font(display(40.0, 800.0).tracking(-1.2).lh(40.0))
                .color(th.fg),
        )
        .child(
            text("daw")
                .font(display(40.0, 700.0).tracking(-1.2).lh(40.0))
                .color(th.muted_fg),
        );
    let lockup = row()
        .key("start-lockup")
        .w(344.0)
        .h(48.0)
        .items_center()
        .gap(16.0)
        .child(mark)
        .child(word);
    let stage = col().size(560.0, 132.0).center().child(lockup);
    let tagline = text("Sample\u{a0}\u{a0}·\u{a0}\u{a0}Play\u{a0}\u{a0}·\u{a0}\u{a0}Record")
        .font(daw_label(10.5).tracking(10.5 * 0.28))
        .text_case(rune::TextCase::Uppercase)
        .color(th.muted_fg);
    let kbd = row()
        .h(24.0)
        .min_w(24.0)
        .px(6.0)
        .center()
        .radius(R_SM)
        .border(
            1.0,
            if busy {
                th.border
            } else {
                th.primary.alpha(0.5)
            },
        )
        .fill(if busy {
            rune::Color::TRANSPARENT
        } else {
            th.primary
        })
        .child(text("↵").font(mono(11.0, 500.0)).color(if busy {
            th.muted_fg
        } else {
            th.primary_fg
        }));
    let button = row()
        .key("start-button")
        .role("button")
        .label("Start session")
        .h(40.0)
        .items_center()
        .gap(12.0)
        .pl(16.0)
        .pr(8.0)
        .radius(R_MD)
        .border(
            1.0,
            if busy {
                th.border
            } else {
                th.primary.alpha(0.6)
            },
        )
        .fill(if busy {
            th.surface2
        } else {
            th.primary.alpha(0.10)
        })
        .child(
            text(if busy {
                "Starting audio engine…"
            } else {
                "Start session"
            })
            .font(ui(14.0, 600.0))
            .color(if busy { th.muted_fg } else { th.fg }),
        )
        .child(kbd);
    let button = if busy {
        button
    } else {
        button
            .hover(th.primary.alpha(0.20))
            .on_press(|s: &mut App| s.start_session())
    };
    let hint = text(s.start_error.clone().unwrap_or_else(|| {
        "Click anywhere or press any key. This turns on audio and MIDI.".into()
    }))
    .font(ui(12.0, 400.0))
    .color(if s.start_error.is_some() {
        th.rec
    } else {
        th.muted_fg
    });
    let cta = col()
        .mt(40.0)
        .items_center()
        .gap(12.0)
        .child(button)
        .child(hint);
    let content = col()
        .grow(1.0)
        .w_full()
        .center()
        .pb(40.0)
        .child(stage)
        .child(tagline.mt(24.0))
        .child(cta);
    let footer_text = |s: &str| {
        text(s.to_owned())
            .font(mono(10.5, 400.0).tracking(10.5 * 0.14))
            .text_case(rune::TextCase::Uppercase)
            .color(th.muted_fg)
    };
    let led = el()
        .size(6.0, 6.0)
        .radius(3.0)
        .fill(if s.start_error.is_some() {
            th.rec
        } else if busy {
            th.primary
        } else {
            th.play
        });
    let footer = row()
        .h(36.0)
        .w_full()
        .items_center()
        .px(20.0)
        .child(footer_text(&format!("v{}", env!("CARGO_PKG_VERSION"))))
        .child(spacer())
        .child(
            row()
                .items_center()
                .gap(8.0)
                .child(led)
                .child(footer_text(&format!("Audio engine · {status}"))),
        );
    let root = col()
        .key("start-screen")
        .w_full()
        .h_full()
        .fill(th.intro_bg)
        .child(el().h(40.0).w_full().window_drag())
        .child(content)
        .child(footer);
    let root = if let StartPhase::Exiting { .. } = s.start {
        root.opacity(0.0)
    } else {
        root
    };
    if busy {
        root
    } else {
        root.on_background_press(|s: &mut App| s.start_session())
    }
}
