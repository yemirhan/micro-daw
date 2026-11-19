//! Sound shaping on real `SamplerKit` renders: neutral = classic path, the amp envelope in
//! one-shot and gate modes, the filter, the pitch envelope, drive, smoothing and the audio-thread
//! rules (no allocation).

use super::*;
use crate::test_alloc::allocations_during;
use mdaw_dsp::buffer::AudioBuffer;
use mdaw_model::types::PadFilterType;

const SR: u32 = 48_000;
const SRF: f64 = SR as f64;

fn sine(freq: f64, secs: f64, amp: f32) -> Vec<f32> {
    (0..(secs * SRF) as usize)
        .map(|i| amp * (2.0 * std::f64::consts::PI * freq * i as f64 / SRF).sin() as f32)
        .collect()
}

fn kit_with(data: Vec<f32>, f: impl FnOnce(&mut PadParams)) -> SamplerKit {
    let mut pads = [PadParams::default(); PAD_COUNT];
    f(&mut pads[0]);
    let mut k = SamplerKit::new(SR, pads, 0);
    k.set_pad_buffer(
        0,
        Some(PadBuffer {
            forward: Arc::new(AudioBuffer::mono(SR, data)),
            reversed: None,
        }),
    );
    k
}

fn render(k: &mut SamplerKit, secs: f64) -> Vec<f32> {
    let mut out = Vec::new();
    let mut f = 0u64;
    while (out.len() as f64) < secs * SRF {
        let (mut l, mut r) = ([0f32; 128], [0f32; 128]);
        Instrument::process(k, f, &mut l, &mut r);
        out.extend_from_slice(&l);
        f += 128;
    }
    out
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0f32, |m, v| m.max(v.abs()))
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

fn at(t: f64) -> usize {
    (t * SRF) as usize
}

#[test]
fn a_neutral_shape_takes_the_classic_path_bit_for_bit() {
    // Same kit, shape default vs. a shape that was edited and set back: identical samples.
    let data = sine(220.0, 0.5, 0.7);
    let mut a = kit_with(data.clone(), |p| p.filter_cutoff = 3000.0);
    let mut b = kit_with(data, |p| {
        p.filter_cutoff = 3000.0;
        p.shape = PadShape {
            amp_decay: 1.7,
            filter_env_decay: 0.9,
            pitch_env_decay: 0.3,
            ..PadShape::default()
        };
    });
    assert!(b.pads[0].shape.is_neutral());
    a.trigger(0, 0.9, 0.01);
    b.trigger(0, 0.9, 0.01);
    let (ra, rb) = (render(&mut a, 0.6), render(&mut b, 0.6));
    assert!(ra.iter().zip(&rb).all(|(x, y)| x.to_bits() == y.to_bits()));
    assert!(!b.voices.iter().any(|v| v.sh.active));
}

#[test]
fn amp_attack_hold_decay_sustain_in_one_shot() {
    let mut k = kit_with(vec![0.5; at(1.0)], |p| {
        p.shape.amp_attack = 0.02;
        p.shape.amp_hold = 0.03;
        p.shape.amp_decay = 0.1;
        p.shape.amp_sustain = 0.25;
    });
    k.trigger(0, 1.0, 0.0);
    let out = render(&mut k, 1.1);
    let full = out[at(0.03)].abs();
    assert!(full > 0.3, "{full}");
    // Attack: linear rise, half way at 10 ms.
    assert!((out[at(0.01)].abs() / full - 0.5).abs() < 0.02);
    assert!(out[1].abs() < 0.01 * full);
    // Hold at full level until 50 ms.
    assert!((out[at(0.045)].abs() / full - 1.0).abs() < 1e-3);
    // Decay lands on the sustain at 150 ms and stays there.
    assert!((out[at(0.16)].abs() / full - 0.25).abs() < 0.01);
    assert!((out[at(0.6)].abs() / full - 0.25).abs() < 0.01);
    // The one-shot release (the pad's `decay`, 50 ms) still fades the sample end: silent at 1 s.
    assert!(out[at(1.0) + 10..].iter().all(|x| x.abs() < 1e-6));
}

#[test]
fn zero_sustain_ends_the_voice_after_its_stages() {
    let mut k = kit_with(vec![0.5; at(2.0)], |p| {
        p.shape.amp_decay = 0.2;
        p.shape.amp_sustain = 0.0;
    });
    k.trigger(0, 1.0, 0.0);
    let out = render(&mut k, 0.5);
    assert!(out[at(0.1)].abs() > 0.01);
    assert!(out[at(0.2) + 1..].iter().all(|x| *x == 0.0));
    // The voice is gone (list pruned), long before the 2 s sample ends.
    assert_eq!(k.live_voices(0), 0);
}

