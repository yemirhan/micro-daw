//! [`BufferPlayer`] vs headless Chromium (`rune/parity/fixtures/playback_reference.json`,
//! written by `rune/parity/chromium/capture-playback.steps.mjs`).
//!
//! Every case is compared over its full render length:
//! - by FNV-1a 64 digest of the f32 output (bit-exactness),
//! - by per-1024-frame RMS,
//! - and, for the cases whose waveform is stored, sample by sample.

use std::sync::Arc;

use serde_json::Value;

use super::{BufferPlayer, StartParams, render_offline};
use crate::audio_io::resample;
use crate::buffer::AudioBuffer;
use crate::classic::synthesize_classic_voice;
use mdaw_model::ClassicMachineId;

fn fixture() -> Value {
    serde_json::from_str(include_str!(
        "../../../../parity/fixtures/playback_reference.json"
    ))
    .unwrap()
}

fn b64(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => continue,
            _ => panic!("bad base64 {c}"),
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

fn f32s(bytes: &[u8]) -> Vec<f32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect()
}

/// `synthChannels` of capture-playback-page.ts.
fn synth_channels(frames: usize, channels: usize, seed: u32) -> Vec<Vec<f32>> {
    (0..channels as u32)
        .map(|c| {
            let mut s = seed.wrapping_add(c.wrapping_mul(0x9e37_79b9));
            let mut g = 1.0f32;
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

/// The input buffer of a case. `reverse` = `getReversedBuffer` (only for raw cases; SamplerKit
/// reverses the pad buffer itself).
fn build_buffer(spec: &Value, honor_reverse: bool) -> AudioBuffer {
    let mut b = match spec["kind"].as_str().unwrap() {
        "classic" => {
            let m = ClassicMachineId::from_id(spec["machine"].as_str().unwrap()).unwrap();
            AudioBuffer::mono(
                44100,
                synthesize_classic_voice(m, spec["voice"].as_str().unwrap()).unwrap(),
            )
        }
        "synth" => AudioBuffer {
            sample_rate: spec["sampleRate"].as_u64().unwrap() as u32,
            channels: synth_channels(
                spec["frames"].as_u64().unwrap() as usize,
                spec["channels"].as_u64().unwrap() as usize,
                spec["seed"].as_u64().unwrap() as u32,
            ),
        },
        k => panic!("unknown buffer kind {k}"),
    };
    if honor_reverse && spec["reverse"].as_bool() == Some(true) {
        reverse(&mut b);
    }
    b
}

fn reverse(b: &mut AudioBuffer) {
    for c in &mut b.channels {
        c.reverse();
    }
}

fn fnv64(channels: &[Vec<f32>]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for ch in channels {
        for x in ch {
            for b in x.to_le_bytes() {
                h = (h ^ b as u64).wrapping_mul(0x100_0000_01b3);
            }
        }
    }
    format!("{h:016x}")
}

fn block_rms(x: &[f32]) -> Vec<f64> {
    x.chunks(1024)
        .map(|c| (c.iter().map(|&v| v as f64 * v as f64).sum::<f64>() / c.len() as f64).sqrt())
        .collect()
}

/// Error energy relative to the reference signal, in dB (−inf when identical).
fn error_db(got: &[f32], want: &[f32]) -> f64 {
    let (mut e, mut s) = (0.0f64, 0.0f64);
    for (a, b) in got.iter().zip(want) {
        e += (*a as f64 - *b as f64).powi(2);
        s += (*b as f64).powi(2);
    }
    10.0 * (e / s).log10()
}

fn raw_params(k: &Value) -> StartParams {
    let when = k["when"].as_f64().unwrap();
    let offset = k["offset"].as_f64().unwrap();
    let rate = k["playbackRate"].as_f64().unwrap();
    let duration = k["duration"].as_f64();
    let stop = k["stop"].as_f64();
    match k["rateMode"].as_str().unwrap() {
        // playbackRate.setValueAtTime(rate, 0): the intrinsic value is still 1 at start().
        "param" => StartParams {
            duration,
            ..StartParams::sampler(when, offset, rate, stop)
        },
        "value" => StartParams::with_value(when, offset, duration, stop, rate),
        m => panic!("rate mode {m}"),
    }
}

/// Renders the case, checks digest, block RMS and (if stored) the waveform.
/// Returns (id, error dB on the stored waveform, or None).
fn check_case(k: &Value, params: StartParams, buffer: AudioBuffer) -> Option<f64> {
    let id = k["id"].as_str().unwrap();
    let frames = k["outFrames"].as_u64().unwrap() as usize;
    let out = render_offline(
        Arc::new(buffer),
        k["ctxRate"].as_u64().unwrap() as u32,
        params,
        frames,
    );
    assert_eq!(
        out.len() as u64,
        k["outChannels"].as_u64().unwrap(),
        "{id}: channels"
    );
    let mut err = None;
    if let Some(wave) = k["wave"].as_array() {
        for (c, w) in wave.iter().enumerate() {
            let want = f32s(&b64(w.as_str().unwrap()));
            assert_eq!(want.len(), frames, "{id}");
            let db = error_db(&out[c], &want);
            let first = out[c]
                .iter()
                .zip(&want)
                .position(|(a, b)| a.to_bits() != b.to_bits());
            assert!(
                first.is_none(),
                "{id} ch{c}: first differing frame {first:?}, error {db:.1} dB"
            );
            err = Some(db);
        }
    }
    for (c, rms) in k["rms"].as_array().unwrap().iter().enumerate() {
        let got = block_rms(&out[c]);
        let want: Vec<f64> = rms
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        assert_eq!(got.len(), want.len(), "{id}");
        for (i, (g, w)) in got.iter().zip(&want).enumerate() {
            assert!(
                (g - w).abs() <= 1e-6 * w.abs().max(1e-30),
                "{id} ch{c} block {i}: rms {g} vs {w}"
            );
        }
    }
    assert_eq!(
        fnv64(&out),
        k["fnv64"].as_str().unwrap(),
        "{id}: output digest differs from Chromium"
    );
    err
}

#[test]
fn raw_cases_bit_exact_vs_chromium() {
    let f = fixture();
    let cases = f["raw"].as_array().unwrap();
    assert!(cases.len() >= 60);
    let mut rates = std::collections::BTreeSet::new();
    for k in cases {
        rates.insert(k["ctxRate"].as_u64().unwrap());
        let err = check_case(k, raw_params(k), build_buffer(&k["buffer"], true));
        if let Some(db) = err {
            println!("{}: waveform error {db} dB", k["id"].as_str().unwrap());
        }
    }
    assert_eq!(rates.into_iter().collect::<Vec<_>>(), [44100, 48000, 96000]);
}

/// The app path: `SamplerKit.trigger` in Tone.Offline, with the native parameters it produced
/// checked against `computeSlice` (proves the mapping), then rendered from the pad settings.
#[test]
fn sampler_kit_cases_bit_exact_vs_chromium() {
    let f = fixture();
    let cases = f["sampler"].as_array().unwrap();
    assert_eq!(cases.len(), 8);
    for k in cases {
        let id = k["id"].as_str().unwrap();
        let mut buffer = build_buffer(&k["buffer"], false);
        let pad = &k["pad"];
        let reverse_pad = pad["reverse"].as_bool().unwrap();
        // computeSlice.
        let dur = buffer.duration();
        let start = pad["startSec"].as_f64().unwrap().clamp(0.0, dur);
        let end = pad["endSec"].as_f64().map_or(dur, |e| e.clamp(0.0, dur));
        let rate = 2f64.powf(pad["pitch"].as_f64().unwrap() / 12.0);
        let offset = if reverse_pad { dur - end } else { start };
        let when = k["when"].as_f64().unwrap();

        let native = &k["native"];
        let s = native["start"].as_array().unwrap();
        assert_eq!(s[0].as_f64().unwrap(), when, "{id}: start time");
        assert_eq!(s[1].as_f64().unwrap(), offset, "{id}: offset");
        assert!(s[2].is_null(), "{id}: no duration is passed");
        let ev = native["rateEvents"].as_array().unwrap();
        if rate == 1.0 {
            assert!(ev.is_empty(), "{id}");
        } else {
            assert_eq!(ev.len(), 1, "{id}");
            assert_eq!(ev[0]["kind"], "setValueAtTime");
            assert_eq!(ev[0]["args"][0].as_f64().unwrap(), rate, "{id}: rate");
            assert_eq!(ev[0]["args"][1].as_f64().unwrap(), 0.0);
        }
        let stop = native["stop"][0].as_f64().unwrap();
        assert!(
            stop >= when + (end - start) / rate,
            "{id}: Tone stops the native node after the voice end"
        );

        if reverse_pad {
            reverse(&mut buffer);
        }
        check_case(
            k,
            StartParams::sampler(when, offset, rate, Some(stop)),
            buffer,
        );
    }
}

/// Why the engine must NOT pre-resample 44.1 kHz voices to the device rate: SincResampler +
/// rate-1 playback is a different signal from Chromium's linear-interpolating node.
#[test]
fn pre_resampling_does_not_match() {
    let f = fixture();
    let k = f["raw"]
        .as_array()
        .unwrap()
        .iter()
        .find(|k| k["id"] == "chh808_48000_p0_plain")
        .unwrap();
    let want = f32s(&b64(k["wave"][0].as_str().unwrap()));
    let voice = build_buffer(&k["buffer"], false);
    let pre = resample(&voice, 48000);
    let got = render_offline(
        Arc::new(pre),
        48000,
        StartParams::with_value(0.0, 0.0, None, None, 1.0),
        want.len(),
    );
    let db = error_db(&got[0], &want);
    println!("pre-resampled (SincResampler) vs Chromium node: {db:.1} dB");
    assert!(db > -60.0, "pre-resampling unexpectedly matches ({db} dB)");
    // The node itself is exact on the same case.
    let direct = render_offline(
        Arc::new(voice),
        48000,
        StartParams::with_value(0.0, 0.0, None, None, 1.0),
        want.len(),
    );
    assert_eq!(direct[0], want);
}

/// The offset rounding is real: the same pitched start is a different signal depending on
/// whether the rate was automated (Tone, offset snapped) or assigned to `.value` (exact offset).
#[test]
fn offset_snapping_depends_on_how_the_rate_was_set() {
    let f = fixture();
    for sr in [44100, 48000, 96000] {
        let get = |m: &str| {
            f["raw"]
                .as_array()
                .unwrap()
                .iter()
                .find(|k| k["id"] == format!("chh808_{sr}_p7_{m}").as_str())
                .unwrap()
                .clone()
        };
        let (param, value) = (get("param"), get("value"));
        assert_ne!(param["fnv64"], value["fnv64"]);
        let mut flipped = raw_params(&param);
        flipped.snap_offset = false;
        let out = render_offline(
            Arc::new(build_buffer(&param["buffer"], true)),
            sr,
            flipped,
            value["outFrames"].as_u64().unwrap() as usize,
        );
        assert_eq!(fnv64(&out), value["fnv64"].as_str().unwrap());
    }
}

/// Exactness doesn't depend on the engine's block size outside grain mode: 37-frame blocks.
#[test]
fn odd_block_sizes_match_outside_grain_mode() {
    let f = fixture();
    for k in f["raw"].as_array().unwrap() {
        if k["duration"].is_number() {
            continue;
        }
        let buffer = Arc::new(build_buffer(&k["buffer"], true));
        let frames = k["outFrames"].as_u64().unwrap() as usize;
        let mut p = BufferPlayer::new(
            buffer.clone(),
            k["ctxRate"].as_u64().unwrap() as u32,
            raw_params(k),
        );
        let mut out = vec![vec![0.0f32; frames]; buffer.channel_count()];
        let mut pos = 0;
        while pos < frames {
            let n = 37.min(frames - pos);
            let mut s: Vec<&mut [f32]> = out.iter_mut().map(|c| &mut c[pos..pos + n]).collect();
            p.process(&mut s, pos as u64);
            pos += n;
        }
        assert_eq!(fnv64(&out), k["fnv64"].as_str().unwrap(), "{}", k["id"]);
    }
}

/// Full-waveform comparison against raw captures:
/// `PARITY_DEBUG_DIR=<dir> cargo test -p mdaw-dsp abs_node_debug -- --ignored --nocapture`
/// (the dir written by `PARITY_DEBUG_DIR=<dir> node scripts/ui-snap.mjs --steps
/// rune/parity/chromium/capture-playback.steps.mjs`).
#[test]
#[ignore]
fn abs_node_debug_full_waveforms() {
    let Ok(dir) = std::env::var("PARITY_DEBUG_DIR") else {
        eprintln!("PARITY_DEBUG_DIR not set");
        return;
    };
    let f = fixture();
    let all = f["raw"]
        .as_array()
        .unwrap()
        .iter()
        .map(|k| (k, raw_params(k), true))
        .chain(f["sampler"].as_array().unwrap().iter().map(|k| {
            let n = &k["native"];
            let p = StartParams::sampler(
                n["start"][0].as_f64().unwrap(),
                n["start"][1].as_f64().unwrap(),
                n["rateEvents"][0]["args"][0].as_f64().unwrap_or(1.0),
                n["stop"][0].as_f64(),
            );
            (k, p, false)
        }));
    for (k, p, raw) in all {
        let id = k["id"].as_str().unwrap();
        let Ok(bytes) = std::fs::read(format!("{dir}/{id}.f32")) else {
            println!("{id}: no dump");
            continue;
        };
        let want = f32s(&bytes);
        let mut b = build_buffer(&k["buffer"], raw);
        if !raw && k["pad"]["reverse"].as_bool() == Some(true) {
            reverse(&mut b);
        }
        let frames = k["outFrames"].as_u64().unwrap() as usize;
        let out = render_offline(
            Arc::new(b),
            k["ctxRate"].as_u64().unwrap() as u32,
            p,
            frames,
        );
        let got: Vec<f32> = out.concat();
        let max = got
            .iter()
            .zip(&want)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        let first = got
            .iter()
            .zip(&want)
            .position(|(a, b)| a.to_bits() != b.to_bits());
        println!(
            "{id}: max |err| {max:e}, error {:.1} dB, first diff {first:?}",
            error_db(&got, &want)
        );
    }
}
