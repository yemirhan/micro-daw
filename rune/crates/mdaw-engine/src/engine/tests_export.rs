//! WAV export: ExportService.test.ts (length rules) and renders on the real core, plus the
//! end-to-end parity test against the Chromium capture of the Electron app's
//! `ExportService.exportWav` (`rune/parity/fixtures/export_reference.json`, captured by
//! `rune/parity/chromium/capture-export.steps.mjs`).
//!
//! Parity (48 kHz, 3 drum projects: factory kit, TR-808 kit with swing 0.5 on 1/16, gate pads,
//! choke, pitch / reverse / pan / filter / trim, factory + TR-909 with swing 0.3 on 1/8 —
//! fractional ticks —, a soloed track (ignored) and a muted one (skipped)): WAV length exact;
//! full-render error ≤ −60 dB required, measured −86.5 dB on all three (phase 4: the real track
//! strips, convolver included; phase 3 had −63 to −66 dB) against the app with
//! Tone's double-fired Part events removed. The app as-is differs by −9.3 / −8.5 dB on the first
//! two: `Tone.Offline` hands some Part events out TWICE (4 of 35 and 11 of 58 SamplerKit triggers,
//! all at times on a 128-frame block boundary: 1.0000000000000062 s, 3.0000000000000355 s, …,
//! the offline clock's accumulated `+= 128/sr` against rounded ticks), which doubles those hits;
//! the Rust engine plays every note once (a Tone bug not reproduced). Getting there needed
//! (both now in the core): Part notes on the nearest integer tick (`ToneEvent` rounds its
//! offset), and the disabled effects' −56 dB wet leaks (`fx/track.rs`, `fx/xfade.rs`).
//!
//! Phase 4 adds synth projects (same capture, `rune/parity/chromium/capture-export.steps.mjs`):
//! the "Ambient Pad" and "EDM Drop" templates as `loadFromTemplate` installs them, `keys_piano`
//! (FM keys with overrides, rhodes, the sampled grand piano, a mono legato bass) and `fx_tracks`
//! (filter LFO, chorus, reverb, delay, distortion, EQ and compressor on two synths and a drum
//! track), `pluck_harp` (pluck voices, incl. a Part event Tone fires a tick early and a fresh
//! voice whose comb params are swapped in after its first quantum) and `saw_lead` (sub
//! oscillator + fat stack whose construction writes land at Tone's clock time). They need
//! `Tone.Offline`'s clock model (`SynthPlayer::new_offline`). A fixture entry with
//! `knownDeviation` is reported, not asserted (none at present). See
//! docs/rune-port/phase-4-instruments.md for the numbers.

use std::sync::Arc;

use mdaw_dsp::factory::Mulberry32;
use mdaw_model::kit::create_factory_kit;
use mdaw_model::{Arrangement, PadPlayMode, Track};
use serde_json::{Value, json};

use super::export_wav::{EXPORT_TAIL_SECONDS, effects_tail_for_tests, end_seconds_for_tests};
use super::test_rig::*;
use super::{ExportError, NoSamples, export_wav, render_export};
use crate::buffers::{FsReader, PadBufferCache};

fn drum_track(notes: Value) -> Track {
    serde_json::from_value(json!({
        "id": "d", "name": "Drums",
        "instrument": {"type": "drums", "presetIndex": 0, "kit": create_factory_kit()},
        "regions": [{"id": "r", "startBeat": 0, "lengthBeats": 4, "notes": notes}],
        "volume": 0, "pan": 0, "muted": false, "solo": false, "color": "#f00"
    }))
    .unwrap()
}

fn hit(pad: usize, start: f64, dur: f64) -> Value {
    json!({"note": 36 + pad, "velocity": 0.8, "startBeat": start, "durationBeats": dur, "isDrum": true})
}

fn arrangement(tracks: Vec<Track>) -> Arrangement {
    let mut a: Arrangement =
        serde_json::from_value(json!({"id": "a", "name": "x", "bpm": 120, "lengthBeats": 16}))
            .unwrap();
    a.tracks = tracks;
    a
}

#[test]
fn a_long_one_shot_outlives_its_note_unknown_buffers_fall_back_to_the_note() {
    let arr = arrangement(vec![drum_track(json!([hit(0, 2.0, 0.25)]))]);
    assert!((end_seconds_for_tests(&arr, &|_| Some(6.0)) - 7.0).abs() < 1e-9);
    assert!((end_seconds_for_tests(&arr, &|_| None) - 1.125).abs() < 1e-9);
}

