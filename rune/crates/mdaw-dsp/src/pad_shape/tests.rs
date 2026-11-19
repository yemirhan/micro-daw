use super::*;

const SR: f32 = 48_000.0;

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

#[test]
fn decay_curve_endpoints_and_shape() {
    assert_eq!(decay_curve(-1.0), 1.0);
    assert_eq!(decay_curve(0.0), 1.0);
    assert_eq!(decay_curve(1.0), 0.0);
    assert_eq!(decay_curve(2.0), 0.0);
    assert!(close(decay_curve(0.5), 0.125, 1e-12));
    // Monotonic.
    let mut prev = 1.0;
    for i in 1..=100 {
        let v = decay_curve(i as f64 / 100.0);
        assert!(v <= prev);
        prev = v;
    }
}

#[test]
fn ahds_flat_is_one_everywhere() {
    for t in [0.0, 0.001, 0.5, 10.0] {
        assert_eq!(Ahds::FLAT.level(t), 1.0);
    }
    assert_eq!(Ahds::FLAT.silent_after(), None);
}

#[test]
fn ahds_stage_timing() {
    let e = Ahds {
        attack: 0.01,
        hold: 0.02,
        decay: 0.1,
        sustain: 0.25,
    };
    assert_eq!(e.level(-0.001), 0.0);
    assert_eq!(e.level(0.0), 0.0);
    assert!(close(e.level(0.005), 0.5, 1e-12), "half way up the attack");
    assert!(
        close(e.level(0.01), 1.0, 1e-12),
        "attack ends at full level"
    );
    assert_eq!(e.level(0.02), 1.0, "hold");
    assert_eq!(e.level(0.0299), 1.0, "hold until a + h");
    // Decay: 0.25 + 0.75·(1 − x)³.
    assert!(close(e.level(0.03 + 0.05), 0.25 + 0.75 * 0.125, 1e-12));
    assert!(close(e.level(0.13), 0.25, 1e-12), "decay lands on sustain");
    assert_eq!(e.level(5.0), 0.25, "sustain holds");
    assert_eq!(e.silent_after(), None);
    // Monotonic decay.
    let mut prev = 1.0;
    let mut t = 0.03;
    while t < 0.2 {
        let v = e.level(t);
        assert!(v <= prev + 1e-15);
        prev = v;
        t += 0.001;
    }
}

#[test]
fn ahds_zero_sustain_goes_silent_after_its_stages() {
    let e = Ahds {
        attack: 0.0,
        hold: 0.05,
        decay: 0.2,
        sustain: 0.0,
    };
    assert_eq!(e.level(0.0), 1.0, "no attack: full level at the hit");
    assert_eq!(e.silent_after(), Some(0.25));
    assert_eq!(e.level(0.25), 0.0);
    assert!(e.level(0.2) > 0.0);
    // Decay 0 with sustain 0: a pure hold gate.
    let g = Ahds { decay: 0.0, ..e };
    assert_eq!(g.level(0.049), 1.0);
    assert_eq!(g.level(0.05), 0.0);
}

#[test]
fn mod_env_decays_to_zero() {
    let e = ModEnv {
        amount: 12.0,
        decay: 0.1,
    };
    assert_eq!(e.value(0.0), 12.0);
    assert!(close(e.value(0.05), 1.5, 1e-12));
    assert_eq!(e.value(0.1), 0.0);
    assert_eq!(e.value(1.0), 0.0);
    assert_eq!(ModEnv::OFF.value(0.0), 0.0);
}

#[test]
fn pitch_envelope_duration() {
    // No envelope: length / rate.
    assert_eq!(pitched_duration(1.0, 2.0, ModEnv::OFF), 0.5);
    // Upward bend plays faster: shorter, by at most what the envelope can save.
    let up = ModEnv {
        amount: 12.0,
        decay: 0.1,
    };
    let d = pitched_duration(1.0, 1.0, up);
    // ∫ (2^(12·(1−t/0.1)³/12) − 1) dt over 0.1 s, computed by fine numeric integration.
    let mut extra = 0.0;
    let n = 100_000;
    for i in 0..n {
        let t = (i as f64 + 0.5) / n as f64 * 0.1;
        extra += ((up.value(t) / 12.0).exp2() - 1.0) * (0.1 / n as f64);
    }
    assert!(close(d, 1.0 - extra, 1e-5), "{d} vs {}", 1.0 - extra);
    // Downward bend plays slower.
    let down = ModEnv {
        amount: -12.0,
        decay: 0.1,
    };
    assert!(pitched_duration(1.0, 1.0, down) > 1.0);
    // A slice shorter than the envelope ends inside it.
    let short = pitched_duration(0.01, 1.0, up);
    assert!(short < 0.01 && short > 0.005, "{short}");
}

fn sine_gain(cutoff: f32, q: f32, mode: SvfMode, freq: f32) -> f32 {
    let c = SvfCoefs::new(cutoff, q, SR);
    let mut f = Svf::default();
    let n = 48_000;
    let mut peak = 0.0f32;
    for i in 0..n {
        let x = (2.0 * PI * freq * i as f32 / SR).sin();
        let y = f.tick(x, &c, mode);
        if i > n / 2 {
            peak = peak.max(y.abs());
        }
    }
    peak
}

#[test]
fn svf_low_pass_response() {
    let q = BASE_Q as f32;
    assert!((sine_gain(1000.0, q, SvfMode::LowPass, 50.0) - 1.0).abs() < 0.01);
    // At the cutoff a 2-pole has gain Q.
    assert!((sine_gain(1000.0, q, SvfMode::LowPass, 1000.0) - q).abs() < 0.02);
    // −12 dB/oct: two octaves above, about 1/16.
    let g = sine_gain(1000.0, q, SvfMode::LowPass, 4000.0);
    assert!(g < 0.08, "{g}");
}

