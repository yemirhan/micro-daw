//! micro-daw's Rune app (phases 7–9 of the port: UI shell, Beat view, Song view, the remaining
//! views). See `docs/rune-port/phase-7-beat-ui.md` … `phase-9-views.md`.

pub mod actions;
pub mod app;
pub mod buffers;
pub mod icons;
pub mod icons_song;
pub mod icons_views;
pub mod keys;
pub mod kit_preview;
pub mod piano;
pub mod samples;
pub mod scenes;
pub mod scenes_song;
pub mod scenes_views;
pub mod services;
pub mod signal_preview;
pub mod smoke;
pub mod song;
pub mod song_keys;
pub mod theme;
pub mod ui;
pub mod views;

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
    s.browser.waker = Some(waker.clone());
    s.waker = Some(waker);
}

/// The `rune::App` around `spec` with the close / quit hooks (`beforeunload` + `before-quit`):
/// [`App::shutdown`] flushes the autosave, stops the transport and notes and pauses installs.
pub fn app_with_hooks(spec: AppSpec<App>) -> rune::App<App> {
    rune::App::new(spec)
        .on_close(|s: &mut App, _tx: &mut Tx| s.shutdown())
        .on_quit(|s: &mut App, _tx: &mut Tx| s.shutdown())
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
