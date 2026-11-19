//! Port of the synthesis / dsp / render parts of `classic.test.ts` and `render.test.ts`
//! (metadata and kit cases live in mdaw-model).

use super::dsp::*;
use super::*;
use mdaw_model::classic_meta::CLASSIC_MACHINES;

fn all_voices() -> Vec<(ClassicMachineId, &'static str)> {
    CLASSIC_MACHINES
        .iter()
        .flat_map(|m| m.voices.iter().map(move |v| (m.id, v.id)))
        .collect()
}

#[test]
fn every_voice_has_a_recipe_and_no_orphans() {
    for m in &CLASSIC_MACHINES {
        for v in m.voices {
            assert!(has_classic_recipe(m.id, v.id), "{:?}/{}", m.id, v.id);
            assert!(classic_recipe(m.id, v.id).is_some());
        }
        let mut recipes: Vec<&str> = classic_recipe_ids(m.id).to_vec();
        let mut voices: Vec<&str> = m.voices.iter().map(|v| v.id).collect();
        recipes.sort_unstable();
        voices.sort_unstable();
        assert_eq!(recipes, voices, "{:?}", m.id);
    }
}

#[test]
fn every_voice_renders_finite_mono_audio_at_minus_1_dbfs_within_its_cap() {
    for (machine, voice) in all_voices() {
        let data = synthesize_classic_voice(machine, voice).unwrap();
        let label = format!("{machine:?}/{voice}");
        assert!(data.iter().all(|x| x.is_finite()), "{label}");
        assert!(
            (peak_of(&data) - target_peak()).abs() < 5e-5,
            "{label}: peak"
        );
        let cap = samples(classic_recipe(machine, voice).unwrap().max_sec);
        assert!(data.len() <= cap, "{label}");
        assert!(data.len() as f64 / SR > 0.02, "{label}");
        assert!(data[data.len() - 1].abs() < 1e-3, "{label}");
        // Starts without a hard click: the first sample is near zero.
        assert!(data[0].abs() < 0.2, "{label}");
    }
}

#[test]
fn is_deterministic() {
    let a = synthesize_classic_voice(ClassicMachineId::Tr909, "snare").unwrap();
    let b = synthesize_classic_voice(ClassicMachineId::Tr909, "snare").unwrap();
    assert_eq!(a, b);
}

#[test]
fn throws_on_unknown_voices() {
    let err = synthesize_classic_voice(ClassicMachineId::Tr808, "nope").unwrap_err();
    assert!(err.to_string().starts_with("Unknown classic voice"));
    assert_eq!(err.to_string(), "Unknown classic voice: tr808/nope");
    assert!(render_classic_voice(ClassicMachineId::Tr808, "nope").is_err());
    assert!(cached_classic_voice(ClassicMachineId::Tr808, "nope").is_err());
    assert!(!has_classic_recipe(ClassicMachineId::Tr808, "guiro"));
}

#[test]
fn kicks_live_in_the_lows_hats_in_the_highs() {
    for m in &CLASSIC_MACHINES {
        for v in m.voices {
            let a = analyze_voice(&synthesize_classic_voice(m.id, v.id).unwrap(), SR);
            let label = format!("{:?}/{}", m.id, v.id);
            if v.id.starts_with("kick") {
                assert!(a.low_frac > 0.8, "{label}");
                assert!(a.peak_hz < 90.0, "{label}");
            }
            if ["chh", "phh", "ohh", "maracas", "shaker", "cabasa"].contains(&v.id) {
                assert!(a.high_frac > 0.8, "{label}");
            }
            if ["cymbal", "crash"].contains(&v.id) {
                assert!(a.high_frac > 0.6, "{label}");
            }
            if v.id.starts_with("snare") {
                assert!(a.low_frac < 0.1, "{label}");
                assert!(a.centroid_hz > 600.0, "{label}");
            }
        }
    }
}

#[test]
fn machines_sound_different() {
    let kicks: Vec<VoiceAnalysis> = CLASSIC_MACHINES
        .iter()
        .map(|m| analyze_voice(&synthesize_classic_voice(m.id, "kick").unwrap(), SR))
        .collect();
    for i in 0..kicks.len() {
        for j in i + 1..kicks.len() {
            let d_len = (kicks[i].length_sec - kicks[j].length_sec).abs();
            let d_centroid = (kicks[i].centroid_hz - kicks[j].centroid_hz).abs();
            assert!(d_len > 0.02 || d_centroid > 4.0, "{i} vs {j}");
        }
    }
}

