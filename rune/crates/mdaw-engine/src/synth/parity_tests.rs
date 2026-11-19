//! Synth instruments vs the Chromium reference (`rune/parity/fixtures/synth_reference_*.json`,
//! captured by `rune/parity/chromium/capture-synth.steps.mjs` from the app's real
//! `PolyInstrument` + voices with real Tone.js, tapped before CharacterChain). Every non-sampler
//! preset (plus FALLBACK_PIANO and four live `set()` cases) replays its scripted case here through [`SynthEngine`] with the same
//! ops at the same quanta and the same noise seeds, and is compared over its FULL length through
//! the random-projection block evidence of `stats.mjs` (both channels when the capture is stereo):
//!
//! - exact length (block count) and onset (first sample above −200 dBFS, first non-silent block);
//! - whole-render error-to-signal energy ≤ [`MAX_ERR_DB`] (measured: −103.6 dB for pluck-bass,
//!   the worst, to −146.6 dB for harp; see the module docs of `synth` and the report test);
//! - every 1024-frame block above −60 dBFS within [`MAX_BLOCK_DB`] of the reference energy.
//!
//! `cargo test -p mdaw-engine synth_parity_report -- --ignored --nocapture` prints the per-preset
//! table.

use super::SynthEngine;
use super::bank::{fallback_piano, synth_presets};
use super::presets::SynthPreset;
use crate::contract::Instrument;
use mdaw_dsp::factory::Mulberry32;
use serde_json::Value;

const BLOCK: usize = 1024;
const PROJ: usize = 16;
const PROJ_SEED: u32 = 0x9a0f_0001;
/// Required ≤ −60 dB; every preset measures −103 to −147 dB, so regressions are caught at −90.
const MAX_ERR_DB: f64 = -90.0;
const MAX_BLOCK_DB: f64 = 0.5;
const LEVEL_FLOOR_DB: f64 = -60.0;

