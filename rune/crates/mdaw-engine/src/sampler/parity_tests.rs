//! SamplerKit vs the Chromium reference (`rune/parity/fixtures/sampler_reference.json`, captured
//! by `rune/parity/chromium/capture-sampler.steps.mjs` from the app's real SamplerKit with real
//! Tone.js). Every case is replayed here through [`KitController`] + [`SamplerKit`] with the same
//! script (ops applied at the same quanta, loads settled like `await kit.ready`) and compared over
//! its FULL length through the random-projection block evidence of `stats.mjs`:
//! error-to-signal energy ≤ −90 dB over the whole render (both channels) and ≤ −80 dB in every
//! 1024-frame block above −60 dBFS. Measured: 13 of 19 cases are bit-exact (≈ −195 dB, the
//! evidence's 10-digit floor); the others are within an ulp here and there (−141 to −166 dB):
//! filter coefficients (`fdlibm` sin/cos vs the platform's) and the order Chromium sums 3+ voices
//! or pads (a pointer-hash set).

use super::control::{KitController, PadLoader};
use super::*;
use crate::buffers::PadBufferCache;
use mdaw_dsp::buffer::AudioBuffer;
use mdaw_dsp::classic::synthesize_classic_voice;
use mdaw_dsp::factory::Mulberry32;
use mdaw_model::types::{ClassicMachineId, KitState};
use serde_json::Value;
use std::collections::HashMap;

const BLOCK: usize = 1024;
const PROJ: usize = 16;
const PROJ_SEED: u32 = 0x9a0f_0001;
const MAX_ERR_DB: f64 = -90.0;
const MAX_BLOCK_ERR_DB: f64 = -80.0;
const LEVEL_FLOOR_DB: f64 = -60.0;

