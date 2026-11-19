//! Ogg Vorbis decoding vs Chromium (FFmpeg), checked on every channel of every `.ogg` test file.
//!
//! Reference: `rune/parity/fixtures/vorbis_channels.json`, captured by
//! `rune/parity/chromium/capture-vorbis.mjs`. Rust decodes through the vendored, patched
//! `rune/vendor/symphonia-codec-vorbis`. That patch has two parts:
//! - Inverse coupling treats a magnitude of +0.0 as positive, like FFmpeg. With the upstream
//!   code, the second channel of the FFmpeg-encoded `ogg_ffvorbis_44k_stereo.ogg` was off by up
//!   to 1.2.
//! - Output is not clamped to ±1. Chromium keeps overs, and the libvorbis files peak above 1.

use std::path::PathBuf;

use mdaw_dsp::audio_io::decode_file;
use serde_json::Value;

/// Same order as the libvorbis-vs-Chromium agreement (f32 MDCT rounding).
const TOL: f64 = 1e-6;

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn floats(v: &Value) -> Vec<f64> {
    v.as_array()
        .expect("array")
        .iter()
        .map(|x| x.as_f64().expect("number"))
        .collect()
}

fn max_err(rust: impl Iterator<Item = f32>, chrome: &[f64]) -> f64 {
    rust.zip(chrome)
        .map(|(a, &b)| (f64::from(a) - b).abs())
        .fold(0.0, f64::max)
}

#[test]
fn vorbis_every_channel_matches_chromium() {
    let path = manifest().join("../../parity/fixtures/vorbis_channels.json");
    let reference: Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("vorbis_channels.json"))
            .expect("valid json");
    let stride = reference["stride"].as_u64().expect("stride") as usize;
    let files = reference["files"].as_object().expect("files");
    assert!(files.len() >= 3, "expected the three .ogg test files");

    let mut overs = 0;
    for (name, e) in files {
        let buf = decode_file(manifest().join("testdata").join(name)).expect(name);
        let chrome_len = e["length"].as_u64().unwrap() as usize;
        let chans = e["channels"].as_array().unwrap();
        assert_eq!(
            buf.sample_rate as u64,
            e["sampleRate"].as_u64().unwrap(),
            "{name}"
        );
        assert_eq!(buf.channels.len(), chans.len(), "{name}: channel count");
        // Chromium can end single-page streams early (documented deviation); Rust keeps the
        // granule length, so compare over Chromium's length.
        assert!(
            buf.frames() >= chrome_len,
            "{name}: {} < {chrome_len}",
            buf.frames()
        );

        let start = e["windowStart"].as_u64().unwrap() as usize;
        for (c, (rust, chrome)) in buf.channels.iter().zip(chans).enumerate() {
            let rust = &rust[..chrome_len];

            let s = floats(&chrome["stride"]);
            assert_eq!(
                s.len(),
                chrome_len.div_ceil(stride),
                "{name} ch{c}: stride count"
            );
            let err_s = max_err(rust.iter().step_by(stride).copied(), &s);

            let w = floats(&chrome["window"]);
            let err_w = max_err(rust[start..].iter().copied(), &w);

            let peak = rust.iter().fold(0.0f64, |m, &x| m.max(f64::from(x).abs()));
            let rms = (rust.iter().map(|&x| f64::from(x).powi(2)).sum::<f64>() / chrome_len as f64)
                .sqrt();
            let chrome_peak = chrome["peak"].as_f64().unwrap();
            let chrome_rms = chrome["rms"].as_f64().unwrap();

            assert!(
                err_s <= TOL && err_w <= TOL,
                "{name} ch{c}: max |err| stride {err_s:.3e}, window {err_w:.3e}"
            );
            assert!(
                (peak - chrome_peak).abs() <= TOL,
                "{name} ch{c}: peak {peak} vs Chromium {chrome_peak} (clamped?)"
            );
            assert!(
                (rms - chrome_rms).abs() <= TOL,
                "{name} ch{c}: rms {rms} vs Chromium {chrome_rms}"
            );
            if chrome_peak > 1.0 {
                overs += 1;
            }
        }
    }
    // Make sure the reference keeps exercising the "no ±1 clamp" behaviour.
    assert!(overs > 0, "no Vorbis channel with overs in the reference");
}
