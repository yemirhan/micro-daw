//! CharacterChain vs the Chromium reference (`rune/parity/fixtures/character_reference.json`,
//! captured by `rune/parity/chromium/capture-character.steps.mjs` from the app's real
//! CharacterChain with real Tone.js): every distinct preset character, each stage alone, a mono
//! input and live updates, fed by the same deterministic test signal, compared over the full
//! render through the random-projection block evidence of `stats.mjs`.
//!
//! Limits: full-render error ≤ −60 dB (per channel); per-block energy within 0.5 dB for blocks
//! above −60 dBFS. The per-case results are printed (`--nocapture`).

use serde_json::Value;

use super::*;
use crate::contract::RENDER_QUANTUM;
use mdaw_dsp::factory::Mulberry32;

pub(crate) const BLOCK: usize = 1024;
pub(crate) const PROJ: usize = 16;
const PROJ_SEED: u32 = 0x9a0f_0001;
pub(crate) const MAX_ERR_DB: f64 = -60.0;
pub(crate) const MAX_BLOCK_ENERGY_DB: f64 = 0.5;
const LEVEL_FLOOR_DB: f64 = -60.0;

pub(crate) fn fixture(name: &str) -> Value {
    let p = format!(
        "{}/../../parity/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(&p).expect(name)).unwrap()
}

/// `blockEvidence` of stats.mjs.
pub(crate) fn block_evidence(x: &[f32]) -> Vec<[f64; PROJ + 1]> {
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

/// Comparison of one channel with its reference evidence.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ChannelCmp {
    pub err: f64,
    pub sig: f64,
    /// Worst |Δ block energy| (dB) over blocks above −60 dBFS.
    pub worst_energy_db: f64,
    /// Worst block error relative to its energy (dB) over the same blocks.
    pub worst_block_err_db: f64,
}

pub(crate) fn compare_channel(x: &[f32], reference: &[f64]) -> ChannelCmp {
    let ours = block_evidence(x);
    assert_eq!(ours.len() * (PROJ + 1), reference.len(), "evidence size");
    let floor = 10f64.powf(LEVEL_FLOOR_DB / 10.0);
    let mut c = ChannelCmp {
        err: 0.0,
        sig: 0.0,
        worst_energy_db: 0.0,
        worst_block_err_db: -400.0,
    };
    for (i, (o, r)) in ours.iter().zip(reference.chunks(PROJ + 1)).enumerate() {
        let len = BLOCK.min(x.len() - i * BLOCK) as f64;
        let e = (1..=PROJ).map(|k| (o[k] - r[k]).powi(2)).sum::<f64>() / PROJ as f64;
        c.err += e;
        c.sig += r[0];
        if r[0] / len >= floor {
            let d = 10.0 * (o[0].max(1e-300) / r[0]).log10();
            c.worst_energy_db = c.worst_energy_db.max(d.abs());
            c.worst_block_err_db = c
                .worst_block_err_db
                .max(10.0 * (e.max(1e-300) / r[0]).log10());
        }
    }
    c
}

/// Error-to-signal of both channels together (dB).
pub(crate) fn total_db(cmp: &[ChannelCmp]) -> f64 {
    let err: f64 = cmp.iter().map(|c| c.err).sum();
    let sig: f64 = cmp.iter().map(|c| c.sig).sum();
    10.0 * (err.max(1e-300) / sig.max(1e-300)).log10()
}

/// `testSignal` of capture-piano-page.ts.
pub(crate) fn test_signal(frames: usize, sr: f64) -> [Vec<f32>; 2] {
    let mut l = vec![0f32; frames];
    let mut r = vec![0f32; frames];
    let dur = frames as f64 / sr;
    let mut ps = 0f64;
    let mut pc = [0.0, 0.25, 0.5];
    let fc = [220.0, 277.1826, 329.6276];
    for i in 0..frames {
        let t = i as f64 / sr;
        let f = 60.0 + (6000.0 - 60.0) * (t / dur);
        ps += f / sr;
        ps -= ps.floor();
        let saw = 2.0 * ps - 1.0;
        let mut chord = 0.0;
        for k in 0..3 {
            pc[k] += fc[k] / sr;
            pc[k] -= pc[k].floor();
            let x = pc[k];
            chord += if x < 0.5 {
                4.0 * x - 1.0
            } else {
                3.0 - 4.0 * x
            };
        }
        let u = t / dur;
        let mut a = (t / 0.01).min(1.0);
        if (0.45..0.5).contains(&u) {
            a = 0.0;
        }
        l[i] = (a * (0.3 * saw + 0.15 * chord)) as f32;
        r[i] = (a * (0.15 * saw + 0.2 * chord - 0.05)) as f32;
    }
    [l, r]
}

/// `withCharacterDefaults` of a JSON object.
pub(crate) fn character_from(v: &Value) -> CharacterParams {
    serde_json::from_value(v.clone()).unwrap()
}

fn render_case(case: &Value) -> [Vec<f32>; 2] {
    let sr = case["sr"].as_u64().unwrap() as u32;
    let frames = case["frames"].as_u64().unwrap() as usize;
    let [mut l, mut r] = test_signal(frames, sr as f64);
    if case["mono"].as_bool().unwrap() {
        r.copy_from_slice(&l);
    }
    let mut chain = CharacterChain::new(
        sr,
        character_from(&case["character"]),
        case["gain"].as_f64().unwrap(),
        0.0,
    );
    let steps = case["steps"].as_array().unwrap();
    let mut next = 0;
    for q in (0..frames).step_by(RENDER_QUANTUM) {
        while next < steps.len() && steps[next]["frame"].as_u64().unwrap() as usize == q {
            for op in steps[next]["ops"].as_array().unwrap() {
                chain.update(
                    character_from(&op["character"]),
                    op["gain"].as_f64().unwrap(),
                    q as f64 / sr as f64,
                );
            }
            next += 1;
        }
        let e = (q + RENDER_QUANTUM).min(frames);
        chain.process(q as u64, &mut l[q..e], &mut r[q..e]);
    }
    [l, r]
}

#[test]
fn character_chain_matches_chromium() {
    let fx = fixture("character_reference.json");
    let mut failures = Vec::new();
    println!("case                         total dB   worst block err dB   worst |Δenergy| dB");
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let out = render_case(case);
        let cmp: Vec<ChannelCmp> = out
            .iter()
            .zip(case["evidence"].as_array().unwrap())
            .map(|(x, ev)| {
                let ev: Vec<f64> = ev
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_f64().unwrap())
                    .collect();
                compare_channel(x, &ev)
            })
            .collect();
        let total = total_db(&cmp);
        let block = cmp
            .iter()
            .map(|c| c.worst_block_err_db)
            .fold(-400.0, f64::max);
        let energy = cmp.iter().map(|c| c.worst_energy_db).fold(0.0, f64::max);
        println!("{id:<28} {total:>8.1}   {block:>18.1}   {energy:>18.4}");
        if total > MAX_ERR_DB || energy > MAX_BLOCK_ENERGY_DB {
            failures.push(format!("{id}: total {total:.1} dB, Δenergy {energy:.3} dB"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// `PARITY_DEBUG_DIR=/dir cargo test -p mdaw-engine character_parity_debug -- --ignored`: writes
/// each Rust render as `char_<id>.rust.f32` (L then R) next to the capture's `char_<id>.f32`.
#[test]
#[ignore]
fn character_parity_debug() {
    let dir = std::env::var("PARITY_DEBUG_DIR").expect("PARITY_DEBUG_DIR");
    let fx = fixture("character_reference.json");
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let [l, r] = render_case(case);
        let bytes: Vec<u8> = l.iter().chain(&r).flat_map(|x| x.to_le_bytes()).collect();
        std::fs::write(format!("{dir}/char_{id}.rust.f32"), bytes).unwrap();
    }
}