fn fixture() -> Value {
    let p = format!(
        "{}/../../parity/fixtures/sampler_reference.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(&p).expect("sampler_reference.json")).unwrap()
}

/// `synthChannels` of capture-playback-page.ts.
fn synth_channels(frames: usize, channels: usize, seed: u32) -> Vec<Vec<f32>> {
    (0..channels)
        .map(|c| {
            let mut s = seed.wrapping_add((c as u32).wrapping_mul(0x9e37_79b9));
            let mut g = 1f32;
            (0..frames)
                .map(|n| {
                    s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    let noise = (s >> 8) as f64 / 16_777_216.0 - 0.5;
                    let k = n % 200;
                    let tri = (if k < 100 { k } else { 200 - k }) as f64 / 100.0 - 0.5;
                    let x = ((noise * 0.5 + tri) * g as f64) as f32;
                    g = (g as f64 * 0.99995) as f32;
                    x
                })
                .collect()
        })
        .collect()
}

fn build_buffer(spec: &Value) -> SharedBuffer {
    let b = match spec["kind"].as_str().unwrap() {
        "classic" => {
            let m = ClassicMachineId::from_id(spec["machine"].as_str().unwrap()).unwrap();
            AudioBuffer::mono(
                44100,
                synthesize_classic_voice(m, spec["voice"].as_str().unwrap()).unwrap(),
            )
        }
        _ => AudioBuffer {
            sample_rate: spec["sampleRate"].as_u64().unwrap() as u32,
            channels: synth_channels(
                spec["frames"].as_u64().unwrap() as usize,
                spec["channels"].as_u64().unwrap() as usize,
                spec["seed"].as_u64().unwrap() as u32,
            ),
        },
    };
    Arc::new(b)
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

/// (error energy, signal energy, worst block dB above the floor) of one channel.
fn channel_error(x: &[f32], reference: &[f64]) -> (f64, f64, f64) {
    let ours = block_evidence(x);
    assert_eq!(ours.len() * (PROJ + 1), reference.len(), "evidence size");
    let floor = 10f64.powf(LEVEL_FLOOR_DB / 10.0);
    let (mut err, mut sig, mut worst) = (0.0, 0.0, -400f64);
    for (i, (o, r)) in ours.iter().zip(reference.chunks(PROJ + 1)).enumerate() {
        let len = BLOCK.min(x.len() - i * BLOCK) as f64;
        let e = (1..=PROJ).map(|k| (o[k] - r[k]).powi(2)).sum::<f64>() / PROJ as f64;
        err += e;
        sig += r[0];
        if r[0] / len >= floor {
            worst = worst.max(10.0 * (e.max(1e-300) / r[0]).log10());
        }
    }
    (err, sig, worst)
}

pub(super) struct Rendered {
    pub channels: [Vec<f32>; 2],
    pub log: Vec<Value>,
}

/// Replay one scripted case (the page's `renderScript`).
pub(super) fn render_case(case: &Value, buffers: &HashMap<String, SharedBuffer>) -> Rendered {
    let sr = case["sr"].as_u64().unwrap() as u32;
    let frames = case["frames"].as_u64().unwrap() as usize;
    let kit: KitState = serde_json::from_value(case["kit"].clone()).unwrap();
    let bufs = buffers.clone();
    let loader: Arc<PadLoader> = Arc::new(move |_pad, sample| {
        Ok(match &sample.source {
            mdaw_model::types::PadSampleSource::File { path, .. } => bufs.get(path).cloned(),
            _ => None,
        })
    });
    let (mut ctl, mut cmds) =
        KitController::new(&kit, PadBufferCache::with_fs(sr, None), Some(loader));
    cmds.extend(ctl.wait_ready());
    let mut kitx = SamplerKit::new(sr, ctl.pad_params(), 0);
    for c in cmds {
        kitx.apply(c);
    }
    let mut handles: Vec<VoiceHandle> = Vec::new();
    let mut log = Vec::new();
    let steps = case["steps"].as_array().unwrap();
    let mut next = 0;
    let mut out = [vec![0f32; frames], vec![0f32; frames]];
    let mut garbage = 0usize;
    for start in (0..frames).step_by(Q) {
        while next < steps.len() && steps[next]["frame"].as_u64().unwrap() as usize == start {
            let step = &steps[next];
            for o in step["ops"].as_array().unwrap() {
                let t = |k: &str| o[k].as_f64();
                match o["op"].as_str().unwrap() {
                    "trigger" => {
                        let vel = o["vel"].as_f64().unwrap_or(f64::NAN);
                        let h = kitx.trigger(
                            o["pad"].as_u64().unwrap() as usize,
                            vel,
                            t("time").unwrap(),
                        );
                        log.push(serde_json::json!({
                            "frame": start, "op": "trigger", "voices": h.is_some() as u32
                        }));
                        handles.extend(h);
                    }
                    "release" => {
                        kitx.release(o["pad"].as_u64().unwrap() as usize, t("time").unwrap())
                    }
                    "stopAll" => kitx.stop_all(t("time")),
                    "setKit" => {
                        let k: KitState = serde_json::from_value(o["kit"].clone()).unwrap();
                        let cmd = ctl.set_kit(&k);
                        kitx.apply(cmd);
                    }
                    "cancel" => {
                        let i = o["handle"].as_u64().unwrap() as usize;
                        let r = handles.get(i).map(|h| kitx.cancel_if_pending(h));
                        log.push(serde_json::json!({
                            "frame": start, "op": "cancel", "cancelled": r
                        }));
                    }
                    other => panic!("unknown op {other}"),
                }
            }
            for c in ctl.wait_ready() {
                kitx.apply(c);
            }
            next += 1;
        }
        let n = Q.min(frames - start);
        let [l, r] = &mut out;
        kitx.process(
            start as u64,
            &mut l[start..start + n],
            &mut r[start..start + n],
        );
        kitx.drain_garbage(|_| garbage += 1);
    }
    Rendered { channels: out, log }
}

pub(super) fn buffers_of(fx: &Value) -> HashMap<String, SharedBuffer> {
    fx["buffers"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, spec)| (format!("/parity/{name}"), build_buffer(spec)))
        .collect()
}

#[test]
fn sampler_kit_matches_chromium_full_length() {
    let fx = fixture();
    let buffers = buffers_of(&fx);
    let mut failures = Vec::new();
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let r = render_case(case, &buffers);
        let ev = case["evidence"].as_array().unwrap();
        let (mut err, mut sig, mut worst) = (0.0, 0.0, -400f64);
        for (ch, e) in r.channels.iter().zip(ev) {
            let reference: Vec<f64> = e
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap())
                .collect();
            let (e, s, w) = channel_error(ch, &reference);
            err += e;
            sig += s;
            worst = worst.max(w);
        }
        let db = 10.0 * (err.max(1e-300) / sig).log10();
        let peak = r
            .channels
            .iter()
            .flatten()
            .fold(0f32, |m, &v| m.max(v.abs()));
        eprintln!(
            "{id:>24}: full render {db:7.1} dB, worst block {worst:7.1} dB, peak {peak:.4} (ref {})",
            case["peak"]
        );
        assert_eq!(&Value::from(r.log.clone()), &case["log"], "{id}: op log");
        if db > MAX_ERR_DB || worst > MAX_BLOCK_ERR_DB {
            failures.push(format!("{id}: {db:.1} dB (worst block {worst:.1} dB)"));
        }
    }
    assert!(failures.is_empty(), "over {MAX_ERR_DB} dB: {failures:?}");
}

/// Sample-level comparison against the raw captures (`PARITY_DEBUG_DIR`, see the steps script).
#[test]
#[ignore]
fn sampler_parity_debug() {
    let Ok(dir) = std::env::var("PARITY_DEBUG_DIR") else {
        return;
    };
    let fx = fixture();
    let buffers = buffers_of(&fx);
    let only = std::env::var("CASE").ok();
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        if only.as_deref().is_some_and(|o| o != id) {
            continue;
        }
        let bytes = std::fs::read(format!("{dir}/{id}.f32")).unwrap();
        let all: Vec<f32> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect();
        let frames = all.len() / 2;
        let r = render_case(case, &buffers);
        for ch in 0..2 {
            let want = &all[ch * frames..(ch + 1) * frames];
            let got = &r.channels[ch];
            let first = (0..frames).find(|&i| got[i] != want[i]);
            let (mut e, mut s, mut max_d, mut at) = (0f64, 0f64, 0f64, 0);
            for i in 0..frames {
                let d = (got[i] - want[i]) as f64;
                e += d * d;
                s += (want[i] as f64).powi(2);
                if d.abs() > max_d {
                    (max_d, at) = (d.abs(), i);
                }
            }
            eprintln!(
                "{id} ch{ch}: err {:.1} dB, max |d| {max_d:.3e} at {at} (got {} want {}), first diff {first:?}",
                10.0 * (e.max(1e-300) / s).log10(),
                got[at],
                want[at]
            );
        }
    }
}
