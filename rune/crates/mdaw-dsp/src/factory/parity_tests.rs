//! Factory voices vs the Chromium reference (`rune/parity/fixtures/factory_reference.json`,
//! captured by `rune/parity/chromium/capture-factory.steps.mjs` from the app's real render path
//! with real Tone.js). Statistic definitions mirror `rune/parity/chromium/stats.mjs`.
//!
//! - seeded: the capture replaced `Math.random` by mulberry32 with the seeds of
//!   [`NoiseSeeds::factory`], so every voice is deterministic and must match closely:
//!   exact length, peak ±0.5 dB, 5 ms envelope ±1 dB where the reference is above −40 dBFS,
//!   spectral centroids ±5 %, correlation of the first 2048 samples ≥ 0.99.
//! - random: the app as it runs (real `Math.random`), 8 renders per voice and rate; compared
//!   statistically with 8 Rust renders using other offset seeds (see `random_statistics`).

use super::*;
use serde_json::Value;

const RATES: [u32; 2] = [44100, 48000];
const FRAME_SEC: f64 = 0.005;
const CENTROID_N: usize = 2048;
const HEAD: usize = 2048;

fn reference() -> Value {
    let p = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../parity/fixtures/factory_reference.json"
    );
    serde_json::from_str(&std::fs::read_to_string(p).expect("factory_reference.json")).unwrap()
}

fn to_db(x: f64) -> f64 {
    if x > 1e-10 { 20.0 * x.log10() } else { -200.0 }
}

fn envelope(x: &[f32], sr: u32) -> Vec<f64> {
    let frame = (FRAME_SEC * sr as f64).round() as usize;
    x.chunks(frame)
        .map(|c| (c.iter().map(|&v| v as f64 * v as f64).sum::<f64>() / c.len() as f64).sqrt())
        .collect()
}

fn fft_mag(input: &[f64]) -> Vec<f64> {
    let n = input.len();
    let mut re = input.to_vec();
    let mut im = vec![0f64; n];
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * std::f64::consts::PI / len as f64;
        for i in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (wr, wi) = ((ang * k as f64).cos(), (ang * k as f64).sin());
                let (ar, ai) = (re[i + k + len / 2], im[i + k + len / 2]);
                let (tr, ti) = (ar * wr - ai * wi, ar * wi + ai * wr);
                re[i + k + len / 2] = re[i + k] - tr;
                im[i + k + len / 2] = im[i + k] - ti;
                re[i + k] += tr;
                im[i + k] += ti;
            }
        }
        len <<= 1;
    }
    (0..=n / 2).map(|k| re[k].hypot(im[k])).collect()
}

fn centroids(x: &[f32], sr: u32) -> [f64; 4] {
    let n = CENTROID_N;
    let mut out = [0f64; 4];
    for (w, o) in out.iter_mut().enumerate() {
        let start = w * x.len() / 4;
        let buf: Vec<f64> = (0..n)
            .map(|i| {
                let v = x.get(start + i).copied().unwrap_or(0.0) as f64;
                v * (0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos())
            })
            .collect();
        let mag = fft_mag(&buf);
        let (mut num, mut den) = (0.0, 0.0);
        for (k, m) in mag.iter().enumerate() {
            num += k as f64 * sr as f64 / n as f64 * m * m;
            den += m * m;
        }
        *o = if den > 0.0 { num / den } else { 0.0 };
    }
    out
}

struct Stats {
    length: usize,
    peak: f64,
    rms: f64,
    env: Vec<f64>,
    centroid: [f64; 4],
    onset: i64,
    zcr: f64,
}

fn stats(x: &[f32], sr: u32) -> Stats {
    let peak = x.iter().fold(0f64, |m, &v| m.max((v as f64).abs()));
    let rms = (x.iter().map(|&v| v as f64 * v as f64).sum::<f64>() / x.len().max(1) as f64).sqrt();
    let onset = x
        .iter()
        .position(|&v| (v as f64).abs() >= 0.1 * peak)
        .map_or(-1, |i| i as i64);
    let zc = x
        .windows(2)
        .filter(|w| (w[1] < 0.0) != (w[0] < 0.0))
        .count();
    Stats {
        length: x.len(),
        peak,
        rms,
        env: envelope(x, sr),
        centroid: centroids(x, sr),
        onset,
        zcr: zc as f64 / (x.len() as f64 / sr as f64),
    }
}

fn floats(v: &Value) -> Vec<f64> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_f64().unwrap())
        .collect()
}