#[test]
fn svf_high_pass_and_band_pass_response() {
    let q = BASE_Q as f32;
    assert!(sine_gain(1000.0, q, SvfMode::HighPass, 60.0) < 0.01);
    assert!((sine_gain(1000.0, q, SvfMode::HighPass, 15_000.0) - 1.0).abs() < 0.05);
    // Band-pass: unity at the centre, low far off.
    assert!((sine_gain(1000.0, q, SvfMode::BandPass, 1000.0) - 1.0).abs() < 0.02);
    assert!(sine_gain(1000.0, q, SvfMode::BandPass, 50.0) < 0.1);
    assert!(sine_gain(1000.0, q, SvfMode::BandPass, 16_000.0) < 0.1);
}

#[test]
fn svf_resonance_raises_the_peak() {
    let lo = sine_gain(1000.0, resonance_q(0.0) as f32, SvfMode::LowPass, 1000.0);
    let hi = sine_gain(1000.0, resonance_q(1.0) as f32, SvfMode::LowPass, 1000.0);
    assert!(hi > lo * 10.0, "{lo} → {hi}");
    assert!(close(resonance_q(0.0), BASE_Q, 1e-9));
}

#[test]
fn svf_stays_stable_under_fast_modulation() {
    let mut f = Svf::default();
    let q = resonance_q(1.0) as f32;
    let mut peak = 0.0f32;
    for i in 0..96_000 {
        // Cutoff sweeping 30 Hz ↔ 20 kHz every 10 ms.
        let ph = (i % 480) as f32 / 480.0;
        let cutoff = 30.0 * (20_000.0f32 / 30.0).powf(ph);
        let c = SvfCoefs::new(cutoff, q, SR);
        let x = if i % 100 < 50 { 1.0 } else { -1.0 };
        let y = f.tick(x, &c, SvfMode::LowPass);
        assert!(y.is_finite());
        peak = peak.max(y.abs());
    }
    assert!(peak < 100.0, "{peak}");
}

#[test]
fn drive_zero_is_an_exact_bypass() {
    let d = Drive::new(0.0);
    assert!(d.is_bypass());
    for x in [-1.0f32, -0.3, 0.0, 1e-30, 0.123_456_79, 1.0, 3.0] {
        assert_eq!(d.tick(x).to_bits(), x.to_bits());
    }
}

#[test]
fn drive_soft_clips_without_a_step() {
    let full = Drive::new(1.0);
    // Peaks stay at ±1, the curve is odd and monotonic, quiet input is lifted.
    assert!((full.tick(1.0) - 1.0).abs() < 1e-6);
    assert!((full.tick(-1.0) + 1.0).abs() < 1e-6);
    assert!(full.tick(0.05) > 0.5, "{}", full.tick(0.05));
    let mut prev = -2.0f32;
    for i in -100..=100 {
        let y = full.tick(i as f32 / 100.0);
        assert!(y >= prev);
        assert!(y.abs() <= 1.0 + 1e-6);
        prev = y;
    }
    // Smooth: a tiny amount barely changes the signal.
    let tiny = Drive::new(0.001);
    assert!((tiny.tick(0.5) - 0.5).abs() < 0.01);
    // Smooth curve: no corner (second difference stays small everywhere).
    let h = 1e-3f32;
    for i in -900..900 {
        let x = i as f32 / 1000.0;
        let d2 = full.tick(x + h) - 2.0 * full.tick(x) + full.tick(x - h);
        assert!(d2.abs() < 0.01, "corner at {x}: {d2}");
    }
}

#[test]
fn smoother_glides_then_snaps() {
    let mut s = Smoother::new(0.0);
    let (a, b) = s.block(1.0, 128, 48_000.0);
    assert_eq!(a, 0.0);
    assert!(b > 0.0 && b < 0.5, "{b}");
    for _ in 0..1000 {
        s.block(1.0, 128, 48_000.0);
    }
    assert_eq!(s.value, 1.0);
    for _ in 0..1000 {
        s.block(0.0, 128, 48_000.0);
    }
    assert_eq!(s.value, 0.0);
}

/// Fine numeric reference: when `rate · 2^(env/12)` integrated from 0 reaches `length`.
fn reference_duration(length: f64, rate: f64, env: ModEnv) -> f64 {
    let dt = env.decay / 50_000.0;
    let (mut t, mut read) = (0.0, 0.0);
    loop {
        let r = rate * (env.value(t + dt * 0.5) / 12.0).exp2();
        if read + r * dt >= length {
            return t + (length - read) / r;
        }
        read += r * dt;
        t += dt;
        if t >= env.decay {
            return t + (length - read) / rate;
        }
    }
}

#[test]
fn pitch_envelope_duration_is_closed_form_over_the_whole_range() {
    // the model's pitch range (−24..48 st) and decay range (5 ms..2 s), slices ending inside
    // and after the envelope, at several rates
    for &amount in &[-24.0, -7.0, 3.0, 12.0, 48.0] {
        for &decay in &[0.005, 0.1, 2.0] {
            for &length in &[0.002, 0.05, 0.7, 3.0] {
                for &rate in &[0.5, 1.0, 2.0] {
                    let env = ModEnv { amount, decay };
                    let got = pitched_duration(length, rate, env);
                    let want = reference_duration(length, rate, env);
                    assert!(
                        close(got, want, 1e-6 * want.max(1.0)),
                        "{amount} st, {decay} s, {length} s @ {rate}: {got} vs {want}"
                    );
                }
            }
        }
    }
    // the series itself: e^0 integrates to u
    assert!(close(bend_integral(0.0, 0.3), 0.3, 1e-15));
}
