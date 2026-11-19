//! Live pads: the drum sink (ArrangementEngine.beat.test.ts "drum tracks and the active drum
//! track", #3), DrumEngine.test.ts (routing, fallback kit, held pads across a sink switch,
//! release cancelling repeats handed out ahead) and useMidiNotes' pad release order
//! (useMidiNotes.routing.test.ts "note repeat"), on the real engine + core.

use serde_json::json;

use super::EngineEvent;
use super::test_rig::*;
use crate::core::CaptureKind;
use mdaw_model::InstrumentType;

fn level(r: &Rig, slot: usize) -> f32 {
    r.e.shared().slot_mean_square(slot)
}

/// Peak mean-square of `slot` over the next `seconds`.
fn slot_peak(r: &mut Rig, slot: usize, seconds: f64) -> f32 {
    let mut m = 0f32;
    let n = (seconds * SR as f64 / 128.0).ceil() as usize;
    for _ in 0..n {
        r.run(128.0 / SR as f64);
        m = m.max(level(r, slot));
    }
    m
}

#[test]
fn a_new_drum_track_gets_the_factory_kit_becomes_active_and_plays_live_hits() {
    let mut r = Rig::new();
    assert_eq!(r.e.doc().get_active_drum_track_id(), None);
    assert_eq!(r.e.live_sink_slot(), Some(r.e.fallback_slot()));
    let id = r.e.edit(|d| d.ensure_drum_track());
    assert_eq!(
        r.e.doc().get_active_drum_track_id().as_deref(),
        Some(id.as_str())
    );
    let kit = r.e.doc().get_kit(&id);
    assert_eq!(kit.pads.len(), 16);
    assert_eq!(
        serde_json::to_value(&kit.pads[0].sample.as_ref().unwrap().source).unwrap(),
        json!({"kind": "factory", "voice": "kick"})
    );
    assert_eq!(r.e.edit(|d| d.ensure_drum_track()), id);
    let slot = r.e.track_slot(&id).unwrap();
    assert_eq!(r.e.live_sink_slot(), Some(slot));
    r.e.wait_kits_ready();
    r.pad_down(0, 1.0, None);
    assert!(
        slot_peak(&mut r, slot, 0.05) > 1e-4,
        "the factory kick sounds on the track"
    );
    assert_eq!(level(&r, r.e.fallback_slot()), 0.0);
}

#[test]
fn the_sink_follows_the_active_track_and_falls_back_when_it_is_removed() {
    let mut r = Rig::new();
    let a = r.e.edit(|d| d.add_track(InstrumentType::Drums, 0)).id;
    let b = r.e.edit(|d| d.add_track(InstrumentType::Drums, 0)).id;
    let (sa, sb) = (r.e.track_slot(&a).unwrap(), r.e.track_slot(&b).unwrap());
    assert_eq!(r.e.live_sink_slot(), Some(sa));
    r.e.edit(|d| d.set_active_drum_track(&b));
    assert_eq!(r.e.live_sink_slot(), Some(sb));
    r.e.wait_kits_ready();
    r.pad_down(0, 1.0, None);
    r.run(0.03);
    assert!(level(&r, sb) > 0.0 || slot_peak(&mut r, sb, 0.02) > 0.0);
    assert_eq!(level(&r, sa), 0.0);
    r.e.edit(|d| d.remove_track(&b));
    assert_eq!(
        r.e.doc().get_active_drum_track_id().as_deref(),
        Some(a.as_str())
    );
    assert_eq!(r.e.live_sink_slot(), Some(sa));
    r.e.edit(|d| d.remove_track(&a));
    assert_eq!(r.e.doc().get_active_drum_track_id(), None);
    assert_eq!(r.e.live_sink_slot(), Some(r.e.fallback_slot()));
}

#[test]
fn the_sink_is_rebound_after_undo_rebuilds_the_track_audio() {
    let mut r = Rig::new();
    let id = r.drum_track();
    let p = json!({"gainDb": -3}).as_object().unwrap().clone();
    r.e.edit(|d| d.update_pad(&id, 0, &p));
    r.e.edit(|d| d.perform_undo());
    let slot = r.e.track_slot(&id).unwrap();
    assert_eq!(r.e.live_sink_slot(), Some(slot));
    r.e.wait_kits_ready();
    r.pad_down(1, 1.0, None);
    assert!(slot_peak(&mut r, slot, 0.03) > 1e-4);
}

#[test]
fn without_a_drum_track_live_hits_play_the_fallback_factory_kit_at_minus_6_db() {
    let mut r = Rig::new();
    r.e.wait_kits_ready();
    let fb = r.e.fallback_slot();
    r.pad_down(0, 0.8, None);
    assert!(slot_peak(&mut r, fb, 0.05) > 1e-4);
    // Stop leaves the fallback kit alone (it is not a track)
    r.pad_down(3, 0.8, None);
    r.run(0.003);
    r.e.stop();
    assert!(
        slot_peak(&mut r, fb, 0.02) > 1e-5,
        "still ringing after Stop"
    );
}

