//! Convolver tests: exactness vs an f64 direct convolution (every partition boundary, mono IR,
//! zero partitions, the silence path), no allocation in `process`, the Chromium `ConvolverNode`
//! reference (`rune/parity/fixtures/fx_convolver.json`, captured by
//! `rune/parity/chromium/capture-convolver.steps.mjs`) and an `#[ignore]` CPU benchmark:
//! `cargo test -p mdaw-engine --release fx::convolver -- --ignored --nocapture`.

use super::*;
use mdaw_dsp::factory::Mulberry32;
use mdaw_dsp::reverb_ir::{
    REVERB_DEFAULTS, ReverbOptions, convolver_normalization_scale, create_impulse_response,
};
use serde_json::Value;

/// Run `l`/`r` (any length; padded to whole quanta) through `c`.
fn run(c: &mut Convolver, l: &[f32], r: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let frames = l.len().div_ceil(BLOCK) * BLOCK;
    let (mut ol, mut or) = (vec![0f32; frames], vec![0f32; frames]);
    let (mut il, mut ir) = ([0f32; BLOCK], [0f32; BLOCK]);
    for q in 0..frames / BLOCK {
        for i in 0..BLOCK {
            il[i] = l.get(q * BLOCK + i).copied().unwrap_or(0.0);
            ir[i] = r.get(q * BLOCK + i).copied().unwrap_or(0.0);
        }
        let (a, b) = (&mut [0f32; BLOCK], &mut [0f32; BLOCK]);
        c.process(&il, &ir, a, b);
        ol[q * BLOCK..(q + 1) * BLOCK].copy_from_slice(a);
        or[q * BLOCK..(q + 1) * BLOCK].copy_from_slice(b);
    }
    (ol, or)
}

/// f64 direct convolution `(x ⊛ scale·h)[0..frames)` (skips zero input samples).
fn direct(x: &[f32], h: &[f32], scale: f64, frames: usize) -> Vec<f64> {
    let mut y = vec![0f64; frames];
    for (i, &v) in x.iter().enumerate() {
        if v == 0.0 {
            continue;
        }
        let v = v as f64 * scale;
        for (j, &hv) in h.iter().enumerate().take(frames.saturating_sub(i)) {
            y[i + j] += v * hv as f64;
        }
    }
    y
}

fn err_db(ours: &[&[f32]], refs: &[&[f64]]) -> f64 {
    let (mut e, mut s) = (0f64, 0f64);
    for (o, r) in ours.iter().zip(refs) {
        for (&a, &b) in o.iter().zip(r.iter()) {
            e += (a as f64 - b).powi(2);
            s += b * b;
        }
    }
    10.0 * (e.max(1e-300) / s).log10()
}

struct Rng(Mulberry32);
impl Rng {
    fn new(seed: u32) -> Self {
        Self(Mulberry32::new(seed))
    }
    fn uni(&mut self) -> f32 {
        (self.0.next_f64() * 2.0 - 1.0) as f32
    }
}

/// Input with a noise burst, odd-offset impulses, a gap long enough to go idle, then more input
/// and a tail long enough to flush the IR.
fn test_input(rng: &mut Rng, idle_after: usize, ir_len: usize) -> (Vec<f32>, Vec<f32>, usize) {
    let gap_start = 2600;
    let resume = gap_start + idle_after + 3 * BLOCK + 77;
    let frames = resume + 1500 + ir_len + 2 * BLOCK;
    let (mut l, mut r) = (vec![0f32; frames], vec![0f32; frames]);
    for i in 0..2000 {
        l[i] = rng.uni() * 0.5;
        r[i] = if i > 300 { rng.uni() * 0.3 } else { 0.0 };
    }
    l[2101] = 0.9;
    r[2333] = -0.7;
    l[2599] = 0.25;
    for i in resume..resume + 1500 {
        l[i] = rng.uni();
        r[i] = (i as f32 * 0.05).sin() * 0.4;
    }
    (l, r, resume)
}

fn random_ir(
    rng: &mut Rng,
    len: usize,
    lead_zeros: usize,
    hole: Option<(usize, usize)>,
) -> Vec<f32> {
    (0..len)
        .map(|i| {
            let v = rng.uni() * (-(i as f32) / (len as f32 * 0.3)).exp();
            let in_hole = hole.is_some_and(|(a, b)| (a..b).contains(&i));
            if i < lead_zeros || in_hole { 0.0 } else { v }
        })
        .collect()
}