pub(super) fn fixture(engine: &str) -> Value {
    let p = format!(
        "{}/../../parity/fixtures/synth_reference_{engine}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(&p).expect("synth_reference fixture")).unwrap()
}

/// `blockEvidence` of stats.mjs.
fn block_evidence(x: &[f32]) -> Vec<[f64; PROJ + 1]> {
    let mut rng = Mulberry32::new(PROJ_SEED);
    x.chunks(BLOCK)
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

pub(super) fn preset_of(index: i64) -> SynthPreset {
    if index < 0 {
        SynthPreset::Fm(fallback_piano().clone())
    } else {
        synth_presets()[index as usize].clone()
    }
}

/// Replay a case: ops are applied before the quantum at their step frame.
pub(super) fn render_case(case: &Value) -> [Vec<f32>; 2] {
    let sr = case["sr"].as_f64().unwrap();
    let frames = case["frames"].as_u64().unwrap() as usize;
    let mut preset = preset_of(case["preset"].as_i64().unwrap());
    if let Some(o) = case.get("overrides").filter(|o| o.is_object()) {
        preset = super::params::merge_preset_params(&preset, Some(o));
    }
    let seed = case["seed"].as_u64().unwrap() as u32;
    let mut inst = SynthEngine::new(&preset, sr, 0.0, seed);
    let mut l = vec![0f32; frames];
    let mut r = vec![0f32; frames];
    let steps = case["steps"].as_array().unwrap();
    let mut si = 0;
    let mut frame = 0usize;
    while frame < frames {
        while si < steps.len() && steps[si]["frame"].as_u64().unwrap() as usize <= frame {
            for o in steps[si]["ops"].as_array().unwrap() {
                let t = o["time"].as_f64().unwrap_or(0.0);
                let note = o["note"].as_i64().unwrap_or(0) as i32;
                match o["op"].as_str().unwrap() {
                    "on" => inst.note_on_midi(note, o["vel"].as_f64().unwrap(), t),
                    "off" => inst.note_off_midi(note, t),
                    "ar" => inst.trigger_attack_release(
                        note,
                        o["dur"].as_f64().unwrap(),
                        t,
                        o["vel"].as_f64().unwrap(),
                    ),
                    "releaseAll" => crate::core::SynthInstrument::release_all(&mut inst, t),
                    "set" => {
                        preset = super::params::merge_preset_params(&preset, Some(&o["params"]));
                        assert!(inst.set_preset(&preset));
                    }
                    other => panic!("op {other}"),
                }
            }
            si += 1;
        }
        let end = (frame + 128).min(frames);
        inst.process(frame as u64, &mut l[frame..end], &mut r[frame..end]);
        frame = end;
    }
    [l, r]
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ChannelResult {
    pub err_db: f64,
    pub worst_block_err_db: f64,
    pub worst_energy_db: f64,
    pub onset_block: Option<usize>,
    pub ref_onset_block: Option<usize>,
    /// First sample above −200 dBFS (ours, the capture's).
    pub onset: i64,
    pub ref_onset: i64,
    pub blocks_ok: bool,
}

pub(super) fn compare(x: &[f32], reference: &[f64]) -> ChannelResult {
    let ours = block_evidence(x);
    assert_eq!(
        ours.len() * (PROJ + 1),
        reference.len(),
        "evidence size (length)"
    );
    let floor = 10f64.powf(LEVEL_FLOOR_DB / 10.0);
    let (mut err, mut sig) = (0.0, 0.0);
    let (mut worst, mut worst_energy) = (-400f64, 0f64);
    let mut onset = None;
    let mut ref_onset = None;
    for (i, (o, r)) in ours.iter().zip(reference.chunks(PROJ + 1)).enumerate() {
        let len = BLOCK.min(x.len() - i * BLOCK) as f64;
        let e = (1..=PROJ).map(|k| (o[k] - r[k]).powi(2)).sum::<f64>() / PROJ as f64;
        err += e;
        sig += r[0];
        if onset.is_none() && o[0] > 0.0 {
            onset = Some(i);
        }
        if ref_onset.is_none() && r[0] > 0.0 {
            ref_onset = Some(i);
        }
        if r[0] / len >= floor {
            worst = worst.max(10.0 * (e.max(1e-300) / r[0]).log10());
            let d = 10.0 * (o[0].max(1e-300) / r[0]).log10();
            if d.abs() > worst_energy.abs() {
                worst_energy = d;
            }
        }
    }
    ChannelResult {
        err_db: 10.0 * (err.max(1e-300) / sig.max(1e-300)).log10(),
        worst_block_err_db: worst,
        worst_energy_db: worst_energy,
        onset_block: onset,
        ref_onset_block: ref_onset,
        onset: x
            .iter()
            .position(|&v| v.abs() > 1e-10)
            .map_or(-1, |i| i as i64),
        ref_onset: -1,
        blocks_ok: worst_energy.abs() <= MAX_BLOCK_DB,
    }
}

pub(super) fn check_case(case: &Value) -> Vec<ChannelResult> {
    let [l, r] = render_case(case);
    let ev = case["evidence"].as_array().unwrap();
    let refs: Vec<Vec<f64>> = ev
        .iter()
        .map(|c| {
            c.as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap())
                .collect()
        })
        .collect();
    let onsets: Vec<i64> = case["onset"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap())
        .collect();
    let mut out = vec![compare(&l, &refs[0])];
    if refs.len() > 1 {
        out.push(compare(&r, &refs[1]));
    } else {
        assert_eq!(l, r, "{}: the capture is mono, so must ours be", case["id"]);
    }
    for (c, o) in out.iter_mut().enumerate() {
        o.ref_onset = onsets[c];
    }
    out
}

fn run_engine(engine: &str) -> Vec<(String, Vec<ChannelResult>)> {
    let fx = fixture(engine);
    fx["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| (c["id"].as_str().unwrap().to_string(), check_case(c)))
        .collect()
}

fn assert_engine(engine: &str) {
    for (id, chans) in run_engine(engine) {
        for (c, r) in chans.iter().enumerate() {
            assert_eq!(r.onset_block, r.ref_onset_block, "{id} ch{c}: onset block");
            assert_eq!(
                r.onset, r.ref_onset,
                "{id} ch{c}: first sample above -200 dBFS"
            );
            assert!(
                r.err_db <= MAX_ERR_DB,
                "{id} ch{c}: error {:.1} dB",
                r.err_db
            );
            assert!(
                r.blocks_ok,
                "{id} ch{c}: block energy off by {:.3} dB",
                r.worst_energy_db
            );
        }
    }
}

#[test]
fn synth_parity_subtractive() {
    assert_engine("subtractive");
}

#[test]
fn synth_parity_fm() {
    assert_engine("fm");
}

#[test]
fn synth_parity_pluck() {
    assert_engine("pluck");
}

/// Prints the per-preset parity table.
#[test]
#[ignore]
fn synth_parity_report() {
    let only: Option<Vec<String>> = std::env::var("CASES")
        .ok()
        .map(|s| s.split(',').map(String::from).collect());
    for engine in ["subtractive", "fm", "pluck"] {
        let fx = fixture(engine);
        for c in fx["cases"].as_array().unwrap() {
            let id = c["id"].as_str().unwrap();
            if only.as_ref().is_some_and(|o| !o.iter().any(|x| x == id)) {
                continue;
            }
            let preset = preset_of(c["preset"].as_i64().unwrap());
            for (ch, r) in check_case(c).iter().enumerate() {
                println!(
                    "{id} {:<16} ch{ch}: err {:>7.1} dB, worst block {:>7.1} dB, block energy Δ {:>7.3} dB, onset {}/{}",
                    preset.id(),
                    r.err_db,
                    r.worst_block_err_db,
                    r.worst_energy_db,
                    r.onset,
                    r.ref_onset
                );
            }
        }
    }
}

/// With `PARITY_DEBUG_DIR` (raw captures from the steps script): first diverging sample per case.
#[test]
#[ignore]
fn synth_parity_debug() {
    let Ok(dir) = std::env::var("PARITY_DEBUG_DIR") else {
        return;
    };
    let only: Option<Vec<String>> = std::env::var("CASES")
        .ok()
        .map(|s| s.split(',').map(String::from).collect());
    let sets: Vec<Value> = if std::env::var("PROBE").is_ok() {
        let probe: Value =
            serde_json::from_str(&std::fs::read_to_string(format!("{dir}/probe.json")).unwrap())
                .unwrap();
        vec![serde_json::json!({ "cases": probe })]
    } else {
        ["subtractive", "fm", "pluck"]
            .iter()
            .map(|e| fixture(e))
            .collect()
    };
    for fx in sets {
        for c in fx["cases"].as_array().unwrap() {
            let id = c["id"].as_str().unwrap();
            if only.as_ref().is_some_and(|o| !o.iter().any(|x| x == id)) {
                continue;
            }
            let Ok(bytes) = std::fs::read(format!("{dir}/{id}.f32")) else {
                continue;
            };
            let raw: Vec<f32> = bytes
                .chunks(4)
                .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                .collect();
            let n = raw.len() / 2;
            let [l, r] = render_case(c);
            for (ch, (ours, theirs)) in [(&l, &raw[..n]), (&r, &raw[n..])].into_iter().enumerate() {
                let sig: f64 = theirs.iter().map(|&v| (v as f64).powi(2)).sum();
                let err: f64 = ours
                    .iter()
                    .zip(theirs)
                    .map(|(a, b)| ((a - b) as f64).powi(2))
                    .sum();
                let (imax, max) = ours
                    .iter()
                    .zip(theirs)
                    .map(|(a, b)| (a - b).abs())
                    .enumerate()
                    .fold((0, 0f32), |m, (i, d)| if d > m.1 { (i, d) } else { m });
                // First 1024-block whose error is above -80 dB of its energy (or of -60 dBFS).
                let first_bad = (0..n.div_ceil(1024)).find(|&b| {
                    let r = b * 1024..((b + 1) * 1024).min(n);
                    let e: f64 = r
                        .clone()
                        .map(|k| ((ours[k] - theirs[k]) as f64).powi(2))
                        .sum();
                    let s: f64 = r
                        .clone()
                        .map(|k| (theirs[k] as f64).powi(2))
                        .sum::<f64>()
                        .max(1e-6 * 1024.0);
                    e / s > 1e-8
                });
                println!(
                    "{id} ch{ch}: err {:.1} dB, max |Δ| {max:.3e} at {imax} ({:.4} s), first bad block {first_bad:?}",
                    10.0 * (err.max(1e-300) / sig.max(1e-300)).log10(),
                    imax as f64 / 48000.0
                );
                if let Ok(range) = std::env::var("RANGE") {
                    let (a, b) = range.split_once("..").unwrap();
                    let (a, b): (usize, usize) = (a.parse().unwrap(), b.parse().unwrap());
                    if ch == 0 {
                        for k in a..b.min(n) {
                            println!(
                                "   [{k}] ours {:.9} ref {:.9}  Δ {:.3e}",
                                ours[k],
                                theirs[k],
                                ours[k] - theirs[k]
                            );
                        }
                    }
                    continue;
                }
                if let Some(b) = first_bad {
                    let lo = b * 1024;
                    let worst = (lo..(lo + 1024).min(n))
                        .max_by(|&x, &y| {
                            (ours[x] - theirs[x])
                                .abs()
                                .total_cmp(&(ours[y] - theirs[y]).abs())
                        })
                        .unwrap();
                    for k in worst.saturating_sub(3)..(worst + 3).min(n) {
                        println!("   [{k}] ours {:.9} ref {:.9}", ours[k], theirs[k]);
                    }
                }
            }
        }
    }
}
