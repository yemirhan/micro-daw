//! Port of the PolyInstrument / SampledPiano parts of `src/audio/instruments.test.ts`, plus the
//! realtime contract (no allocation in note events or `process`).

use super::bank::{fallback_piano, synth_presets};
use super::common::{midi_to_name, norm_velocity, note_name_to_midi, velocity_gain};
use super::params::merge_preset_params;
use super::presets::{SamplerPreset, SynthPreset};
use super::{SampledPianoSlot, SynthEngine};
use crate::contract::Instrument;
use crate::core::SynthInstrument;
use serde_json::json;

const SR: f64 = 48000.0;

fn by_id(id: &str) -> SynthPreset {
    synth_presets()
        .iter()
        .find(|p| p.id() == id)
        .unwrap()
        .clone()
}

fn render(inst: &mut SynthEngine, from: u64, frames: usize) -> (Vec<f32>, Vec<f32>) {
    let (mut l, mut r) = (vec![0f32; frames], vec![0f32; frames]);
    for (k, (cl, cr)) in l.chunks_mut(128).zip(r.chunks_mut(128)).enumerate() {
        inst.process(from + 128 * k as u64, cl, cr);
    }
    (l, r)
}

#[test]
fn re_striking_a_held_note_reuses_its_voice_and_one_note_off_silences_it() {
    let pad = by_id("warm-pad");
    let mut inst = SynthEngine::new(&pad, SR, 0.0, 1);
    inst.note_on_midi(60, 0.8, 0.0);
    inst.note_on_midi(60, 0.8, 0.1);
    inst.note_on_midi(60, 0.8, 0.2);
    assert_eq!(inst.pool_size(), 1);
    assert_eq!(inst.held_notes(), vec![60]);
    inst.note_off_midi(60, 0.3);
    assert!(inst.held_notes().is_empty());
    assert_eq!(inst.active_voice_count(0.3 + pad.release_tail() + 0.05), 0);
}

#[test]
fn two_attacks_in_the_same_tick_are_fine() {
    let mut inst = SynthEngine::new(&by_id("warm-pad"), SR, 0.0, 1);
    inst.note_on_midi(60, 0.8, 0.0);
    inst.note_on_midi(60, 0.8, 0.0);
    inst.note_off_midi(60, 0.0);
    assert!(inst.held_notes().is_empty());
    let (l, _) = render(&mut inst, 0, 4800);
    assert!(l.iter().all(|v| v.is_finite()));
}

#[test]
fn caps_voices_at_the_preset_polyphony_and_steals() {
    let pad = merge_preset_params(&by_id("warm-pad"), Some(&json!({ "polyphony": 3 })));
    let mut inst = SynthEngine::new(&pad, SR, 0.0, 1);
    for n in 0..6 {
        inst.note_on_midi(60 + n, 0.8, n as f64 * 0.01);
    }
    assert_eq!(inst.pool_size(), 3);
    assert_eq!(inst.held_notes().len(), 3);
    for n in 0..6 {
        inst.note_off_midi(60 + n, 1.0);
    }
    assert!(inst.held_notes().is_empty());
}

#[test]
fn normalises_velocities_and_note_names() {
    assert_eq!(norm_velocity(Some(127.0)), 1.0);
    assert!((norm_velocity(Some(64.0)) - 0.504).abs() < 0.005);
    assert_eq!(norm_velocity(Some(0.5)), 0.5);
    assert_eq!(norm_velocity(None), 0.8);
    assert_eq!(norm_velocity(Some(f64::NAN)), 0.8);
    assert_eq!(velocity_gain(1.0, 1.0), 1.0);
    assert!(velocity_gain(0.5, 1.0) < 0.4);
    assert_eq!(velocity_gain(0.2, 0.0), 1.0);
    assert_eq!(note_name_to_midi("C4"), 60);
    assert_eq!(note_name_to_midi("C#4"), 61);
    assert_eq!(note_name_to_midi("Db4"), 61);
    assert_eq!(note_name_to_midi("Fs3"), 54);
    assert_eq!(note_name_to_midi("A0"), 21);
    assert_eq!(note_name_to_midi("C-1"), 0);
    assert_eq!(note_name_to_midi("nope"), -1);
    assert_eq!(midi_to_name(61.0), "C#4");
    assert_eq!(midi_to_name(0.0), "C-1");
}

