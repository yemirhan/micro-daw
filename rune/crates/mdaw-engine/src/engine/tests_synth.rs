//! Synth tracks and the live instrument through the engine + core (phase 4 wiring):
//! - tracks: pattern notes via `triggerAttackRelease` (the tempo at event time, R2-11),
//!   `setTrackInstrument` / `setTrackSynthOverrides` rebuilds while playing, mute, Stop releasing;
//! - the live instrument: a port of the non-FX cases of `src/services/AudioEngine.test.ts`, and
//!   the engine side of `src/test/stuckNotes.test.tsx` (one voice per pitch, every voice
//!   released: checked on the rendered audio, not only on the counters).
//!
//! The sound itself is compared with Chromium in `synth/parity_tests.rs`, `piano/parity_tests.rs`,
//! `character/parity_tests.rs` and, end to end, by the WAV export parity (`tests_export.rs`).

use mdaw_model::{InstrumentType, Region, RegionNote};
use serde_json::{Map, Value, json};

use super::test_rig::*;
use super::{LIVE_OFFSET, RETIRE_MS, RETIRE_RELEASE_DELAY};
use crate::core::SynthNoteOp;
use crate::synth::bank::synth_presets;
use crate::synth::presets::SynthPreset;

fn note(n: f64, start: f64, dur: f64) -> RegionNote {
    RegionNote {
        note: n,
        velocity: 0.8,
        start_beat: start,
        duration_beats: dur,
        is_drum: false,
    }
}

/// A synth track (−6 dB) with `notes` in one region.
fn synth_track(r: &mut Rig, preset: i64, notes: Vec<RegionNote>) -> String {
    let id = r.e.edit(|d| d.add_track(InstrumentType::Synth, preset)).id;
    let region = Region {
        id: format!("{id}-r"),
        start_beat: 0.0,
        length_beats: 16.0,
        notes,
        color: None,
        name: None,
        audio: None,
        extra: Default::default(),
    };
    r.e.edit(|d| d.add_region(&id, region));
    id
}

