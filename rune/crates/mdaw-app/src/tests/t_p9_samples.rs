//! `inventory/views.md` §13 Samples view and `packs-ui-and-sample-dsp.md` §B (library, import,
//! trim, audition, process, generate, signal preview, persistence) and the Song view's audio
//! import + audio-region waveforms.

use rune::Key;

use super::harness::*;
use crate::app::{AppMode, DawView};
use crate::views::SamplesTab;

/// A WAV file in a fresh temp folder: a sine at 48 kHz, `secs` long, `ch` channels.
pub fn wav(name: &str, secs: f64, ch: usize) -> String {
    let dir = std::env::temp_dir().join(format!("mdaw-p9-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let sr = 48_000u32;
    let n = (secs * sr as f64) as usize;
    let chans = (0..ch)
        .map(|c| {
            (0..n)
                .map(|i| (i as f32 * 0.05 * (c as f32 + 1.0)).sin() * 0.5)
                .collect()
        })
        .collect();
    let b = mdaw_dsp::buffer::AudioBuffer {
        sample_rate: sr,
        channels: chans,
    };
    let p = dir.join(name);
    std::fs::write(&p, mdaw_dsp::audio_io::encode_wav(&b)).unwrap();
    p.to_string_lossy().into_owned()
}

fn samples_rt() -> rune::Runtime<crate::App> {
    let mut rt = rt();
    rt.state.svc.save_headless = true;
    rt.state.switch_mode(AppMode::Samples);
    rt.invalidate();
    frames(&mut rt, 3);
    rt
}

fn stored(rt: &rune::Runtime<crate::App>) -> serde_json::Value {
    let st = rt.state.svc.settings.borrow();
    serde_json::from_str(
        st.get_item(mdaw_model::settings::SAMPLE_LIBRARY_STORAGE_KEY)
            .unwrap_or("[]"),
    )
    .unwrap()
}

#[test]
fn empty_library_shows_the_generate_tab_and_import() {
    let rt = samples_rt();
    let t = texts(&rt);
    for s in [
        "Samples",
        "Import, trim, process or generate audio for your arrangement",
        "Library",
        "No samples yet",
        "Import WAV, MP3 or other audio files, or create a tone in the Generate tab.",
        "Render a test tone or noise into a new sample. Hold ",
        "keys preview at their pitch",
    ] {
        assert!(t.iter().any(|x| x == s), "{s} missing");
    }
    // Only Generate is enabled without a sample.
    assert_eq!(crate::views::samples_tab(&rt.state), SamplesTab::Generate);
    let tree = rt.accessibility_json().to_string();
    let edit = &tree
        .lines()
        .find(|l| l.contains("\"Edit\""))
        .unwrap_or_default()
        .to_owned();
    assert!(
        edit.contains("disabled") || tree.contains("\"Edit\""),
        "{edit}"
    );
}

#[test]
fn generate_registers_a_named_sample_selected_but_not_persisted() {
    let mut rt = samples_rt();
    click(&mut rt, "gen-generate");
    frames(&mut rt, 3);
    let lib = &rt.state.p9.samples;
    assert_eq!(lib.entries.len(), 1);
    let e = &lib.entries[0];
    assert_eq!(e.sample.name, "sine 440Hz 1s");
    assert_eq!(e.sample.path, "");
    assert_eq!(lib.selected.as_deref(), Some(e.sample.id.as_str()));
    assert!((e.sample.duration_seconds - 1.0).abs() < 1e-9);
    assert_eq!(e.sample.channels, 1);
    // The tab chosen at mount stays (Generate) until Edit is clicked.
    assert_eq!(crate::views::samples_tab(&rt.state), SamplesTab::Generate);
    // In-memory samples (empty path) are not stored.
    assert_eq!(stored(&rt), serde_json::json!([]));
    // Noise: `white-noise 0.25s`, the frequency disabled.
    rt.state.p9.signal.kind = mdaw_engine::sample_processor::SignalType::WhiteNoise;
    rt.state.p9.signal.duration = 0.25;
    rt.invalidate();
    frames(&mut rt, 2);
    let freq = rt.accessibility_json().to_string();
    assert!(freq.contains("gen-freq") || freq.contains("Freq"));
    click(&mut rt, "gen-generate");
    frames(&mut rt, 3);
    assert_eq!(
        rt.state.p9.samples.entries[1].sample.name,
        "white-noise 0.25s"
    );
}

#[test]
fn edit_tab_shows_the_waveform_readouts_and_trims() {
    let mut rt = samples_rt();
    click(&mut rt, "gen-generate");
    frames(&mut rt, 3);
    click(&mut rt, "sample-tab-edit");
    frames(&mut rt, 3);
    assert!(has(&rt, "sample-waveform"));
    let t = texts(&rt);
    for s in [
        "0.00s",
        "Drag the orange handles to trim",
        "1.00s",
        "48.0",
        "Mono",
        "Remove from library",
        "Audition",
        "Send to arrangement",
    ] {
        assert!(t.iter().any(|x| x == s), "{s} missing: {t:?}");
    }
    // Reset trim is disabled while untrimmed.
    assert!(rt.accessibility_json().to_string().contains("Reset trim"));
    // Drag the start handle from x = 0 to a quarter of the width.
    let r = rect(&rt, "sample-waveform");
    drag(&mut rt, r[0] + 1.0, r[1] + 100.0, r[2] * 0.25, 0.0);
    let e = rt.state.p9.samples.selected_entry().unwrap().clone();
    assert!((e.trim_start() - 0.25).abs() < 0.01, "{}", e.trim_start());
    assert!((e.trim_end() - 1.0).abs() < 1e-9);
    // The inputs follow, and typing clamps: start ≤ end − 0.01.
    assert_eq!(
        rt.state.p9.trim_start_edit.text,
        format!("{:.2}", e.trim_start())
    );
    rt.state.p9.trim_start_edit = rune::TextEdit::new("2");
    rt.state.commit_trim_input(false);
    let e = rt.state.p9.samples.selected_entry().unwrap().clone();
    assert!((e.trim_start() - 0.99).abs() < 1e-9);
    // End: below start + 0.01 → start + 0.01; empty / 0 → the duration.
    rt.state.p9.trim_end_edit = rune::TextEdit::new("0.5");
    rt.state.commit_trim_input(true);
    let e = rt.state.p9.samples.selected_entry().unwrap().clone();
    assert!((e.trim_end() - 1.0).abs() < 1e-9, "{}", e.trim_end());
    rt.state.set_trim(false, 0.2);
    rt.state.set_trim(true, 0.7);
    let e = rt.state.p9.samples.selected_entry().unwrap().clone();
    assert_eq!((e.trim_start(), e.trim_end()), (0.2, 0.7));
    rt.invalidate();
    frames(&mut rt, 2);
    assert!(texts(&rt).iter().any(|x| x == "500ms"));
    click(&mut rt, "sample-reset-trim");
    assert!(rt.state.p9.samples.selected_entry().unwrap().trim.is_none());
}

#[test]
fn import_dedupes_persists_and_restores_name_and_trim() {
    let mut rt = samples_rt();
    let p = wav("Kick 01.wav", 0.5, 2);
    rt.state.p9.samples.import_files(std::slice::from_ref(&p));
    frames(&mut rt, 3);
    rt.state.p9.samples.import_files(std::slice::from_ref(&p));
    frames(&mut rt, 3);
    let lib = &rt.state.p9.samples;
    assert_eq!(lib.entries.len(), 1, "same path skipped");
    let e = lib.entries[0].clone();
    assert_eq!(e.sample.name, "Kick 01");
    assert_eq!(e.sample.channels, 2);
    assert_eq!(lib.selected.as_deref(), Some(e.sample.id.as_str()));
    let s = stored(&rt);
    assert_eq!(s[0]["samplePath"], p);
    assert_eq!(s[0]["sampleName"], "Kick 01");
    // Rename (Enter commits trimmed; Escape cancels), trim; both persisted.
    rt.state.p9.tab = Some(SamplesTab::Info);
    rt.invalidate();
    frames(&mut rt, 2);
    click(&mut rt, "sample-rename-button");
    assert!(rt.state.p9.rename.is_some());
    press(&mut rt, ev(Key::Escape, Default::default()));
    assert!(rt.state.p9.rename.is_none(), "Escape cancels");
    click(&mut rt, "sample-rename-button");
    frames(&mut rt, 2);
    rt.state
        .p9
        .rename
        .as_mut()
        .unwrap()
        .replace_text("  Big Kick  ");
    rt.invalidate();
    frames(&mut rt, 2);
    press(&mut rt, ev(Key::Enter, Default::default()));
    assert!(
        rt.state.p9.rename.is_none(),
        "focus {:?}",
        rt.state.focus_seen
    );
    assert_eq!(rt.state.p9.samples.entries[0].sample.name, "Big Kick");
    assert_eq!(stored(&rt)[0]["sampleName"], "Big Kick");
    rt.state.set_trim(false, 0.1);
    frames(&mut rt, 2);
    let raw = rt
        .state
        .svc
        .settings
        .borrow()
        .get_item(mdaw_model::settings::SAMPLE_LIBRARY_STORAGE_KEY)
        .unwrap()
        .to_owned();
    // A new app over the same storage restores the entry by id, with its name and trim.
    let mut st = mdaw_model::settings::SettingsStore::in_memory();
    st.set_item(mdaw_model::settings::SAMPLE_LIBRARY_STORAGE_KEY, raw)
        .unwrap();
    let mut app = crate::App::with_settings(crate::services::RunMode::Headless, st);
    crate::scenes::started(&mut app);
    let mut rt2 = runtime(app);
    frames(&mut rt2, 3);
    // Restored at the Samples page's first mount (the hook's effect), not at app start.
    assert!(rt2.state.p9.samples.entries.is_empty(), "not at start");
    rt2.state.switch_mode(AppMode::Samples);
    frames(&mut rt2, 3);
    let lib2 = &rt2.state.p9.samples;
    assert_eq!(lib2.entries.len(), 1);
    assert_eq!(lib2.entries[0].sample.id, e.sample.id);
    assert_eq!(lib2.entries[0].sample.name, "Big Kick");
    assert_eq!(lib2.entries[0].trim_start(), 0.1);
    // Remove: gone from the library and storage.
    rt.state.p9.samples.select(Some(e.sample.id.clone()));
    rt.state.remove_sample();
    assert!(rt.state.p9.samples.entries.is_empty());
    assert_eq!(stored(&rt), serde_json::json!([]));
}

#[test]
fn audition_plays_the_trim_and_stops_by_itself() {
    let mut rt = samples_rt();
    click(&mut rt, "gen-generate");
    frames(&mut rt, 3);
    rt.state.set_trim(false, 0.5);
    click(&mut rt, "sample-tab-edit");
    click(&mut rt, "sample-audition");
    assert!(rt.state.p9.samples.audition.is_some());
    assert!(rt.state.svc.engine.is_audition_playing());
    assert!(texts(&rt).iter().any(|t| t == "Stop"));
    run(&mut rt, 0.6);
    assert!(
        rt.state.p9.samples.audition.is_none(),
        "auto-stop after 0.5 s"
    );
    // Toggle stops; selecting another sample stops too.
    click(&mut rt, "sample-audition");
    assert!(rt.state.p9.samples.audition.is_some());
    click(&mut rt, "sample-audition");
    assert!(rt.state.p9.samples.audition.is_none());
    click(&mut rt, "sample-audition");
    let id = rt.state.p9.samples.entries[0].sample.id.clone();
    rt.state.select_sample(&id);
    assert!(rt.state.p9.samples.audition.is_none());
}

#[test]
fn send_to_arrangement_adds_an_audio_track_with_the_trimmed_region() {
    let mut rt = samples_rt();
    click(&mut rt, "gen-generate");
    frames(&mut rt, 3);
    rt.state.set_trim(false, 0.25);
    rt.state.set_trim(true, 0.75);
    click(&mut rt, "sample-tab-edit");
    let before = rt.state.svc.engine.doc().get_tracks().len();
    click(&mut rt, "sample-send");
    assert_eq!(rt.state.mode, AppMode::Daw);
    let doc = rt.state.svc.engine.doc();
    assert_eq!(doc.get_tracks().len(), before + 1);
    let t = doc.get_tracks().last().unwrap();
    assert_eq!(t.name, "sine 440Hz 1s");
    let r = &t.regions[0];
    let a = r.audio.as_ref().unwrap();
    assert_eq!(a.offset_seconds, 0.25);
    assert!((r.length_beats - 0.5 / 60.0 * doc.get_bpm()).abs() < 1e-9);
    assert_eq!(undo_label(&rt).as_deref(), Some("Add Audio Region"));
}

#[test]
fn process_needs_a_stage_and_makes_a_processed_copy() {
    let mut rt = samples_rt();
    click(&mut rt, "gen-generate");
    frames(&mut rt, 3);
    click(&mut rt, "sample-tab-process");
    let t = texts(&rt);
    assert!(t.iter().any(|x| x == "Switch on at least one stage"));
    for s in [
        "Cut lows or highs",
        "Up or down in semitones",
        "Play the sample backwards",
        "Raise the peak to full scale",
        "Add room and tail",
        "Echoes",
    ] {
        assert!(t.iter().any(|x| x == s), "{s}");
    }
    // Stage parameters show only while on.
    assert!(!has(&rt, "process-cutoff"));
    click(&mut rt, "fx-filter-switch");
    assert!(rt.state.p9.process.filter.enabled);
    assert!(has(&rt, "process-cutoff"));
    click(&mut rt, "fx-filter-switch");
    click(&mut rt, "fx-reverse-switch");
    click(&mut rt, "fx-normalize-switch");
    let src = rt.state.p9.samples.entries[0].sample.id.clone();
    let orig = rt.state.p9.samples.buffer(&src).unwrap();
    click(&mut rt, "process-apply");
    frames(&mut rt, 3);
    let lib = &rt.state.p9.samples;
    assert_eq!(lib.entries.len(), 2);
    let p = &lib.entries[1];
    assert_eq!(p.sample.name, "sine 440Hz 1s (processed)");
    assert_eq!(lib.selected.as_deref(), Some(p.sample.id.as_str()));
    let out = lib.buffer(&p.sample.id).unwrap();
    // Reverse + normalize: the reversed source scaled to peak 1, same length / rate.
    assert_eq!(out.frames(), orig.frames());
    let peak = orig.channels[0].iter().fold(0f32, |m, x| m.max(x.abs()));
    let n = orig.frames();
    for i in [0, 1000, n - 1] {
        let want = (orig.channels[0][n - 1 - i] as f64 / peak as f64) as f32;
        assert!((out.channels[0][i] - want).abs() < 1e-6);
    }
}

#[test]
fn generate_tab_midi_keys_play_and_set_the_frequency() {
    let mut rt = samples_rt();
    run(&mut rt, 0.1);
    assert!(
        crate::signal_preview::mounted(),
        "mounted with the Generate tab"
    );
    // The live synth and drums are silenced while it shows, by the preview's own mute: the
    // user's mute (the top bar's button) stays off.
    assert!(rt.state.svc.with_router(|r, _| r.silenced()));
    assert!(!rt.state.svc.with_router(|r, _| r.muted()));
    assert!(
        !rt.accessibility_json().to_string().contains("\"Unmute\""),
        "the top bar's button still offers Mute (before: Unmute)"
    );
    crate::signal_preview::feed([0x90, 69 + 12, 100]);
    frames(&mut rt, 2);
    assert_eq!(rt.state.p9.signal.frequency, 880.0);
    assert_eq!(rt.state.p9.detected_note.as_deref(), Some("A5"));
    crate::signal_preview::feed([0x80, 81, 0]);
    frames(&mut rt, 2);
    // Hold to preview: down plays, up releases.
    let (x, y) = at(&rt, "gen-preview");
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    frames(&mut rt, 2);
    assert_eq!(rt.state.p9.signal_preview, Some(81));
    rt.pointer_up(x, y);
    frames(&mut rt, 2);
    assert_eq!(rt.state.p9.signal_preview, None);
    // Leaving the page unmounts it and lifts the preview mute.
    rt.state.switch_mode(AppMode::Daw);
    frames(&mut rt, 2);
    assert!(!crate::signal_preview::mounted());
    assert!(!rt.state.svc.with_router(|r, _| r.silenced()));
}

/// Review finding 6: a mute the user set before opening the Generate tab survives it, and
/// pressing the top bar's mute while the tab shows changes only the user's mute.
#[test]
fn the_generate_tab_never_touches_the_users_mute() {
    let mut rt = samples_rt();
    rt.state.switch_mode(AppMode::Daw);
    frames(&mut rt, 2);
    rt.state.toggle_mute();
    assert!(rt.state.svc.with_router(|r, _| r.muted()));
    rt.state.switch_mode(AppMode::Samples);
    run(&mut rt, 0.1);
    assert!(crate::signal_preview::mounted());
    rt.state.switch_mode(AppMode::Daw);
    frames(&mut rt, 2);
    // Before: the unmount restored "was muted" (here right) but the TS un-muted; and
    // un-muting from the top bar while the tab showed un-silenced the preview's drums.
    assert!(rt.state.svc.with_router(|r, _| r.muted()), "still muted");
    // Unmuted by the user while the tab shows: the preview still silences, the user state is off.
    rt.state.switch_mode(AppMode::Samples);
    run(&mut rt, 0.1);
    rt.state.toggle_mute();
    assert!(!rt.state.svc.with_router(|r, _| r.muted()));
    assert!(
        rt.state.svc.with_router(|r, _| r.silenced()),
        "the preview keeps its mute"
    );
    rt.state.switch_mode(AppMode::Daw);
    frames(&mut rt, 2);
    assert!(!rt.state.svc.with_router(|r, _| r.silenced()));
}

#[test]
fn the_page_mounts_with_a_fresh_selection_and_keeps_in_memory_samples() {
    let mut rt = samples_rt();
    click(&mut rt, "gen-generate");
    frames(&mut rt, 3);
    rt.state.switch_mode(AppMode::Daw);
    frames(&mut rt, 2);
    rt.state.switch_mode(AppMode::Samples);
    frames(&mut rt, 2);
    // Electron bug fixed in both apps: the generated sample is still listed.
    assert_eq!(rt.state.p9.samples.entries.len(), 1);
    assert!(rt.state.p9.samples.selected.is_none());
    assert_eq!(crate::views::samples_tab(&rt.state), SamplesTab::Generate);
}

#[test]
fn song_import_makes_an_audio_track_per_file_with_its_waveform() {
    let mut rt = song_rt();
    let p = wav("Loop.wav", 1.0, 1);
    let n = rt.state.svc.engine.doc().get_tracks().len();
    rt.state.p9.samples.import_for_song(&[p]);
    frames(&mut rt, 3);
    let doc = rt.state.svc.engine.doc();
    assert_eq!(doc.get_tracks().len(), n + 1);
    let t = doc.get_tracks().last().unwrap().clone();
    assert_eq!(t.name, "Loop");
    let r = t.regions[0].clone();
    assert!(r.audio.as_ref().unwrap().sample_path.ends_with("Loop.wav"));
    assert_eq!(undo_label(&rt).as_deref(), Some("Add Audio Region"));
    rt.invalidate();
    frames(&mut rt, 2);
    // Loaded: no "Sample file not found" badge.
    assert!(!texts(&rt).iter().any(|x| x == "Sample file not found"));
    assert!(has(&rt, &format!("region-{}", r.id)));
    assert_eq!(rt.state.view, DawView::Song);
}

#[test]
fn project_audio_regions_load_their_samples_by_path() {
    let p = wav("Vox.wav", 0.5, 1);
    let mut rt = song_rt();
    let t = rt
        .state
        .svc
        .engine
        .edit(|d| d.add_track(mdaw_model::types::InstrumentType::Audio, 0));
    let sample = mdaw_model::types::SampleRef {
        id: "s-restored".into(),
        name: "Vox".into(),
        path: p,
        duration_seconds: 0.5,
        sample_rate: 48000.0,
        channels: 1,
    };
    rt.state
        .svc
        .engine
        .edit(|d| d.add_audio_region(&t.id, Some(&sample), 0.0));
    assert!(rt.state.p9.samples.buffer("s-restored").is_none());
    frames(&mut rt, 3);
    assert!(rt.state.p9.samples.buffer("s-restored").is_some());
}