#[test]
fn matches_direct_convolution_at_every_partition_boundary() {
    // (IR length, leading zeros, zero hole, stage sizes (None = cost model), mono)
    type Case = (
        usize,
        usize,
        Option<(usize, usize)>,
        Option<&'static [usize]>,
        bool,
    );
    let cases: &[Case] = &[
        (1, 0, None, None, false),
        (127, 0, None, Some(&[]), false),
        (128, 0, None, Some(&[]), true),
        (129, 5, None, Some(&[]), false),
        (1791, 0, None, Some(&[1024]), false),
        (1793, 0, None, Some(&[1024]), false),
        (5000, 0, None, Some(&[256, 512, 1024]), false),
        (9000, 300, Some((1000, 5000)), Some(&[256, 2048]), false),
        (3000, 0, None, Some(&[512]), true),
        (7000, 2000, None, Some(&[1024, 2048]), false),
        (20000, 0, None, None, false),
        (20000, 1056, None, None, true),
    ];
    let mut rng = Rng::new(0x5eed_0001);
    for (n, &(len, lead, hole, sizes, mono)) in cases.iter().enumerate() {
        let hl = random_ir(&mut rng, len, lead, hole);
        let hr = if mono {
            hl.clone()
        } else {
            random_ir(&mut rng, len, lead, hole)
        };
        let ir = if mono {
            AudioBuffer::mono(48000, hl.clone())
        } else {
            AudioBuffer::from_channels(48000, vec![hl.clone(), hr.clone()]).unwrap()
        };
        let scale = 0.37f32;
        let mut c = match sizes {
            Some(s) => Convolver::with_stage_sizes(&ir, scale, s),
            None => Convolver::new(&ir, scale),
        };
        assert!(c.is_idle(), "a new convolver starts idle");
        let (l, r, resume) = test_input(&mut rng, c.idle_after, len);
        let (ol, or) = run(&mut c, &l, &r);
        let frames = ol.len();
        let (rl, rr) = (
            direct(&l, &hl, scale as f64, frames),
            direct(&r, &hr, scale as f64, frames),
        );
        let db = err_db(&[&ol, &or], &[&rl, &rr]);
        // Idle by the end of the gap: output exactly zero there.
        let gap_end = resume / BLOCK * BLOCK;
        let idle_from = gap_end - 2 * BLOCK;
        assert!(
            ol[idle_from..gap_end]
                .iter()
                .chain(&or[idle_from..gap_end])
                .all(|&x| x == 0.0)
        );
        println!(
            "case {n}: IR {len} (stages {:?}) error {db:.1} dB, {:.0} weight/quantum",
            c.stage_sizes(),
            c.weight_per_quantum()
        );
        assert!(db <= -120.0, "case {n}: error {db:.1} dB");
        // Idle check by running a long silence through: must go idle and stay exactly zero.
        let z = vec![0f32; c.idle_after + 2 * BLOCK];
        let (zl, zr) = run(&mut c, &z, &z);
        assert!(c.is_idle());
        assert!(
            zl[zl.len() - BLOCK..]
                .iter()
                .chain(&zr[zr.len() - BLOCK..])
                .all(|&x| x == 0.0)
        );
        // reset() == fresh.
        c.reset();
        let (al, ar) = run(&mut c, &l[..3000], &r[..3000]);
        let mut fresh = match sizes {
            Some(s) => Convolver::with_stage_sizes(&ir, scale, s),
            None => Convolver::new(&ir, scale),
        };
        let (bl, br) = run(&mut fresh, &l[..3000], &r[..3000]);
        assert_eq!(
            (al, ar),
            (bl, br),
            "case {n}: reset differs from a new convolver"
        );
    }
}

#[test]
fn room_reverb_ir_with_sparse_input_matches_direct() {
    for sr in [44100u32, 48000, 96000] {
        let ir = create_impulse_response(sr, &REVERB_DEFAULTS);
        let scale = convolver_normalization_scale(&ir) as f32;
        let mut c = Convolver::new(&ir, scale);
        let len = ir.frames();
        let frames = len + 30_000;
        let (mut l, mut r) = (vec![0f32; frames], vec![0f32; frames]);
        let mut rng = Rng::new(sr);
        for i in 0..600 {
            l[1000 + i] = rng.uni() * 0.3;
            r[9000 + i] = rng.uni() * 0.3;
        }
        l[3] = 1.0;
        r[20_001] = -0.5;
        let (ol, or) = run(&mut c, &l, &r);
        let rl = direct(&l, &ir.channels[0], scale as f64, ol.len());
        let rr = direct(&r, &ir.channels[1], scale as f64, ol.len());
        let db = err_db(&[&ol, &or], &[&rl, &rr]);
        println!(
            "RoomReverb IR at {sr} Hz ({len} frames): stages {:?}, error {db:.1} dB",
            c.stage_sizes()
        );
        assert!(db <= -120.0, "{sr}: {db:.1} dB");
    }
}