fn obj(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

#[test]
fn a_synth_track_plays_its_notes_through_its_strip() {
    let mut r = Rig::new();
    synth_track(&mut r, 5, vec![note(60.0, 1.0, 0.5)]);
    r.e.play();
    let t0 = r.time_of_beat(0.0);
    let t1 = r.time_of_beat(1.0);
    r.run_until(t1 + 1.5);
    assert!(r.rms(t0, t1 - 0.01) < 1e-4, "silent before the note");
    let on = r.onset_after(t0, 1e-3).expect("the note sounds");
    assert!((on - t1).abs() < 0.01, "onset {on} vs {t1}");
    assert!(r.rms(t1, t1 + 0.1) > 1e-3);
    // synth-pluck: 0.25 s note, then a short release
    assert!(r.rms(t1 + 1.2, t1 + 1.5) < r.rms(t1, t1 + 0.1) * 1e-2);
}

#[test]
fn note_lengths_use_the_tempo_at_event_time() {
    // drawbar organ: sustain 1, release 60 ms; a 1-beat note at beat 4
    let level_late = |slow: bool| {
        let mut r = Rig::new();
        synth_track(&mut r, 9, vec![note(60.0, 4.0, 1.0)]);
        r.e.play();
        r.run_until(r.time_of_beat(0.0) + 0.5);
        if slow {
            r.e.edit(|d| d.set_bpm(60.0));
        }
        r.run(0.05);
        let t4 = r.time_of_beat(4.0);
        r.run_until(t4 + 1.2);
        (r.rms(t4 + 0.05, t4 + 0.25), r.rms(t4 + 0.75, t4 + 0.95))
    };
    let (fast_on, fast_late) = level_late(false); // 0.5 s note
    let (slow_on, slow_late) = level_late(true); // 1 s note
    assert!(fast_on > 1e-3 && slow_on > 1e-3);
    assert!(slow_late > slow_on * 0.5, "still held at 60 BPM");
    assert!(fast_late < fast_on * 0.1, "released at 120 BPM");
}

#[test]
fn synth_overrides_and_instrument_changes_rebuild_the_track_while_playing() {
    let mut r = Rig::new();
    let id = synth_track(&mut r, 9, vec![note(60.0, 0.0, 64.0)]);
    r.e.play();
    let t0 = r.time_of_beat(0.0);
    r.run_until(t0 + 0.5);
    let loud = r.rms(t0 + 0.3, t0 + 0.5);
    r.e.edit(|d| d.set_track_synth_overrides(&id, &obj(json!({ "gain": -16.0 }))));
    assert_eq!(r.e.get_state(), super::TransportState::Playing);
    // withPlaybackRestart: play resumes from the same beat; the held note restarts with it
    // (its start lies before the resume beat, Tone's Part does not replay it: silence)
    r.run(0.3);
    let t = r.now();
    r.e.stop();
    r.run(0.2);
    r.e.play();
    let t2 = r.time_of_beat(0.0);
    r.run_until(t2 + 0.5);
    let quiet = r.rms(t2 + 0.3, t2 + 0.5);
    let ratio_db = 20.0 * (quiet / loud).log10();
    assert!(
        (ratio_db + 20.0).abs() < 1.0,
        "gain 4 → −16 dB: {ratio_db:.2} dB ({t})"
    );
    // a sampler preset: the engine starts decoding the shared piano samples
    let inst =
        r.e.doc()
            .get_tracks()
            .iter()
            .find(|t| t.id == id)
            .unwrap()
            .instrument
            .clone();
    let mut piano = inst.clone();
    piano.preset_index = 0;
    r.e.edit(|d| d.set_track_instrument(&id, &piano));
    assert!(r.e.piano.is_some());
    r.e.stop();
    r.run(0.1);
    r.e.play();
    r.run(0.5);
    assert!(
        r.rms(r.now() - 0.3, r.now()) > 1e-4,
        "the piano (or its fallback) plays"
    );
}

#[test]
fn stop_releases_synth_notes_and_muted_tracks_are_silent() {
    let mut r = Rig::new();
    let id = synth_track(&mut r, 3, vec![note(60.0, 0.0, 32.0)]);
    r.e.play();
    let t0 = r.time_of_beat(0.0);
    r.run_until(t0 + 1.0);
    assert!(r.rms(t0 + 0.8, t0 + 1.0) > 1e-3);
    r.e.stop();
    let ts = r.now();
    r.run(3.0);
    // warm-pad release 0.8 s (+ strip leaks)
    assert!(r.rms(ts + 2.5, ts + 3.0) < 1e-4);
    r.e.edit(|d| d.set_track_mute(&id, true));
    r.e.play();
    let t1 = r.time_of_beat(0.0);
    r.run_until(t1 + 1.0);
    assert!(r.rms(t1, t1 + 1.0) < 1e-5);
}

// --- The live instrument (AudioEngine.test.ts, non-FX parts) -----------------------------------

/// Long enough for a note command to be applied and published (4 ms + a quantum or two).
const SETTLE: f64 = 0.02;

fn live_rig() -> Rig {
    let mut r = Rig::new();
    r.e.live().start();
    r.run(SETTLE);
    r
}

#[test]
fn note_on_note_off_and_the_active_voice_count() {
    let mut r = live_rig();
    r.e.live().set_preset(1.0);
    r.e.live().note_on(60.0, 0.8);
    r.e.live().note_on(64.0, 0.8);
    r.run(SETTLE);
    assert_eq!(r.e.live().held_notes(), vec![60, 64]);
    assert_eq!(r.e.live().active_voice_count(), 2);
    r.e.live().note_off(60.0);
    r.e.live().note_off(64.0);
    r.run(SETTLE);
    assert!(r.e.live().held_notes().is_empty());
    r.run(5.0);
    assert_eq!(r.e.live().active_voice_count(), 0);
    assert!(r.rms(r.now() - 0.5, r.now()) < 1e-5, "nothing sounding");
}

#[test]
fn re_triggering_a_sounding_note_does_not_leak_a_voice() {
    let mut r = live_rig();
    r.e.live().set_preset(3.0);
    for _ in 0..3 {
        r.e.live().note_on(60.0, 0.8);
    }
    r.run(SETTLE);
    assert_eq!(r.e.live().active_voice_count(), 1);
    r.e.live().note_off(60.0);
    r.run(10.0);
    assert_eq!(r.e.live().active_voice_count(), 0);
    assert!(r.rms(r.now() - 0.5, r.now()) < 1e-5);
}

#[test]
fn release_all_silences_voices_whose_note_off_was_lost() {
    let mut r = live_rig();
    for idx in [0.0, 1.0, 7.0, 13.0, 20.0, 28.0] {
        r.e.live().set_preset(idx);
        r.e.live().note_on(48.0, 0.9);
        r.e.live().note_on(55.0, 0.9);
        r.e.live().release_all();
        r.run(SETTLE);
        assert!(r.e.live().held_notes().is_empty(), "preset {idx}");
        r.run(0.1);
        assert_eq!(r.e.live().active_voice_count(), 0, "preset {idx}");
    }
    r.run((RETIRE_MS / 1000.0) + 2.5);
    assert!(
        r.rms(r.now() - 0.3, r.now()) < 1e-5,
        "nothing left sounding"
    );
}

#[test]
fn note_off_is_honoured_while_muted() {
    let mut r = live_rig();
    r.e.live().set_preset(1.0);
    r.e.live().note_on(60.0, 0.8);
    r.run(SETTLE);
    r.e.live().mute();
    r.e.live().note_off(60.0);
    r.e.live().unmute();
    r.run(SETTLE);
    assert!(r.e.live().held_notes().is_empty());
    assert!(!r.e.live_muted());
    r.run(3.0);
    assert!(r.rms(r.now() - 0.3, r.now()) < 1e-5);
}

#[test]
fn switching_presets_with_held_keys_leaves_nothing_held_or_sounding() {
    let mut r = live_rig();
    r.e.live().set_preset(6.0);
    r.e.live().note_on(60.0, 0.8);
    r.run(0.3);
    let t = r.now();
    r.e.live().set_preset(9.0);
    r.e.live().note_off(60.0); // reaches the new instrument: harmless no-op
    r.run(0.15);
    assert!(r.e.live().held_notes().is_empty());
    assert_eq!(r.e.live().preset_index(), 9);
    // the old instrument rings until its release (Tone's now() = +100 ms), then fades
    assert!(
        r.rms(t + 0.03, t + RETIRE_RELEASE_DELAY) > 1e-3,
        "still sounding"
    );
    r.run(RETIRE_MS / 1000.0 + 1.0);
    assert!(r.rms(r.now() - 0.3, r.now()) < 1e-5, "retired");
    r.e.tick();
    assert_eq!(r.e.queued_commands(), 0);
}

#[test]
fn every_preset_can_be_selected_and_played() {
    let mut r = live_rig();
    for i in 0..synth_presets().len() {
        r.e.live().set_preset(i as f64);
        r.e.live().note_on(60.0, 0.9);
        r.e.live().note_off(60.0);
        r.run(SETTLE);
        assert!(r.e.live().held_notes().is_empty(), "preset {i}");
    }
    r.run(10.0);
    assert_eq!(r.e.live().active_voice_count(), 0);
}

#[test]
fn rejects_invalid_preset_indices() {
    let mut r = live_rig();
    r.e.live().set_preset(2.0);
    for bad in [-1.0, 999.0, 1.5, f64::NAN] {
        r.e.live().set_preset(bad);
    }
    assert_eq!(r.e.live().preset_index(), 2);
}

#[test]
fn set_synth_param_merges_dotted_paths_and_objects_into_the_current_preset() {
    let mut r = live_rig();
    r.e.live().set_preset(1.0);
    r.e.live().set_synth_param("filter.cutoff", json!(1234));
    r.e.live().set_synth_param("amp", json!({ "release": 1.1 }));
    let SynthPreset::Subtractive(p) = r.e.live().current_preset() else {
        panic!()
    };
    assert_eq!(p.filter.cutoff, 1234.0);
    assert_eq!(p.amp.release, 1.1);
    assert_eq!(
        Value::Object(r.e.live().synth_overrides()),
        json!({ "filter": { "cutoff": 1234 }, "amp": { "release": 1.1 } })
    );
    r.e.live().set_preset(1.0); // reset
    assert!(r.e.live().synth_overrides().is_empty());
}

#[test]
fn set_synth_param_applies_live_to_a_held_note_without_rebuilding() {
    let mut r = live_rig();
    r.e.live().set_preset(9.0);
    r.e.live().note_on(60.0, 0.8);
    r.run(0.4);
    let loud = r.rms(r.now() - 0.2, r.now());
    r.e.live().set_synth_param("gain", json!(-16.0));
    r.run(0.4);
    assert_eq!(
        r.e.live().held_notes(),
        vec![60],
        "the same voice keeps playing"
    );
    let quiet = r.rms(r.now() - 0.2, r.now());
    let db = 20.0 * (quiet / loud).log10();
    // −20 dB, less the live reverb tail of the louder part (and the organ's tremolo)
    assert!(
        (-21.0..-17.0).contains(&db),
        "gain 4 → −16 dB live: {db:.2} dB"
    );
}

#[test]
fn live_notes_play_at_the_audio_clock_plus_4_ms_through_the_master_bus() {
    let mut r = live_rig();
    r.e.live().set_preset(9.0);
    r.run(2.0);
    let t = r.now();
    r.e.live().note_on(60.0, 0.8);
    r.run(0.1);
    let on = r.onset_after(t, 1e-4).expect("sounds");
    assert!(on >= t + LIVE_OFFSET - 1.0 / SR as f64, "{on} vs {t}");
    assert!(on < t + LIVE_OFFSET + 0.004, "{on} vs {t}");
    // the master bus: +2 dB drive and the soft clipper (peaks stay under 0.98)
    r.e.live().set_volume(0.0);
    for k in 0..12 {
        r.e.live().note_on(36.0 + 4.0 * k as f64, 1.0);
    }
    r.run(0.5);
    assert!(
        r.left
            .iter()
            .chain(&r.right)
            .all(|v| v.abs() <= 0.98 + 1e-6)
    );
}

#[test]
fn notes_before_start_are_ignored() {
    let mut r = Rig::new();
    r.e.live().note_on(60.0, 0.8);
    r.run(0.2);
    assert!(r.e.live().held_notes().is_empty());
    assert!(r.rms(0.0, r.now()) < 1e-6);
    assert!(!r.e.live().is_started());
}

// --- stuckNotes (engine side) -------------------------------------------------------------------

#[test]
fn duplicate_note_on_shares_one_voice_and_one_note_off_silences_it() {
    let mut r = live_rig();
    r.e.live().set_preset(10.0);
    r.e.live().note_on(60.0, 0.8);
    r.e.live().note_on(60.0, 0.8); // listener attached twice
    r.run(SETTLE);
    assert_eq!(r.e.live().active_voice_count(), 1);
    r.e.live().note_off(60.0);
    r.run(2.0);
    assert!(r.rms(r.now() - 0.3, r.now()) < 1e-5);
}

#[test]
fn midi_thread_and_ui_notes_on_the_same_pitch_share_one_voice() {
    let mut r = live_rig();
    r.e.live().set_preset(10.0);
    r.run(SETTLE);
    let slot = r.e.live_synth_slot();
    r.live.synth_note(
        slot,
        SynthNoteOp::On {
            note: 64.0,
            velocity: 0.8,
        },
        LIVE_OFFSET,
    );
    r.e.live().note_on(64.0, 0.7); // the mouse on the same key
    r.run(SETTLE);
    assert_eq!(r.e.live().active_voice_count(), 1);
    r.e.live().note_off(64.0);
    r.live
        .synth_note(slot, SynthNoteOp::Off { note: 64.0 }, LIVE_OFFSET);
    r.run(2.0);
    assert!(r.e.live().held_notes().is_empty());
    assert!(r.rms(r.now() - 0.3, r.now()) < 1e-5);
}

#[test]
fn all_notes_off_silences_everything_and_later_note_offs_are_harmless() {
    let mut r = live_rig();
    r.e.live().set_preset(26.0); // mono legato lead: the stack must empty too
    r.e.live().note_on(60.0, 0.8);
    r.e.live().note_on(64.0, 0.8);
    r.run(0.2);
    r.e.live().release_all();
    r.run(SETTLE);
    assert!(r.e.live().held_notes().is_empty());
    r.e.live().note_off(60.0);
    r.e.live().note_on(67.0, 0.8);
    r.e.live().release_all();
    r.run(2.0);
    assert_eq!(r.e.live().active_voice_count(), 0);
    assert!(r.rms(r.now() - 0.3, r.now()) < 1e-5);
}

#[test]
fn a_song_recording_on_a_synth_track_plays_back_through_its_synth() {
    let mut r = Rig::new();
    let id = synth_track(&mut r, 9, vec![]);
    r.e.edit(|d| d.arm_track_for_recording(Some(&id)));
    r.e.live().start();
    r.e.start_recording_to_track(None);
    r.run(0.01);
    let t = r.time_of_beat(1.0);
    r.run_until(t);
    // the keyboard path: the live instrument plays, the engine captures
    r.e.live().note_on(64.0, 0.8);
    r.e.capture_note_on(64.0, 0.8, Some(t));
    r.run_until(t + 0.5);
    r.e.live().note_off(64.0);
    r.e.capture_note_off(64.0, Some(t + 0.5));
    r.run_until(t + 0.8);
    assert!(r.rms(t + 0.1, t + 0.4) > 1e-3, "heard live while recording");
    let region = r.e.stop_recording_to_track().unwrap();
    assert_eq!(region.notes.len(), 1);
    r.run(1.0);
    r.e.play();
    let b1 = r.time_of_beat(1.0);
    r.run_until(b1 + 0.6);
    assert!(r.rms(b1 - 0.2, b1 - 0.05) < 1e-4);
    assert!(
        r.rms(b1 + 0.1, b1 + 0.4) > 1e-3,
        "the recorded note plays back"
    );
}