#[test]
fn honours_trim_and_pitch_of_the_pad() {
    let mut t = drum_track(json!([hit(0, 0.0, 0.25)]));
    let p = &mut t.instrument.kit.as_mut().unwrap().pads[0];
    (p.start_sec, p.end_sec, p.pitch) = (1.0, Some(3.0), 12.0); // 2 s slice at double speed
    let arr = arrangement(vec![t]);
    assert!((end_seconds_for_tests(&arr, &|_| Some(10.0)) - 1.0).abs() < 1e-9);
}

#[test]
fn gate_pads_stop_at_note_end_plus_decay_never_past_the_slice() {
    let mut t = drum_track(json!([hit(0, 0.0, 2.0)]));
    let p = &mut t.instrument.kit.as_mut().unwrap().pads[0];
    (p.mode, p.decay) = (PadPlayMode::Gate, 0.5);
    let arr = arrangement(vec![t]);
    assert!((end_seconds_for_tests(&arr, &|_| Some(10.0)) - 1.5).abs() < 1e-9);
    assert!((end_seconds_for_tests(&arr, &|_| Some(1.2)) - 1.2).abs() < 1e-9);
}

#[test]
fn applies_swing_to_drum_note_starts_like_playback() {
    let mut arr = arrangement(vec![drum_track(json!([hit(0, 3.75, 0.25)]))]);
    arr.beat = serde_json::from_value(json!({"loop": {"swing": 0.5, "quantize": "1/16"}})).unwrap();
    let want = (3.75 + 0.125) * 0.5 + 1.0;
    assert!((end_seconds_for_tests(&arr, &|_| Some(1.0)) - want).abs() < 1e-9);
}

#[test]
fn adds_a_feedback_delay_tail_and_skips_muted_tracks() {
    let mut t = drum_track(json!([hit(0, 0.0, 0.25)]));
    t.effects = Some(Default::default());
    let d = &mut t.effects.as_mut().unwrap().delay;
    (d.enabled, d.wet, d.time, d.feedback) = (true, 0.5, 0.5, 0.5);
    let tail = 0.5 * (1e-3f64.ln() / 0.5f64.ln());
    assert!((effects_tail_for_tests(&t) - tail).abs() < 1e-9);
    let arr = arrangement(vec![t.clone()]);
    assert!((end_seconds_for_tests(&arr, &|_| Some(1.0)) - (1.0 + tail)).abs() < 1e-9);
    t.muted = true;
    assert_eq!(
        end_seconds_for_tests(&arrangement(vec![t]), &|_| Some(1.0)),
        0.0
    );
}

fn test_cache() -> PadBufferCache {
    PadBufferCache::new(SR, None, Arc::new(FsReader), Arc::new(TestFiles::default()))
}

fn test_track(id: &str, notes: Value) -> Track {
    let mut t = drum_track(notes);
    t.id = id.into();
    t.instrument.kit = Some(test_kit(&[]));
    t
}

#[test]
fn nothing_to_export_and_the_wav_has_the_render_length() {
    let cache = test_cache();
    let empty = arrangement(vec![]);
    assert_eq!(
        render_export(&empty, SR, &cache, &NoSamples).unwrap_err(),
        ExportError::NothingToExport
    );
    let arr = arrangement(vec![test_track("a", json!([hit(0, 1.0, 0.25)]))]);
    let wav = export_wav(&arr, SR, &cache, &NoSamples).unwrap();
    // note at 0.5 s + the 20 ms click + 3 s tail
    let frames = ((0.5 + 0.02 + EXPORT_TAIL_SECONDS) * SR as f64).floor() as usize;
    assert_eq!(wav.len(), 44 + frames * 4);
    assert_eq!(&wav[0..4], b"RIFF");
}

#[test]
fn renders_drums_at_their_swung_times_skips_muted_tracks_and_ignores_solo() {
    let cache = test_cache();
    let a = test_track("a", json!([hit(0, 0.25, 0.25)]));
    let mut b = test_track("b", json!([hit(1, 1.0, 0.25)]));
    b.solo = true; // ignored: a still plays
    let mut c = test_track("c", json!([hit(2, 2.0, 0.25)]));
    c.muted = true;
    let mut arr = arrangement(vec![a, b, c]);
    arr.beat = serde_json::from_value(json!({"loop": {"swing": 0.5}})).unwrap();
    let buf = render_export(&arr, SR, &cache, &NoSamples).unwrap();
    let l = &buf.channels[0];
    let onset = |from: f64| {
        let f = (from * SR as f64) as usize;
        (f..l.len()).find(|&i| l[i].abs() > 5e-3).unwrap()
    };
    // 0.25 is an odd 16th: swung to 0.375 beat = 0.1875 s
    assert_eq!(onset(0.0), (0.1875 * SR as f64).ceil() as usize);
    assert_eq!(onset(0.3), (0.5 * SR as f64) as usize);
    // muted track skipped (only the strips' −56 dB FX-leak echoes ring on)
    assert!(
        l[(0.9 * SR as f64) as usize..]
            .iter()
            .all(|x| x.abs() < 2e-3)
    );
}

