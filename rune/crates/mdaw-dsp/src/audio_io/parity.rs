//! Test-side mirror of `rune/parity/chromium/decode-stats.mjs` (keep in sync) and the per-channel
//! comparison against a Chromium decode summary from `decode_reference.json`.

use serde_json::Value;

pub const BLOCK: usize = 1024;
pub const PROJECTIONS: usize = 4;
pub const SEEDS: [u32; PROJECTIONS] = [0x9e37_79b9, 0x7f4a_7c15, 0x85eb_ca6b, 0xc2b2_ae35];

pub fn fnv1a64(data: &[f32]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for x in data {
        for b in x.to_le_bytes() {
            h = (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    format!("{h:016x}")
}

/// Per-block RMS and projections (non-finite samples count as 0).
pub fn blocks(d: &[f32]) -> (Vec<f64>, Vec<[f64; PROJECTIONS]>) {
    let mut state = SEEDS;
    let mut rms = Vec::new();
    let mut proj = Vec::new();
    for chunk in d.chunks(BLOCK) {
        let mut sq = 0f64;
        let mut p = [0f64; PROJECTIONS];
        for &x in chunk {
            let x = if x.is_finite() { f64::from(x) } else { 0.0 };
            sq += x * x;
            for (s, acc) in state.iter_mut().zip(p.iter_mut()) {
                let mut v = *s;
                v ^= v << 13;
                v ^= v >> 17;
                v ^= v << 5;
                *s = v;
                *acc += if v & 0x8000_0000 != 0 { -x } else { x };
            }
        }
        rms.push((sq / chunk.len() as f64).sqrt());
        proj.push(p);
    }
    (rms, proj)
}

/// Comparison of one decoded channel against Chromium's summary of the same channel.
#[derive(Debug, Clone, Copy)]
pub struct ChannelDiff {
    pub bit_exact: bool,
    /// Full-length error-to-signal (dB) estimated from the block projections (−∞ when equal).
    pub err_db: f64,
    /// Worst block error-to-signal (dB) among blocks with signal (RMS > −80 dBFS).
    pub worst_block_db: f64,
    /// Worst per-block RMS difference (dB) among blocks with signal.
    pub block_rms_db: f64,
    /// Max |Δ| over the stored waveform (head or window).
    pub wave_err: f32,
    pub non_finite: u64,
}

pub fn b64(s: &str) -> Vec<u8> {
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
        for i in 0..chunk.len() - 1 {
            out.push((acc >> (16 - 8 * i)) as u8);
        }
    }
    out
}

pub fn f32s_b64(s: &str) -> Vec<f32> {
    b64(s)
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect()
}

fn floats(v: &Value) -> Vec<f64> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_f64().unwrap())
        .collect()
}

/// `ours` vs the Chromium summary `reference` (`perChannel[c]`); `wave` is the stored waveform
/// (`head` from 0 or `window` from `start`). Only the common length is compared block-wise.
pub fn compare_channel(ours: &[f32], reference: &Value, start: usize) -> ChannelDiff {
    let (rms, proj) = blocks(ours);
    let ref_rms = floats(&reference["blockRms"]);
    let ref_proj: Vec<Vec<f64>> = reference["blockProj"]
        .as_array()
        .unwrap()
        .iter()
        .map(floats)
        .collect();
    let (mut e_total, mut s_total) = (0f64, 0f64);
    let (mut worst_block_db, mut block_rms_db) = (f64::NEG_INFINITY, 0f64);
    for b in 0..rms.len().min(ref_rms.len()) {
        let n = BLOCK.min(ours.len() - b * BLOCK) as f64;
        if b + 1 == rms.len() && rms.len() < ref_rms.len() && !ours.len().is_multiple_of(BLOCK) {
            break; // a partial block of a shorter signal
        }
        let e2 = (0..PROJECTIONS)
            .map(|k| (proj[b][k] - ref_proj[b][k]).powi(2))
            .sum::<f64>()
            / PROJECTIONS as f64;
        let s2 = ref_rms[b].powi(2) * n;
        e_total += e2;
        s_total += s2;
        if ref_rms[b] > 1e-4 {
            worst_block_db = worst_block_db.max(10.0 * (e2 / s2).log10());
            block_rms_db = block_rms_db.max((20.0 * (rms[b] / ref_rms[b]).log10()).abs());
        }
    }
    let wave = reference
        .get("head")
        .or_else(|| reference.get("window"))
        .and_then(Value::as_str)
        .map(f32s_b64)
        .unwrap_or_default();
    let wave_err = wave
        .iter()
        .enumerate()
        .map(|(i, &w)| match ours.get(start + i) {
            Some(&o) if o.to_bits() == w.to_bits() => 0.0,
            Some(&o) => (o - w).abs(),
            None => f32::INFINITY,
        })
        .fold(
            0f32,
            |m, x| if x.is_nan() { f32::INFINITY } else { m.max(x) },
        );
    ChannelDiff {
        bit_exact: fnv1a64(ours) == reference["digest"].as_str().unwrap(),
        err_db: if s_total > 0.0 {
            10.0 * (e_total / s_total).log10()
        } else {
            f64::NEG_INFINITY
        },
        worst_block_db,
        block_rms_db,
        wave_err,
        non_finite: ours.iter().filter(|x| !x.is_finite()).count() as u64,
    }
}
