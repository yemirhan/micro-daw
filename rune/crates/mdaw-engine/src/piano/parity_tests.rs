//! SampledPiano vs the Chromium reference (`rune/parity/fixtures/piano_reference.json`, captured
//! by `rune/parity/chromium/capture-piano.steps.mjs` from the app's real SampledPiano with real
//! Tone.js and the app's own sample loading):
//! - the 30 decoded + trimmed buffers at 48 kHz: lengths and FNV-1a digests of every channel;
//! - every scripted case replayed with the same ops at the same quanta, compared over the full
//!   render through the block evidence of `stats.mjs` (≤ −60 dB, per-block energy within 0.5 dB),
//!   plus the logged held notes / active voice counts and merged presets.

use std::sync::Arc;

use serde_json::Value;

use super::samples::{PianoSamples, piano_samples};
use super::*;
use crate::character::parity_tests::{
    ChannelCmp, MAX_BLOCK_ENERGY_DB, MAX_ERR_DB, compare_channel, fixture, total_db,
};
use crate::contract::RENDER_QUANTUM;

const SR: u32 = 48000;

/// A fallback that must never be used (the samples are loaded before every case).
pub(crate) struct NoFallback;

impl Instrument for NoFallback {
    fn process(&mut self, _: u64, _: &mut [f32], _: &mut [f32]) {}
}

impl PianoFallback for NoFallback {
    fn note_on(&mut self, _: i32, _: f64, _: f64) {
        panic!("fallback used");
    }
    fn note_off(&mut self, _: i32, _: f64) {}
    fn attack_release(&mut self, _: i32, _: f64, _: f64, _: f64) {
        panic!("fallback used");
    }
    fn release_all(&mut self, _: f64) {}
    fn active_voice_count(&self, _: f64) -> usize {
        0
    }
    fn is_held(&self, _: i32) -> bool {
        false
    }
}

fn fnv(data: &[f32]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for x in data {
        for b in x.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
    }
    format!("{h:016x}")
}

/// Channels whose decode is not bit-exact: Fs7 (102) right, the last 7 of 231541 frames differ
/// by ≤ 2.7e-7 (−136 dB over the channel; the resampler's tail), measured with
/// `piano_buffer_debug` against the Chromium buffer. Every other channel is bit-exact.
const KNOWN_TAIL_DIFF: [(i32, usize); 1] = [(102, 1)];

fn samples() -> Arc<PianoSamples> {
    piano_samples(SR).expect("piano samples")
}

#[test]
fn decoded_buffers_are_bit_exact() {
    let fx = fixture("piano_reference.json");
    let s = samples();
    let refs = fx["buffers"].as_array().unwrap();
    assert_eq!(refs.len(), 30);
    assert_eq!(s.len(), 30);
    for b in refs {
        let midi = b["midi"].as_i64().unwrap() as i32;
        let buf = s.get(midi).unwrap();
        assert_eq!(buf.sample_rate, b["sampleRate"].as_u64().unwrap() as u32);
        assert_eq!(
            buf.frames() as u64,
            b["frames"].as_u64().unwrap(),
            "frames of {midi}"
        );
        let digests: Vec<String> = buf.channels.iter().map(|c| fnv(c)).collect();
        let want: Vec<&str> = b["digests"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d.as_str().unwrap())
            .collect();
        for (ch, (ours, theirs)) in digests.iter().zip(&want).enumerate() {
            if KNOWN_TAIL_DIFF.contains(&(midi, ch)) {
                assert_ne!(
                    ours, theirs,
                    "{midi}/{ch} is bit-exact now: drop it from KNOWN_TAIL_DIFF"
                );
                continue;
            }
            assert_eq!(ours, theirs, "digest of {midi}/{ch}");
        }
    }
}

pub(crate) fn params_from(v: &Value) -> PianoParams {
    serde_json::from_value(v.clone()).unwrap()
}

pub(crate) struct Rendered {
    pub channels: [Vec<f32>; 2],
    pub log: Vec<Value>,
}

