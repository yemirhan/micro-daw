//! The Sound Designer's sounds in the doc and the project file.

use super::tests::counter_doc;
use crate::migrate::{build_project_file, project_file_to_json};
use crate::open::open_project;
use crate::sound_design::{DesignedSound, Template};
use crate::types::{EffectParams, PadSampleSource};

fn sound(id: &str, t: Template) -> DesignedSound {
    DesignedSound {
        id: id.into(),
        name: t.name().into(),
        recipe: t.recipe(),
    }
}

fn label(doc: &super::ArrangementDoc) -> Option<&str> {
    doc.undo_manager().undo_label()
}

#[test]
fn save_is_one_undo_step_and_unchanged_saves_write_nothing() {
    let mut doc = counter_doc();
    let k = sound("s1", Template::Kick808);
    doc.save_sound(&k);
    assert_eq!(doc.get_sounds(), &[k.clone()]);
    assert_eq!(label(&doc), Some("Save Sound"));
    let n = doc.undo_manager().undo_len();
    doc.save_sound(&k);
    assert_eq!(doc.undo_manager().undo_len(), n);
    let mut k2 = k.clone();
    k2.recipe.length = 0.3;
    doc.save_sound(&k2);
    assert_eq!(doc.get_sound("s1").unwrap().recipe.length, 0.3);
    doc.perform_undo();
    assert_eq!(doc.get_sound("s1"), Some(&k));
    doc.perform_undo();
    assert!(doc.get_sounds().is_empty());
}

#[test]
fn send_to_pad_saves_and_assigns_in_one_step() {
    let mut doc = counter_doc();
    let id = doc.ensure_drum_track();
    let n = doc.undo_manager().undo_len();
    let s = sound("snd", Template::Snare);
    doc.send_sound_to_pad(&id, 3, &s);
    assert_eq!(doc.undo_manager().undo_len(), n + 1);
    assert_eq!(label(&doc), Some("Send to Pad"));
    let pad = doc.get_kit(&id).pads[3].clone();
    let smp = pad.sample.unwrap();
    assert_eq!(
        smp.source,
        PadSampleSource::Sound {
            sound_id: "snd".into()
        }
    );
    assert_eq!(smp.name, "Snare");
    assert_eq!(doc.sound_users("snd"), vec![(id.clone(), 3)]);
    doc.perform_undo();
    assert!(doc.get_sounds().is_empty());
    assert!(doc.sound_users("snd").is_empty());
}

#[test]
fn rename_follows_pads_and_remove_clears_them() {
    let mut doc = counter_doc();
    let id = doc.ensure_drum_track();
    doc.send_sound_to_pad(&id, 0, &sound("a", Template::Clap));
    doc.rename_sound("a", "  Big Clap ");
    assert_eq!(doc.get_sound("a").unwrap().name, "Big Clap");
    assert_eq!(
        doc.get_kit(&id).pads[0].sample.as_ref().unwrap().name,
        "Big Clap"
    );
    assert_eq!(label(&doc), Some("Rename Sound"));
    doc.remove_sound("a");
    assert!(doc.get_sound("a").is_none());
    assert!(doc.get_kit(&id).pads[0].sample.is_none());
    doc.perform_undo();
    assert!(doc.get_sound("a").is_some());
    assert!(doc.get_kit(&id).pads[0].sample.is_some());
}

fn round_trip(doc: &super::ArrangementDoc) -> crate::types::Arrangement {
    let file = build_project_file(
        doc.get_arrangement().clone(),
        -12.0,
        EffectParams::default(),
        "c",
        "m",
        &|_| None,
    );
    let json = project_file_to_json(&file);
    let mut n = 0;
    open_project(&json, None, &mut || {
        n += 1;
        format!("new-{n}")
    })
    .unwrap()
    .arrangement
}

#[test]
fn recipes_round_trip_through_the_project_file_and_autosave() {
    let mut doc = counter_doc();
    let id = doc.ensure_drum_track();
    let mut s = sound("z", Template::Zap);
    s.recipe.layers[2].on = true;
    s.recipe.fm.amount = 0.7;
    doc.send_sound_to_pad(&id, 5, &s);
    doc.save_sound(&sound("b", Template::BassPluck));
    let a = round_trip(&doc);
    assert_eq!(a.sounds, doc.get_sounds());
    let pad = &a
        .tracks
        .iter()
        .find(|t| t.id == id)
        .unwrap()
        .instrument
        .kit
        .as_ref()
        .unwrap()
        .pads[5];
    assert_eq!(
        pad.sample.as_ref().unwrap().source,
        PadSampleSource::Sound {
            sound_id: "z".into()
        }
    );
    // Autosave (local storage) too.
    let raw = doc.save_to_local_storage();
    let mut doc2 = counter_doc();
    doc2.load_from_local_storage(Some(&raw)).unwrap();
    assert_eq!(doc2.get_sounds(), doc.get_sounds());
}

#[test]
fn older_project_files_without_sounds_still_load() {
    let doc = counter_doc();
    let a = round_trip(&doc);
    assert!(a.sounds.is_empty());
    let json = project_file_to_json(&build_project_file(
        doc.get_arrangement().clone(),
        -12.0,
        EffectParams::default(),
        "c",
        "m",
        &|_| None,
    ));
    assert!(!json.contains("\"sounds\""), "no key written when empty");
    // A bad entry is dropped, a partial one gets defaults and clamps.
    let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
    v["arrangement"]["sounds"] = serde_json::json!([
        {"id": "ok", "name": "Ok", "recipe": {"length": 99}},
        {"name": "no id"},
        "junk"
    ]);
    let mut n = 0;
    let opened = open_project(&v.to_string(), None, &mut || {
        n += 1;
        format!("x{n}")
    })
    .unwrap();
    assert_eq!(opened.arrangement.sounds.len(), 1);
    assert_eq!(opened.arrangement.sounds[0].recipe.length, 4.0);
    assert_eq!(opened.warnings.len(), 2);
}
