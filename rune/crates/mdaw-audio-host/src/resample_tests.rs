use super::*;
use std::f64::consts::TAU;

/// Resample a sine of `freq` Hz (amplitude 0.5) from `cr` to `dr` in odd-sized blocks; returns
/// the output and the resampler.
fn sine_through(cr: u32, dr: u32, freq: f64, out_frames: usize) -> (Vec<f32>, Resampler) {
    let mut rs = Resampler::new(cr, dr, 300);
    let mut n_in = 0u64;
    let mut pull = |l: &mut [f32], r: &mut [f32]| {
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            let v = (0.5 * (TAU * freq * n_in as f64 / cr as f64).sin()) as f32;
            *a = v;
            *b = -v;
            n_in += 1;
        }
    };
    let mut out = vec![0f32; out_frames];
    let mut right = vec![0f32; out_frames];
    let mut done = 0;
    for size in [1usize, 37, 512, 1000, 4096].iter().cycle() {
        if done == out_frames {
            break;
        }
        let n = (*size).min(out_frames - done);
        rs.process(
            &mut out[done..done + n],
            &mut right[done..done + n],
            &mut pull,
        );
        done += n;
    }
    for (l, r) in out.iter().zip(&right) {
        assert_eq!(*l, -*r, "channels are independent and identical");
    }
    (out, rs)
}

fn rms(x: impl Iterator<Item = f64>) -> f64 {
    let (mut s, mut n) = (0.0, 0usize);
    for v in x {
        s += v * v;
        n += 1;
    }
    (s / n as f64).sqrt()
}

fn db(x: f64) -> f64 {
    20.0 * x.log10()
}

#[test]
fn sine_error_is_below_minus_80_db() {
    for (cr, dr) in [
        (48_000u32, 44_100u32),
        (44_100, 48_000),
        (48_000, 96_000),
        (96_000, 44_100),
        (44_100, 47_999),
    ] {
        let frames = dr as usize; // 1 s
        let (out, rs) = sine_through(cr, dr, 1000.0, frames);
        // Output n is centred on input time n·cr/dr = n/dr s: compare with the ideal sine,
        // skipping the start-up transient (zero history) of one kernel length.
        let skip = rs.taps() * 2;
        let ideal = |n: usize| 0.5 * (TAU * 1000.0 * n as f64 / dr as f64).sin();
        let err = rms((skip..frames).map(|n| out[n] as f64 - ideal(n)));
        let sig = rms((skip..frames).map(ideal));
        let e = db(err / sig);
        eprintln!("{cr} -> {dr}: 1 kHz error {e:.1} dB ({} taps)", rs.taps());
        assert!(e <= -80.0, "{cr} -> {dr}: error {e:.1} dB");
    }
}

#[test]
fn tones_above_the_output_nyquist_are_rejected() {
    for (cr, dr, f) in [
        (48_000u32, 44_100u32, 23_000.0),
        (48_000, 44_100, 22_500.0),
        (96_000, 48_000, 30_000.0),
        (96_000, 44_100, 40_000.0),
    ] {
        let frames = dr as usize / 2;
        let (out, rs) = sine_through(cr, dr, f, frames);
        let skip = rs.taps() * 2;
        let level = rms((skip..frames).map(|n| out[n] as f64));
        let att = db(level / (0.5 / 2f64.sqrt()));
        eprintln!("{cr} -> {dr}: {f} Hz attenuated to {att:.1} dB");
        assert!(att <= -60.0, "{cr} -> {dr}: {f} Hz only {att:.1} dB");
    }
}

#[test]
fn step_is_exact_rational() {
    let mut rs = Resampler::new(48_000, 44_100, 64);
    let (mut l, mut r) = (vec![0f32; 4410], vec![0f32; 4410]);
    let mut pull = |_: &mut [f32], _: &mut [f32]| {};
    for _ in 0..100 {
        rs.process(&mut l, &mut r, &mut pull);
    }
    // 441 000 outputs at 48/44.1: the next output sits exactly on input 480 000.
    assert_eq!(rs.produced(), 441_000);
    let next_pos = rs.consumed() as i64 - (rs.len as i64 - rs.pos as i64);
    assert_eq!((next_pos, rs.frac), (480_000, 0));
    // Exactly floor((n - 1)·M/L) + latency + 1 frames pulled.
    let expect = (441_000u64 - 1) * 160 / 147 + rs.latency_frames() as u64 + 1;
    assert_eq!(rs.consumed(), expect);
}