#[test]
fn ignores_out_of_range_notes() {
    let mut inst = SynthEngine::new(&by_id("warm-pad"), SR, 0.0, 1);
    inst.note_on_midi(-1, 1.0, 0.0);
    inst.note_on_midi(128, 1.0, 0.0);
    inst.trigger_attack_release(-1, 1.0, 0.0, 0.8);
    assert_eq!(inst.pool_size(), 0);
}

#[test]
fn live_set_updates_voices_and_the_release_bookkeeping() {
    let pad = by_id("warm-pad");
    let mut inst = SynthEngine::new(&pad, SR, 0.0, 1);
    inst.note_on_midi(60, 0.8, 0.0);
    let next = merge_preset_params(
        &pad,
        Some(
            &json!({ "filter": { "cutoff": 3000 }, "oscA": { "unison": 5 }, "amp": { "release": 2 } }),
        ),
    );
    assert!(inst.set_preset(&next));
    assert_eq!(inst.pool_size(), 1);
    inst.note_off_midi(60, 0.1);
    // The new release time is honoured by the bookkeeping.
    assert_eq!(inst.active_voice_count(1.5), 1);
    assert_eq!(inst.active_voice_count(2.2), 0);
    let (l, _) = render(&mut inst, 0, 48000);
    assert!(l.iter().all(|v| v.is_finite()));
    // A preset of another engine is refused (the caller rebuilds).
    assert!(!inst.set_preset(&by_id("rhodes")));
}

#[test]
fn mono_legato_glides_and_falls_back_to_the_held_key() {
    let bass = by_id("reese-bass");
    let mut inst = SynthEngine::new(&bass, SR, 0.0, 1);
    inst.note_on_midi(36, 0.8, 0.0);
    inst.note_on_midi(43, 0.8, 0.1);
    assert_eq!(inst.held_notes(), vec![36, 43]);
    inst.note_off_midi(43, 0.2); // back to 36 (still held)
    assert_eq!(inst.held_notes(), vec![36]);
    assert_eq!(inst.active_voice_count(5.0), 1);
    inst.note_off_midi(36, 0.3);
    assert!(inst.held_notes().is_empty());
    assert_eq!(inst.pool_size(), 1);
    assert_eq!(inst.active_voice_count(0.3 + bass.release_tail() + 0.05), 0);
}

#[test]
fn mono_releasing_a_non_sounding_key_keeps_playing_and_release_all_silences() {
    let mut inst = SynthEngine::new(&by_id("reese-bass"), SR, 0.0, 1);
    inst.note_on_midi(36, 0.8, 0.0);
    inst.note_on_midi(38, 0.8, 0.1);
    inst.note_off_midi(36, 0.2);
    assert_eq!(inst.active_voice_count(5.0), 1); // 38 still held
    inst.release_all(5.0);
    assert!(inst.held_notes().is_empty());
    assert_eq!(inst.active_voice_count(5.1), 0);
}

/// A sampled piano stand-in: records note events.
#[derive(Default)]
struct FakeSlot {
    sampled: bool,
    events: Vec<String>,
}