#[test]
fn gate_mode_sustains_while_held_and_releases_over_the_pad_decay() {
    let mut k = kit_with(vec![0.5; at(2.0)], |p| {
        p.gate = true;
        p.decay = 0.1; // release
        p.shape.amp_decay = 0.05;
        p.shape.amp_sustain = 0.5;
    });
    k.trigger(0, 1.0, 0.0);
    k.release(0, 0.4);
    let out = render(&mut k, 0.7);
    let full = out[0].abs();
    assert!((out[at(0.3)].abs() / full - 0.5).abs() < 0.01, "sustain");
    // Linear release from the sustain level: half way at 50 ms after the note-off.
    assert!((out[at(0.45)].abs() / full - 0.25).abs() < 0.02);
    assert!(
        out[at(0.5) + 2..].iter().all(|x| x.abs() < 1e-6),
        "released"
    );
}

#[test]
fn the_filter_types_shape_the_spectrum() {
    let low = sine(100.0, 0.5, 0.5);
    let high = sine(8000.0, 0.5, 0.5);
    let level = |data: &Vec<f32>, ty: PadFilterType, cutoff: f64| {
        let mut k = kit_with(data.clone(), |p| {
            p.filter_cutoff = cutoff;
            p.shape.filter_type = ty;
        });
        k.trigger(0, 1.0, 0.0);
        rms(&render(&mut k, 0.4)[at(0.1)..at(0.4)])
    };
    let lp = (
        level(&low, PadFilterType::Lp, 1000.0),
        level(&high, PadFilterType::Lp, 1000.0),
    );
    // LP alone is neutral (classic biquad) — use resonance 0 HP / BP for the SVF.
    assert!(lp.1 < lp.0 * 0.05, "lp {lp:?}");
    let hp = (
        level(&low, PadFilterType::Hp, 1000.0),
        level(&high, PadFilterType::Hp, 1000.0),
    );
    assert!(hp.0 < hp.1 * 0.05, "hp {hp:?}");
    let bp_mid = level(&sine(1000.0, 0.5, 0.5), PadFilterType::Bp, 1000.0);
    let bp = (
        level(&low, PadFilterType::Bp, 1000.0),
        level(&high, PadFilterType::Bp, 1000.0),
    );
    assert!(
        bp.0 < bp_mid * 0.2 && bp.1 < bp_mid * 0.2,
        "bp {bp:?} vs {bp_mid}"
    );
}

#[test]
fn resonance_and_the_filter_envelope_change_the_output() {
    let tone = sine(2000.0, 0.5, 0.3);
    let run = |f: &dyn Fn(&mut PadParams)| {
        let mut k = kit_with(tone.clone(), |p| {
            p.filter_cutoff = 2000.0;
            f(p)
        });
        k.trigger(0, 1.0, 0.0);
        render(&mut k, 0.4)
    };
    let plain = run(&|_| {});
    let res = run(&|p| p.shape.filter_resonance = 0.8);
    // At the cutoff, resonance lifts the tone.
    assert!(rms(&res[at(0.1)..]) > rms(&plain[at(0.1)..]) * 3.0);
    // A negative filter envelope closes the filter at the hit, then opens back up.
    let env = run(&|p| {
        p.shape.filter_env = -4.0;
        p.shape.filter_env_decay = 0.2;
    });
    assert!(rms(&env[at(0.01)..at(0.03)]) < rms(&plain[at(0.01)..at(0.03)]) * 0.2);
    let late = (rms(&env[at(0.3)..]), rms(&plain[at(0.3)..]));
    assert!((late.0 / late.1 - 1.0).abs() < 0.05, "{late:?}");
}

/// Zero crossings per second over `x`.
fn crossings(x: &[f32]) -> f64 {
    let n = x
        .windows(2)
        .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
        .count();
    n as f64 / (x.len() as f64 / SRF) / 2.0
}

#[test]
fn the_pitch_envelope_bends_the_hit_and_shortens_it() {
    let mut k = kit_with(sine(100.0, 1.0, 0.5), |p| {
        p.shape.pitch_env = 12.0;
        p.shape.pitch_env_decay = 0.1;
    });
    let h = k.trigger(0, 1.0, 0.0).unwrap();
    let out = render(&mut k, 1.1);
    // Start: about an octave up (≥ 150 Hz over the first 15 ms), settled at 100 Hz later.
    let early = crossings(&out[..at(0.015)]);
    assert!(early > 150.0, "{early}");
    let late = crossings(&out[at(0.3)..at(0.6)]);
    assert!((late - 100.0).abs() < 3.0, "{late}");
    // Played faster at first: ends before the 1 s slice would.
    let end = k.voices[h.slot as usize].end;
    assert!(end < 0.99 && end > 0.9, "{end}");
    assert!(out[at(end) + 2..].iter().all(|x| x.abs() < 1e-6));
}

