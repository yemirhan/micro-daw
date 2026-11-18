//! Effects through the engine (phase 4): `setTrackEffect` → `applyTrackEffect`, play →
//! `resetTrackEffects` + `scheduleAutomation` (fired on the core's transport), track volume /
//! mute through the strip, the master fader ramp, `getTrackLevel` / `getMasterLevel`, and the
//! live bus's legacy CCs. The DSP itself is compared with Chromium in `fx/parity_tests.rs`;
//! these check the wiring on the real engine + core.

use serde_json::{Map, Value, json};

use super::test_rig::*;
use crate::fx::cc::CcAction;
use crate::fx::live::LiveOp;
use mdaw_model::doc::EffectName;
use mdaw_model::types::AutomationParameter;

/// A drum track (new tracks are at −6 dB) whose pad 0 is 2 s of DC 0.3, hit once on beat 0.
fn dc_track(r: &mut Rig) -> String {
    let id = r.e.edit(|d| d.ensure_drum_track());
    r.e.edit(|d| {
        d.set_kit(&id, &test_kit(&[0]), Default::default());
        d.toggle_step(&id, 0, 0, 0.25, None);
        d.clear_history();
    });
    r.e.wait_kits_ready();
    id
}

fn params(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

/// Output RMS over [t, t + 20 ms).
fn level_at(r: &Rig, t: f64) -> f64 {
    r.rms(t, t + 0.02)
}

/// The DC track's steady output level as it is now (one play from beat 0, then stop).
fn reference(r: &mut Rig) -> f64 {
    r.e.play();
    let t0 = r.time_of_beat(0.0);
    r.run_until(t0 + 0.2);
    let v = level_at(r, t0 + 0.1);
    r.e.stop();
    r.run(0.05);
    assert!(v > 0.01, "{v}");
    v
}

#[test]
fn volume_automation_ramps_on_the_transport_and_play_resets_it() {
    let mut r = Rig::new();
    let id = dc_track(&mut r);
    r.e.edit(|d| d.set_track_volume(&id, 0.0));
    let dc = reference(&mut r);
    r.e.edit(|d| {
        d.add_automation_lane(&id, AutomationParameter::Volume);
        // 0 dB at beat 0 → −20 dB at beat 2 (linear in gain: 1 → 0.1 over 1 s at 120 BPM)
        d.set_automation_point(&id, AutomationParameter::Volume, 0.0, 1.0, 0.0);
        d.set_automation_point(&id, AutomationParameter::Volume, 2.0, 0.5, 0.0);
    });
    r.e.play();
    let t0 = r.time_of_beat(0.0);
    r.run_until(t0 + 1.5);
    let early = level_at(&r, t0 + 0.05);
    let mid = level_at(&r, t0 + 0.5);
    let late = level_at(&r, t0 + 1.2);
    assert!((early / dc - 0.95).abs() < 0.05, "{early} vs {dc}");
    assert!((mid / dc - 0.55).abs() < 0.05, "{mid}");
    assert!((late / dc - 0.1).abs() < 0.01, "{late}");
    // a new play resets the strip (`resetTrackEffects`) before the automation starts again
    r.e.stop();
    r.run(0.1);
    r.e.play();
    let t1 = r.time_of_beat(0.0);
    r.run_until(t1 + 0.2);
    assert!((level_at(&r, t1 + 0.05) / dc - 0.95).abs() < 0.05);
}

#[test]
fn hidden_lanes_are_silent() {
    let mut r = Rig::new();
    let id = dc_track(&mut r);
    let dc = reference(&mut r);
    r.e.edit(|d| {
        d.add_automation_lane(&id, AutomationParameter::Volume);
        d.set_automation_point(&id, AutomationParameter::Volume, 0.0, 0.0, 0.0);
        d.toggle_automation_lane_visibility(&id, AutomationParameter::Volume);
    });
    r.e.play();
    let t0 = r.time_of_beat(0.0);
    r.run_until(t0 + 0.3);
    assert!((level_at(&r, t0 + 0.1) / dc - 1.0).abs() < 0.02);
}

#[test]
fn a_track_effect_change_reaches_the_strip() {
    let mut r = Rig::new();
    let id = dc_track(&mut r);
    let dc = reference(&mut r);
    r.e.play();
    let t0 = r.time_of_beat(0.0);
    r.run_until(t0 + 0.3);
    assert!((level_at(&r, t0 + 0.2) / dc - 1.0).abs() < 0.02);
    // reverb fully wet: its send has a 200 Hz highpass, so the DC disappears
    r.e.edit(|d| {
        d.set_track_effect(
            &id,
            EffectName::Reverb,
            &params(json!({"enabled": true, "wet": 1.0})),
        )
    });
    r.run(0.5);
    let wet = level_at(&r, r.now() - 0.05);
    assert!(wet < 0.01 * dc, "{wet}");
}

#[test]
fn volume_and_mute_go_through_the_strip_and_a_volume_change_keeps_mute() {
    let mut r = Rig::new();
    let id = dc_track(&mut r);
    let dc = reference(&mut r); // at the new track's −6 dB
    r.e.play();
    let t0 = r.time_of_beat(0.0);
    r.run_until(t0 + 0.1);
    r.e.edit(|d| d.set_track_volume(&id, -26.0));
    r.run(0.1);
    let v = level_at(&r, r.now() - 0.03);
    assert!((v / dc - 0.1).abs() < 0.005, "{v} vs {dc}");
    r.e.edit(|d| d.set_track_mute(&id, true));
    r.run(0.05);
    // Tone: `volume.volume.value = db` on a muted Volume un-mutes it (fx_chains.json,
    // strip_mute_volume); here mute is a flag and stays.
    r.e.edit(|d| d.set_track_volume(&id, -6.0));
    r.run(0.1);
    assert_eq!(level_at(&r, r.now() - 0.03), 0.0);
}

#[test]
fn master_fader_ramps_in_30_ms_and_meters_read_like_tone_meter() {
    let mut r = Rig::new();
    let id = dc_track(&mut r);
    let dc = reference(&mut r);
    r.e.play();
    let t0 = r.time_of_beat(0.0);
    r.run_until(t0 + 0.2);
    // the track meter: its mono down-mix before the master bus's +2 dB drive
    let track_db = r.e.get_track_level(&id);
    let want = 20.0 * (dc / 10f64.powf(2.0 / 20.0)).log10();
    assert!((track_db - want).abs() < 0.05, "{track_db} vs {want}");
    let master_db = r.e.get_master_level();
    assert!((master_db - (want + 2.0)).abs() < 0.05, "{master_db}");
    // −32 dB on the fader = −20 dB of master gain, reached 30 ms later
    r.e.set_master_volume_db(-32.0);
    assert_eq!(r.e.get_master_volume_db(), -32.0);
    let t = r.now();
    r.run(0.1);
    assert!(level_at(&r, t + 0.04) / dc < 0.11);
    assert!(level_at(&r, t) / dc > 0.2, "still ramping at the start");
    // a falling level decays by 0.8 per read (Tone.Meter smoothing)
    r.e.edit(|d| d.set_track_mute(&id, true));
    r.run(0.05);
    let a = r.e.get_track_level(&id);
    let b = r.e.get_track_level(&id);
    assert!((b - a - 20.0 * 0.8f64.log10()).abs() < 1e-9, "{a} {b}");
    r.e.set_master_volume_db(10.0);
    assert_eq!(r.e.get_master_volume_db(), 0.0, "clamped");
}

#[test]
fn legacy_ccs_drive_the_live_bus_with_pickup() {
    let mut r = Rig::new();
    // reverb is at 0.12: a knob at 0 must cross it first
    assert_eq!(r.e.handle_legacy_cc(72, 0, false), None);
    assert_eq!(r.e.live_effect_params().reverb_wet, 0.12);
    assert!(
        r.e.handle_legacy_cc(72, 20, false).is_some(),
        "crossed 0.12"
    );
    assert!((r.e.live_effect_params().reverb_wet - 20.0 / 127.0).abs() < 1e-12);
    assert_eq!(
        r.e.handle_legacy_cc(5, 127, true),
        Some(CcAction::Volume(0.0))
    );
    assert_eq!(r.e.get_master_volume_db(), 0.0);
    assert_eq!(
        r.e.handle_legacy_cc(74, 0, false),
        Some(CcAction::Volume(-40.0))
    );
    assert_eq!(r.e.live_volume(), -40.0);
    r.e.live_fx(LiveOp::ChorusDepth(0.5));
    assert_eq!(r.e.live_effect_params().chorus_depth, 0.5);
    r.run(0.05);
}