impl SampledPianoSlot for FakeSlot {
    fn is_sampled(&self) -> bool {
        self.sampled
    }
    fn note_on(&mut self, midi: i32, _velocity: f64, time: f64) {
        self.events.push(format!("on {midi} {time}"));
    }
    fn note_off(&mut self, midi: i32, time: f64) {
        self.events.push(format!("off {midi} {time}"));
    }
    fn trigger_attack_release(&mut self, midi: i32, duration: f64, time: f64, _velocity: f64) {
        self.events.push(format!("ar {midi} {duration} {time}"));
    }
    fn release_all(&mut self, time: f64) {
        self.events.push(format!("all {time}"));
    }
    fn active_voice_count(&self, _time: f64) -> usize {
        0
    }
    fn process(&mut self, _frame: u64, _left: &mut [f32], _right: &mut [f32]) {}
}

#[test]
fn sampled_piano_plays_the_fallback_until_samples_load_and_routes_note_offs_back() {
    let piano = &synth_presets()[0];
    let SynthPreset::Sampler(_) = piano else {
        panic!()
    };
    let mut inst = SynthEngine::new(piano, SR, 0.0, 1);
    inst.note_on_midi(60, 0.8, 0.0);
    inst.note_on_midi(64, 0.8, 0.0);
    assert_eq!(inst.held_notes(), vec![60, 64]);
    inst.note_off_midi(60, 0.5);
    inst.note_off_midi(64, 0.5);
    assert!(inst.held_notes().is_empty());
    assert_eq!(
        inst.active_voice_count(0.5 + fallback_piano().amp.release + 0.05),
        0
    );
    let (l, _) = render(&mut inst, 0, 24000);
    assert!(l.iter().any(|v| *v != 0.0), "the fallback piano sounds");
}

#[test]
fn sampled_piano_routes_to_the_slot_once_sampled() {
    let mut inst = SynthEngine::new(&synth_presets()[15], SR, 0.0, 1);
    inst.note_on_midi(60, 0.8, 0.0); // fallback
    let host = inst.sampler_host().unwrap();
    assert!(
        host.set_slot(Box::new(FakeSlot {
            sampled: true,
            ..Default::default()
        }))
        .is_none()
    );
    assert!(host.is_sampled());
    assert_eq!(host.preset().meta.id, "bright-piano");
    inst.note_on_midi(62, 0.8, 0.1); // sampled
    inst.note_off_midi(60, 0.2); // back to the fallback (and the slot sees it too)
    inst.trigger_attack_release(64, 0.01, 0.3, 0.8);
    inst.release_all(0.4);
    assert!(inst.held_notes().is_empty());
    assert_eq!(inst.active_voice_count(1.0), 0);
    let mut sampler = SamplerPreset::clone(match &synth_presets()[15] {
        SynthPreset::Sampler(p) => p,
        _ => unreachable!(),
    });
    sampler.release = 0.9;
    assert!(inst.set_preset(&SynthPreset::Sampler(sampler)));
}

#[test]
fn note_events_and_process_do_not_allocate() {
    for id in [
        "saw-lead",
        "airy-pad",
        "rhodes",
        "harp",
        "reese-bass",
        "grand-piano",
        "string-ensemble",
    ] {
        let mut inst = SynthEngine::new(&by_id(id), SR, 0.0, 1);
        let (mut l, mut r) = ([0f32; 128], [0f32; 128]);
        // Warm up (first use builds nothing either, but keep the check strict).
        inst.process(0, &mut l, &mut r);
        let n = crate::test_alloc::allocations_during(|| {
            let mut frame = 128u64;
            for i in 0..40 {
                let t = frame as f64 / SR;
                inst.note_on(48.0 + (i % 12) as f64, 0.8, t + 0.001);
                if i % 3 == 0 {
                    inst.note_off(48.0 + ((i + 5) % 12) as f64, t + 0.002);
                }
                for _ in 0..8 {
                    inst.process(frame, &mut l, &mut r);
                    frame += 128;
                }
            }
            inst.release_all(frame as f64 / SR);
            inst.process(frame, &mut l, &mut r);
        });
        assert_eq!(n, 0, "{id}: {n} allocations on the audio path");
    }
}
