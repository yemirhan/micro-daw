//! `processAudio` / `generateSignal` against Chromium renders of the real TS
//! (`rune/parity/fixtures/sample_processor.json`, from
//! `rune/parity/chromium/capture-sample-processor.steps.mjs`): per case and channel the output
//! length, the peak and the full-length block evidence of `stats.mjs` (`blockEvidence`), from
//! which the error-to-signal ratio of the whole render is estimated. `Math.random` is the page's
//! seeded mulberry32 per case; Tone's noise buffer is shared from the first reverb case on.
//!
//! `PARITY_DEBUG_DIR=<dir>` (with the capture run the same way) compares the raw renders sample by
//! sample: `cargo test -p mdaw-engine sample_processor_parity_debug -- --ignored --nocapture`.

use serde_json::Value;

use super::*;

const BLOCK: usize = 1024;
const PROJ: usize = 16;
const PROJ_SEED: u32 = 0x9a0f_0001;
/// Required error-to-signal ratio of every case (whole render, every channel).
const MAX_ERR_DB: f64 = -60.0;

struct Mulberry32(u32);

impl Mulberry32 {
    fn next_u32(&mut self) -> u32 {
        self.0 = self.0.wrapping_add(0x6d2b_79f5);
        let a = self.0;
        let mut t = (a ^ (a >> 15)).wrapping_mul(1 | a);
        t = (t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t))) ^ t;
        t ^ (t >> 14)
    }
    fn next_f64(&mut self) -> f64 {
        self.next_u32() as f64 / 4_294_967_296.0
    }
}