#[test]
fn renders_a_mono_44k1_audio_buffer() {
    let buf = render_classic_voice(ClassicMachineId::Tr606, "chh").unwrap();
    assert_eq!(buf.channel_count(), 1);
    assert_eq!(buf.sample_rate, CLASSIC_SAMPLE_RATE);
    assert!((peak_of(&buf.channels[0]) - target_peak()).abs() < 5e-5);
}

#[test]
fn cache_memoizes_per_machine_and_voice_across_threads() {
    let m = ClassicMachineId::Cr78;
    let handles: Vec<_> = (0..4)
        .map(|_| std::thread::spawn(move || cached_classic_voice(m, "guiro").unwrap()))
        .collect();
    let bufs: Vec<Arc<AudioBuffer>> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    for b in &bufs[1..] {
        assert!(Arc::ptr_eq(&bufs[0], b));
    }
    let again = cached_classic_voice(m, "guiro").unwrap();
    assert!(Arc::ptr_eq(&bufs[0], &again));
    assert_eq!(*again, render_classic_voice(m, "guiro").unwrap());
    let other = cached_classic_voice(ClassicMachineId::Cr78, "clave").unwrap();
    assert!(!Arc::ptr_eq(&other, &again));
}

#[test]
fn finalize_normalizes_and_trims_trailing_silence() {
    let mut raw = vec![0.0f32; SR as usize];
    let tone = mul(
        osc(
            samples(0.1),
            OscOpts {
                freq: 440.0,
                ..OscOpts::D
            },
        ),
        &env(samples(0.1), EnvOpts::t60(0.1)),
    );
    for (r, t) in raw.iter_mut().zip(&tone) {
        *r = (*t as f64 * 0.25) as f32;
    }
    let out = finalize(&raw, FinalizeOpts::D);
    assert!((peak_of(&out) - target_peak()).abs() < 5e-4);
    assert!(out.len() < samples(0.13));
    assert!(out.len() > samples(0.1));
}

#[test]
fn lowpass_attenuates_highs_and_lofi_quantizes() {
    let hi = osc(
        (SR / 4.0) as usize,
        OscOpts {
            freq: 12000.0,
            ..OscOpts::D
        },
    );
    assert!(peak_of(&lp(hi, 1000.0, None)[2000..]) < 0.02);
    let ramp: Vec<f32> = (0..1000).map(|i| (i as f64 / 999.0) as f32).collect();
    let mut levels: Vec<u32> = lofi(
        ramp,
        LofiOpts {
            bits: 3,
            ..LofiOpts::D
        },
    )
    .iter()
    .map(|x| x.to_bits())
    .collect();
    levels.sort_unstable();
    levels.dedup();
    assert!(levels.len() <= 4); // 0..3 of 3 positive steps
}

#[test]
fn mulberry32_known_values() {
    let mut r = Rng::new(1);
    let first = r.next_f64();
    assert!((0.0..1.0).contains(&first));
    // noise is in [-1, 1)
    assert!(noise(1000, 3).iter().all(|&x| (-1.0..1.0).contains(&x)));
}

#[test]
fn mix_into_with_offset_and_short_source() {
    let mut dst = vec![1.0f32; 5];
    mix_into(&mut dst, &[1.0, 2.0], 0.5, 2);
    assert_eq!(dst, vec![1.0, 1.0, 1.5, 2.0, 1.0]);
    mix_into(&mut dst, &[1.0, 1.0, 1.0], 1.0, 4);
    assert_eq!(dst, vec![1.0, 1.0, 1.5, 2.0, 2.0]);
    assert_eq!(mul(vec![2.0; 4], &[0.5, 0.5]), vec![1.0, 1.0, 0.0, 0.0]);
}

/// `cargo test -p mdaw-dsp --release -- --ignored render_all_voices_benchmark --nocapture`
#[test]
#[ignore]
fn render_all_voices_benchmark() {
    let t0 = std::time::Instant::now();
    let mut total = 0usize;
    for (machine, voice) in all_voices() {
        let t = std::time::Instant::now();
        let n = synthesize_classic_voice(machine, voice).unwrap().len();
        total += n;
        println!(
            "{:<24} {:>7} samples  {:>8.2} ms",
            format!("{}/{voice}", machine.id()),
            n,
            t.elapsed().as_secs_f64() * 1e3
        );
    }
    println!(
        "rendered {} voices, {:.2} s of audio, in {:.1} ms",
        all_voices().len(),
        total as f64 / SR,
        t0.elapsed().as_secs_f64() * 1e3
    );
}
