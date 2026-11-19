//! The Samples page (the Sound Designer: templates, live editing, save, send to pad, add to
//! song, the library, imported audio, MIDI preview) and the Song view's audio import +
//! audio-region waveforms.

use mdaw_model::sound_design::Template;
use mdaw_model::types::PadSampleSource;
use rune::Key;

use super::harness::*;
use crate::app::{AppMode, DawView};

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

pub fn samples_rt() -> rune::Runtime<crate::App> {
    let mut rt = rt();
    rt.state.svc.save_headless = true;
    rt.state.switch_mode(AppMode::Samples);
    rt.invalidate();
    frames(&mut rt, 3);
    rt
}

pub fn draft(rt: &rune::Runtime<crate::App>) -> mdaw_model::sound_design::DesignedSound {
    rt.state.p9.designer.draft_sound().cloned().unwrap()
}

pub fn library_ids(rt: &rune::Runtime<crate::App>) -> Vec<String> {
    let st = rt.state.svc.settings.borrow();
    mdaw_model::settings::read_sound_library(&st)
        .into_iter()
        .map(|d| d.id)
        .collect()
}

/// Frames until the active drum track's pad `p` plays a buffer equal to `want` (loads run on
/// worker threads).
#[track_caller]
pub fn wait_pad(
    rt: &mut rune::Runtime<crate::App>,
    p: usize,
    want: &mdaw_dsp::buffer::AudioBuffer,
) {
    let track = rt
        .state
        .svc
        .engine
        .doc()
        .get_active_drum_track_id()
        .unwrap();
    for _ in 0..400 {
        if let Some(b) = rt.state.svc.engine.pad_buffer(&track, p)
            && *b == *want
        {
            return;
        }
        frames(rt, 1);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let got = rt
        .state
        .svc
        .engine
        .pad_buffer(&track, p)
        .map(|b| b.frames());
    panic!(
        "pad {p} never got the expected buffer ({} frames): {got:?}",
        want.frames()
    );
}

#[test]
fn the_page_opens_on_a_template_with_the_library_and_actions() {
    let rt = samples_rt();
    let t = texts(&rt);
    for s in [
        "Sound Designer",
        "Start from",
        "My sounds",
        "Kick 808",
        "Open Hat",
        "Bass Pluck",
        "Play",
        "Save",
        "Add to song",
        "Amp envelope",
        "Filter",
        "OSC 1",
        "New — not saved",
    ] {
        assert!(t.iter().any(|x| x == s), "missing {s:?}");
    }
    assert!(t.iter().any(|x| x.starts_with("Send to pad")));
    let d = draft(&rt);
    assert_eq!(d.name, "Kick 808");
    assert_eq!(d.recipe, Template::Kick808.recipe());
    // Nothing is stored until the sound is saved or used.
    assert!(rt.state.svc.engine.doc().get_sounds().is_empty());
    assert!(
        rt.state.p9.designer.preview.is_some(),
        "rendered for the waveform"
    );
    assert!(has(&rt, "sd-waveform"));
    assert!(crate::sound_preview::mounted());
}

#[test]
fn picking_a_template_plays_it_and_a_knob_edits_the_live_render() {
    let mut rt = samples_rt();
    click(&mut rt, "sd-tpl-snare");
    frames(&mut rt, 2);
    assert_eq!(draft(&rt).name, "Snare");
    assert!(rt.state.svc.engine.is_audition_playing(), "auto-preview");
    let before = rt.state.p9.designer.preview.clone().unwrap();
    // The amp decay knob, from the keyboard: one tick up.
    click(&mut rt, "sd-k-1102");
    press(&mut rt, ev(Key::Up, Default::default()));
    frames(&mut rt, 2);
    let d = draft(&rt);
    assert!(d.recipe.amp.decay > Template::Snare.recipe().amp.decay);
    assert!(rt.state.p9.designer.draft.as_ref().unwrap().dirty);
    let after = rt.state.p9.designer.preview.clone().unwrap();
    assert_ne!(*before, *after, "re-rendered at once");
    // The preview synth plays the edited recipe.
    assert_eq!(crate::sound_preview::current_recipe(), Some(d.recipe));
    assert!(texts(&rt).iter().any(|x| x == "New — not saved"));
}

#[test]
fn save_stores_the_recipe_in_the_project_and_the_library() {
    let mut rt = samples_rt();
    click(&mut rt, "sd-tpl-clap");
    rt.state.edit_recipe(|r| r.drive = 0.5);
    rt.invalidate();
    frames(&mut rt, 1);
    click(&mut rt, "sd-save");
    frames(&mut rt, 2);
    let d = draft(&rt);
    assert_eq!(rt.state.svc.engine.doc().get_sound(&d.id), Some(&d));
    assert_eq!(library_ids(&rt), vec![d.id.clone()]);
    assert_eq!(undo_label(&rt).as_deref(), Some("Save Sound"));
    assert!(texts(&rt).iter().any(|x| x == "Saved"));
    assert!(has(&rt, &format!("sd-sound-{}", d.id)));
    // Its render is registered under its id (pads and the Song read it there).
    let b = rt.state.p9.samples.buffer(&d.id).unwrap();
    assert_eq!(*b, *rt.state.p9.designer.preview.clone().unwrap());
}

#[test]
fn send_to_pad_assigns_the_render_and_re_editing_updates_the_pad() {
    let mut rt = samples_rt();
    rt.state.select_pad(5);
    click(&mut rt, "sd-tpl-tom");
    frames(&mut rt, 1);
    assert!(texts(&rt).iter().any(|x| x == "Send to pad 6"));
    click(&mut rt, "sd-send");
    frames(&mut rt, 2);
    let d = draft(&rt);
    assert_eq!(undo_label(&rt).as_deref(), Some("Send to Pad"));
    let p = pad(&rt, 5);
    let smp = p.sample.unwrap();
    assert_eq!(
        smp.source,
        PadSampleSource::Sound {
            sound_id: d.id.clone()
        }
    );
    assert_eq!(smp.name, "Tom");
    let first = mdaw_dsp::sound_design::render(&d.recipe, rt.state.p9.samples.rate());
    wait_pad(&mut rt, 5, &first);
    assert!(texts(&rt).iter().any(|x| x.contains("is on pad 6")));

    // Leave, come back, reopen it from the library, edit, save: the pad follows.
    rt.state.switch_mode(AppMode::Daw);
    frames(&mut rt, 2);
    rt.state.switch_mode(AppMode::Samples);
    frames(&mut rt, 2);
    click(&mut rt, &format!("sd-sound-{}", d.id));
    frames(&mut rt, 1);
    assert_eq!(draft(&rt).id, d.id);
    assert_eq!(draft(&rt).recipe, d.recipe, "every knob restored");
    rt.state.edit_recipe(|r| r.layers[0].coarse = 7.0);
    rt.invalidate();
    frames(&mut rt, 1);
    click(&mut rt, "sd-save");
    frames(&mut rt, 2);
    let second = mdaw_dsp::sound_design::render(&draft(&rt).recipe, rt.state.p9.samples.rate());
    assert_ne!(first, second);
    wait_pad(&mut rt, 5, &second);
    // Undo brings the old recipe back, and the pad with it.
    rt.state.undo();
    frames(&mut rt, 2);
    wait_pad(&mut rt, 5, &first);
}

#[test]
fn the_pad_picker_sends_to_the_chosen_pad() {
    let mut rt = samples_rt();
    click(&mut rt, "sd-send-pick");
    frames(&mut rt, 2);
    assert!(has(&rt, "sd-send-menu"));
    click(&mut rt, "sd-send-pad-12");
    frames(&mut rt, 2);
    assert!(!has(&rt, "sd-send-menu"));
    let id = draft(&rt).id;
    assert_eq!(
        pad(&rt, 12).sample.unwrap().source,
        PadSampleSource::Sound { sound_id: id }
    );
    // The choice sticks for the next send.
    assert!(texts(&rt).iter().any(|x| x == "Send to pad 13"));
}

#[test]
fn leaving_a_draft_with_edits_saves_it_and_an_untouched_template_is_dropped() {
    let mut rt = samples_rt();
    click(&mut rt, "sd-tpl-zap");
    click(&mut rt, "sd-tpl-sub");
    assert!(rt.state.svc.engine.doc().get_sounds().is_empty());
    rt.state.edit_recipe(|r| r.length = 2.0);
    let id = draft(&rt).id;
    rt.state.switch_mode(AppMode::Daw);
    frames(&mut rt, 2);
    let saved = rt.state.svc.engine.doc().get_sound(&id).unwrap().clone();
    assert_eq!(saved.recipe.length, 2.0);
    assert_eq!(library_ids(&rt), vec![id]);
}

#[test]
fn space_plays_and_arrows_browse_the_library_with_auto_preview() {
    let mut rt = samples_rt();
    for t in [Template::Kick808, Template::Snare, Template::ClosedHat] {
        rt.state.pick_template(t, false);
        rt.state.save_sound();
    }
    rt.state.stop_sound();
    frames(&mut rt, 2);
    press(&mut rt, text_key(" "));
    assert!(rt.state.svc.engine.is_audition_playing());
    press(&mut rt, text_key(" "));
    assert!(!rt.state.svc.engine.is_audition_playing());
    assert_eq!(draft(&rt).name, "Closed Hat");
    press(&mut rt, ev(Key::Up, Default::default()));
    frames(&mut rt, 1);
    assert_eq!(draft(&rt).name, "Snare");
    assert!(rt.state.svc.engine.is_audition_playing());
    press(&mut rt, ev(Key::Up, Default::default()));
    press(&mut rt, ev(Key::Up, Default::default()));
    assert_eq!(draft(&rt).name, "Kick 808");
    press(&mut rt, ev(Key::Down, Default::default()));
    assert_eq!(draft(&rt).name, "Snare");
}

#[test]
fn search_rename_duplicate_and_delete() {
    let mut rt = samples_rt();
    for t in [Template::Kick808, Template::Snare] {
        rt.state.pick_template(t, false);
        rt.state.save_sound();
    }
    rt.state.p9.designer.search = rune::TextEdit::new("kick");
    rt.invalidate();
    frames(&mut rt, 1);
    let names: Vec<String> = crate::designer::listed(&rt.state)
        .into_iter()
        .map(|d| d.name)
        .collect();
    assert_eq!(names, vec!["Kick 808"]);
    rt.state.p9.designer.search = rune::TextEdit::new("");
    // Rename the open Snare (and the pad it is on follows).
    rt.state.send_sound_to_pad();
    let id = draft(&rt).id;
    click(&mut rt, "sd-rename");
    frames(&mut rt, 1);
    rt.state.p9.designer.rename = Some(rune::TextEdit::new("Crack"));
    rt.state.commit_sound_rename();
    frames(&mut rt, 1);
    assert_eq!(
        rt.state.svc.engine.doc().get_sound(&id).unwrap().name,
        "Crack"
    );
    let sel = rt.state.selected_pad();
    assert_eq!(pad(&rt, sel).sample.unwrap().name, "Crack");
    // Duplicate: a stored copy opens.
    click(&mut rt, "sd-duplicate");
    let copy = draft(&rt);
    assert_ne!(copy.id, id);
    assert_eq!(copy.name, "Crack copy");
    assert_eq!(
        copy.recipe,
        rt.state.svc.engine.doc().get_sound(&id).unwrap().recipe
    );
    // Delete the original: it is on a pad, so Delete asks first (review finding 6).
    rt.state.open_sound(&id, false);
    click(&mut rt, "sd-delete");
    frames(&mut rt, 2);
    assert!(
        rt.state.svc.engine.doc().get_sound(&id).is_some(),
        "not yet"
    );
    assert!(has(&rt, "sd-delete-warning"));
    assert!(texts(&rt).iter().any(|x| x == "Confirm delete"));
    assert!(
        texts(&rt)
            .iter()
            .any(|x| x.starts_with("1 pad play this sound"))
    );
    // The second click deletes: out of the project and library, its pad cleared.
    click(&mut rt, "sd-delete");
    frames(&mut rt, 2);
    assert!(rt.state.svc.engine.doc().get_sound(&id).is_none());
    assert!(!library_ids(&rt).contains(&id));
    assert!(pad(&rt, sel).sample.is_none());
    // Undo restores the pad, the render and the library entry (at its place); redo deletes
    // again.
    let before = library_ids(&rt);
    rt.state.undo();
    frames(&mut rt, 2);
    assert!(pad(&rt, sel).sample.is_some());
    assert!(rt.state.p9.samples.buffer(&id).is_some(), "rendered again");
    let ids = library_ids(&rt);
    assert_eq!(ids.len(), before.len() + 1);
    assert_eq!(ids[1], id, "back at its place");
    rt.state.redo();
    frames(&mut rt, 2);
    assert_eq!(library_ids(&rt), before);
}

/// Review finding 6: a sound only in the library asks before it goes, and undo brings it back.
#[test]
fn deleting_a_library_only_sound_asks_and_undo_restores_it() {
    let mut rt = samples_rt();
    rt.state.pick_template(Template::Zap, false);
    rt.state.save_sound();
    let id = draft(&rt).id;
    // Another project: the sound is only in the library now.
    rt.state.svc.engine.edit(|d| d.reset_to_starter());
    rt.state.svc.engine.edit(|d| d.clear_history());
    frames(&mut rt, 2);
    rt.state.open_sound(&id, false);
    frames(&mut rt, 1);
    assert!(rt.state.svc.engine.doc().get_sound(&id).is_none());
    assert!(rt.state.delete_needs_confirm());
    click(&mut rt, "sd-delete");
    frames(&mut rt, 1);
    assert!(library_ids(&rt).contains(&id), "asks first");
    assert!(
        texts(&rt)
            .iter()
            .any(|x| x.starts_with("This sound is only in your library"))
    );
    // The confirmation expires.
    run(&mut rt, crate::designer::CONFIRM_DELETE + 0.5);
    assert!(rt.state.p9.designer.confirm_delete.is_none());
    click(&mut rt, "sd-delete");
    click(&mut rt, "sd-delete");
    frames(&mut rt, 1);
    assert!(!library_ids(&rt).contains(&id));
    assert_eq!(undo_label(&rt).as_deref(), Some("Delete Sound"));
    rt.state.undo();
    frames(&mut rt, 1);
    assert!(
        library_ids(&rt).contains(&id),
        "undo restores the library entry"
    );
}

/// Review finding 6: a sound in the Song asks first; deleting it removes its clips (none is
/// left pointing at nothing) and undo brings them back.
#[test]
fn deleting_a_sound_in_the_song_removes_its_clips_undoably() {
    let mut rt = samples_rt();
    click(&mut rt, "sd-tpl-open-hat");
    click(&mut rt, "sd-song");
    frames(&mut rt, 2);
    let id = rt
        .state
        .svc
        .engine
        .doc()
        .get_sounds()
        .first()
        .unwrap()
        .id
        .clone();
    rt.state.switch_mode(AppMode::Samples);
    frames(&mut rt, 2);
    rt.state.open_sound(&id, false);
    assert!(rt.state.delete_needs_confirm());
    rt.state.delete_sound_click();
    rt.invalidate();
    frames(&mut rt, 1);
    assert!(texts(&rt).iter().any(|x| x.starts_with("1 Song clip play")));
    rt.state.delete_sound_click();
    frames(&mut rt, 1);
    let doc = rt.state.svc.engine.doc();
    assert!(doc.sound_regions(&id).is_empty());
    assert!(
        !doc.get_tracks()
            .iter()
            .flat_map(|t| t.regions.iter())
            .filter_map(|r| r.audio.as_ref())
            .any(|a| a.sample_id == id)
    );
    rt.state.undo();
    frames(&mut rt, 2);
    assert_eq!(rt.state.svc.engine.doc().sound_regions(&id).len(), 1);
    assert!(rt.state.p9.samples.buffer(&id).is_some());
}

#[test]
fn importing_a_file_makes_an_editable_sample_sound() {
    let mut rt = samples_rt();
    let p = wav("Rim Shot.wav", 0.5, 2);
    rt.state.import_sound_files(std::slice::from_ref(&p));
    frames(&mut rt, 2);
    let d = draft(&rt);
    assert_eq!(d.name, "Rim Shot");
    assert_eq!(d.sample.as_ref().unwrap().path, p);
    assert!(library_ids(&rt).contains(&d.id), "in the library at once");
    assert!(rt.state.svc.engine.is_audition_playing());
    for s in ["Edit", "Normalize", "Reverse", "Fade out"] {
        assert!(texts(&rt).iter().any(|x| x == s), "missing {s:?}");
    }
    let raw = rt.state.p9.designer.preview.clone().unwrap();
    assert_eq!(raw.channel_count(), 2);
    // Trim, normalize, reverse: one panel, non-destructive.
    click(&mut rt, "sd-normalize");
    rt.state.edit_sound(|d| {
        if let Some(e) = d.sample.as_mut() {
            e.end = Some(0.25);
            e.reverse = true;
        }
    });
    rt.invalidate();
    frames(&mut rt, 1);
    let edited = rt.state.p9.designer.preview.clone().unwrap();
    assert_eq!(edited.frames(), 12_000);
    let peak = edited.channels[0].iter().fold(0f32, |m, x| m.max(x.abs()));
    assert!((peak - 1.0).abs() < 1e-3, "normalized: {peak}");
    // On a pad it plays the edited audio.
    rt.state.select_pad(2);
    click(&mut rt, "sd-send");
    frames(&mut rt, 2);
    wait_pad(&mut rt, 2, &edited);
    // The file on disk is untouched.
    let st = std::fs::metadata(&p).unwrap().len();
    assert!(st > 48_000 * 2);
    // An unreadable file is reported.
    rt.state
        .import_sound_files(&["/nope/missing.wav".to_owned()]);
    rt.invalidate();
    frames(&mut rt, 1);
    assert!(
        texts(&rt)
            .iter()
            .any(|x| x.contains("Couldn't read missing"))
    );
}

#[test]
fn dropping_files_on_the_page_imports_them() {
    let mut rt = samples_rt();
    let p = wav("Dropped.wav", 0.2, 1);
    let r = rect(&rt, "sd-editor");
    rt.drop_files(
        vec![std::path::PathBuf::from(&p)],
        Some((r[0] + 40.0, r[1] + 40.0)),
    );
    frames(&mut rt, 2);
    assert_eq!(draft(&rt).name, "Dropped");
}

#[test]
fn add_to_song_makes_an_audio_track_playing_the_render() {
    let mut rt = samples_rt();
    click(&mut rt, "sd-tpl-open-hat");
    let n = rt.state.svc.engine.doc().get_tracks().len();
    click(&mut rt, "sd-song");
    frames(&mut rt, 3);
    assert_eq!(rt.state.mode, AppMode::Daw);
    assert_eq!(rt.state.view, DawView::Song);
    let doc = rt.state.svc.engine.doc();
    assert_eq!(doc.get_tracks().len(), n + 1);
    let t = doc.get_tracks().last().unwrap().clone();
    let a = t.regions[0].audio.clone().unwrap();
    let id = a.sample_id.clone();
    assert!(doc.get_sound(&id).is_some(), "saved with the project");
    assert!(rt.state.p9.samples.buffer(&id).is_some());
    assert!(!texts(&rt).iter().any(|x| x == "Sample file not found"));
}

#[test]
fn midi_keys_play_the_sound_at_pitch_through_the_preview_synth() {
    let mut rt = samples_rt();
    assert!(crate::sound_preview::mounted());
    crate::sound_preview::feed([0x90, 72, 100]);
    crate::sound_preview::feed([0x99, 36, 127]); // a pad: as designed
    crate::sound_preview::feed([0x80, 72, 0]);
    frames(&mut rt, 2);
    // Drained (the notes went to the preview slot); leaving the page unmounts the synth.
    assert!(crate::sound_preview::take_note_ons().is_empty());
    rt.state.switch_mode(AppMode::Daw);
    frames(&mut rt, 2);
    assert!(!crate::sound_preview::mounted());
}

#[test]
fn the_preview_synth_plays_exactly_the_offline_render() {
    use mdaw_engine::contract::Instrument;
    use mdaw_engine::core::SynthInstrument;
    let r = Template::PunchyKick.recipe();
    let shared = std::sync::Arc::new(std::sync::Mutex::new((1, r)));
    let mut synth = crate::sound_preview::SoundPreviewSynth::new(48_000, shared);
    synth.note_on(crate::sound_preview::ROOT, 1.0, 0.0);
    let want = mdaw_dsp::sound_design::render_raw(&r, 48_000, 0.0, 1.0);
    let mut got = Vec::new();
    let (mut l, mut rr) = ([0f32; 128], [0f32; 128]);
    while got.len() < want.len() {
        l.fill(0.0);
        rr.fill(0.0);
        synth.process(0, &mut l, &mut rr);
        got.extend_from_slice(&l);
    }
    assert_eq!(&got[..want.len()], &want[..]);
}

#[test]
fn old_sample_library_entries_become_sample_sounds_once() {
    let p = wav("Old.wav", 0.3, 1);
    let mut st = mdaw_model::settings::SettingsStore::in_memory();
    let _ = mdaw_model::settings::write_sample_library(
        &mut st,
        &[mdaw_model::settings::SerializedSampleEntry {
            sample_id: "old-1".into(),
            sample_path: p.clone(),
            sample_name: "Old One".into(),
            trim: Some(mdaw_model::types::SampleTrim {
                start_seconds: 0.1,
                end_seconds: 0.2,
            }),
            added_at: 1.0,
        }],
    );
    let mut app = crate::App::with_settings(crate::services::RunMode::Headless, st);
    crate::scenes::started(&mut app);
    let mut rt = runtime(app);
    rt.state.switch_mode(AppMode::Samples);
    frames(&mut rt, 3);
    let d = draft(&rt);
    assert_eq!(d.name, "Old One");
    let e = d.sample.clone().unwrap();
    assert_eq!((e.path, e.start, e.end), (p, 0.1, Some(0.2)));
    assert_eq!(library_ids(&rt), vec![d.id.clone()]);
    // Review finding 1: a fresh id, never the old entry's (Song regions of older projects play
    // that id with the trim applied through the region).
    assert_ne!(d.id, "old-1");
}

/// Review finding 1: a Song region playing an old library sample keeps its audio (the whole
/// file; the region applies the trim) after the migrated sound is used, edited and deleted.
#[test]
fn migrated_sounds_never_touch_the_song_regions_of_the_old_sample() {
    let p = wav("Old.wav", 0.3, 1);
    let mut st = mdaw_model::settings::SettingsStore::in_memory();
    let _ = mdaw_model::settings::write_sample_library(
        &mut st,
        &[mdaw_model::settings::SerializedSampleEntry {
            sample_id: "old-1".into(),
            sample_path: p.clone(),
            sample_name: "Old One".into(),
            trim: Some(mdaw_model::types::SampleTrim {
                start_seconds: 0.1,
                end_seconds: 0.2,
            }),
            added_at: 1.0,
        }],
    );
    let mut app = crate::App::with_settings(crate::services::RunMode::Headless, st);
    crate::scenes::started(&mut app);
    let mut rt = runtime(app);
    let region = mdaw_model::types::SampleRef {
        id: "old-1".into(),
        name: "Old One".into(),
        path: p.clone(),
        duration_seconds: 0.3,
        sample_rate: 48_000.0,
        channels: 1,
    };
    rt.state
        .svc
        .engine
        .edit(|d| d.send_sample_to_arrangement(&region, None));
    frames(&mut rt, 3);
    let full = rt.state.p9.samples.buffer("old-1").unwrap().frames();
    assert_eq!(full, 14_400);
    rt.state.switch_mode(AppMode::Samples);
    frames(&mut rt, 3);
    rt.state.select_pad(4);
    rt.state.send_sound_to_pad();
    rt.state
        .edit_sound(|d| d.sample.as_mut().unwrap().gain_db = -3.0);
    rt.state.save_sound();
    frames(&mut rt, 2);
    assert_eq!(rt.state.p9.samples.buffer("old-1").unwrap().frames(), full);
    rt.state.delete_sound_click();
    rt.state.delete_sound_click();
    frames(&mut rt, 2);
    let b = rt.state.p9.samples.buffer("old-1");
    assert_eq!(b.map(|b| b.frames()), Some(full), "the region still plays");
}

#[test]
fn a_reopened_project_renders_its_sounds_for_the_pads() {
    let mut rt = samples_rt();
    rt.state.pick_template(Template::Clap, false);
    rt.state.select_pad(1);
    rt.state.send_sound_to_pad();
    let d = draft(&rt);
    let raw = rt.state.svc.engine.doc().save_to_local_storage();
    // A fresh app (empty library) opens the saved project.
    let mut rt2 = samples_rt();
    rt2.state.switch_mode(AppMode::Daw);
    frames(&mut rt2, 1);
    rt2.state
        .svc
        .engine
        .edit(|doc| doc.load_from_local_storage(Some(&raw)).map(|_| ()))
        .unwrap();
    frames(&mut rt2, 3);
    let want = mdaw_dsp::sound_design::render(&d.recipe, rt2.state.p9.samples.rate());
    wait_pad(&mut rt2, 1, &want);
    // The project's sound is in the list and opens with all its knobs.
    rt2.state.switch_mode(AppMode::Samples);
    frames(&mut rt2, 2);
    rt2.state.open_sound(&d.id, false);
    assert_eq!(draft(&rt2).recipe, d.recipe);
}

/// Review finding 6 (kept for the Sound Designer's preview synth): a mute the user set before
/// opening the Samples page survives it, and the top bar's mute there changes only the user's.
#[test]
fn the_sound_preview_never_touches_the_users_mute() {
    let mut rt = samples_rt();
    rt.state.switch_mode(AppMode::Daw);
    frames(&mut rt, 2);
    rt.state.toggle_mute();
    assert!(rt.state.svc.with_router(|r, _| r.muted()));
    rt.state.switch_mode(AppMode::Samples);
    run(&mut rt, 0.1);
    assert!(crate::sound_preview::mounted());
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

/// In a window shorter than the editor, the editor column scrolls (the user's 1000×560 window
/// cut off the Character / Output rows with no way to reach them).
#[test]
fn designer_editor_scrolls_in_a_short_window() {
    let mut rt = samples_rt();
    rt.set_viewport((1000.0, 560.0), 1.0);
    rt.invalidate();
    frames(&mut rt, 3);
    rt.state.pick_template(Template::Kick808, false);
    rt.invalidate();
    frames(&mut rt, 3);
    let before = rect(&rt, "sd-editor");
    let port = rect(&rt, "sd-scroll");
    let page = rect(&rt, "samples-view");
    let bottom = page[1] + page[3];
    assert!(
        port[1] + port[3] <= bottom + 1.0,
        "the scroll viewport is bounded by the window: {port:?}"
    );
    assert!(
        before[1] + before[3] > bottom,
        "editor should overflow the short window: {before:?}"
    );
    // The editor's left padding: knobs under the pointer take the wheel themselves.
    let (x, y) = (before[0] + 6.0, 400.0);
    for _ in 0..100 {
        wheel(&mut rt, x, y, -10.0);
        frames(&mut rt, 1);
    }
    frames(&mut rt, 10);
    let after = rect(&rt, "sd-editor");
    assert!(
        after[1] < before[1] - 100.0,
        "wheel should scroll the editor: {before:?} -> {after:?}"
    );
    assert!(
        after[1] + after[3] <= bottom + 1.0,
        "the bottom of the editor is reachable: {after:?}"
    );
}
