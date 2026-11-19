//! The Sound Designer's review fixes (`docs/rune-port/sound-designer.md`, "Review fixes"):
//! drafts and project saves / switches / undo, the background renders, the library at
//! startup, render retries, the sample manifest.

use mdaw_dsp::sound_design::render;
use mdaw_model::settings::{ARRANGEMENT_STORAGE_KEY, read_sound_library, write_sound_library};
use mdaw_model::sound_design::{DesignedSound, SampleEdit, Template};
use mdaw_model::types::{PadSample, PadSampleSource};

use super::harness::*;
use super::t_p9_samples::{draft, library_ids, samples_rt, wait_pad, wav};
use crate::app::AppMode;

fn temp_file(name: &str) -> String {
    let d = std::env::temp_dir().join(format!("mdaw-sd-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d.join(name).to_string_lossy().into_owned()
}

/// The sound `id` of the project file at `path` (its JSON).
fn file_sound(path: &str, id: &str) -> Option<serde_json::Value> {
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    v["arrangement"]["sounds"]
        .as_array()?
        .iter()
        .find(|s| s["id"] == id)
        .cloned()
}

/// Review finding 2: unsaved draft edits make the project dirty, ⌘S writes them, and the
/// recovery checkpoint carries them without an undo step.
#[test]
fn draft_edits_are_unsaved_project_changes_and_saving_writes_them() {
    let mut rt = samples_rt();
    rt.state.pick_template(Template::Clap, false);
    rt.state.save_sound();
    let id = draft(&rt).id;
    let f = temp_file("p.mdaw");
    assert!(rt.state.write_project(&f));
    assert!(!rt.state.dirty());
    rt.state.edit_recipe(|r| r.drive = 0.77);
    assert!(rt.state.dirty(), "a draft edit is an unsaved change");
    // The checkpoint (autosave) takes it at once, without saving the sound.
    let undo = undo_depth(&rt);
    run(&mut rt, 0.5);
    let arr = rt
        .state
        .svc
        .settings
        .borrow()
        .get_item(ARRANGEMENT_STORAGE_KEY)
        .unwrap()
        .to_owned();
    let a: mdaw_model::Arrangement = serde_json::from_str(&arr).unwrap();
    let s = a.sounds.iter().find(|s| s.id == id).unwrap();
    assert_eq!(s.recipe.drive, 0.77);
    assert_eq!(undo_depth(&rt), undo);
    assert!(rt.state.p9.designer.draft_dirty());
    // Save writes the edit.
    assert!(rt.state.write_project(&f));
    assert_eq!(file_sound(&f, &id).unwrap()["recipe"]["drive"], 0.77);
    assert!(!rt.state.dirty());
}

/// Review finding 3: switching projects saves the draft into the project being left (and the
/// library), never into the next one; the editor starts over on the new project.
#[test]
fn a_draft_never_leaks_into_the_next_project() {
    let mut rt = samples_rt();
    rt.state.pick_template(Template::Snare, false);
    rt.state.select_pad(0);
    rt.state.send_sound_to_pad();
    let id = draft(&rt).id;
    let v1 = draft(&rt);
    let b = temp_file("b.mdaw");
    assert!(rt.state.write_project(&b));
    rt.state.edit_recipe(|r| r.drive = 0.6);
    rt.state.save_sound();
    let a = temp_file("a.mdaw");
    assert!(rt.state.write_project(&a));
    // Unsaved edit in A, then B opens.
    rt.state.edit_recipe(|r| r.drive = 0.9);
    assert!(rt.state.load_project_file(&b));
    frames(&mut rt, 2);
    let lib = read_sound_library(&rt.state.svc.settings.borrow());
    assert_eq!(
        lib.iter().find(|s| s.id == id).unwrap().recipe.drive,
        0.9,
        "A's draft went to the library"
    );
    assert_eq!(rt.state.svc.engine.doc().get_sound(&id), Some(&v1));
    assert!(!rt.state.p9.designer.draft_dirty());
    assert_eq!(draft(&rt).recipe, v1.recipe, "the editor shows B's sound");
    // Leaving the page doesn't write anything into B.
    rt.state.switch_mode(AppMode::Daw);
    frames(&mut rt, 2);
    assert_eq!(rt.state.svc.engine.doc().get_sound(&id), Some(&v1));
    assert!(!rt.state.dirty());
    // New project: the same.
    rt.state.switch_mode(AppMode::Samples);
    frames(&mut rt, 2);
    rt.state.edit_recipe(|r| r.drive = 0.3);
    let n = temp_file("n.mdaw");
    assert!(rt.state.new_project_at(&n, None));
    frames(&mut rt, 2);
    assert!(rt.state.svc.engine.doc().get_sound(&id).is_none());
    let lib = read_sound_library(&rt.state.svc.settings.borrow());
    assert_eq!(lib.iter().find(|s| s.id == id).unwrap().recipe.drive, 0.3);
}

/// Review finding 4: undoing a save brings the editor, its render, the preview synth and the
/// library back to the older recipe; the next edit starts from it.
#[test]
fn undo_reloads_the_editor_and_the_library() {
    let mut rt = samples_rt();
    rt.state.pick_template(Template::Tom, false);
    rt.state.save_sound();
    let v1 = draft(&rt);
    rt.state.edit_recipe(|r| r.layers[0].coarse = 5.0);
    rt.state.save_sound();
    frames(&mut rt, 1);
    rt.state.undo();
    frames(&mut rt, 2);
    assert_eq!(draft(&rt), v1);
    let rate = rt.state.p9.samples.rate();
    let want = render(&v1.recipe, rate);
    assert_eq!(*rt.state.p9.designer.preview.clone().unwrap(), want);
    assert_eq!(crate::sound_preview::current_recipe(), Some(v1.recipe));
    let lib = read_sound_library(&rt.state.svc.settings.borrow());
    assert_eq!(lib.iter().find(|s| s.id == v1.id), Some(&v1));
    // A nudge edits the older recipe (no silent redo).
    rt.state.edit_recipe(|r| r.drive += 0.01);
    assert_eq!(
        draft(&rt).recipe.layers[0].coarse,
        v1.recipe.layers[0].coarse
    );
    // Redo: forward again.
    rt.state.p9.designer.draft.as_mut().unwrap().dirty = false;
    rt.state.redo();
    frames(&mut rt, 2);
    assert_eq!(draft(&rt).recipe.layers[0].coarse, 5.0);
}

/// Review finding 5: a file load for an id that has since become a designed sound (a slow
/// region / recovery decode) never replaces the sound's render; Song sounds get no recovery WAV.
#[test]
fn late_file_loads_never_overwrite_a_designed_sound() {
    let mut rt = samples_rt();
    let p = wav("Stale.wav", 0.4, 1);
    // The load starts first (its result waits in the queue)...
    let now = rt.state.now;
    rt.state
        .p9
        .samples
        .ensure_region_samples(&[("snd".to_owned(), p)], now);
    // ...the render is registered meanwhile...
    let r = render(&Template::Clap.recipe(), rt.state.p9.samples.rate());
    rt.state.p9.samples.put_sound("snd", "Clap", r.clone());
    // ...and the load lands after it.
    frames(&mut rt, 2);
    assert_eq!(*rt.state.p9.samples.buffer("snd").unwrap(), r);
    // A sound added to the Song is rendered from the project, not saved as a recovery WAV.
    let dir = std::env::temp_dir().join(format!("mdaw-sd-rec-{}", uuid::Uuid::new_v4()));
    rt.state.recovery_dir = Some(dir.clone());
    click(&mut rt, "sd-tpl-open-hat");
    click(&mut rt, "sd-song");
    frames(&mut rt, 2);
    rt.state.checkpoint_now();
    frames(&mut rt, 2);
    let rec = crate::recovery::session_record_of(&rt.state.svc.settings.borrow()).unwrap();
    assert_eq!(rec["assets"], serde_json::json!([]));
    assert!(!dir.exists() || std::fs::read_dir(&dir).unwrap().next().is_none());
}

/// Review finding 7: the library loads at startup; a kit whose pads play library sounds brings
/// their recipes into the project (the pads play without visiting Samples), and a sound found
/// nowhere is reported.
#[test]
fn kits_with_library_sounds_play_at_once_and_unknown_sounds_are_reported() {
    let snd = DesignedSound {
        id: "lib-clap".into(),
        name: "My Clap".into(),
        recipe: Template::Clap.recipe(),
        sample: None,
    };
    let mut st = mdaw_model::settings::SettingsStore::in_memory();
    let _ = write_sound_library(&mut st, std::slice::from_ref(&snd));
    let mut app = crate::App::with_settings(crate::services::RunMode::Headless, st);
    crate::scenes::started(&mut app);
    let mut rt = runtime(app);
    frames(&mut rt, 2);
    let mut kit = mdaw_model::kit::create_factory_kit();
    kit.pads[0].sample = Some(PadSample {
        source: PadSampleSource::Sound {
            sound_id: "lib-clap".into(),
        },
        name: "My Clap".into(),
    });
    kit.pads[1].sample = Some(PadSample {
        source: PadSampleSource::Sound {
            sound_id: "gone".into(),
        },
        name: "Lost Snare".into(),
    });
    rt.state.load_kit(&kit);
    frames(&mut rt, 3);
    assert_eq!(rt.state.svc.engine.doc().get_sound("lib-clap"), Some(&snd));
    let want = render(&snd.recipe, rt.state.p9.samples.rate());
    wait_pad(&mut rt, 0, &want);
    assert_eq!(rt.state.p9.designer.missing, vec!["Lost Snare".to_owned()]);
    assert!(has(&rt, "missing-sounds"));
    assert!(texts(&rt).iter().any(|t| t.contains("“Lost Snare”")));
    click(&mut rt, "missing-sounds-dismiss");
    assert!(!has(&rt, "missing-sounds"));
    // A pasted pad from elsewhere: the same.
    let track = rt.state.track();
    let mut pad = rt.state.svc.engine.doc().get_kit(&track).pads[0].clone();
    pad.sample = Some(PadSample {
        source: PadSampleSource::Sound {
            sound_id: "lib-clap".into(),
        },
        name: "My Clap".into(),
    });
    rt.state.clipboard = Some(pad);
    rt.state.paste_pad(5);
    frames(&mut rt, 2);
    wait_pad(&mut rt, 5, &want);
}

/// Review finding 8: knob edits render off the UI thread and are coalesced (one render in
/// flight; the edits made meanwhile render together when it lands); the preview keeps the last
/// result until then. Stale store renders are dropped.
#[test]
fn renders_run_in_the_background_and_are_coalesced() {
    let mut rt = samples_rt();
    rt.state.pick_template(Template::BassPluck, false);
    rt.state.edit_recipe(|r| r.length = 4.0);
    let shown = rt.state.p9.designer.preview.clone().unwrap();
    rt.state.p9.samples.store.pool.set_sync(false);
    let n = rt.state.p9.designer.preview_renders;
    for k in 0..5 {
        rt.state.edit_recipe(|r| r.drive = 0.1 * k as f64);
    }
    assert_eq!(rt.state.p9.designer.preview_renders, n + 1, "one in flight");
    assert!(!rt.state.p9.designer.preview_current());
    assert_eq!(
        *rt.state.p9.designer.preview.clone().unwrap(),
        *shown,
        "the last result shows meanwhile"
    );
    let rate = rt.state.p9.samples.rate();
    let want = render(&draft(&rt).recipe, rate);
    for _ in 0..2000 {
        if rt.state.p9.designer.preview_current() {
            break;
        }
        frames(&mut rt, 1);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(rt.state.p9.designer.preview_current());
    assert_eq!(*rt.state.p9.designer.preview.clone().unwrap(), want);
    assert_eq!(rt.state.p9.designer.preview_renders, n + 2, "coalesced");
    // Two saves in a row: the store ends with the second (the first is dropped if late).
    rt.state.save_sound();
    rt.state.edit_recipe(|r| r.drive = 0.95);
    rt.state.save_sound();
    let id = draft(&rt).id;
    let last = render(&draft(&rt).recipe, rate);
    for _ in 0..2000 {
        if rt.state.p9.samples.buffer(&id).is_some_and(|b| *b == last) {
            break;
        }
        frames(&mut rt, 1);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    // Let any late render land: it must not replace the latest.
    for _ in 0..50 {
        frames(&mut rt, 1);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(*rt.state.p9.samples.buffer(&id).unwrap(), last);
}

/// Review finding 8: the decoded files of sample sounds are bounded by size.
#[test]
fn the_decode_cache_is_bounded() {
    let mut rt = samples_rt();
    rt.state.p9.designer.decode_budget = 100_000;
    let files: Vec<String> = (0..4).map(|i| wav(&format!("F{i}.wav"), 0.2, 1)).collect();
    rt.state.import_sound_files(&files);
    frames(&mut rt, 2);
    let used = rt.state.p9.designer.decoded_bytes();
    assert!(used > 0 && used <= 100_000, "{used}");
    // An evicted file is decoded again when needed.
    rt.state.open_sound(&library_ids(&rt)[0], false);
    frames(&mut rt, 2);
    assert!(rt.state.p9.designer.preview_current());
    assert!(rt.state.p9.designer.preview.is_some());
}

/// Review finding 10: a sound whose file can't be read is tried again (a drive reconnected).
#[test]
fn unreadable_sound_files_are_retried() {
    let mut rt = samples_rt();
    let path = temp_file("Later.wav");
    let snd = DesignedSound {
        id: "later".into(),
        name: "Later".into(),
        recipe: Default::default(),
        sample: Some(SampleEdit::new(&path)),
    };
    rt.state.svc.engine.edit(|d| d.save_sound(&snd));
    frames(&mut rt, 2);
    assert!(rt.state.p9.samples.buffer("later").is_none());
    std::fs::copy(wav("Src.wav", 0.25, 1), &path).unwrap();
    run(&mut rt, crate::designer::RENDER_RETRY + 0.5);
    frames(&mut rt, 2);
    let b = rt.state.p9.samples.buffer("later").expect("retried");
    assert_eq!(b.frames(), 12_000);
}

/// Review finding 11: a project with a sound in the Song reopens without a load warning.
#[test]
fn a_song_sound_reopens_without_warnings() {
    let mut rt = samples_rt();
    click(&mut rt, "sd-tpl-tom");
    click(&mut rt, "sd-song");
    frames(&mut rt, 2);
    let f = temp_file("song.mdaw");
    assert!(rt.state.write_project(&f));
    let raw = std::fs::read_to_string(&f).unwrap();
    assert!(!raw.contains("sampleManifest"), "{raw}");
    assert!(rt.state.load_project_file(&f));
    frames(&mut rt, 2);
    assert_eq!(rt.state.project.report, None);
    let id = rt.state.svc.engine.doc().get_sounds()[0].id.clone();
    assert!(rt.state.p9.samples.buffer(&id).is_some());
    // A file saved before the fix lists the sound with `path: ""`: no warning either.
    let mut v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    v["sampleManifest"] = serde_json::json!([{"sampleId": id, "path": "", "name": "Tom"}]);
    let old = temp_file("old.mdaw");
    std::fs::write(&old, v.to_string()).unwrap();
    assert!(rt.state.load_project_file(&old));
    frames(&mut rt, 2);
    assert_eq!(rt.state.project.report, None);
}