#[test]
fn drive_saturates_smoothly_and_follows_the_knob() {
    let quiet = sine(110.0, 0.5, 0.1);
    let run = |drive: f64| {
        let mut k = kit_with(quiet.clone(), |p| p.shape.drive = drive);
        k.trigger(0, 1.0, 0.0);
        render(&mut k, 0.4)
    };
    let dry = run(0.0);
    let wet = run(1.0);
    assert!(rms(&wet[at(0.1)..]) > rms(&dry[at(0.1)..]) * 3.0, "lifted");
    assert!(peak(&wet) <= 1.0);
    // The knob turned mid-note glides: no step between consecutive samples.
    let mut k = kit_with(sine(110.0, 0.5, 0.5), |_| {});
    k.trigger(0, 1.0, 0.0);
    let mut out = render(&mut k, 0.1);
    let mut pads = k.pads;
    pads[0].shape.drive = 1.0;
    k.set_kit(pads, Default::default());
    // The voice was hit while the pad was neutral: it stays classic (latched), the next hit is
    // driven.
    out.extend(render(&mut k, 0.05));
    let max_step = out
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0f32, f32::max);
    assert!(max_step < 0.01, "{max_step}");
}

#[test]
fn cutoff_moves_glide_on_a_shaped_voice() {
    let mut k = kit_with(sine(300.0, 1.0, 0.5), |p| {
        p.filter_cutoff = 200.0;
        p.shape.filter_type = PadFilterType::Hp;
    });
    k.trigger(0, 1.0, 0.0);
    let mut out = render(&mut k, 0.2);
    let mut pads = k.pads;
    pads[0].filter_cutoff = 5000.0;
    k.set_kit(pads, Default::default());
    // Render quantum by quantum from where we are.
    let mut f = (out.len()) as u64;
    for _ in 0..40 {
        let (mut l, mut r) = ([0f32; 128], [0f32; 128]);
        Instrument::process(&mut k, f, &mut l, &mut r);
        out.extend_from_slice(&l);
        f += 128;
    }
    let max_step = out
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0f32, f32::max);
    // A 300 Hz sine at 0.5 moves ≤ 0.02 per sample; a jump would show as a larger step.
    assert!(max_step < 0.03, "{max_step}");
    // And it did take effect: the high-pass at 5 kHz removes most of the 300 Hz tone.
    let n = out.len();
    assert!(rms(&out[n - 1000..]) < rms(&out[at(0.1)..at(0.2)]) * 0.1);
}

#[test]
fn the_export_length_follows_the_shaping() {
    let p = |f: &dyn Fn(&mut PadShape)| {
        let mut s = PadShape::default();
        f(&mut s);
        s
    };
    let slice = compute_slice(&PadParams::default(), 1.0).unwrap();
    assert_eq!(voice_seconds(&PadShape::default(), &slice), 1.0);
    let short = voice_seconds(
        &p(&|s| {
            s.amp_sustain = 0.0;
            s.amp_hold = 0.1;
            s.amp_decay = 0.2;
        }),
        &slice,
    );
    assert!((short - 0.3).abs() < 1e-12);
    assert!(voice_seconds(&p(&|s| s.pitch_env = -12.0), &slice) > 1.0);
}

#[test]
fn shaped_voices_do_not_allocate() {
    let mut k = kit_with(sine(100.0, 1.0, 0.5), |p| {
        p.shape = PadShape {
            amp_attack: 0.005,
            amp_hold: 0.01,
            amp_decay: 0.2,
            amp_sustain: 0.3,
            filter_type: PadFilterType::Bp,
            filter_resonance: 0.5,
            filter_env: 3.0,
            filter_env_decay: 0.1,
            pitch_env: 24.0,
            pitch_env_decay: 0.05,
            drive: 0.6,
        };
        p.gate = true;
    });
    let (mut l, mut r) = ([0f32; 128], [0f32; 128]);
    let mut pads = k.pads;
    pads[0].filter_cutoff = 800.0;
    pads[0].shape.drive = 0.1;
    let n = allocations_during(|| {
        for i in 0..20 {
            k.trigger(0, 0.9, i as f64 * 0.01);
        }
        k.release(0, 0.3);
        k.set_kit(pads, Default::default());
        for q in 0..200u64 {
            k.process(q * 128, &mut l, &mut r);
            k.drain_garbage(drop);
        }
    });
    assert_eq!(n, 0, "allocations on the audio path");
}
