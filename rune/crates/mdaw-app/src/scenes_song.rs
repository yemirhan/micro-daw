//! Headless Song-view scenes (phase 8): the states `scripts/parity-phase-8/steps.mjs` puts the
//! Electron app in, over the rock-song template (`src/data/templates/rock-song.json`).

use mdaw_model::Arrangement;
use mdaw_model::doc::EffectName;
use mdaw_model::{AutomationParameter, InstrumentType};
use serde_json::json;

use crate::app::{App, DawView, Menu};
use crate::scenes::started;
use crate::song::SongMenu;

/// The template the Song scenes open (`template.arrangement`).
pub const ROCK_SONG: &str = include_str!("../../../../src/data/templates/rock-song.json");

/// Started, rock-song loaded, Song view.
pub fn song(s: &mut App) {
    started(s);
    let v: serde_json::Value = serde_json::from_str(ROCK_SONG).expect("template JSON");
    let arr: Arrangement = serde_json::from_value(v["arrangement"].clone()).expect("template");
    s.svc.engine.edit(|d| d.load_from_template(&arr)).expect("load template");
    s.view = DawView::Song;
}

/// Track id by name.
pub fn track_id(s: &App, name: &str) -> String {
    s.tracks()
        .iter()
        .find(|t| t.name == name)
        .map(|t| t.id.clone())
        .unwrap_or_default()
}

/// The first region of a track.
pub fn first_region(s: &App, track: &str) -> String {
    s.find_track(track)
        .and_then(|t| t.regions.first())
        .map(|r| r.id.clone())
        .unwrap_or_default()
}

/// Selected drum region.
pub fn region_selected(s: &mut App) {
    song(s);
    let d = track_id(s, "Drums");
    let r = first_region(s, &d);
    s.select_region(&d, &r, false);
}

/// Split, duplicate and trim edits, then ⌘A.
pub fn regions_edited(s: &mut App) {
    song(s);
    let d = track_id(s, "Drums");
    let b = track_id(s, "Bass");
    let rd = first_region(s, &d);
    let rb = first_region(s, &b);
    s.svc.engine.edit(|doc| {
        doc.split_region(&d, &rd, 8.0);
        doc.duplicate_region(&b, &rb);
        doc.add_track(InstrumentType::Synth, 3);
    });
    s.select_all_regions();
}

/// A Bass volume lane with four points and a Lead pan lane.
pub fn automation(s: &mut App) {
    song(s);
    let b = track_id(s, "Bass");
    s.svc.engine.edit(|d| {
        d.add_automation_lane(&b, AutomationParameter::Volume);
        d.set_automation_point(&b, AutomationParameter::Volume, 0.0, 0.8, 1.0);
        d.set_automation_point(&b, AutomationParameter::Volume, 8.0, 0.3, 1.0);
        d.set_automation_point(&b, AutomationParameter::Volume, 16.0, 0.9, 1.0);
        d.set_automation_point(&b, AutomationParameter::Volume, 28.0, 0.5, 1.0);
    });
}

/// Loop 8..24 on, the template's markers.
pub fn markers_loop(s: &mut App) {
    song(s);
    s.svc.engine.edit(|d| {
        d.set_loop_markers(8.0, 24.0);
        d.set_loop_enabled(true);
    });
}

/// The track FX drawer on Bass, filter and reverb on.
pub fn track_fx(s: &mut App) {
    song(s);
    let b = track_id(s, "Bass");
    s.svc.engine.edit(|d| {
        d.set_track_effect(&b, EffectName::Filter, json!({"enabled": true, "cutoff": 2400}).as_object().unwrap());
        d.set_track_effect(&b, EffectName::Reverb, json!({"enabled": true}).as_object().unwrap());
    });
    s.toggle_fx(&b);
}

/// The piano roll on the Bass region.
pub fn piano_roll(s: &mut App) {
    song(s);
    let b = track_id(s, "Bass");
    let r = first_region(s, &b);
    s.open_piano_roll(&b, &r);
}

/// The piano roll with every note selected (velocity stems lit).
pub fn piano_velocity(s: &mut App) {
    piano_roll(s);
    let n = s.piano_region_notes().len();
    s.song.piano.selection = (0..n).collect();
}

/// The drum piano roll.
pub fn piano_drums(s: &mut App) {
    song(s);
    let d = track_id(s, "Drums");
    let r = first_region(s, &d);
    s.open_piano_roll(&d, &r);
}

/// Two keys held on the dock keyboard.
pub fn dock_keys(s: &mut App) {
    song(s);
    s.svc.with_router(|r, out| {
        r.note_on(60, 0.8, out);
        r.note_on(64, 0.8, out);
    });
}

pub fn mixer(s: &mut App) {
    song(s);
    s.song.show_mixer = true;
}

pub fn empty(s: &mut App) {
    song(s);
    let ids: Vec<String> = s.tracks().iter().map(|t| t.id.clone()).collect();
    s.svc.engine.edit(|d| {
        for id in &ids {
            d.remove_track(id);
        }
    });
}

pub fn automation_menu(s: &mut App) {
    song(s);
    let b = track_id(s, "Bass");
    s.menu = Some(Menu::Song(SongMenu::Automation { track: b }));
}

pub fn instrument_menu(s: &mut App) {
    song(s);
    let b = track_id(s, "Bass");
    s.menu = Some(Menu::Song(SongMenu::Instrument { track: b }));
}

pub fn region_menu(s: &mut App) {
    song(s);
    let d = track_id(s, "Drums");
    let r = first_region(s, &d);
    s.menu = Some(Menu::Song(SongMenu::Region {
        x: 420.0,
        y: 160.0,
        track: d,
        region: r,
    }));
}

pub fn grouped(s: &mut App) {
    song(s);
    let d = track_id(s, "Drums");
    let b = track_id(s, "Bass");
    s.svc.engine.edit(|doc| {
        doc.create_group("Rhythm", &[d, b]);
    });
}

pub fn playing(s: &mut App) {
    song(s);
    s.song_play();
}
