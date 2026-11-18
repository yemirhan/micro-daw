//! The assembled instrument: chains, the sampled piano's fallback path, live `set`, status, and
//! the SampledPiano parts of `src/audio/instruments.test.ts`.

use std::sync::Arc;

use super::*;
use crate::character::{CharacterParams, db_to_gain};
use crate::piano::samples::piano_samples;
use crate::piano::{PianoSlot, SAMPLER_MAKEUP};
use crate::synth::bank::{fallback_piano, synth_presets};
use crate::synth::params::merge_preset_params;
use crate::synth::presets::preset_at;
use serde_json::json;

const SR: u32 = 48000;

fn render(inst: &mut dyn Instrument, from: u64, frames: usize) -> (Vec<f32>, Vec<f32>) {
    let (mut l, mut r) = (vec![0f32; frames], vec![0f32; frames]);
    for (k, (cl, cr)) in l.chunks_mut(Q).zip(r.chunks_mut(Q)).enumerate() {
        inst.process(from + (Q * k) as u64, cl, cr);
    }
    (l, r)
}

fn max_diff(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f32::max)
}

#[test]
fn a_poly_preset_plays_its_voices_through_its_character_chain() {
    for (i, p) in synth_presets().iter().enumerate() {
        if matches!(p, SynthPreset::Sampler(_)) {
            continue;
        }
        let m = p.meta();
        let mut player = SynthPlayer::new(p, SR, 0.0, &PianoSlot::empty(), 7);
        let mut raw = SynthEngine::new(p, SR as f64, 0.0, 7);
        let mut chain = CharacterChain::new(SR, m.character, m.gain, 0.0);
        player.note_on_midi(60, 0.8, 0.01);
        raw.note_on_midi(60, 0.8, 0.01);
        let (pl, pr) = render(&mut player, 0, 9600);
        let (mut rl, mut rr) = render(&mut raw, 0, 9600);
        for k in (0..9600).step_by(Q) {
            chain.process(k as u64, &mut rl[k..k + Q], &mut rr[k..k + Q]);
        }
        assert_eq!(max_diff(&pl, &rl), 0.0, "preset {i} ({})", m.id);
        assert_eq!(max_diff(&pr, &rr), 0.0, "preset {i} ({})", m.id);
        assert!(pl.iter().any(|v| *v != 0.0), "preset {i} sounds");
    }
}

#[test]
fn the_chain_applies_the_preset_gain() {
    // A preset without character stages: the chain is the gain alone.
    let p = synth_presets()
        .iter()
        .find(|p| {
            !matches!(p, SynthPreset::Sampler(_))
                && p.meta().character == CharacterParams::default()
                && p.meta().gain != 0.0
        })
        .unwrap();
    let mut player = SynthPlayer::new(p, SR, 0.0, &PianoSlot::empty(), 1);
    let mut raw = SynthEngine::new(p, SR as f64, 0.0, 1);
    player.note_on_midi(57, 0.9, 0.0);
    raw.note_on_midi(57, 0.9, 0.0);
    let (pl, _) = render(&mut player, 0, 4800);
    let (rl, _) = render(&mut raw, 0, 4800);
    let g = db_to_gain(p.meta().gain) as f32;
    for (a, b) in pl.iter().zip(&rl) {
        assert!(
            (a - b * g).abs() <= 1e-6 * (1.0 + b.abs()),
            "{a} vs {b}·{g}"
        );
    }
}