#[test]
fn process_never_allocates() {
    let ir = create_impulse_response(48000, &REVERB_DEFAULTS);
    let mut c = Convolver::new(&ir, convolver_normalization_scale(&ir) as f32);
    let mut rng = Rng::new(7);
    let (mut l, mut r) = ([0f32; BLOCK], [0f32; BLOCK]);
    let (mut ol, mut or) = ([0f32; BLOCK], [0f32; BLOCK]);
    let n = crate::test_alloc::allocations_during(|| {
        for q in 0..3000 {
            for i in 0..BLOCK {
                l[i] = if q < 1500 { rng.uni() } else { 0.0 };
                r[i] = if q < 1500 { rng.uni() } else { 0.0 };
            }
            c.process(&l, &r, &mut ol, &mut or);
        }
        c.reset();
        c.process(&l, &r, &mut ol, &mut or);
    });
    assert!(c.is_idle());
    assert_eq!(n, 0, "process allocated");
}

#[test]
fn scheduler_spreads_work_evenly() {
    let ir = create_impulse_response(48000, &REVERB_DEFAULTS);
    let mut c = Convolver::new(&ir, convolver_normalization_scale(&ir) as f32);
    let (l, r) = ([0.1f32; BLOCK], [0.1f32; BLOCK]);
    let (mut ol, mut or) = ([0f32; BLOCK], [0f32; BLOCK]);
    let (mut max, mut sum, mut prev) = (0u64, 0u64, 0u64);
    for q in 0..4096 {
        c.process(&l, &r, &mut ol, &mut or);
        let d = c.work_done() - prev;
        prev = c.work_done();
        if q >= 256 {
            max = max.max(d);
            sum += d;
        }
    }
    let mean = sum as f64 / (4096 - 256) as f64;
    println!(
        "stages {:?}: op weight per quantum mean {mean:.0} (model {:.0}), max {max} (max/mean {:.3})",
        c.stage_sizes(),
        c.weight_per_quantum(),
        max as f64 / mean
    );
    assert!((max as f64) < 1.15 * mean);
}

// ---------------------------------------------------------------------------------------------
// Chromium parity.

const EV_BLOCK: usize = 1024;
const PROJ: usize = 16;
const PROJ_SEED: u32 = 0x9a0f_0001;
const MAX_ERR_DB: f64 = -60.0;
const MAX_BLOCK_ENERGY_DB: f64 = 0.5;
const LEVEL_FLOOR_DB: f64 = -100.0;

/// `blockEvidence` of stats.mjs.
fn block_evidence(x: &[f32]) -> Vec<[f64; PROJ + 1]> {
    let mut rng = Mulberry32::new(PROJ_SEED);
    x.chunks(EV_BLOCK)
        .map(|b| {
            let mut out = [0f64; PROJ + 1];
            out[0] = b.iter().map(|&v| v as f64 * v as f64).sum();
            for p in &mut out[1..] {
                for w in b.chunks(32) {
                    let word = (rng.next_f64() * 4_294_967_296.0) as u32;
                    for (t, &v) in w.iter().enumerate() {
                        let v = v as f64;
                        *p += if (word >> t) & 1 == 1 { -v } else { v };
                    }
                }
            }
            out
        })
        .collect()
}

/// (error energy, signal energy, worst |block energy difference| dB above the floor).
fn channel_error(x: &[f32], reference: &[f64]) -> (f64, f64, f64) {
    let ours = block_evidence(x);
    assert_eq!(ours.len() * (PROJ + 1), reference.len(), "evidence size");
    let floor = 10f64.powf(LEVEL_FLOOR_DB / 10.0);
    let (mut err, mut sig, mut worst) = (0.0, 0.0, 0f64);
    for (i, (o, r)) in ours.iter().zip(reference.chunks(PROJ + 1)).enumerate() {
        let len = EV_BLOCK.min(x.len() - i * EV_BLOCK) as f64;
        err += (1..=PROJ).map(|k| (o[k] - r[k]).powi(2)).sum::<f64>() / PROJ as f64;
        sig += r[0];
        if r[0] / len >= floor {
            worst = worst.max((10.0 * (o[0].max(1e-300) / r[0]).log10()).abs());
        }
    }
    (err, sig, worst)
}

