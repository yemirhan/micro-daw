//! Headless scenes (`--snap out.png --scene NAME`): the states the phase-7 parity screenshots
//! compare with the Electron app (`scripts/parity-phase-7/`, same steps on both sides).

use mdaw_model::doc::LoopSettingsPatch;
use mdaw_model::types::{KitState, PadSample, PadSampleSource};

use crate::app::{App, Menu, StartPhase};

/// `init` of a headless run: the app at its start screen, offline engine.
pub fn headless_app() -> App {
    App::headless()
}

/// Past the start screen (`handleStart` done, intro gone).
pub fn started(s: &mut App) {
    s.start_session();
    s.start = StartPhase::Done;
    // The browser starts closed in the scenes (its Electron reference has no file system).
    s.browser_open = false;
}

/// The demo pattern of the screenshots: kick on beats, snare on 2 and 4, hats on 8ths
/// (one quiet hat), over the default 2 bars at 1/16.
pub fn demo_pattern(s: &mut App) {
    let sb = 0.25;
    for step in (0..32).step_by(4) {
        s.toggle_step(0, step, sb);
    }
    for step in [4, 12, 20, 28] {
        s.toggle_step(1, step, sb);
    }
    for step in (0..32).step_by(2) {
        s.toggle_step(2, step, sb);
    }
    s.set_step_velocity(2, 6, sb, 0.35);
    s.toggle_step(3, 14, sb);
    s.toggle_step(3, 30, sb);
}

/// A kit whose first four pads play a pack that isn't installed.
pub fn missing_pack_kit(s: &mut App) {
    let mut kit: KitState = s.kit();
    for (i, file) in ["kick.wav", "snare.wav", "hat.wav", "clap.wav"]
        .iter()
        .enumerate()
    {
        kit.pads[i].sample = Some(PadSample {
            name: format!("Uzu {}", ["Kick", "Snare", "Hat", "Clap"][i]),
            source: PadSampleSource::Pack {
                pack_id: "uzu-drumkit".into(),
                file: (*file).into(),
            },
        });
    }
    kit.name = "Uzu Kit".into();
    s.load_kit(&kit);
    s.svc.packs.load_catalog(false);
    let _ = s
        .svc
        .packs
        .settle(&mut s.svc.engine, std::time::Duration::from_secs(5));
}

/// A named headless state.
pub type Scene = (&'static str, fn(&mut App));

pub static SCENES: &[Scene] = &[
    ("start", |_| {}),
    ("beat", started),
    ("browser", |s| {
        started(s);
        s.browser_open = true;
    }),
    ("pad-selected", |s| {
        started(s);
        s.select_pad(12);
    }),
    ("pattern", |s| {
        started(s);
        demo_pattern(s);
    }),
    ("playing", |s| {
        started(s);
        demo_pattern(s);
        s.toggle_play();
    }),
    ("pad-menu", |s| {
        started(s);
        s.select_pad(0);
        s.menu = Some(Menu::Pad {
            pad: 0,
            x: 104.0,
            y: 415.0,
            renaming: false,
        });
    }),
    ("kit-menu", |s| {
        started(s);
        s.beat.kit_name = rune::TextEdit::new("");
        s.menu = Some(Menu::Kit);
    }),
    ("drum-track-menu", |s| {
        started(s);
        s.menu = Some(Menu::DrumTrack);
    }),
    ("quantize-menu", |s| {
        started(s);
        s.menu = Some(Menu::Quantize);
    }),
    ("chop", |s| {
        started(s);
        s.select_pad(0);
        s.menu = Some(Menu::Chop);
        s.recompute_slices();
    }),
    ("missing-packs", |s| {
        started(s);
        missing_pack_kit(s);
    }),
    ("count-in", |s| {
        started(s);
        s.toggle_record();
    }),
    ("recording", |s| {
        started(s);
        s.set_loop_settings(LoopSettingsPatch {
            count_in_bars: Some(0),
            ..Default::default()
        });
        demo_pattern(s);
        s.toggle_record();
    }),
    ("note-repeat", |s| {
        started(s);
        s.toggle_note_repeat();
    }),
    ("key-hints", |s| {
        started(s);
        s.toggle_key_hints();
    }),
    ("layout-2x8", |s| {
        started(s);
        s.set_layout(mdaw_model::beat_helpers::PadLayout::TwoByEight);
    }),
    ("dock-open", |s| {
        started(s);
        s.dock_open = true;
    }),
    ("edited-pad", |s| {
        started(s);
        s.select_pad(0);
        s.edit_pad(
            0,
            serde_json::json!({ "gainDb": -6.0, "pitch": 5, "pan": -0.4, "startSec": 0.05 }),
        );
        s.end_pad_edit();
    }),
];
