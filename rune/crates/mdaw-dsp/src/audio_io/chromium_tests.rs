//! Rust decoding vs Chromium `decodeAudioData` (`rune/parity/fixtures/decode_reference.json`,
//! captured by `rune/parity/chromium/capture-decode.mjs`), EVERY channel of every file.
//!
//! Per channel the fixture holds a full-length FNV-1a digest, per-1024-frame-block RMS and four
//! random ±1 projections (see `parity.rs`), and the first 4096 frames (app decode at the
//! context rate) or 512 frames around the onset (native decode). Lossless formats must be
//! bit-exact over the full length (digest); lossy ones within an error-to-signal bound over the
//! full length and in every block with signal, estimated from the projections.
//!
//! Print the table: `cargo test -p mdaw-dsp chromium -- --nocapture`.

use super::parity::{ChannelDiff, compare_channel};
use super::*;
use crate::buffer::AudioBuffer;
use serde_json::Value;
use std::path::PathBuf;

fn testdata(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("testdata")
        .join(name)
}

fn reference() -> Value {
    let p = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../parity/fixtures/decode_reference.json"
    );
    serde_json::from_str(&std::fs::read_to_string(p).expect("decode_reference.json")).unwrap()
}

fn lossy(name: &str) -> bool {
    name.contains("mp3") || name.contains("ogg") || name.contains("m4a_aac")
}

/// Short Vorbis streams whose audio fits in one Ogg page: Chromium (FFmpeg's Ogg demuxer) ends
/// them before the final granule (13120 / 12288 frames instead of 13230 / 13248). Rust follows
/// the spec (end at the granule), which is also what Chromium does for multi-page streams.
const CHROMIUM_SHORT_OGG: [&str; 2] = [
    "ogg_ffvorbis_44k_stereo.ogg",
    "ogg_libvorbis_44k_stereo.ogg",
];

/// Formats Chromium refuses but Rust decodes (documented deviation).
fn rust_only(name: &str) -> bool {
    name.starts_with("aif") || name.contains("alac") || name.contains("f64")
}

/// Full-length and per-block error-to-signal bounds (dB) for lossy codecs (AAC differs by
/// ~4e-7 from FFmpeg's float decoder, Vorbis by the f32 IMDCT rounding).
const LOSSY_FULL_DB: f64 = -90.0;
const LOSSY_BLOCK_DB: f64 = -80.0;

struct Checker {
    failures: Vec<String>,
}

impl Checker {
    fn check(&mut self, ok: bool, msg: impl FnOnce() -> String) {
        if !ok {
            self.failures.push(msg());
        }
    }

    /// Every channel of `ours` vs the decode summary `r`; returns the per-channel diffs.
    fn channels(
        &mut self,
        label: &str,
        ours: &AudioBuffer,
        r: &Value,
        native: bool,
    ) -> Vec<ChannelDiff> {
        let per = r["perChannel"].as_array().unwrap();
        self.check(per.len() == ours.channel_count(), || {
            format!(
                "{label}: {} channels vs Chromium {}",
                ours.channel_count(),
                per.len()
            )
        });
        let start = if native {
            r["windowStart"].as_u64().unwrap() as usize
        } else {
            0
        };
        let lossy = lossy(label);
        let mut diffs = Vec::new();
        for (c, rc) in per.iter().enumerate().take(ours.channel_count()) {
            let d = compare_channel(&ours.channels[c], rc, start);
            self.check(d.non_finite == rc["nonFinite"].as_u64().unwrap(), || {
                format!(
                    "{label} ch{c}: {} non-finite samples vs Chromium {}",
                    d.non_finite, rc["nonFinite"]
                )
            });
            if lossy {
                self.check(d.err_db <= LOSSY_FULL_DB, || {
                    format!("{label} ch{c}: error/signal {:.1} dB", d.err_db)
                });
                self.check(d.worst_block_db <= LOSSY_BLOCK_DB, || {
                    format!("{label} ch{c}: worst block {:.1} dB", d.worst_block_db)
                });
                self.check(d.block_rms_db <= 0.01, || {
                    format!("{label} ch{c}: block rms {:.4} dB", d.block_rms_db)
                });
                self.check(d.wave_err <= 2e-6, || {
                    format!("{label} ch{c}: waveform max |err| {:e}", d.wave_err)
                });
            } else {
                self.check(d.bit_exact && d.wave_err == 0.0, || {
                    format!(
                        "{label} ch{c}: not bit-exact (error/signal {:.1} dB, max |err| {:e})",
                        d.err_db, d.wave_err
                    )
                });
            }
            diffs.push(d);
        }
        diffs
    }
}

fn row(label: &str, kind: &str, len_diff: i64, diffs: &[ChannelDiff]) {
    let per: Vec<String> = diffs
        .iter()
        .map(|d| {
            if d.bit_exact {
                "exact".to_owned()
            } else {
                format!(
                    "{:.0}/{:.0} dB rms±{:.3} |e|{:.0e}",
                    d.err_db, d.worst_block_db, d.block_rms_db, d.wave_err
                )
            }
        })
        .collect();
    println!("{label:40} {kind:6} len {len_diff:+5}  {}", per.join(" | "));
}