#[test]
fn sampled_piano_plays_the_fallback_until_samples_load_and_routes_note_offs_back() {
    let piano = &synth_presets()[0];
    let mut inst = SynthPlayer::new(piano, SR, 0.0, &PianoSlot::empty(), 1);
    assert!(!inst.is_sampled());
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
fn before_its_samples_a_sampler_preset_is_fallback_chain_then_makeup_then_its_chain() {
    let SynthPreset::Sampler(sp) = &synth_presets()[15] else {
        panic!()
    };
    let mut player = SynthPlayer::new(&synth_presets()[15], SR, 0.0, &PianoSlot::empty(), 3);
    let mut fallback = FallbackPiano::new(SR, 0.0, 3);
    let mut chain = CharacterChain::new(SR, sp.meta.character, sp.meta.gain, 0.0);
    player.note_on_midi(62, 0.7, 0.002);
    PianoFallback::note_on(&mut fallback, 62, 0.7, 0.002);
    player.note_off_midi(62, 0.15);
    PianoFallback::note_off(&mut fallback, 62, 0.15);
    let (pl, pr) = render(&mut player, 0, 19200);
    let (mut fl, mut fr) = render(&mut fallback, 0, 19200);
    for k in (0..19200).step_by(Q) {
        for x in fl[k..k + Q].iter_mut().chain(fr[k..k + Q].iter_mut()) {
            *x *= SAMPLER_MAKEUP;
        }
        chain.process(k as u64, &mut fl[k..k + Q], &mut fr[k..k + Q]);
    }
    assert_eq!(max_diff(&pl, &fl), 0.0);
    assert_eq!(max_diff(&pr, &fr), 0.0);
    assert!(pl.iter().any(|v| v.abs() > 1e-3));
}

#[test]
fn a_sampler_preset_switches_to_the_samples_once_the_shared_slot_is_filled() {
    let slot = PianoSlot::empty();
    let mut inst = SynthPlayer::new(&synth_presets()[0], SR, 0.0, &slot, 1);
    inst.note_on_midi(60, 0.8, 0.0); // fallback
    let _ = render(&mut inst, 0, 1280);
    slot.fill(piano_samples(SR).expect("bundled piano samples"));
    assert!(inst.is_sampled());
    inst.note_on_midi(64, 0.8, 0.03); // sampled
    assert_eq!(inst.held_notes(), vec![60, 64]);
    inst.note_off_midi(60, 0.05); // back to the fallback
    assert_eq!(inst.held_notes(), vec![64]);
    let (l, _) = render(&mut inst, 1280, 9600);
    assert!(l.iter().any(|v| v.abs() > 1e-3));
    SynthInstrument::release_all(&mut inst, 0.3);
    assert!(inst.held_notes().is_empty());
    assert_eq!(inst.active_voice_count(6.0), 0);
}

#[test]
fn piano_params_are_a_view_of_the_sampler_preset() {
    let SynthPreset::Sampler(grand) = &synth_presets()[0] else {
        panic!()
    };
    assert_eq!(PianoParams::from(grand), PianoParams::GRAND);
    // The preset's JSON is a valid PianoParams (one serde form).
    let v = synth_presets()[15].to_json();
    let p: PianoParams = serde_json::from_value(v).unwrap();
    let SynthPreset::Sampler(bright) = &synth_presets()[15] else {
        panic!()
    };
    assert_eq!(p, PianoParams::from(bright));
    assert_eq!(p.character.presence, 5.0);
}

#[test]
fn set_changes_parameters_live_and_keeps_held_notes() {
    let base = preset_at(1.0).clone();
    let mut inst = SynthPlayer::new(&base, SR, 0.0, &PianoSlot::empty(), 1);
    inst.note_on_midi(60, 0.8, 0.0);
    let _ = render(&mut inst, 0, 4800);
    let next = merge_preset_params(
        &base,
        Some(&json!({ "gain": -20.0, "filter": { "cutoff": 400 } })),
    );
    assert!(inst.set(&next, 0.1));
    assert_eq!(inst.held_notes(), vec![60]);
    assert_eq!(inst.preset().meta().gain, -20.0);
    let (late, _) = render(&mut inst, 4800 + 9600, 4800);
    let mut fresh = SynthPlayer::new(&base, SR, 0.0, &PianoSlot::empty(), 1);
    fresh.note_on_midi(60, 0.8, 0.0);
    let (orig, _) = render(&mut fresh, 0, 4800 * 4);
    let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
    assert!(rms(&late) < rms(&orig[14400..]) * 0.2, "quieter and darker");
    // another engine: refused, nothing changes
    assert!(!inst.set(preset_at(2.0), 0.2));
    assert_eq!(inst.preset().meta().gain, -20.0);
    // a sampler preset takes sampler params
    let mut piano = SynthPlayer::new(&synth_presets()[0], SR, 0.0, &PianoSlot::empty(), 1);
    let p2 = merge_preset_params(&synth_presets()[0], Some(&json!({ "release": 1.5 })));
    assert!(piano.set(&p2, 0.0));
}

#[test]
fn status_publishes_voices_and_held_notes_after_each_quantum() {
    let status = Arc::new(SynthStatus::default());
    let mut inst = SynthPlayer::new(preset_at(3.0), SR, 0.0, &PianoSlot::empty(), 1)
        .with_status(status.clone());
    inst.note_on_midi(60, 0.8, 0.0);
    inst.note_on_midi(127, 0.8, 0.0);
    inst.note_on_midi(0, 0.8, 0.0);
    let _ = render(&mut inst, 0, Q);
    assert_eq!(status.held_notes(), vec![0, 60, 127]);
    assert_eq!(status.active_voice_count(), 3);
    SynthInstrument::release_all(&mut inst, Q as f64 / SR as f64);
    let _ = render(&mut inst, Q as u64, SR as usize);
    assert!(status.held_notes().is_empty());
    assert_eq!(status.active_voice_count(), 0);
}

#[test]
fn trigger_attack_release_holds_at_least_24_ms_and_releases() {
    let mut inst = SynthPlayer::new(preset_at(5.0), SR, 0.0, &PianoSlot::empty(), 1);
    SynthInstrument::trigger_attack_release(&mut inst, 60.0, 0.001, 0.0, 0.8);
    assert!(inst.held_notes().is_empty(), "one-shots are never held");
    assert_eq!(inst.active_voice_count(0.0), 1);
    assert_eq!(inst.active_voice_count(10.0), 0);
    let (l, _) = render(&mut inst, 0, 4800);
    assert!(l[..1200].iter().any(|v| v.abs() > 1e-3));
}

#[test]
fn note_events_and_process_do_not_allocate() {
    let ready = PianoSlot::ready(piano_samples(SR).expect("bundled piano samples"));
    for (preset, slot) in [
        (preset_at(1.0), PianoSlot::empty()),
        (preset_at(13.0), PianoSlot::empty()),
        (preset_at(28.0), PianoSlot::empty()),
        (preset_at(24.0), PianoSlot::empty()),
        (preset_at(0.0), PianoSlot::empty()),
        (preset_at(15.0), ready.clone()),
    ] {
        let status = Arc::new(SynthStatus::default());
        let mut inst = SynthPlayer::new(preset, SR, 0.0, &slot, 1).with_status(status);
        let (mut l, mut r) = ([0f32; Q], [0f32; Q]);
        inst.process(0, &mut l, &mut r);
        let n = crate::test_alloc::allocations_during(|| {
            let mut frame = Q as u64;
            for i in 0..40 {
                let t = frame as f64 / SR as f64;
                inst.note_on(48.0 + (i % 12) as f64, 0.8, t + 0.001);
                if i % 3 == 0 {
                    inst.note_off(48.0 + ((i + 5) % 12) as f64, t + 0.002);
                }
                if i % 5 == 0 {
                    SynthInstrument::trigger_attack_release(&mut inst, 72.0, 0.1, t, 0.6);
                }
                for _ in 0..8 {
                    inst.process(frame, &mut l, &mut r);
                    frame += Q as u64;
                }
            }
            SynthInstrument::release_all(&mut inst, frame as f64 / SR as f64);
            inst.process(frame, &mut l, &mut r);
        });
        assert_eq!(n, 0, "{}: {n} allocations on the audio path", preset.id());
    }
}