// --- Chromium parity --------------------------------------------------------------------------

const BLOCK: usize = 1024;
const PROJ: usize = 16;
const PROJ_SEED: u32 = 0x9a0f_0001;
/// Required: error-to-signal energy over the whole render, both channels.
const MAX_ERR_DB: f64 = -60.0;

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
                        *p += if (word >> t) & 1 == 1 {
                            -(v as f64)
                        } else {
                            v as f64
                        };
                    }
                }
            }
            out
        })
        .collect()
}

/// 16-bit WAV → channels of int16 / 32768.
fn decode_wav(bytes: &[u8]) -> Vec<Vec<f32>> {
    let ch = u16::from_le_bytes([bytes[22], bytes[23]]) as usize;
    let frames = (bytes.len() - 44) / (2 * ch);
    (0..ch)
        .map(|c| {
            (0..frames)
                .map(|i| {
                    let o = 44 + (i * ch + c) * 2;
                    i16::from_le_bytes([bytes[o], bytes[o + 1]]) as f32 / 32768.0
                })
                .collect()
        })
        .collect()
}

/// (error energy, signal energy, worst block error dB, its block) of one channel.
fn channel_error(x: &[f32], reference: &[f64]) -> (f64, f64, f64, usize) {
    let ours = block_evidence(x);
    assert_eq!(ours.len() * (PROJ + 1), reference.len(), "evidence size");
    let (mut err, mut sig, mut worst, mut at) = (0.0, 0.0, -400f64, 0);
    for (i, (o, r)) in ours.iter().zip(reference.chunks(PROJ + 1)).enumerate() {
        let e = (1..=PROJ).map(|k| (o[k] - r[k]).powi(2)).sum::<f64>() / PROJ as f64;
        err += e;
        sig += r[0];
        // blocks above −60 dBFS
        if r[0] / BLOCK as f64 >= 1e-6 {
            let db = 10.0 * (e.max(1e-300) / r[0]).log10();
            if db > worst {
                (worst, at) = (db, i);
            }
        }
    }
    (err, sig, worst, at)
}

#[test]
fn wav_export_matches_the_electron_app_in_chromium() {
    let p = format!(
        "{}/../../parity/fixtures/export_reference.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let fixture: Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
    let rate = fixture["meta"]["sampleRate"].as_u64().unwrap() as u32;
    let cache = PadBufferCache::with_fs(rate, None);
    let mut report = Vec::new();
    let mut failed = Vec::new();
    for case in fixture["projects"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let arr: Arrangement = serde_json::from_value(case["arrangement"].clone()).unwrap();
        let wav = export_wav(&arr, rate, &cache, &NoSamples).unwrap();
        assert_eq!(
            wav.len() as u64,
            case["wav"]["bytes"].as_u64().unwrap(),
            "{id}: WAV length"
        );
        let chans = decode_wav(&wav);
        let error = |key: &str| {
            let (mut err, mut sig) = (0.0, 0.0);
            let mut worst = (-400f64, 0usize);
            for (c, x) in chans.iter().enumerate() {
                let reference: Vec<f64> = case[key][c]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_f64().unwrap())
                    .collect();
                let (e, s, w, at) = channel_error(x, &reference);
                err += e;
                sig += s;
                if w > worst.0 {
                    worst = (w, at);
                }
            }
            (10.0 * (err.max(1e-300) / sig).log10(), worst)
        };
        let (db, worst) = error("evidenceDeduped");
        let (raw, _) = error("evidence");
        let dups = case["duplicates"].as_array().unwrap().len();
        report.push(format!(
            "{id}: {db:.1} dB (worst block {:.1} dB at {:.3} s); vs the app as-is {raw:.1} dB \
             ({dups} of {} triggers doubled by Tone)",
            worst.0,
            (worst.1 * BLOCK) as f64 / rate as f64,
            case["triggers"]
        ));
        match case.get("knownDeviation").and_then(Value::as_str) {
            // documented deviations (see the module docs): reported, not asserted
            Some(why) => report.push(format!("  ^ known deviation: {why}")),
            None if db > MAX_ERR_DB => failed.push(id.to_owned()),
            None => {}
        }
    }
    eprintln!("export parity:\n  {}", report.join("\n  "));
    assert!(failed.is_empty(), "error > {MAX_ERR_DB} dB: {failed:?}");
}