#[test]
fn chromium_decode_parity_every_channel() {
    let reference = reference();
    let ctx_rate = reference["contextRate"].as_u64().unwrap() as u32;
    let mut ck = Checker {
        failures: Vec::new(),
    };
    let mut compared = 0;
    println!(
        "file / decode / length Δ / per channel: full error/signal / worst block / block RMS Δ / waveform max |err|"
    );
    for (name, entry) in reference["files"].as_object().unwrap() {
        let ours = decode_file(testdata(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        let (app, native) = (&entry["app"], &entry["native"]);
        if app.get("error").is_some() {
            assert!(native.get("error").is_some(), "{name}");
            assert!(
                rust_only(name),
                "{name}: Chromium fails, not a known deviation"
            );
            println!(
                "{name:40} Chromium: {} | Rust: {} Hz, {} frames",
                app["error"],
                ours.sample_rate,
                ours.frames()
            );
            continue;
        }
        compared += 1;
        let short = CHROMIUM_SHORT_OGG.contains(&name.as_str());
        // At the context rate (decode + SincResampler), every channel.
        let at_ctx = resample(&ours, ctx_rate);
        assert_eq!(app["sampleRate"].as_u64().unwrap() as u32, ctx_rate);
        let len_diff = at_ctx.frames() as i64 - app["length"].as_i64().unwrap();
        ck.check(if short { len_diff > 0 } else { len_diff == 0 }, || {
            format!("{name}: length {len_diff:+}")
        });
        let n = app["length"].as_u64().unwrap() as usize;
        let common = AudioBuffer {
            sample_rate: ctx_rate,
            channels: at_ctx
                .channels
                .iter()
                .map(|c| c[..n.min(c.len())].to_vec())
                .collect(),
        };
        let d = ck.channels(name, if short { &common } else { &at_ctx }, app, false);
        row(name, "app", len_diff, &d);
        // Native rate (decoder only).
        let nat_diff = ours.frames() as i64 - native["length"].as_i64().unwrap();
        ck.check(short || nat_diff == 0, || {
            format!("{name}: native length {nat_diff:+}")
        });
        let n = native["length"].as_u64().unwrap() as usize;
        let common = AudioBuffer {
            sample_rate: ours.sample_rate,
            channels: ours
                .channels
                .iter()
                .map(|c| c[..n.min(c.len())].to_vec())
                .collect(),
        };
        let d = ck.channels(name, &common, native, true);
        row(name, "native", nat_diff, &d);
    }
    assert!(compared >= 15, "only {compared} files compared");
    assert!(ck.failures.is_empty(), "{:#?}", ck.failures);
}

/// Damaged and unusual files (`testdata/edge`, made by `gen-edge.mjs`): Rust must fail exactly
/// where Chromium fails, and otherwise produce Chromium's length and samples.
#[test]
fn chromium_edge_cases() {
    let reference = reference();
    let mut ck = Checker {
        failures: Vec::new(),
    };
    for (name, entry) in reference["edge"].as_object().unwrap() {
        let ours = decode_file(testdata(&format!("edge/{name}")));
        let native = &entry["native"];
        match (&ours, native.get("error")) {
            (Err(e), Some(_)) => println!("{name:40} both fail (Rust: {e})"),
            (Err(e), None) => ck
                .failures
                .push(format!("{name}: Rust fails ({e}), Chromium decodes")),
            (Ok(b), Some(_)) => ck.failures.push(format!(
                "{name}: Rust decodes {} frames, Chromium fails",
                b.frames()
            )),
            (Ok(b), None) => {
                let len_diff = b.frames() as i64 - native["length"].as_i64().unwrap();
                // Corrupt MP3: FFmpeg conceals the damaged frames (Chromium's output departs
                // from the clean decode at frame 5830 and stops at 9774); symphonia rejects the
                // first damaged frame and decoding stops there (5807 frames). Rust's output must
                // be a bit-exact prefix of Chromium's.
                let prefix = name.starts_with("corrupt_mp3");
                if prefix {
                    ck.check(len_diff < 0, || {
                        format!("{name}: expected a shorter prefix")
                    });
                } else {
                    ck.check(len_diff == 0, || {
                        format!("{name}: native length {len_diff:+}")
                    });
                }
                let n = if prefix {
                    b.frames() / 1024 * 1024
                } else {
                    native["length"].as_u64().unwrap() as usize
                };
                let common = AudioBuffer {
                    sample_rate: b.sample_rate,
                    channels: b
                        .channels
                        .iter()
                        .map(|c| c[..n.min(c.len())].to_vec())
                        .collect(),
                };
                let d = ck.channels(name, &common, native, true);
                row(name, "native", len_diff, &d);
            }
        }
    }
    assert!(ck.failures.is_empty(), "{:#?}", ck.failures);
}