fn correlation(a: &[f32], b: &[f64]) -> f64 {
    let n = a.len().min(b.len());
    let (mut ab, mut aa, mut bb) = (0.0, 0.0, 0.0);
    for i in 0..n {
        let x = a[i] as f64;
        ab += x * b[i];
        aa += x * x;
        bb += b[i] * b[i];
    }
    if aa == 0.0 && bb == 0.0 {
        1.0
    } else {
        ab / (aa * bb).sqrt().max(1e-300)
    }
}

/// Worst |Δ dB| over envelope frames where the reference is above −40 dBFS (frames past the
/// end of the shorter signal count as silence).
fn env_error(ours: &[f64], reference_db: &[f64]) -> f64 {
    let mut worst = 0f64;
    for (i, &r) in reference_db.iter().enumerate() {
        if r < -40.0 {
            continue;
        }
        let o = to_db(ours.get(i).copied().unwrap_or(0.0));
        worst = worst.max((o - r).abs());
    }
    worst
}

fn centroid_error(ours: &[f64; 4], reference: &[f64]) -> f64 {
    ours.iter()
        .zip(reference)
        .filter(|(_, r)| **r > 0.0)
        .map(|(o, r)| (o - r).abs() / r)
        .fold(0.0, f64::max)
}

/// The capture recorded the noise start offset Tone passed to `AudioBufferSourceNode.start` and
/// the first noise sample of each channel: Rust's seeded draws must reproduce them exactly.
#[test]
fn seeded_noise_draws_match_chromium() {
    let reference = reference();
    for sr in RATES {
        let first = floats(&reference["meta"][format!("noiseFirst_{sr}")]);
        let buf = NoiseBuffer::white(FACTORY_NOISE_SEED);
        assert_eq!(buf.channels[0][0] as f64, first[0]);
        assert_eq!(buf.channels[1][0] as f64, first[1]);
        for id in FactoryVoiceId::ALL {
            let r = &reference["voices"][id.id()][sr.to_string()]["seeded"];
            assert_eq!(r["randomCalls"].as_u64(), Some(1), "{}", id.id());
            let draw = Mulberry32::new(NoiseSeeds::factory(id).offset).next_f64();
            let offset = draw * (noise::NOISE_BUFFER_FRAMES as f64 / sr as f64 - 0.001);
            assert_eq!(
                offset,
                r["noiseOffsetSec"].as_f64().unwrap(),
                "{} @ {sr}",
                id.id()
            );
        }
    }
}

