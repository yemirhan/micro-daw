//! Headless Song-view scenes (phase 8): the states `scripts/parity-phase-8/steps.mjs` puts the
//! Electron app in, over the rock-song template (`mdaw-model/assets/templates/rock-song.json`, a copy of `src/data/templates/rock-song.json`).

use mdaw_model::Arrangement;
use mdaw_model::doc::EffectName;
use mdaw_model::{AutomationParameter, InstrumentType};
use serde_json::json;

use crate::app::{App, DawView, Menu};
use crate::scenes::started;
use crate::song::SongMenu;

/// The template the Song scenes open (`template.arrangement`).
pub const ROCK_SONG: &str = mdaw_model::templates_data::TEMPLATE_FILES[1].1;

/// Started, rock-song loaded, Song view.
pub fn song(s: &mut App) {
    started(s);
    let v: serde_json::Value = serde_json::from_str(ROCK_SONG).expect("template JSON");
    let arr: Arrangement = serde_json::from_value(v["arrangement"].clone()).expect("template");
    s.svc
        .engine
        .edit(|d| d.load_from_template(&arr))
        .expect("load template");
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
        d.set_track_effect(
            &b,
            EffectName::Filter,
            json!({"enabled": true, "cutoff": 2400})
                .as_object()
                .unwrap(),
        );
        d.set_track_effect(
            &b,
            EffectName::Reverb,
            json!({"enabled": true}).as_object().unwrap(),
        );
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

pub fn add_track_menu(s: &mut App) {
    song(s);
    s.menu = Some(Menu::Song(SongMenu::AddTrack));
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

/// The installed plugins of the track I/O scenes (a headless run loads none: they are listed).
pub fn io_catalog() -> Vec<mdaw_plugin_host::PluginInfo> {
    use mdaw_plugin_host::{PluginId, PluginInfo, PluginKind};
    let plugin = |kind: PluginKind, codes: [&str; 3], vendor: &str, name: &str| PluginInfo {
        id: PluginId::from_codes(codes[0], codes[1], codes[2]).expect("four-character codes"),
        kind,
        name: name.to_owned(),
        vendor: vendor.to_owned(),
        version: 1,
    };
    vec![
        plugin(
            PluginKind::Instrument,
            ["aumu", "Alav", "Artu"],
            "Arturia",
            "Analog Lab V",
        ),
        plugin(
            PluginKind::Effect,
            ["aufx", "Ni$R", "-NI-"],
            "Native Instruments",
            "Guitar Rig 6 FX",
        ),
    ]
}

/// Track I/O (Rust-only): an armed audio track on input 1 of a four-input interface with an
/// amp plugin and a recorded take, the Lead track played by a plugin instrument, the audio
/// track's FX drawer open.
pub fn track_io(s: &mut App) {
    song(s);
    *s.svc.plugins.0.borrow_mut() = crate::plugins::PluginManager::headless(io_catalog());
    s.p9.audio_devices.inputs = vec![mdaw_audio_host::InputDeviceInfo {
        name: "MiniFuse 4".into(),
        is_default: true,
        default_sample_rate: 44_100,
        channels: 4,
    }];
    let audio = s
        .svc
        .engine
        .edit(|d| d.add_track(InstrumentType::Audio, 0).id);
    // A take: two bars of a decaying strum pattern.
    let rate = s.svc.engine.sample_rate();
    let bpm = s.svc.engine.doc().get_bpm();
    let frames = (8.0 * 60.0 / bpm * rate as f64) as usize;
    let beat = 60.0 / bpm * rate as f64;
    let data: Vec<f32> = (0..frames)
        .map(|i| {
            let in_beat = (i as f64 % (beat / 2.0)) / (beat / 2.0);
            let t = i as f64 / rate as f64;
            (0.7 * (-4.0 * in_beat).exp() * (t * 196.0 * std::f64::consts::TAU).sin()) as f32
        })
        .collect();
    let sample = s.p9.samples.register_file(
        "Audio take 1",
        "",
        mdaw_dsp::buffer::AudioBuffer::mono(rate, data),
    );
    s.svc.engine.edit(|d| {
        d.add_audio_region(&audio, Some(&sample), 0.0);
        d.set_track_armed(&audio, true);
    });
    s.add_track_plugin(&audio, 0);
    let lead = track_id(s, "Lead");
    s.set_track_plugin_instrument(&lead, Some(0));
    s.svc.engine.edit(|d| {
        if let Some(id) = d
            .get_tracks()
            .iter()
            .find(|t| t.instrument.plugin.is_some())
            .map(|t| t.id.clone())
        {
            d.set_track_armed(&id, true);
        }
    });
    s.toggle_fx(&audio);
    // The new track is the last row: bring it into view.
    s.song.scroll[1] = f32::MAX;
    s.clamp_song_scroll();
}

/// The audio track's input select open.
pub fn input_menu(s: &mut App) {
    track_io(s);
    let audio = s
        .tracks()
        .iter()
        .find(|t| t.instrument.kind == InstrumentType::Audio)
        .map(|t| t.id.clone())
        .unwrap_or_default();
    s.song.fx_track = None;
    s.menu = Some(Menu::Song(SongMenu::Input { track: audio }));
}

/// The Track FX drawer's "Add plugin" list open.
pub fn plugin_menu(s: &mut App) {
    track_io(s);
    let audio = s
        .tracks()
        .iter()
        .find(|t| t.instrument.kind == InstrumentType::Audio)
        .map(|t| t.id.clone())
        .unwrap_or_default();
    s.menu = Some(Menu::Song(SongMenu::AddPlugin { track: audio }));
}
