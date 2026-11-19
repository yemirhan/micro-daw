//! Regressions for the phase-3 review findings that need the whole engine: commands queued while
//! the core stalls, suppressions scoped to the take, punch closing at the transport callback's
//! time, track limits (live and export), and critical event overruns.

use serde_json::json;

use super::test_rig::*;
use super::{EngineEvent, ExportError, MAX_TRACKS, NoSamples, TransportState, render_export};
use crate::buffers::{FsReader, PadBufferCache};
use crate::core::{CaptureKind, Phase};
use mdaw_model::doc::LoopSettingsPatch;
use mdaw_model::{Arrangement, InstrumentType, PAD_COUNT, Track};
use std::sync::Arc;

/// Finding 1: core stalled (no render), 1100 commands, then Stop. Before the fix the ring (1024)
/// dropped the excess including Stop: the engine said Stopped while the core kept playing.
#[test]
fn stop_after_a_stall_reaches_the_core() {
    let mut r = Rig::new();
    r.drum_track();
    r.e.play();
    r.run(0.1);
    for i in 0..550 {
        r.e.handle().pad_down(i % PAD_COUNT, 0.5, None);
        r.e.handle().pad_up(i % PAD_COUNT, None);
    }
    r.e.stop();
    assert_eq!(r.e.get_state(), TransportState::Stopped);
    assert!(r.e.queued_commands() > 0);
    r.run(0.05); // one UI frame per quantum flushes the outbox
    assert_eq!(r.e.queued_commands(), 0);
    assert_eq!(r.e.shared().phase(), Phase::Stopped, "the core stopped too");
    let hits = r
        .captures
        .iter()
        .filter(|c| c.kind == CaptureKind::Hit)
        .count();
    assert_eq!(hits, 550, "no command dropped");
}

fn loop_take() -> (Rig, String) {
    let mut r = Rig::new();
    let id = r.drum_track();
    let p: LoopSettingsPatch = serde_json::from_value(
        json!({"bars": 1, "quantize": "1/16", "countInBars": 0, "swing": 0, "metronome": false}),
    )
    .unwrap();
    r.e.edit(|d| d.set_loop_settings(&p));
    r.e.start_loop_recording();
    r.run(0.01);
    (r, id)
}

/// Finding 5: a hit snapped forward across the loop end suppresses the pattern's replay of beat
/// 0 in this pass. Stop before beat 0 comes round, then Play Pattern at once: the first replay
/// must play (before the fix the suppression survived the take and swallowed it).
#[test]
fn a_suppression_does_not_survive_the_take() {
    let (mut r, id) = loop_take();
    r.hit_at_beat(3.95, 4, 0.8); // snapped to 4.0 → wraps to beat 0 of the pattern
    assert_eq!(r.pattern(&id), [(4, 0.0, 0.8)]);
    r.e.stop();
    r.run(0.003);
    let t_play = r.now();
    r.e.play_pattern();
    r.run(0.1);
    let replays: Vec<f64> = r
        .pattern_hits()
        .iter()
        .filter(|h| h.1 == 4 && h.2 >= t_play)
        .map(|h| h.2 - t_play)
        .collect();
    assert_eq!(replays.len(), 1, "the first replay plays: {replays:?}");
    assert!(replays[0] < 0.01);
}

/// Finding 9: Electron closes the notes still open at `loopEnd − 50 ms` with a transport
/// callback (`max(0.125, loopLen − start)`); a note-off between that callback and the loop end
/// then finds nothing open. Before the fix Rust closed at the wrap, so that note-off cut the note
/// to 2.97 beats instead of 3.
#[test]
fn punch_closes_open_notes_at_the_transport_callback_time() {
    let mut r = Rig::new();
    let id = r.drum_track();
    r.e.edit(|d| {
        d.toggle_step(&id, 0, 0, 0.25, None);
        d.set_loop_enabled(true);
        d.set_loop_markers(4.0, 8.0);
    });
    r.e.start_recording_to_track(Some(&id));
    r.run(0.01);
    let on = r.time_of_beat(5.0);
    r.run_until(on);
    r.e.capture_note_on(64.0, 0.5, Some(on));
    let off = r.time_of_beat(7.97); // after loopEnd − 50 ms (7.9), before the loop end
    r.run_until(off + 0.002);
    r.e.capture_note_off(64.0, Some(off));
    r.run(0.1);
    let region = r.e.stop_recording_to_track().unwrap();
    let n: Vec<(f64, f64, f64)> = region
        .notes
        .iter()
        .filter(|n| n.note == 64.0)
        .map(|n| (n.note, n.start_beat, n.duration_beats))
        .collect();
    assert_eq!(n.len(), 1);
    assert!((n[0].1 - 1.0).abs() < 1e-6);
    assert_eq!(n[0].2, 3.0, "closed by the punch callback: {n:?}");
}