fn fixture() -> Value {
    let p = format!(
        "{}/../../parity/fixtures/sample_processor.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(&p).expect("sample_processor.json")).unwrap()
}

fn block_evidence(x: &[f32]) -> Vec<[f64; PROJ + 1]> {
    let mut rng = Mulberry32(PROJ_SEED);
    x.chunks(BLOCK)
        .map(|b| {
            let mut out = [0f64; PROJ + 1];
            out[0] = b.iter().map(|&v| v as f64 * v as f64).sum();
            for p in &mut out[1..] {
                for w in b.chunks(32) {
                    let word = rng.next_u32();
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

/// Whole-render error-to-signal dB from the reference evidence.
fn waveform_error(x: &[f32], reference: &[f64]) -> f64 {
    let ours = block_evidence(x);
    assert_eq!(ours.len() * (PROJ + 1), reference.len(), "evidence size");
    let (mut err, mut sig) = (0.0, 0.0);
    for (o, r) in ours.iter().zip(reference.chunks(PROJ + 1)) {
        err += (1..=PROJ).map(|k| (o[k] - r[k]).powi(2)).sum::<f64>() / PROJ as f64;
        sig += r[0];
    }
    10.0 * (err.max(1e-300) / sig.max(1e-300)).log10()
}

/// `input` of the steps script.
fn case_input(r: &Value, sr: u32, channels: usize) -> AudioBuffer {
    let srf = sr as f64;
    let len = r["len"].as_f64().unwrap();
    let frames = (len * srf).round() as usize;
    let mut out = Vec::new();
    for ch in 0..channels {
        let mut x = vec![0f64; frames];
        for p in r["plucks"].as_array().unwrap() {
            let s = (p["at"].as_f64().unwrap() * srf).round() as usize;
            let (f, d, a) = (
                p["freq"].as_f64().unwrap(),
                p["decay"].as_f64().unwrap(),
                p["amp"][ch].as_f64().unwrap(),
            );
            for (i, v) in x.iter_mut().enumerate().skip(s) {
                let t = (i - s) as f64 / srf;
                *v += a
                    * (-t / d).exp()
                    * (2.0 * std::f64::consts::PI * f * t + ch as f64 * 0.7).sin();
            }
        }
        let n = &r["noise"];
        let mut rnd = Mulberry32((n["seed"].as_i64().unwrap() + ch as i64) as u32);
        let s = (n["at"].as_f64().unwrap() * srf).round() as usize;
        let e = frames.min(s + (n["len"].as_f64().unwrap() * srf).round() as usize);
        let amp = n["amp"][ch].as_f64().unwrap();
        for v in &mut x[s..e] {
            *v += amp * (rnd.next_f64() * 2.0 - 1.0);
        }
        out.push(x.into_iter().map(|v| v as f32).collect());
    }
    AudioBuffer {
        sample_rate: sr,
        channels: out,
    }
}

fn filter_type(s: &str) -> FilterType {
    match s {
        "highpass" => FilterType::Highpass,
        "bandpass" => FilterType::Bandpass,
        _ => FilterType::Lowpass,
    }
}

fn process_params(p: &Value) -> ProcessParams {
    let on = |k: &str| p[k]["enabled"].as_bool().unwrap();
    let f = |k: &str, f: &str| p[k][f].as_f64().unwrap();
    ProcessParams {
        filter: FilterStage {
            enabled: on("filter"),
            kind: filter_type(p["filter"]["type"].as_str().unwrap()),
            cutoff: f("filter", "cutoff"),
            resonance: f("filter", "resonance"),
        },
        pitch_shift: PitchStage {
            enabled: on("pitchShift"),
            semitones: f("pitchShift", "semitones"),
        },
        reverse: on("reverse"),
        normalize: on("normalize"),
        reverb: ReverbStage {
            enabled: on("reverb"),
            decay: f("reverb", "decay"),
            wet: f("reverb", "wet"),
        },
        delay: DelayStage {
            enabled: on("delay"),
            time: f("delay", "time"),
            feedback: f("delay", "feedback"),
            wet: f("delay", "wet"),
        },
    }
}

fn signal_params(p: &Value) -> SignalParams {
    SignalParams {
        kind: match p["type"].as_str().unwrap() {
            "square" => SignalType::Square,
            "sawtooth" => SignalType::Sawtooth,
            "triangle" => SignalType::Triangle,
            "white-noise" => SignalType::WhiteNoise,
            _ => SignalType::Sine,
        },
        frequency: p["frequency"].as_f64().unwrap(),
        duration: p["duration"].as_f64().unwrap(),
        amplitude: p["amplitude"].as_f64().unwrap(),
    }
}

/// Every case, rendered in fixture order (the noise bank is shared like Tone's cache).
fn render_all(fx: &Value) -> Vec<(String, AudioBuffer)> {
    let ctx_rate = fx["contextRate"].as_u64().unwrap() as u32;
    // `seedNoise`: Tone's cached white buffer, overwritten with mulberry32(seed)·2 − 1.
    let n = &fx["noise"];
    let mut nr = Mulberry32(n["seed"].as_u64().unwrap() as u32);
    let frames = n["frames"].as_u64().unwrap() as usize;
    let mut chan = || -> Vec<f32> {
        (0..frames)
            .map(|_| (nr.next_f64() * 2.0 - 1.0) as f32)
            .collect()
    };
    let white = [chan(), chan()];
    let mut noise = NoiseBank::with_white(n["rate"].as_u64().unwrap() as u32, white);
    let mut out = Vec::new();
    for c in fx["cases"].as_array().unwrap() {
        let mut rng = Mulberry32(c["seed"].as_u64().unwrap() as u32);
        let mut rnd = move || rng.next_f64();
        let id = c["id"].as_str().unwrap().to_owned();
        let buf = if c["kind"] == "process" {
            let sr = c["sr"].as_u64().unwrap() as u32;
            let ch = c["channels"].as_u64().unwrap() as usize;
            let input = case_input(&c["recipe"], sr, ch);
            process_audio(&input, &process_params(&c["params"]), &mut rnd, &mut noise)
        } else {
            generate_signal(&signal_params(&c["params"]), ctx_rate, &mut rnd)
        };
        out.push((id, buf));
    }
    out
}

#[test]
fn sample_processor_matches_chromium() {
    let fx = fixture();
    let renders = render_all(&fx);
    let mut failures = Vec::new();
    println!(
        "{:<30} {:>3} {:>8} {:>8} {:>9}",
        "case", "ch", "len", "ref", "err dB"
    );
    for ((id, buf), c) in renders.iter().zip(fx["cases"].as_array().unwrap()) {
        let outs = c["out"].as_array().unwrap();
        if outs.len() != buf.channel_count() {
            failures.push(format!(
                "{id}: {} channels vs {}",
                buf.channel_count(),
                outs.len()
            ));
            continue;
        }
        for (ch, r) in outs.iter().enumerate() {
            let x = &buf.channels[ch];
            let len = r["length"].as_u64().unwrap() as usize;
            if x.len() != len {
                failures.push(format!("{id}/{ch}: length {} vs {len}", x.len()));
                continue;
            }
            let reference: Vec<f64> = r["blocks"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap())
                .collect();
            let err = waveform_error(x, &reference);
            println!("{id:<30} {ch:>3} {:>8} {len:>8} {err:>9.1}", x.len());
            if err > MAX_ERR_DB {
                failures.push(format!("{id}/{ch}: error/signal {err:.1} dB"));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
#[ignore = "needs PARITY_DEBUG_DIR with the raw Chromium renders"]
fn sample_processor_parity_debug() {
    let Some(dir) = std::env::var_os("PARITY_DEBUG_DIR") else {
        return;
    };
    let fx = fixture();
    for (id, buf) in render_all(&fx) {
        let raw = std::fs::read(std::path::Path::new(&dir).join(format!("{id}.f32"))).unwrap();
        let theirs: Vec<f32> = raw
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect();
        let ours: Vec<f32> = buf.channels.concat();
        if let Ok(w) = std::env::var("DUMP")
            && w == id
        {
            let raw: Vec<u8> = ours.iter().flat_map(|v| v.to_le_bytes()).collect();
            std::fs::write(
                std::path::Path::new(&dir).join(format!("{id}.ours.f32")),
                raw,
            )
            .unwrap();
            for i in (0..ours.len().min(theirs.len())).step_by(97).take(60) {
                println!("{i:>6} {:>12.6} {:>12.6}", ours[i], theirs[i]);
            }
        }
        let n = ours.len().min(theirs.len());
        let (mut err, mut sig, mut max, mut at) = (0f64, 0f64, 0f64, 0);
        for i in 0..n {
            let d = (ours[i] - theirs[i]) as f64;
            err += d * d;
            sig += theirs[i] as f64 * theirs[i] as f64;
            if d.abs() > max {
                (max, at) = (d.abs(), i);
            }
        }
        println!(
            "{id:<30} len {} vs {} err {:.1} dB, max |Δ| {max:.2e} @ {at} (ours {} theirs {})",
            ours.len(),
            theirs.len(),
            10.0 * (err.max(1e-300) / sig).log10(),
            ours.get(at).copied().unwrap_or(0.0),
            theirs.get(at).copied().unwrap_or(0.0)
        );
    }
}
