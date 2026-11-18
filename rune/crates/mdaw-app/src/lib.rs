//! micro-daw's Rune app (phase 7 of the port: UI shell + Beat view). See
//! `docs/rune-port/phase-7-beat-ui.md`.

pub mod actions;
pub mod app;
pub mod buffers;
pub mod icons;
pub mod keys;
pub mod scenes;
pub mod services;
pub mod smoke;
pub mod theme;
pub mod ui;

#[cfg(test)]
mod tests;

use rune::{AppSpec, Tx};

pub use app::App;

fn poll(s: &mut App, t: f64, tx: &mut Tx) -> bool {
    s.smoke(t, tx);
    s.poll(t, tx)
}

fn setup(s: &mut App, waker: rune::Waker) {
    // MIDI input (and the outbox thread) wake the UI so `MidiUi::pump` runs at once.
    let w = waker.clone();
    s.svc.midi.host.set_waker(std::sync::Arc::new(move || w()));
    s.waker = Some(waker);
}

/// The window app's spec. `headless`: offline engine, in-memory settings (`--snap`,
/// `--tree`, `--script`).
pub fn spec(headless: bool) -> AppSpec<App> {
    let init: fn() -> App = if headless {
        scenes::headless_app
    } else {
        App::window
    };
    AppSpec {
        min_size: Some((1360.0, 760.0)),
        fonts: theme::FONTS,
        families: theme::FAMILIES,
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(keys::on_key),
        scenes: scenes::SCENES,
        ..AppSpec::new("micro daw", (1440.0, 900.0), init, ui::view)
    }
}
