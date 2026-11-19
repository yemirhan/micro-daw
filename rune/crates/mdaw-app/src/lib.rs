//! micro-daw's Rune app (phases 7–9 of the port: UI shell, Beat view, Song view, the remaining
//! views). See `docs/rune-port/phase-7-beat-ui.md` … `phase-9-views.md`.

pub mod actions;
pub mod app;
pub mod buffers;
pub mod electron_import;
pub mod icons;
pub mod icons_song;
pub mod icons_views;
pub mod keys;
pub mod kit_preview;
pub mod piano;
pub mod project;
pub mod samples;
pub mod scenes;
pub mod scenes_shell;
pub mod scenes_song;
pub mod scenes_views;
pub mod services;
pub mod shell;
pub mod signal_preview;
pub mod smoke;
pub mod song;
pub mod song_keys;
pub mod theme;
pub mod ui;
pub mod updater;
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

/// The `rune::App` around `spec` with the close / quit hooks (`beforeunload` + `before-quit`,
/// phase 10's close guard: [`App::on_window_close`], [`App::shutdown`]). A window run keeps
/// running when its window closes (Rune cannot veto a close: the guard shows the window again),
/// and its state holds the app's handle to do so.
pub fn app_with_hooks(spec: AppSpec<App>, window: bool) -> rune::App<App> {
    let app = rune::App::new(spec)
        .on_close(|s: &mut App, tx: &mut Tx| s.on_window_close(tx))
        .on_quit(|s: &mut App, _tx: &mut Tx| s.shutdown())
        .on_reopen(|s: &mut App, _visible: bool, _tx: &mut Tx| {
            s.on_reopen();
            false
        });
    if !window {
        return app;
    }
    let handle = app.handle();
    app.keep_running()
        .init(move || App::window_with(Some(handle)))
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
        // `minWidth: 1360, minHeight: 768` (Electron's `hiddenInset` window: the content fills
        // the whole frame, as Rune's transparent title bar does).
        min_size: Some((1360.0, 768.0)),
        fonts: theme::FONTS,
        families: theme::FAMILIES,
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(keys::on_key),
        scenes: scenes::SCENES,
        // `width: 1360, height: 800`, `title: 'Untitled - Micro DAW'`.
        ..AppSpec::new("Untitled - Micro DAW", (1360.0, 800.0), init, ui::view)
    }
}