/// `PARITY_DEBUG_DIR=<dir of the capture's <id>.dedup.wav> cargo test -p mdaw-engine
/// export_parity_debug -- --ignored --nocapture` (`PARITY_ZOOM=<s>`: 5 ms windows from there):
/// lag / per-window error vs the full WAVs.
#[test]
#[ignore]
#[allow(clippy::needless_range_loop)]
fn export_parity_debug() {
    let dir = std::env::var("PARITY_DEBUG_DIR").expect("PARITY_DEBUG_DIR");
    let p = format!(
        "{}/../../parity/fixtures/export_reference.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let fixture: Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
    let cache = PadBufferCache::with_fs(48_000, None);
    for case in fixture["projects"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let arr: Arrangement = serde_json::from_value(case["arrangement"].clone()).unwrap();
        if std::env::var("PARITY_ONLY").is_ok_and(|o| !o.split(',').any(|x| x == id)) {
            continue;
        }
        let wav = export_wav(&arr, 48_000, &cache, &NoSamples).unwrap();
        // `PARITY_WRITE=1`: also write the Rust render as <dir>/<id>.rust.wav
        if std::env::var("PARITY_WRITE").is_ok() {
            std::fs::write(format!("{dir}/{id}.rust.wav"), &wav).unwrap();
        }
        let ours = decode_wav(&wav);
        let theirs = decode_wav(&std::fs::read(format!("{dir}/{id}.dedup.wav")).unwrap());
        for c in 0..2 {
            let (a, b) = (&ours[c], &theirs[c]);
            let onset = |x: &[f32]| x.iter().position(|v| v.abs() > 1e-3);
            eprintln!(
                "{id} ch{c}: onset ours {:?} theirs {:?}",
                onset(a),
                onset(b)
            );
            let n = a.len().min(b.len());
            let mut best = (0i64, f64::INFINITY);
            for lag in -300i64..=300 {
                let mut e = 0.0;
                for i in 0..n.min(96_000) {
                    let j = i as i64 + lag;
                    if j >= 0 && (j as usize) < n {
                        e += (a[i] as f64 - b[j as usize] as f64).powi(2);
                    }
                }
                if e < best.1 {
                    best = (lag, e);
                }
            }
            eprintln!("  best lag {} frames", best.0);
            if let Ok(z) = std::env::var("PARITY_ZOOM") {
                let z: f64 = z.parse().unwrap();
                let s0 = (z * 48_000.0) as usize;
                for s in (s0..s0 + 4800).step_by(240) {
                    let r = s..(s + 240).min(n);
                    let e: f64 = r.clone().map(|i| ((a[i] - b[i]) as f64).powi(2)).sum();
                    let sa: f64 = r.clone().map(|i| (a[i] as f64).powi(2)).sum();
                    let sb: f64 = r.map(|i| (b[i] as f64).powi(2)).sum();
                    let ab: f64 = (s..(s + 240).min(n))
                        .map(|i| a[i] as f64 * b[i] as f64)
                        .sum();
                    eprintln!(
                        "    {s}: err {:.2e} ours {:.2e} theirs {:.2e} gain {:.5} [{:.6} {:.6} {:.6} {:.6}]",
                        e,
                        sa,
                        sb,
                        ab / sb.max(1e-30),
                        a[s],
                        b[s],
                        a[s + 1],
                        b[s + 1]
                    );
                }
            }
            let w = 12_000;
            for s in (0..n).step_by(w) {
                let r = s..(s + w).min(n);
                let e: f64 = r.clone().map(|i| ((a[i] - b[i]) as f64).powi(2)).sum();
                let sa: f64 = r.clone().map(|i| (a[i] as f64).powi(2)).sum();
                let sb: f64 = r.map(|i| (b[i] as f64).powi(2)).sum();
                if sb > 1e-9 || sa > 1e-9 {
                    eprintln!(
                        "  {:6.3}s err {:7.1} dB  ours {:7.1} theirs {:7.1}",
                        s as f64 / 48_000.0,
                        10.0 * (e.max(1e-30) / sb.max(1e-30)).log10(),
                        10.0 * sa.max(1e-30).log10(),
                        10.0 * sb.max(1e-30).log10()
                    );
                }
            }
        }
    }
}
