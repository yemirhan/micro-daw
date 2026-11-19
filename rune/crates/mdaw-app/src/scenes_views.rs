//! Headless states of the phase-9 views for the parity screenshots (`docs/parity/phase-9/`,
//! `scripts/parity-phase-9/`: the Electron side runs the same steps through the UI).

use mdaw_model::sound_design::Template;

use crate::App;
use crate::app::{AppMode, DawView, Menu};
use crate::scenes::started;
use crate::views::{Dialog, FxSec, P9Menu};

fn mode(s: &mut App, m: AppMode) {
    started(s);
    s.switch_mode(m);
}

/// The Samples page (Sound Designer) at its first open: the Kick 808 template in the editor.
pub fn samples_empty(s: &mut App) {
    mode(s, AppMode::Samples);
    let _ = s.p9_poll(0.0);
}

/// A few saved sounds in the library; a Snare, tweaked, saved and sent to a pad, is open.
pub fn designer_library(s: &mut App) {
    samples_empty(s);
    for t in [Template::Kick808, Template::ClosedHat, Template::Clap] {
        s.pick_template(t, false);
        s.save_sound();
    }
    s.pick_template(Template::Snare, false);
    s.edit_recipe(|r| {
        r.amp.decay = 0.28;
        r.filter.cutoff = 240.0;
        r.drive = 0.35;
    });
    s.send_sound_to_pad();
    s.p9.designer.notice = None;
    s.edit_recipe(|r| r.layers[0].coarse = 2.0);
    let _ = s.p9_poll(0.0);
}

/// The Send-to-pad picker open.
pub fn designer_send(s: &mut App) {
    designer_library(s);
    s.menu = Some(Menu::P9(P9Menu::SendToPad));
}

/// The Zap template: FM between two layers, a filter envelope.
pub fn designer_zap(s: &mut App) {
    samples_empty(s);
    s.pick_template(Template::Zap, false);
    let _ = s.p9_poll(0.0);
}

/// An imported audio file (a sample-based sound) with edits: the single edit panel.
pub fn designer_sample(s: &mut App) {
    samples_empty(s);
    let dir = std::env::temp_dir().join(format!("mdaw-scene-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("Vinyl Snare.wav");
    let mut r = Template::Snare.recipe();
    r.length = 0.8;
    r.trim_tail = false;
    r.amp.decay = 0.45;
    let b = mdaw_dsp::sound_design::render(&r, 48_000);
    let _ = std::fs::write(&path, mdaw_dsp::audio_io::encode_wav(&b));
    s.import_sound_files(&[path.to_string_lossy().into_owned()]);
    s.stop_sound();
    s.edit_sound(|d| {
        if let Some(e) = d.sample.as_mut() {
            e.start = 0.02;
            e.end = Some(0.6);
            e.fade_out = 0.15;
            e.normalize = true;
        }
    });
    let _ = s.p9_poll(0.0);
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
    // Typed one character at a time, as the Electron side does (cmdk re-sorts per keystroke).
    for q in ["t", "te", "tem", "temp"] {
        s.p9.palette.query = rune::TextEdit::new(q);
        let _ = crate::ui::p9::dialogs::palette_sync(s);
    }
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

/// A designed sound added to the song: an audio track with its waveform.
pub fn song_audio(s: &mut App) {
    crate::scenes_song::song(s);
    s.switch_mode(AppMode::Samples);
    let _ = s.p9_poll(0.0);
    s.pick_template(Template::OpenHat, false);
    s.add_sound_to_song();
    s.view = DawView::Song;
}