/// The case's input (`inputSignal` of capture-convolver.steps.mjs).
fn case_input(case: &Value) -> [Vec<f32>; 2] {
    let frames = case["frames"].as_u64().unwrap() as usize;
    let sr = case["sr"].as_f64().unwrap();
    let mut x = [vec![0f64; frames], vec![0f64; frames]];
    for e in case["events"].as_array().unwrap() {
        let ch = e["ch"].as_u64().unwrap() as usize;
        let at = e["frame"].as_u64().unwrap() as usize;
        let amp = e["amp"].as_f64().unwrap();
        match e["type"].as_str().unwrap() {
            "impulse" => x[ch][at] += amp,
            "noise" => {
                let mut rng = Mulberry32::new(e["seed"].as_u64().unwrap() as u32);
                for i in 0..e["len"].as_u64().unwrap() as usize {
                    x[ch][at + i] += amp * (rng.next_f64() * 2.0 - 1.0);
                }
            }
            "sine" => {
                let f = e["freq"].as_f64().unwrap();
                for i in 0..e["len"].as_u64().unwrap() as usize {
                    x[ch][at + i] += amp * (2.0 * std::f64::consts::PI * f * i as f64 / sr).sin();
                }
            }
            t => panic!("event type {t}"),
        }
    }
    x.map(|c| c.into_iter().map(|v| v as f32).collect())
}

fn case_ir(case: &Value) -> AudioBuffer {
    let sr = case["sr"].as_u64().unwrap() as u32;
    let o = &case["ir"];
    let get = |k: &str, d: f64| o.get(k).and_then(Value::as_f64).unwrap_or(d);
    let opts = ReverbOptions {
        decay: get("decay", REVERB_DEFAULTS.decay),
        pre_delay: get("preDelay", REVERB_DEFAULTS.pre_delay),
        damping: get("damping", REVERB_DEFAULTS.damping),
        wet: 0.0,
    };
    let mut ir = create_impulse_response(sr, &opts);
    if o["mono"].as_bool() == Some(true) {
        ir.channels.truncate(1);
    }
    ir
}