#[test]
fn seeded_voices_match_chromium() {
    let reference = reference();
    let mut failures = Vec::new();
    println!();
    println!(
        "factory voices vs Chromium (seeded noise). Thresholds: length exact, peak ±0.5 dB, env ±1 dB (frames > −40 dBFS), \
         centroid ±5 %, corr(first 2048) ≥ 0.99. Also shown: RMS Δ, max |Δ| of the first 2048 samples, onset, zero crossings/s."
    );
    println!(
        "{:<8} {:>5} | {:>7} {:>7} | {:>7} {:>6} | {:>6} | {:>6} | {:>6} | {:>8} | {:>8} | {:>5} {:>5} | {:>7} {:>7}",
        "voice",
        "sr",
        "len",
        "ref",
        "peak dB",
        "Δpeak",
        "Δrms",
        "Δenv",
        "Δcent%",
        "corr2048",
        "maxΔhead",
        "onset",
        "ref",
        "zcr",
        "ref"
    );
    for id in FactoryVoiceId::ALL {
        for sr in RATES {
            let r = &reference["voices"][id.id()][sr.to_string()]["seeded"];
            let buf = render_factory_voice(id, sr);
            let x = &buf.channels[0];
            let s = stats(x, sr);
            let ref_len = r["length"].as_u64().unwrap() as usize;
            let ref_peak = r["peak"].as_f64().unwrap();
            let d_peak = (to_db(s.peak) - to_db(ref_peak)).abs();
            let d_rms = (to_db(s.rms) - to_db(r["rms"].as_f64().unwrap())).abs();
            let d_env = env_error(&s.env, &floats(&r["env_db"]));
            let d_cent = centroid_error(&s.centroid, &floats(&r["centroid"]));
            let head = floats(&r["head"]);
            let corr = correlation(&x[..HEAD.min(x.len())], &head);
            let max_head = x
                .iter()
                .zip(&head)
                .map(|(a, b)| (*a as f64 - b).abs())
                .fold(0.0, f64::max);
            println!(
                "{:<8} {:>5} | {:>7} {:>7} | {:>7.2} {:>6.3} | {:>6.3} | {:>6.3} | {:>6.2} | {:>8.6} | {:>8.1e} | {:>5} {:>5} | {:>7.1} {:>7.1}",
                id.id(),
                sr,
                s.length,
                ref_len,
                to_db(s.peak),
                d_peak,
                d_rms,
                d_env,
                d_cent * 100.0,
                corr,
                max_head,
                s.onset,
                r["onset"],
                s.zcr,
                r["zcr"].as_f64().unwrap()
            );
            if s.length != ref_len || d_peak > 0.5 || d_env > 1.0 || d_cent > 0.05 || corr < 0.99 {
                failures.push(format!("{} @ {sr}", id.id()));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "voices outside the thresholds: {failures:?}"
    );
}

/// Mean and sample standard deviation.
fn mean_sd(v: &[f64]) -> (f64, f64) {
    let n = v.len() as f64;
    let m = v.iter().sum::<f64>() / n;
    let var = if v.len() > 1 {
        v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (n - 1.0)
    } else {
        0.0
    };
    (m, var.sqrt())
}

/// |Δmean| allowed between two independent samples: 3 standard errors (Welch) + `floor`.
fn allowed(a: &[f64], b: &[f64], floor: f64) -> f64 {
    let (_, sa) = mean_sd(a);
    let (_, sb) = mean_sd(b);
    3.0 * (sa * sa / a.len() as f64 + sb * sb / b.len() as f64).sqrt() + floor
}

/// Rust renders per voice and rate for the statistical comparison.
const RUST_TRIALS: usize = 16;

/// The app's real (unseeded) renders: Tone.Noise draws its start offset (and, once per page, its
/// buffer) from `Math.random`, so noise voices differ per render in level, peak and tail length.
/// Rust is compared as a population: 16 renders with other offset seeds vs the 8 Chromium
/// renders. Per-trial RMS (dB), peak (dB) and length must agree in the mean within 3 standard
/// errors (+0.1 dB / +0.1 dB / +0.5 % for the deterministic voices whose spread is 0); the
/// same rule (+1 %) for the mean spectral centroid of each window. `Δcent%` prints the largest
/// relative difference of the window means.
#[test]
fn random_statistics_match_chromium() {
    let reference = reference();
    let mut failures = Vec::new();
    println!();
    println!(
        "factory voices vs Chromium, real Math.random (8 Chromium renders, {RUST_TRIALS} Rust renders); mean ± sd"
    );
    println!(
        "{:<8} {:>5} | {:>15} {:>15} | {:>13} {:>13} {:>5} | {:>13} {:>13} {:>5} | {:>6}",
        "voice", "sr", "length", "ref", "rms dB", "ref", "tol", "peak dB", "ref", "tol", "Δcent%"
    );
    for (vi, id) in FactoryVoiceId::ALL.into_iter().enumerate() {
        for sr in RATES {
            let r = &reference["voices"][id.id()][sr.to_string()]["random"];
            let (mut lens, mut rmss, mut peaks) = (Vec::new(), Vec::new(), Vec::new());
            let mut cents: [Vec<f64>; 4] = Default::default();
            for k in 0..RUST_TRIALS {
                let seeds = NoiseSeeds {
                    buffer: FACTORY_NOISE_SEED,
                    offset: 0x0bad_0000 + (vi * 64 + k) as u32,
                };
                let buf = render_factory_voice_seeded(id, sr, seeds);
                let s = stats(&buf.channels[0], sr);
                lens.push(s.length as f64);
                rmss.push(to_db(s.rms));
                peaks.push(to_db(s.peak));
                for (c, v) in cents.iter_mut().zip(s.centroid) {
                    c.push(v);
                }
            }
            let ref_lens = floats(&r["length"]);
            let ref_rmss: Vec<f64> = floats(&r["rms"]).iter().map(|&x| to_db(x)).collect();
            let ref_peaks: Vec<f64> = floats(&r["peak"]).iter().map(|&x| to_db(x)).collect();
            let ((len, len_sd), (rlen, rlen_sd)) = (mean_sd(&lens), mean_sd(&ref_lens));
            let ((rms, rms_sd), (rrms, rrms_sd)) = (mean_sd(&rmss), mean_sd(&ref_rmss));
            let ((pk, pk_sd), (rpk, rpk_sd)) = (mean_sd(&peaks), mean_sd(&ref_peaks));
            let tol_len = allowed(&lens, &ref_lens, 0.005 * rlen);
            let tol_rms = allowed(&rmss, &ref_rmss, 0.1);
            let tol_pk = allowed(&peaks, &ref_peaks, 0.1);
            // Centroids per window: |Δmean| ≤ 3 SE + 1 % (windows where the reference has signal).
            let ref_cents: Vec<Vec<f64>> = r["centroid"]
                .as_array()
                .unwrap()
                .iter()
                .map(floats)
                .collect();
            let (mut d_cent, mut cent_ok) = (0f64, true);
            for (w, ours) in cents.iter().enumerate() {
                let theirs: Vec<f64> = ref_cents.iter().map(|t| t[w]).collect();
                let ((m, _), (rm, _)) = (mean_sd(ours), mean_sd(&theirs));
                if rm <= 0.0 {
                    continue;
                }
                d_cent = d_cent.max((m - rm).abs() / rm);
                cent_ok &= (m - rm).abs() <= allowed(ours, &theirs, 0.01 * rm);
            }
            println!(
                "{:<8} {:>5} | {:>8.0} ±{:>5.0} {:>8.0} ±{:>5.0} | {:>6.2} ±{:>5.2} {:>6.2} ±{:>5.2} {:>5.2} | {:>6.2} ±{:>5.2} {:>6.2} ±{:>5.2} {:>5.2} | {:>6.2}",
                id.id(),
                sr,
                len,
                len_sd,
                rlen,
                rlen_sd,
                rms,
                rms_sd,
                rrms,
                rrms_sd,
                tol_rms,
                pk,
                pk_sd,
                rpk,
                rpk_sd,
                tol_pk,
                d_cent * 100.0
            );
            if (len - rlen).abs() > tol_len
                || (rms - rrms).abs() > tol_rms
                || (pk - rpk).abs() > tol_pk
                || !cent_ok
            {
                failures.push(format!("{} @ {sr}", id.id()));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "voices outside the statistical thresholds: {failures:?}"
    );
}

/// Full-waveform debug comparison against raw captures (`PARITY_DEBUG_DIR=<dir> cargo test
/// -p mdaw-dsp debug_full_waveforms -- --ignored --nocapture`).
#[test]
#[ignore]
fn debug_full_waveforms() {
    let Ok(dir) = std::env::var("PARITY_DEBUG_DIR") else {
        return;
    };
    for id in FactoryVoiceId::ALL {
        for sr in RATES {
            let bytes = std::fs::read(format!("{dir}/{}_{sr}.f32", id.id())).unwrap();
            let refx: Vec<f64> = bytes
                .chunks(4)
                .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64)
                .collect();
            let x = render_factory_voice(id, sr).channels.remove(0);
            let n = x.len().min(refx.len());
            let (max_at, max_diff) = (0..n)
                .map(|i| (i, (x[i] as f64 - refx[i]).abs()))
                .fold((0, 0.0), |a, b| if b.1 > a.1 { b } else { a });
            let corr = correlation(&x[..n], &refx[..n]);
            let err_db = {
                let e: f64 = (0..n).map(|i| (x[i] as f64 - refx[i]).powi(2)).sum();
                let s: f64 = refx[..n].iter().map(|v| v * v).sum();
                10.0 * (e / s).log10()
            };
            println!(
                "{:<8} {sr}: len {} vs {}, max |Δ| {max_diff:.2e} @ {max_at}, corr {corr:.6}, error/signal {err_db:.1} dB",
                id.id(),
                x.len(),
                refx.len()
            );
            if std::env::var("PARITY_DEBUG_SEGMENTS").is_ok_and(|v| v == id.id()) {
                for seg in (0..n).step_by(sr as usize / 200) {
                    let e = (seg..(seg + sr as usize / 200).min(n))
                        .map(|i| (x[i] as f64 - refx[i]).powi(2))
                        .sum::<f64>();
                    let s = (seg..(seg + sr as usize / 200).min(n))
                        .map(|i| refx[i].powi(2))
                        .sum::<f64>();
                    println!(
                        "    {:.3}s: signal {:.1} dB, error/signal {:.1} dB",
                        seg as f64 / sr as f64,
                        10.0 * s.log10(),
                        10.0 * (e / s).log10()
                    );
                }
            }
        }
    }
}
