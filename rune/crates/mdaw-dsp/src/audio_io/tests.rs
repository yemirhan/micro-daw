use super::*;
use crate::buffer::AudioBuffer;
use serde_json::Value;
use std::path::PathBuf;

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn testdata(name: &str) -> PathBuf {
    manifest().join("testdata").join(name)
}

fn fixture(name: &str) -> Value {
    let p = manifest()
        .join("../../parity/fixtures")
        .join(format!("{name}.json"));
    serde_json::from_str(
        &std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display())),
    )
    .unwrap()
}

fn b64(s: &str) -> Vec<u8> {
    let val = |c: u8| -> u32 {
        match c {
            b'A'..=b'Z' => u32::from(c - b'A'),
            b'a'..=b'z' => u32::from(c - b'a') + 26,
            b'0'..=b'9' => u32::from(c - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            _ => panic!("bad base64 {c}"),
        }
    };
    let bytes: Vec<u8> = s.bytes().filter(|&c| c != b'=').collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut acc = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            acc |= val(c) << (18 - 6 * i);
        }
        let n = chunk.len() - 1;
        for i in 0..n {
            out.push((acc >> (16 - 8 * i)) as u8);
        }
    }
    out
}

fn f32s(bytes: &[u8]) -> Vec<f32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect()
}

fn rms(d: &[f32]) -> f64 {
    (d.iter().map(|&x| f64::from(x) * f64::from(x)).sum::<f64>() / d.len().max(1) as f64).sqrt()
}

fn peak(d: &[f32]) -> f64 {
    d.iter().fold(0.0f64, |m, &x| m.max(f64::from(x).abs()))
}

fn onset(b: &AudioBuffer, threshold: f64) -> i64 {
    (0..b.frames())
        .find(|&i| b.channels.iter().any(|c| f64::from(c[i]).abs() > threshold))
        .map_or(-1, |i| i as i64)
}

/// Pearson-free normalized cross-correlation of `a` against `b` shifted by `lag` (b[i + lag]).
fn corr_at(a: &[f32], b: &[f32], lag: i64) -> f64 {
    let (mut ab, mut aa, mut bb) = (0.0, 0.0, 0.0);
    for (i, &x) in a.iter().enumerate() {
        let j = i as i64 + lag;
        let y = if j >= 0 && (j as usize) < b.len() {
            f64::from(b[j as usize])
        } else {
            0.0
        };
        ab += f64::from(x) * y;
        aa += f64::from(x) * f64::from(x);
        bb += y * y;
    }
    if aa == 0.0 || bb == 0.0 {
        0.0
    } else {
        ab / (aa * bb).sqrt()
    }
}

fn best_lag(a: &[f32], b: &[f32], max_lag: i64) -> (i64, f64) {
    (-max_lag..=max_lag)
        .map(|l| (l, corr_at(a, b, l)))
        .fold((0, f64::MIN), |m, x| if x.1 > m.1 { x } else { m })
}

fn db(x: f64) -> f64 {
    20.0 * x.log10()
}

// ---------------------------------------------------------------------------------------------
// WAV encoding
// ---------------------------------------------------------------------------------------------

#[test]
fn wav_golden_byte_identical() {
    let cases = fixture("wav");
    let cases = cases.as_array().unwrap();
    assert!(cases.len() >= 8);
    for c in cases {
        let channels: Vec<Vec<f32>> = c["channels"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| f32s(&b64(s.as_str().unwrap())))
            .collect();
        let buffer = AudioBuffer {
            sample_rate: c["sampleRate"].as_u64().unwrap() as u32,
            channels,
        };
        let expected = b64(c["wav"].as_str().unwrap());
        let got = encode_wav(&buffer);
        assert_eq!(got.len(), expected.len(), "{}", c["name"]);
        assert!(
            got == expected,
            "{}: first diff at byte {:?}",
            c["name"],
            got.iter().zip(&expected).position(|(a, b)| a != b)
        );
    }
}

#[test]
fn wav_header_layout() {
    let b = AudioBuffer {
        sample_rate: 48000,
        channels: vec![vec![0.5, -0.5, 1.0], vec![-1.0, 2.0, f32::NAN]],
    };
    let w = encode_wav(&b);
    assert_eq!(w.len(), WAV_HEADER_SIZE + 12);
    assert_eq!(&w[0..4], b"RIFF");
    assert_eq!(u32::from_le_bytes(w[4..8].try_into().unwrap()), 36 + 12);
    assert_eq!(u32::from_le_bytes(w[28..32].try_into().unwrap()), 48000 * 4);
    let s: Vec<i16> = w[44..]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| i16::from_le_bytes(*c))
        .collect();
    assert_eq!(s, vec![16383, -32768, -16384, 32767, 32767, 0]);
}