/// Finding 10 (live): every document track gets a slot (before: 63, the 64th was the fallback
/// kit's); past the documented limit play refuses with an explicit error.
#[test]
fn more_than_64_tracks_play_and_the_limit_is_explicit() {
    let mut r = Rig::new();
    r.e.edit(|d| {
        for _ in 0..69 {
            d.add_track(InstrumentType::Synth, 0);
        }
    });
    let id = r.drum_track();
    assert_eq!(r.e.doc().get_tracks().len(), 70);
    r.e.edit(|d| d.toggle_step(&id, 3, 0, 0.25, None));
    r.e.play();
    r.run(0.2);
    assert!(
        r.pattern_hits().iter().any(|h| h.0 == id && h.1 == 3),
        "the 70th track plays"
    );
    assert_ne!(r.e.track_slot(&id), Some(r.e.fallback_slot()));
    r.e.stop();
    r.run(0.05);
    // past the limit: an explicit error, not silently dropped tracks
    r.e.edit(|d| {
        for _ in 0..(MAX_TRACKS + 1 - 70) {
            d.add_track(InstrumentType::Synth, 0);
        }
    });
    r.e.take_ui_events();
    r.e.play();
    assert_eq!(r.e.get_state(), TransportState::Stopped);
    let ev = r.e.take_ui_events();
    assert!(ev.contains(&EngineEvent::TrackLimitExceeded {
        tracks: MAX_TRACKS + 1,
        max: MAX_TRACKS
    }));
    assert!(r.e.check_track_limit().is_err());
}

fn export_track(id: &str, pad: usize, muted: bool) -> Track {
    let mut t: Track = serde_json::from_value(json!({
        "id": id, "name": id,
        "instrument": {"type": "drums", "presetIndex": 0},
        "regions": [{"id": "r", "startBeat": 0, "lengthBeats": 4, "notes": [
            {"note": 36 + pad, "velocity": 0.8, "startBeat": pad as f64 * 0.5, "durationBeats": 0.25, "isDrum": true}
        ]}],
        "volume": 0, "pan": 0, "muted": muted, "solo": false, "color": "#f00"
    }))
    .unwrap();
    t.instrument.kit = Some(test_kit(&[]));
    t
}

/// Finding 10 (export): muted tracks are skipped before slots are counted (before: 64 muted
/// tracks used up the 64 slots and the audible ones after them were not rendered).
#[test]
fn export_skips_muted_tracks_before_counting() {
    let cache = PadBufferCache::new(SR, None, Arc::new(FsReader), Arc::new(TestFiles::default()));
    let mut tracks: Vec<Track> = (0..64)
        .map(|i| export_track(&format!("m{i}"), 0, true))
        .collect();
    tracks.push(export_track("a", 1, false)); // note at 0.25 s
    tracks.push(export_track("b", 2, false)); // note at 0.5 s
    let mut arr: Arrangement =
        serde_json::from_value(json!({"id": "x", "name": "x", "bpm": 120, "lengthBeats": 4}))
            .unwrap();
    arr.tracks = tracks;
    let buf = render_export(&arr, SR, &cache, &NoSamples).unwrap();
    let peak = |a: f64, b: f64| {
        buf.channels[0][(a * SR as f64) as usize..(b * SR as f64) as usize]
            .iter()
            .fold(0f32, |m, x| m.max(x.abs()))
    };
    assert!(peak(0.0, 0.24) < 1e-3);
    assert!(peak(0.25, 0.27) > 0.05, "track a rendered");
    assert!(peak(0.5, 0.52) > 0.05, "track b rendered");
    // over the limit of unmuted tracks: an explicit error
    arr.tracks = (0..=MAX_TRACKS)
        .map(|i| export_track(&format!("t{i}"), 0, false))
        .collect();
    assert_eq!(
        render_export(&arr, SR, &cache, &NoSamples).unwrap_err(),
        ExportError::TooManyTracks {
            tracks: MAX_TRACKS + 1,
            max: MAX_TRACKS
        }
    );
}

/// Finding 11: captures lost to a full event ring are never silent: the take is marked
/// incomplete and the UI gets an error event (before: counted in `dropped_events` only).
#[test]
fn a_critical_event_overrun_marks_the_take_and_is_reported() {
    let (mut r, _) = loop_take();
    // the UI stalls (no tick) while 9000 hits (18000 captures) sound
    for chunk in 0..9 {
        for i in 0..1000 {
            r.live.pad_down((chunk + i) % PAD_COUNT, 0.5, None);
            r.live.pad_up((chunk + i) % PAD_COUNT, None);
        }
        for _ in 0..4 {
            r.live.flush();
            let (mut l, mut rr) = ([0f32; 128], [0f32; 128]);
            r.core.process(&mut l, &mut rr);
        }
    }
    assert!(r.e.shared().critical_overruns() > 0);
    assert!(!r.e.is_recording_incomplete());
    r.e.tick();
    let ev = r.e.take_ui_events();
    let lost = ev.iter().find_map(|e| match e {
        EngineEvent::CaptureOverrun { lost } => Some(*lost),
        _ => None,
    });
    assert_eq!(lost, Some(r.e.shared().critical_overruns()));
    assert!(r.e.is_recording_incomplete(), "the take is marked");
    // visual events are separate: their drops never count as capture losses
    r.e.tick();
    assert!(
        !r.e.take_ui_events()
            .iter()
            .any(|e| matches!(e, EngineEvent::CaptureOverrun { .. }))
    );
}