#[test]
fn r2_3_switching_the_active_drum_track_releases_the_pads_held_on_the_old_sink() {
    let mut r = Rig::new();
    let a = r.drum_track();
    let p = json!({"mode": "gate", "decay": 0.01})
        .as_object()
        .unwrap()
        .clone();
    let kit = test_kit(&[6, 7]);
    r.e.edit(|d| {
        d.set_kit(&a, &kit, Default::default());
        d.update_pad(&a, 6, &p);
        d.update_pad(&a, 7, &p);
    });
    let b = r.e.edit(|d| d.add_track(InstrumentType::Drums, 0)).id;
    r.e.wait_kits_ready();
    let sa = r.e.track_slot(&a).unwrap();
    r.pad_down(6, 1.0, None);
    r.run(0.1);
    assert!(level(&r, sa) > 1e-3, "the held gate pad sounds on A");
    r.e.edit(|d| d.set_active_drum_track(&b));
    r.run(0.1);
    assert!(level(&r, sa) < 1e-9, "released when the sink switched");
    // the late note-off is harmless and nothing new is released on A
    r.pad_up(6, None);
    r.run(0.02);
    assert!(level(&r, sa) < 1e-9);
}

#[test]
fn a_held_gate_is_released_on_the_track_that_played_it_after_the_sink_changed() {
    // DrumEngine.test "a held pad is released on the sink that played it" (core: pad targets)
    let mut r = Rig::new();
    let a = r.drum_track();
    let p = json!({"mode": "gate", "decay": 0.01})
        .as_object()
        .unwrap()
        .clone();
    let kit = test_kit(&[4]);
    r.e.edit(|d| {
        d.set_kit(&a, &kit, Default::default());
        d.update_pad(&a, 4, &p);
    });
    let b = r.e.edit(|d| d.add_track(InstrumentType::Drums, 0)).id;
    r.e.edit(|d| {
        d.set_kit(&b, &kit, Default::default());
        d.update_pad(&b, 4, &p);
    });
    r.e.wait_kits_ready();
    let (sa, sb) = (r.e.track_slot(&a).unwrap(), r.e.track_slot(&b).unwrap());
    // the next press/release cycle belongs to B only
    r.e.edit(|d| d.set_active_drum_track(&b));
    r.pad_down(4, 1.0, None);
    r.run(0.1);
    assert!(level(&r, sb) > 1e-3 && level(&r, sa) == 0.0);
    r.pad_up(4, None);
    r.run(0.05);
    assert!(level(&r, sb) < 1e-9);
}

#[test]
fn pad_release_order_repeats_heard_before_the_release_are_captured_first() {
    // useMidiNotes: release drops repeats not heard yet and settles the ones already due first;
    // the note-off capture comes after them; a stray second release is harmless.
    let mut r = Rig::new();
    r.drum_track();
    r.e.handle().set_note_repeat(true, 0.25);
    let t = r.now() + 0.01;
    r.pad_down(2, 0.9, Some(t));
    r.pad_down(3, 0.5, Some(t));
    // free-running repeats every 1/16 at 120 BPM = 125 ms; release pad 2 after 2 repeats
    r.pad_up(2, Some(t + 0.3));
    r.run(0.6);
    r.pad_up(3, None);
    r.pad_up(3, None);
    r.run(0.05);
    let caps: Vec<(usize, CaptureKind, f64)> =
        r.captures.iter().map(|c| (c.pad, c.kind, c.time)).collect();
    let pad2: Vec<_> = caps.iter().filter(|c| c.0 == 2).collect();
    let kinds: Vec<CaptureKind> = pad2.iter().map(|c| c.1).collect();
    assert_eq!(
        kinds,
        [
            CaptureKind::Hit,
            CaptureKind::RepeatHit,
            CaptureKind::RepeatHit,
            CaptureKind::Release
        ]
    );
    assert!(
        pad2.iter().all(|c| c.2 <= t + 0.3 + 1e-9),
        "nothing after the release"
    );
    assert!(
        caps.iter()
            .filter(|c| c.0 == 3 && c.1 == CaptureKind::RepeatHit)
            .count()
            >= 4
    );
    let hits = r
        .events
        .iter()
        .filter(|e| matches!(e, EngineEvent::PadHit { pad: 2, .. }))
        .count();
    assert_eq!(hits, 3, "flashes only for what was heard");
}

#[test]
fn live_hits_sound_at_their_exact_frame() {
    let mut r = Rig::new();
    r.drum_track();
    let t = r.now() + 0.0123;
    r.pad_down(0, 1.0, Some(t));
    r.run(0.05);
    let onset = r.onset_after(0.0, 1e-4).unwrap();
    assert_eq!((onset * SR as f64).round(), (t * SR as f64).ceil());
}