/// Replay one scripted case (the page's `renderPiano`).
pub(crate) fn render_case(case: &Value, samples: Arc<PianoSamples>) -> Rendered {
    let frames = case["frames"].as_u64().unwrap() as usize;
    let ref_log = case["log"].as_array().unwrap();
    let mut presets = ref_log.iter().filter_map(|e| e.get("preset"));
    let mut piano = SampledPiano::new(
        SR,
        params_from(presets.next().unwrap()),
        Box::new(NoFallback),
        PianoSlot::ready(samples),
        0.0,
    );
    let mut log = vec![case["log"][0].clone()];
    let mut l = vec![0f32; frames];
    let mut r = vec![0f32; frames];
    let steps = case["steps"].as_array().unwrap();
    let mut next = 0;
    for q in (0..frames).step_by(RENDER_QUANTUM) {
        while next < steps.len() && steps[next]["frame"].as_u64().unwrap() as usize == q {
            for op in steps[next]["ops"].as_array().unwrap() {
                let t = || op["time"].as_f64().unwrap();
                let midi = || op["midi"].as_i64().unwrap() as i32;
                match op["op"].as_str().unwrap() {
                    "on" => piano.note_on(midi(), op["vel"].as_f64().unwrap(), t()),
                    "off" => piano.note_off(midi(), t()),
                    "ar" => piano.trigger_attack_release(
                        midi(),
                        op["dur"].as_f64().unwrap(),
                        t(),
                        op["vel"].as_f64().unwrap(),
                    ),
                    "releaseAll" => piano.release_all(t()),
                    "set" => {
                        let p = presets.next().unwrap();
                        piano.set_params(params_from(p), q as f64 / SR as f64);
                        log.push(serde_json::json!({ "frame": q, "preset": p }));
                    }
                    "probe" => log.push(serde_json::json!({
                        "frame": q,
                        "held": piano.held_notes(),
                        "active": piano.active_voice_count(t()),
                    })),
                    other => panic!("op {other}"),
                }
            }
            next += 1;
        }
        let e = (q + RENDER_QUANTUM).min(frames);
        piano.process(q as u64, &mut l[q..e], &mut r[q..e]);
    }
    Rendered {
        channels: [l, r],
        log,
    }
}

#[test]
fn sampled_piano_matches_chromium() {
    let fx = fixture("piano_reference.json");
    let s = samples();
    let mut failures = Vec::new();
    println!("case                 total dB   worst block err dB   worst |Δenergy| dB");
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let out = render_case(case, s.clone());
        assert_eq!(&out.log, case["log"].as_array().unwrap(), "{id}: log");
        let cmp: Vec<ChannelCmp> = out
            .channels
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
        println!("{id:<20} {total:>8.1}   {block:>18.1}   {energy:>18.4}");
        if total > MAX_ERR_DB || energy > MAX_BLOCK_ENERGY_DB {
            failures.push(format!("{id}: total {total:.1} dB, Δenergy {energy:.3} dB"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// `PARITY_DEBUG_DIR=/dir cargo test -p mdaw-engine piano_buffer_debug -- --ignored`: compares
/// `buf<midi>_<ch>.f32` files (from capture-piano-page.ts `pianoBufferChannel`) with the Rust
/// decode and prints the differences.
#[test]
#[ignore]
fn piano_buffer_debug() {
    let dir = std::env::var("PARITY_DEBUG_DIR").expect("PARITY_DEBUG_DIR");
    let s = samples();
    for midi in s.roots() {
        let buf = s.get(midi as i32).unwrap();
        for (ch, ours) in buf.channels.iter().enumerate() {
            let Ok(bytes) = std::fs::read(format!("{dir}/buf{midi}_{ch}.f32")) else {
                continue;
            };
            let theirs: Vec<f32> = bytes
                .chunks(4)
                .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                .collect();
            let diffs: Vec<usize> = (0..ours.len().min(theirs.len()))
                .filter(|&i| ours[i].to_bits() != theirs[i].to_bits())
                .collect();
            let err: f64 = diffs
                .iter()
                .map(|&i| (ours[i] as f64 - theirs[i] as f64).powi(2))
                .sum();
            let sig: f64 = theirs.iter().map(|&x| (x as f64).powi(2)).sum();
            println!(
                "{midi}/{ch}: len {} vs {}, {} samples differ (first {:?}), err {:.1} dB, max |Δ| {:e}",
                ours.len(),
                theirs.len(),
                diffs.len(),
                diffs.first(),
                10.0 * (err.max(1e-300) / sig).log10(),
                diffs
                    .iter()
                    .map(|&i| (ours[i] - theirs[i]).abs())
                    .fold(0f32, f32::max),
            );
        }
    }
}

/// `PARITY_DEBUG_DIR=/dir cargo test -p mdaw-engine piano_parity_debug -- --ignored`: writes each
/// Rust render as `piano_<id>.rust.f32` (L then R) next to the capture's `piano_<id>.f32`.
#[test]
#[ignore]
fn piano_parity_debug() {
    let dir = std::env::var("PARITY_DEBUG_DIR").expect("PARITY_DEBUG_DIR");
    let fx = fixture("piano_reference.json");
    let s = samples();
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let out = render_case(case, s.clone());
        let bytes: Vec<u8> = out
            .channels
            .iter()
            .flatten()
            .flat_map(|x| x.to_le_bytes())
            .collect();
        std::fs::write(format!("{dir}/piano_{id}.rust.f32"), bytes).unwrap();
    }
}