#[test]
fn wav_roundtrip_through_decoder() {
    let data: Vec<f32> = (0..1000).map(|i| ((i as f32) * 0.05).sin() * 0.8).collect();
    let b = AudioBuffer {
        sample_rate: 44100,
        channels: vec![data.clone(), data.iter().map(|x| -x).collect()],
    };
    let d = decode_bytes(&encode_wav(&b), "wav").unwrap();
    assert_eq!(
        (d.sample_rate, d.channel_count(), d.frames()),
        (44100, 2, 1000)
    );
    for (c, ch) in d.channels.iter().enumerate() {
        for (i, &x) in ch.iter().enumerate() {
            assert!(
                (x - b.channels[c][i]).abs() <= 1.0 / 32767.0,
                "ch{c}[{i}] {x} vs {}",
                b.channels[c][i]
            );
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Decoding
// ---------------------------------------------------------------------------------------------

/// (file, rate, channels, frames) — see testdata/gen.sh.
const FILES: [(&str, u32, usize, usize); 20] = [
    ("aifc_sowt_44k_stereo.aif", 44100, 2, 13230),
    ("aiff_s16_44k_stereo.aif", 44100, 2, 13230),
    ("aiff_s24_48k_mono.aiff", 48000, 1, 14400),
    ("flac_s16_44k_stereo.flac", 44100, 2, 13230),
    ("flac_s24_22k_mono.flac", 22050, 1, 6615),
    ("m4a_aac_44k_stereo.m4a", 44100, 2, 13230),
    ("m4a_aacitunes_44k_stereo.m4a", 44100, 2, 13230),
    ("m4a_alac_48k_stereo.m4a", 48000, 2, 14400),
    ("mp3_44k_stereo.mp3", 44100, 2, 13230),
    ("mp3_48k_mono.mp3", 48000, 1, 14400),
    // ffmpeg's experimental Vorbis encoder writes a final granule of 13248 (not 13230).
    ("ogg_ffvorbis_44k_stereo.ogg", 44100, 2, 13248),
    ("ogg_libvorbis_2s_44k_stereo.ogg", 44100, 2, 88200),
    ("ogg_libvorbis_44k_stereo.ogg", 44100, 2, 13230),
    ("wav_f32_48k_mono.wav", 48000, 1, 14400),
    ("wav_f64_44k_mono.wav", 44100, 1, 8820),
    ("wav_s16_44k_stereo.wav", 44100, 2, 13230),
    ("wav_s24ext_48k_stereo.wav", 48000, 2, 14400),
    ("wav_s24ext_96k_stereo.wav", 96000, 2, 19200),
    ("wav_s32_44k_mono.wav", 44100, 1, 13230),
    ("wav_u8_22k_mono.wav", 22050, 1, 6615),
];

fn is_lossy(name: &str) -> bool {
    name.starts_with("mp3") || name.starts_with("ogg") || name.starts_with("m4a_aac")
}

/// Short Vorbis streams whose audio fits in one Ogg page: Chromium (FFmpeg's Ogg demuxer) ends
/// them before the final granule (13120 / 12288 frames instead of 13230 / 13248). Rust follows the
/// spec (end at the granule), which is also what Chromium does for multi-page streams.
const CHROMIUM_SHORT_OGG: [&str; 2] = [
    "ogg_ffvorbis_44k_stereo.ogg",
    "ogg_libvorbis_44k_stereo.ogg",
];

#[test]
fn decodes_every_accepted_format() {
    for (name, rate, channels, frames) in FILES {
        let b = decode_file(testdata(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(b.sample_rate, rate, "{name}");
        assert_eq!(b.channel_count(), channels, "{name}");
        assert_eq!(b.frames(), frames, "{name}");
        // Vorbis pre-echo crosses the 0.01 threshold early (~110 frames; ~570 for ffmpeg's encoder).
        let o = onset(&b, 0.01);
        let expected = (0.05 * f64::from(rate)) as i64;
        let tol = if name.contains("ffvorbis") {
            600
        } else if is_lossy(name) {
            128
        } else {
            2
        };
        assert!(
            (o - expected).abs() <= tol,
            "{name}: onset {o} vs {expected}"
        );
        // Decoding via a path and via bytes without a hint (content sniffing) agree.
        let bytes = std::fs::read(testdata(name)).unwrap();
        assert_eq!(
            decode_bytes(&bytes, "").unwrap(),
            b,
            "{name}: content sniffing without hint"
        );
    }
    let exts: std::collections::BTreeSet<&str> = FILES
        .iter()
        .map(|f| f.0.rsplit('.').next().unwrap())
        .collect();
    assert_eq!(exts.len(), SUPPORTED_EXTENSIONS.len());
}

/// Formats Chromium refuses (AIFF, ALAC, 64-bit float WAV) decode to the same samples as their
/// lossless siblings.
#[test]
fn aiff_alac_f64_match_lossless_siblings() {
    let same = |a: &str, b: &str, frames: usize, tol: f32| {
        let x = decode_file(testdata(a)).unwrap();
        let y = decode_file(testdata(b)).unwrap();
        for c in 0..x.channel_count() {
            for i in 0..frames {
                let d = (x.channels[c][i] - y.channels[c][i]).abs();
                assert!(d <= tol, "{a} vs {b} ch{c}[{i}]: {d}");
            }
        }
    };
    same(
        "aiff_s16_44k_stereo.aif",
        "wav_s16_44k_stereo.wav",
        13230,
        0.0,
    );
    same(
        "aifc_sowt_44k_stereo.aif",
        "wav_s16_44k_stereo.wav",
        13230,
        0.0,
    );
    same(
        "aiff_s24_48k_mono.aiff",
        "wav_s24ext_48k_stereo.wav",
        14400,
        0.0,
    );
    same(
        "m4a_alac_48k_stereo.m4a",
        "wav_s24ext_48k_stereo.wav",
        14400,
        1.0 / 32768.0,
    );
    same("wav_f64_44k_mono.wav", "wav_s32_44k_mono.wav", 8820, 1e-6);
    same(
        "flac_s16_44k_stereo.flac",
        "wav_s16_44k_stereo.wav",
        13230,
        0.0,
    );
}

#[test]
fn decode_errors() {
    assert!(matches!(
        decode_file(testdata("does-not-exist.wav")),
        Err(DecodeError::Io(_))
    ));
    assert!(decode_bytes(b"definitely not audio at all, just text", "wav").is_err());
    assert!(decode_bytes(&[], "mp3").is_err());
    // Header-only WAV (zero frames).
    let empty = encode_wav(&AudioBuffer {
        sample_rate: 44100,
        channels: vec![vec![]],
    });
    assert!(decode_bytes(&empty, "wav").is_err());
}

#[test]
fn container_trims_and_rate_conversion() {
    let bytes = std::fs::read(testdata("m4a_aac_44k_stereo.m4a")).unwrap();
    let t = mp4_trim(&bytes, 44100).unwrap();
    assert_eq!(
        t,
        Mp4Trim {
            skip: 1024,
            frames: Some(13230)
        }
    );
    let alac = std::fs::read(testdata("m4a_alac_48k_stereo.m4a")).unwrap();
    assert_eq!(mp4_trim(&alac, 48000).unwrap().skip, 0);
    // Apple AAC: iTunSMPB (2112 priming), no edit list.
    let itunes = std::fs::read(testdata("m4a_aacitunes_44k_stereo.m4a")).unwrap();
    assert_eq!(
        mp4_trim(&itunes, 44100).unwrap(),
        Mp4Trim {
            skip: 2112,
            frames: Some(13230)
        }
    );
    // Ogg: final granule position.
    let ogg = std::fs::read(testdata("ogg_libvorbis_2s_44k_stereo.ogg")).unwrap();
    assert_eq!(ogg_final_granule(&ogg), Some(88200));
    // decode_file_at_rate = decode + Chromium resample.
    let b = decode_file_at_rate(testdata("wav_f32_48k_mono.wav"), 44100).unwrap();
    assert_eq!((b.sample_rate, b.frames()), (44100, 13229));
}

// ---------------------------------------------------------------------------------------------
// Chromium comparison (rune/parity/fixtures/decode_reference.json)
// ---------------------------------------------------------------------------------------------

struct Row {
    name: String,
    detail: String,
}

/// Compares every file Chromium decoded: Rust decode + [`resample`] to the context rate vs the
/// app's `decodeAudioData` result, and Rust decode vs Chromium's native-rate decode. Prints the
/// comparison table (`cargo test -p mdaw-dsp chromium -- --nocapture`).
#[test]
fn chromium_decode_parity() {
    let reference = fixture("decode_reference");
    let ctx_rate = reference["contextRate"].as_u64().unwrap() as u32;
    let threshold = reference["onsetThreshold"].as_f64().unwrap();
    let mut rows = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    macro_rules! check {
        ($cond:expr, $($fmt:tt)+) => {
            let ok: bool = $cond;
            if !ok {
                failures.push(format!($($fmt)+));
            }
        };
    }
    let mut compared = 0;
    for (name, entry) in reference["files"].as_object().unwrap() {
        let ours = decode_file(testdata(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        let app = &entry["app"];
        if app.get("error").is_some() {
            assert!(entry["native"].get("error").is_some(), "{name}");
            rows.push(Row {
                name: name.clone(),
                detail: format!(
                    "Chromium: {} | Rust: {} Hz, {} frames",
                    app["error"],
                    ours.sample_rate,
                    ours.frames()
                ),
            });
            continue;
        }
        compared += 1;
        let lossy = is_lossy(name);
        let short_ogg = CHROMIUM_SHORT_OGG.contains(&name.as_str());
        let at_ctx = resample(&ours, ctx_rate);
        assert_eq!(app["sampleRate"].as_u64().unwrap() as u32, ctx_rate);
        assert_eq!(
            app["channels"].as_u64().unwrap() as usize,
            ours.channel_count(),
            "{name}"
        );
        let chrome_len = app["length"].as_u64().unwrap() as usize;
        let len_diff = at_ctx.frames() as i64 - chrome_len as i64;
        if short_ogg {
            check!(
                len_diff > 0,
                "{name}: expected Chromium to end early, len diff {len_diff}"
            );
        } else {
            check!(len_diff == 0, "{name}: length {len_diff}");
        }
        // Over the common length (= the whole buffer except for the short-Ogg cases).
        let n = chrome_len.min(at_ctx.frames());
        // RMS per channel within 0.1 dB.
        let mut worst_db: f64 = 0.0;
        for (c, r) in app["rms"].as_array().unwrap().iter().enumerate() {
            let chrome_rms = r.as_f64().unwrap();
            let d = db(rms(&at_ctx.channels[c][..n]) / chrome_rms).abs();
            worst_db = worst_db.max(d);
        }
        check!(worst_db <= 0.1, "{name}: rms differs by {worst_db} dB");
        let peak_db = db(peak(&at_ctx.channels[0][..n]) / app["peak"][0].as_f64().unwrap());
        let onset_diff = onset(&at_ctx, threshold) - app["onset"].as_i64().unwrap();
        // Channel 0 waveform (the fixture holds the first 16384 frames).
        let chrome_ch0 = f32s(&b64(app["ch0"].as_str().unwrap()));
        let ours0 = &at_ctx.channels[0][..n.min(chrome_ch0.len())];
        let c0 = corr_at(ours0, &chrome_ch0, 0);
        let (lag, _) = best_lag(ours0, &chrome_ch0, 64);
        let max_err = ours0
            .iter()
            .zip(&chrome_ch0)
            .map(|(a, b)| (a - b).abs())
            .fold(0f32, f32::max);
        check!(lag == 0, "{name}: shifted by {lag} frames");
        // Lossy: a sample sitting within ~1e-7 of the 0.01 threshold may flip the onset by a few frames.
        check!(
            onset_diff.abs() <= if lossy { 8 } else { 0 },
            "{name}: onset {onset_diff}"
        );
        check!(c0 >= 0.999, "{name}: correlation {c0}");
        check!(max_err <= 1e-6, "{name}: max |err| {max_err}");
        // Native-rate decode (decoder only, no resampler).
        let native = &entry["native"];
        let nat_len_diff = ours.frames() as i64 - native["length"].as_i64().unwrap();
        let start = native["windowStart"].as_u64().unwrap() as usize;
        let win: Vec<f32> = native["window"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap() as f32)
            .collect();
        let nat_err = win
            .iter()
            .enumerate()
            .map(|(i, &x)| (x - ours.channels[0][start + i]).abs())
            .fold(0f32, f32::max);
        if !short_ogg {
            check!(nat_len_diff == 0, "{name}: native length {nat_len_diff}");
        }
        check!(
            nat_err <= if lossy { 1e-6 } else { 0.0 },
            "{name}: native decode differs by {nat_err}"
        );
        rows.push(Row {
            name: name.clone(),
            detail: format!(
                "native: len {nat_len_diff:+} max|err| {nat_err:.1e} | @{ctx_rate}: len {len_diff:+} rms {worst_db:.4} dB peak {peak_db:+.4} dB onset {onset_diff:+} lag {lag:+} corr {c0:.7} max|err| {max_err:.1e}"
            ),
        });
    }
    for r in rows {
        println!("{:32} {}", r.name, r.detail);
    }
    assert!(compared >= 14, "only {compared} files compared");
    assert!(failures.is_empty(), "{failures:#?}");
}

// ---------------------------------------------------------------------------------------------
// Resampling
// ---------------------------------------------------------------------------------------------

#[test]
fn resample_lengths_match_blink() {
    assert_eq!(resampled_len(14400, 48000, 44100), 13229);
    assert_eq!(resampled_len(6615, 22050, 44100), 13230);
    assert_eq!(resampled_len(19200, 96000, 44100), 8820);
    let b = AudioBuffer::new(48000, 2, 14400);
    let r = resample(&b, 44100);
    assert_eq!(
        (r.sample_rate, r.frames(), r.channel_count()),
        (44100, 13229, 2)
    );
    let same = resample(&b, 48000);
    assert_eq!(same, b);
    assert_eq!(resample(&AudioBuffer::new(44100, 1, 0), 48000).frames(), 0);
}

fn sine(rate: u32, freq: f64, frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|i| {
            (2.0 * std::f64::consts::PI * freq * i as f64 / f64::from(rate)).sin() as f32 * 0.5
        })
        .collect()
}

/// SNR of a resampled sine against the ideal sine at the target rate (edges excluded).
fn sine_snr(from: u32, to: u32, freq: f64, q: ResampleQuality) -> f64 {
    let src = AudioBuffer::mono(from, sine(from, freq, from as usize));
    let out = resample_with(&src, to, q);
    let ideal = sine(to, freq, out.frames());
    let (mut s, mut n) = (0.0, 0.0);
    let m = out.frames() - 200;
    for (&x, &y) in out.channels[0][200..m].iter().zip(&ideal[200..m]) {
        s += f64::from(y).powi(2);
        n += f64::from(x - y).powi(2);
    }
    10.0 * (s / n).log10()
}

#[test]
fn resample_quality() {
    for (from, to) in [
        (48000, 44100),
        (44100, 48000),
        (22050, 44100),
        (96000, 44100),
    ] {
        let c = sine_snr(from, to, 1000.0, ResampleQuality::Chromium);
        let h = sine_snr(from, to, 1000.0, ResampleQuality::High);
        println!("{from} -> {to}: 1 kHz sine SNR chromium {c:.1} dB, high {h:.1} dB");
        assert!(c > 60.0, "{from}->{to} chromium {c}");
        assert!(h > c + 10.0, "{from}->{to} high {h} vs chromium {c}");
    }
}

#[test]
fn resample_zero_latency_alignment() {
    // An impulse at source frame 4800 (0.1 s @ 48k) peaks at 0.1 s @ 44.1k = 4410.
    let mut d = vec![0f32; 9600];
    d[4800] = 1.0;
    let out = resample(&AudioBuffer::mono(48000, d), 44100);
    let (imax, _) = out.channels[0]
        .iter()
        .enumerate()
        .fold((0, 0f32), |m, (i, &x)| if x > m.1 { (i, x) } else { m });
    assert_eq!(imax, 4410);
}

// ---------------------------------------------------------------------------------------------
// Peaks
// ---------------------------------------------------------------------------------------------

#[test]
fn peaks_from_audio_buffer() {
    let b = AudioBuffer {
        sample_rate: 8,
        channels: vec![vec![0.1, -0.5, 0.2, 0.0], vec![0.0, 0.1, -0.9, 0.3]],
    };
    assert_eq!(abs_peaks(&b, 2), vec![0.5, 0.9]);
    let p = min_max_peaks(&b, 2.0);
    assert_eq!(p.min.len(), 2);
    assert_eq!((p.min[1], p.max[1]), (-0.9, 0.3));
}
