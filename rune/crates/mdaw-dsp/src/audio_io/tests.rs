use super::parity::b64;
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

fn f32s(bytes: &[u8]) -> Vec<f32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect()
}

fn onset(b: &AudioBuffer, threshold: f64) -> i64 {
    (0..b.frames())
        .find(|&i| b.channels.iter().any(|c| f64::from(c[i]).abs() > threshold))
        .map_or(-1, |i| i as i64)
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
const FILES: [(&str, u32, usize, usize); 21] = [
    ("aifc_sowt_44k_stereo.aif", 44100, 2, 13230),
    ("aiff_s16_44k_stereo.aif", 44100, 2, 13230),
    ("aiff_s24_48k_mono.aiff", 48000, 1, 14400),
    ("flac_s16_44k_stereo.flac", 44100, 2, 13230),
    ("flac_s24_22k_mono.flac", 22050, 1, 6615),
    ("m4a_aac_44k_stereo.m4a", 44100, 2, 13230),
    // Fragmented (ffmpeg `frag_keyframe+empty_moov`): no edit list → priming kept, like Chromium.
    ("m4a_aacfrag_44k_stereo.m4a", 44100, 2, 14254),
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

#[test]
fn decodes_every_accepted_format() {
    for (name, rate, channels, frames) in FILES {
        let b = decode_file(testdata(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(b.sample_rate, rate, "{name}");
        assert_eq!(b.channel_count(), channels, "{name}");
        assert_eq!(b.frames(), frames, "{name}");
        // Vorbis pre-echo crosses the 0.01 threshold early (~110 frames; ~570 for ffmpeg's encoder).
        let o = onset(&b, 0.01);
        // The fragmented M4A keeps its 1024 priming frames (no edit list), like Chromium.
        let priming = if name.contains("aacfrag") { 1024 } else { 0 };
        let expected = (0.05 * f64::from(rate)) as i64 + priming;
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

#[test]
fn float_wav_round_trips_bit_exactly_through_the_decoder() {
    let b = AudioBuffer {
        sample_rate: 44_100,
        channels: vec![
            vec![0.0, 0.5, -1.0, 1.25, 1e-7, f32::NAN],
            vec![-0.25, 0.125, 0.999_999, -1.5, 0.0, 0.0],
        ],
    };
    let bytes = encode_wav_f32(&b);
    assert_eq!(bytes.len(), 58 + 6 * 2 * 4);
    let back = decode_bytes(&bytes, "wav").expect("decodes");
    assert_eq!(back.sample_rate, 44_100);
    assert_eq!(back.channels.len(), 2);
    assert_eq!(back.channels[0][..5], b.channels[0][..5]);
    assert_eq!(back.channels[0][5], 0.0, "NaN is written as silence");
    assert_eq!(back.channels[1], b.channels[1]);
}