#[test]
fn matches_chromium_convolver_node() {
    let p = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../parity/fixtures/fx_convolver.json"
    );
    let fx: Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let ir = case_ir(case);
        let scale = convolver_normalization_scale(&ir) as f32;
        let mut c = Convolver::new(&ir, scale);
        let [l, r] = case_input(case);
        let (ol, or) = run(&mut c, &l, &r);
        let frames = l.len();
        let (mut err, mut sig, mut worst) = (0.0, 0.0, 0f64);
        for (ch, x) in [&ol[..frames], &or[..frames]].into_iter().enumerate() {
            let reference: Vec<f64> = case["evidence"][ch]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap())
                .collect();
            let (e, s, w) = channel_error(x, &reference);
            err += e;
            sig += s;
            worst = worst.max(w);
        }
        let db = 10.0 * (err.max(1e-300) / sig).log10();
        println!(
            "{id}: sr {} IR {} frames ×{}ch, stages {:?}: full-render error {db:.1} dB, worst block energy diff {worst:.2e} dB",
            case["sr"],
            ir.frames(),
            ir.channel_count(),
            c.stage_sizes()
        );
        assert!(db <= MAX_ERR_DB, "{id}: error {db:.1} dB");
        assert!(
            worst <= MAX_BLOCK_ENERGY_DB,
            "{id}: block energy off by {worst:.2} dB"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// CPU benchmark.

#[test]
#[ignore = "benchmark: run with --release -- --ignored --nocapture"]
fn bench_room_reverb_48k() {
    use std::time::Instant;
    let ir = create_impulse_response(48000, &REVERB_DEFAULTS);
    let mut c = Convolver::new(&ir, convolver_normalization_scale(&ir) as f32);
    let mut rng = Rng::new(1);
    const PERIOD: usize = 64; // lcm of the stages' quanta per block
    const CYCLES: usize = 400;
    let (mut l, mut r) = ([0f32; BLOCK], [0f32; BLOCK]);
    let (mut ol, mut or) = ([0f32; BLOCK], [0f32; BLOCK]);
    for _ in 0..2000 {
        // warm-up (fill every FDL)
        for i in 0..BLOCK {
            l[i] = rng.uni();
            r[i] = rng.uni();
        }
        c.process(&l, &r, &mut ol, &mut or);
    }
    let mut times = vec![0u64; PERIOD * CYCLES];
    let mut sink = 0f32;
    for t in times.iter_mut() {
        for i in 0..BLOCK {
            l[i] = rng.uni();
            r[i] = rng.uni();
        }
        let t0 = Instant::now();
        c.process(&l, &r, &mut ol, &mut or);
        *t = t0.elapsed().as_nanos() as u64;
        sink += ol[0] + or[BLOCK - 1];
    }
    let mean = times.iter().sum::<u64>() as f64 / times.len() as f64;
    let mut sorted = times.clone();
    sorted.sort_unstable();
    let pct = |p: f64| sorted[((sorted.len() - 1) as f64 * p) as usize];
    // Per schedule phase: median over cycles (removes OS noise) → burstiness of the schedule.
    let phase_median: Vec<u64> = (0..PERIOD)
        .map(|ph| {
            let mut v: Vec<u64> = (0..CYCLES).map(|k| times[k * PERIOD + ph]).collect();
            v.sort_unstable();
            v[CYCLES / 2]
        })
        .collect();
    let pm_mean = phase_median.iter().sum::<u64>() as f64 / PERIOD as f64;
    let pm_max = *phase_median.iter().max().unwrap();
    // Silence path.
    let z = [0f32; BLOCK];
    for _ in 0..(c.idle_after / BLOCK + 2) {
        c.process(&z, &z, &mut ol, &mut or);
    }
    assert!(c.is_idle());
    let t0 = Instant::now();
    for _ in 0..100_000 {
        c.process(&z, &z, &mut ol, &mut or);
    }
    let idle = t0.elapsed().as_nanos() as f64 / 100_000.0;
    println!(
        "RoomReverb 48 kHz, stages {:?}: process mean {mean:.0} ns, median {} ns, p99 {} ns, p99.9 {} ns, max {} ns \
         (quantum = 2 666 667 ns → {:.2} % of a core per track); per-phase median max/mean {:.3}; idle {idle:.0} ns (sink {sink})",
        c.stage_sizes(),
        pct(0.5),
        pct(0.99),
        pct(0.999),
        sorted[sorted.len() - 1],
        mean / 2_666_667.0 * 100.0,
        pm_max as f64 / pm_mean,
    );
}

/// Sample-level comparison against raw captures (`PARITY_DEBUG_DIR`, see the steps script).
#[test]
#[ignore]
fn convolver_parity_debug() {
    let Ok(dir) = std::env::var("PARITY_DEBUG_DIR") else {
        return;
    };
    let p = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../parity/fixtures/fx_convolver.json"
    );
    let fx: Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
    let read = |f: String| -> Vec<f32> {
        std::fs::read(f)
            .unwrap()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect()
    };
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        if !std::path::Path::new(&format!("{dir}/{id}.f32")).exists() {
            continue;
        }
        let raw = read(format!("{dir}/{id}.f32"));
        let raw_in = read(format!("{dir}/{id}.in.f32"));
        let frames = raw.len() / 2;
        let [l, r] = case_input(case);
        let in_diff: Vec<usize> = (0..frames)
            .filter(|&i| raw_in[i] != l[i] || raw_in[frames + i] != r[i])
            .take(5)
            .collect();
        let ir = case_ir(case);
        let scale = convolver_normalization_scale(&ir) as f32;
        let mut c = Convolver::new(&ir, scale);
        let (ol, or) = run(&mut c, &raw_in[..frames], &raw_in[frames..]);
        let mut seg = Vec::new();
        for s in (0..frames).step_by(16384) {
            let (mut e, mut sg) = (0f64, 0f64);
            for i in s..(s + 16384).min(frames) {
                for (ours, theirs) in [(ol[i], raw[i]), (or[i], raw[frames + i])] {
                    e += (ours as f64 - theirs as f64).powi(2);
                    sg += (theirs as f64).powi(2);
                }
            }
            seg.push(format!(
                "{s}:{:.1}",
                10.0 * (e.max(1e-300) / sg.max(1e-300)).log10()
            ));
        }
        eprintln!("{id}: input diffs at {in_diff:?}; per-segment error dB {seg:?}");
    }
}
