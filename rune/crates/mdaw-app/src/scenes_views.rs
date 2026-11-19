//! Headless states of the phase-9 views for the parity screenshots (`docs/parity/phase-9/`,
//! `scripts/parity-phase-9/`: the Electron side runs the same steps through the UI).

use mdaw_engine::sample_processor::{SignalParams, SignalType};

use crate::app::{AppMode, DawView, Menu};
use crate::scenes::started;
use crate::views::{Dialog, FxSec, P9Menu, SamplesTab};
use crate::App;

fn mode(s: &mut App, m: AppMode) {
    started(s);
    s.switch_mode(m);
}

/// The Samples page, empty library (the Generate tab).
pub fn samples_empty(s: &mut App) {
    mode(s, AppMode::Samples);
}

/// A generated 440 Hz sine (1 s, 80 %) selected, the Edit tab clicked.
pub fn samples_sine(s: &mut App) {
    mode(s, AppMode::Samples);
    let _ = s.p9_poll(0.0);
    s.generate_sample();
    let _ = s.p9_poll(0.0);
    s.sync_trim_edits();
    s.p9.tab = Some(SamplesTab::Info);
}

/// The sine trimmed to 0.20 – 0.70 s.
pub fn samples_trim(s: &mut App) {
    samples_sine(s);
    s.set_trim(false, 0.2);
    s.set_trim(true, 0.7);
}

/// The Process tab with the filter and the reverb on.
pub fn samples_process(s: &mut App) {
    samples_sine(s);
    s.p9.tab = Some(SamplesTab::Process);
    s.p9.process.filter.enabled = true;
    s.p9.process.reverb.enabled = true;
}

/// The Generate tab with the square wave chosen.
pub fn samples_generate(s: &mut App) {
    mode(s, AppMode::Samples);
    s.p9.signal = SignalParams {
        kind: SignalType::Square,
        ..s.p9.signal
    };
}

/// The Packs page.
pub fn packs(s: &mut App) {
    mode(s, AppMode::Packs);
    s.svc.packs.load_catalog(false);
    let _ = s
        .svc
        .packs
        .settle(&mut s.svc.engine, std::time::Duration::from_secs(5));
}

/// The 808 built-in selected.
pub fn packs_builtin(s: &mut App) {
    packs(s);
    s.select_pack("builtin:tr808");
}

/// The first downloadable pack selected.
pub fn packs_remote(s: &mut App) {
    packs(s);
    let (_, r) = crate::views::pack_items(s);
    if let Some(i) = r.first() {
        let id = i.id().to_owned();
        s.select_pack(&id);
    }
}

/// Search "909".
pub fn packs_search(s: &mut App) {
    packs(s);
    s.p9.packs.query = rune::TextEdit::new("909");
}

pub fn settings(s: &mut App) {
    mode(s, AppMode::Settings);
}

pub fn dev(s: &mut App) {
    mode(s, AppMode::Dev);
}

/// The MIDI monitor with a few messages (note on / off, CC, pitch bend, program change).
pub fn dev_events(s: &mut App) {
    mode(s, AppMode::Dev);
    let rows: [(&[u8], f64); 6] = [
        (&[0x90, 60, 100], 1234.5),
        (&[0x80, 60, 0], 1480.25),
        (&[0xB0, 74, 64], 1600.0),
        (&[0xE0, 0x00, 0x50], 1700.75),
        (&[0xC0, 5], 1810.0),
        (&[0x91, 36, 0], 1900.5),
    ];
    let batch = rows
        .iter()
        .enumerate()
        .map(|(i, (b, t))| mdaw_midi::monitor::format_event(i as u64, b, *t))
        .collect();
    s.p9.monitor.flush(batch);
}

/// ⌘K.
pub fn palette(s: &mut App) {
    started(s);
    s.open_dialog(Dialog::Palette);
}

/// ⌘K, "temp".
pub fn palette_search(s: &mut App) {
    palette(s);
    s.p9.palette.query = rune::TextEdit::new("temp");
}

/// The export dialog over the Song view (rock-song).
pub fn export(s: &mut App) {
    crate::scenes_song::song(s);
    s.open_dialog(Dialog::Export);
}

/// The template picker (Beat view).
pub fn templates(s: &mut App) {
    started(s);
    s.open_dialog(Dialog::Templates);
}

/// The controller setup dialog.
pub fn controller(s: &mut App) {
    started(s);
    s.open_dialog(Dialog::Controller);
}

/// The synth editor open over the Beat view.
pub fn synth_editor(s: &mut App) {
    started(s);
    s.p9.synth_editor = true;
}

/// The top bar's instrument selector.
pub fn instrument_menu(s: &mut App) {
    started(s);
    s.menu = Some(Menu::P9(P9Menu::Instrument));
}

/// The Song view with the Live FX strip's LFO section open.
pub fn live_fx(s: &mut App) {
    crate::scenes_song::song(s);
    s.p9.fx_section = Some(FxSec::Lfo);
}

/// A generated sine sent to the arrangement: an audio track with its waveform.
pub fn song_audio(s: &mut App) {
    crate::scenes_song::song(s);
    s.switch_mode(AppMode::Samples);
    let _ = s.p9_poll(0.0);
    s.generate_sample();
    let _ = s.p9_poll(0.0);
    s.send_to_arrangement();
    s.view = DawView::Song;
}
