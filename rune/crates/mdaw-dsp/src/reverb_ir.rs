//! `src/audio/RoomReverb.ts`: the synchronous stereo plate/room impulse response, bit-for-bit
//! (golden: `rune/parity/fixtures/reverb_ir.json`), plus the surrounding `RoomReverb` routing as
//! data for the realtime engine (which implements the convolver itself).
//!
//! IR per channel (seed `0x2545f491` left / `0x9e3779b9` right):
//! ```text
//! pre = floor(clamp(preDelay, 0, 0.2)·sr); len = pre + ceil(clamp(decay, 0.2, 8)·sr)
//! for i in pre..len:  t = (i − pre)/sr
//!     env = exp(−6.91·t/decay);  a = exp(−4.5·damping·t/decay)·0.9 + 0.05
//!     lp += a·(rnd() − lp);  fadeIn = min(1, (i − pre)/(0.004·sr))
//!     data[i] = f32(lp·env·fadeIn)
//! early taps (ms) L 7/13/21/34/47, R 9/17/26/39/53: data[pre + floor(tap·sr)] += (0.55 − 0.09k)·(−1)^k
//! ```
//! `rnd` is the xorshift32 (13, 17, 5) of the TS file, mapped to `(s / 4294967295)·2 − 1`.
//! Arithmetic is f64 (JS numbers) with f32 stores, exactly as with a `Float32Array`.
//!
//! Routing of `RoomReverb` (for the engine):
//! ```text
//! input ─┬──────────────────────────────▶ xfade.a ─┐
//!        └─▶ HP 200 Hz (Tone.Filter −12) ─▶ Convolver(IR, normalize) ─▶ xfade.b ─┴▶ output
//! ```
//! `Tone.CrossFade` is equal power: a constant 1 through a mono `StereoPannerNode` with
//! `pan = 2·wet − 1` gives `gain_a = cos(wet·π/2)`, `gain_b = sin(wet·π/2)` (a-rate, so `wet` is
//! automatable). The low cut is one Web Audio biquad highpass at 200 Hz with Q = 1 (dB, Tone's
//! default). The ConvolverNode normalizes the IR with [`convolver_normalization_scale`].

use crate::buffer::AudioBuffer;

/// `ReverbOptions` (`decay` s, `preDelay` s, `damping` 0..1, `wet` 0..1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReverbOptions {
    pub decay: f64,
    pub pre_delay: f64,
    pub damping: f64,
    pub wet: f64,
}

/// `DEFAULTS` of RoomReverb.ts.
pub const REVERB_DEFAULTS: ReverbOptions = ReverbOptions {
    decay: 1.8,
    pre_delay: 0.022,
    damping: 0.55,
    wet: 0.0,
};

impl Default for ReverbOptions {
    fn default() -> Self {
        REVERB_DEFAULTS
    }
}

/// Cutoff of the high-pass on the reverb send (Hz).
pub const SEND_LOW_CUT_HZ: f64 = 200.0;
/// Q of that high-pass (Web Audio lowpass/highpass Q is in dB; Tone.Filter default 1).
pub const SEND_LOW_CUT_Q_DB: f64 = 1.0;

const SEEDS: [u32; 2] = [0x2545_f491, 0x9e37_79b9];
const TAPS: [[f64; 5]; 2] = [
    [0.007, 0.013, 0.021, 0.034, 0.047],
    [0.009, 0.017, 0.026, 0.039, 0.053],
];

fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    // `Math.min(hi, Math.max(lo, v))` (NaN propagates like JS).
    hi.min(lo.max(v))
}

/// The TS `xorshift(seed)` generator.
#[derive(Debug, Clone)]
pub struct XorShift32(u32);

impl XorShift32 {
    pub fn new(seed: u32) -> Self {
        Self(if seed == 0 { 1 } else { seed })
    }

    /// Next value in [−1, 1].
    pub fn next_f64(&mut self) -> f64 {
        let mut s = self.0;
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        self.0 = s;
        (s as f64 / 4_294_967_295.0) * 2.0 - 1.0
    }
}

/// `createImpulseResponse(ctx, opts)` at `sample_rate`: a 2-channel buffer.
pub fn create_impulse_response(sample_rate: u32, opts: &ReverbOptions) -> AudioBuffer {
    let sr = sample_rate as f64;
    let decay = clamp(opts.decay, 0.2, 8.0);
    let pre = (clamp(opts.pre_delay, 0.0, 0.2) * sr).floor() as usize;
    let len = pre + (decay * sr).ceil() as usize;
    let damping = clamp(opts.damping, 0.0, 1.0);
    let mut buf = AudioBuffer::new(sample_rate, 2, len);
    for (ch, data) in buf.channels.iter_mut().enumerate() {
        let mut rnd = XorShift32::new(SEEDS[ch]);
        let mut lp = 0.0f64;
        for (i, out) in data.iter_mut().enumerate().skip(pre) {
            let t = (i - pre) as f64 / sr;
            let env = ((-6.91 * t) / decay).exp();
            let a = ((-4.5 * damping * t) / decay).exp() * 0.9 + 0.05;
            lp += a * (rnd.next_f64() - lp);
            let fade_in = 1f64.min((i - pre) as f64 / (0.004 * sr));
            *out = (lp * env * fade_in) as f32;
        }
        for (k, tap) in TAPS[ch].iter().enumerate() {
            let idx = pre + (tap * sr).floor() as usize;
            if idx < len {
                let g = (0.55 - k as f64 * 0.09) * if k % 2 == 1 { -1.0 } else { 1.0 };
                data[idx] = (data[idx] as f64 + g) as f32;
            }
        }
    }
    buf
}

/// Web Audio ConvolverNode `normalize = true` scale as Chromium computes it
/// (`Reverb.cc` `CalculateNormalizationScale`):
/// `1 / max(√(Σx² / (channels·len)), 0.000125) · powf(10, −58·0.05) · 44100 / sr` (·0.5 for 4
/// channels). Chromium's calibration is −58 dB (`0.0012589`), not the spec's `0.00125`: using the
/// spec value plays every convolution 0.06 dB quieter than Chromium (a −43 dB error).
pub fn convolver_normalization_scale(ir: &AudioBuffer) -> f64 {
    let gain_calibration = 10f32.powf(-58.0 * 0.05) as f64;
    const GAIN_CALIBRATION_SAMPLE_RATE: f64 = 44100.0;
    const MIN_POWER: f64 = 0.000125;
    let channels = ir.channel_count();
    let len = ir.frames();
    if channels == 0 || len == 0 {
        return 1.0;
    }
    let mut power = 0.0f64;
    for ch in &ir.channels {
        for &x in ch {
            power += x as f64 * x as f64;
        }
    }
    let power = (power / (channels * len) as f64).sqrt();
    let power = if !power.is_finite() || power < MIN_POWER {
        MIN_POWER
    } else {
        power
    };
    let mut scale = 1.0 / power * gain_calibration;
    scale *= GAIN_CALIBRATION_SAMPLE_RATE / ir.sample_rate as f64;
    if channels == 4 {
        scale *= 0.5;
    }
    scale
}

/// Equal-power crossfade gains `(dry a, wet b)` of `Tone.CrossFade` for `wet` ∈ [0, 1].
pub fn equal_power_gains(wet: f64) -> (f64, f64) {
    let w = clamp(wet, 0.0, 1.0) * std::f64::consts::FRAC_PI_2;
    (w.cos(), w.sin())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn fnv1a64(data: &[f32]) -> String {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for x in data {
            for b in x.to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        format!("{h:016x}")
    }

    #[test]
    fn xorshift_matches_js_semantics() {
        let mut r = XorShift32::new(1);
        // s = 1 → 1 ^ (1<<13) = 8193; ^ (8193>>17 = 0) = 8193; ^ (8193<<5) = 270369
        assert_eq!(r.next_f64(), (270_369f64 / 4_294_967_295.0) * 2.0 - 1.0);
        assert_eq!(XorShift32::new(0).0, 1);
    }

    #[test]
    fn impulse_response_matches_ts_golden() {
        let p = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../parity/fixtures/reverb_ir.json"
        );
        let cases: Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        for case in cases.as_array().unwrap() {
            let sr = case["sr"].as_u64().unwrap() as u32;
            let o = &case["opts"];
            let get = |k: &str, d: f64| o.get(k).and_then(Value::as_f64).unwrap_or(d);
            let opts = ReverbOptions {
                decay: get("decay", REVERB_DEFAULTS.decay),
                pre_delay: get("preDelay", REVERB_DEFAULTS.pre_delay),
                damping: get("damping", REVERB_DEFAULTS.damping),
                wet: 0.0,
            };
            let ir = create_impulse_response(sr, &opts);
            assert_eq!(
                ir.frames() as u64,
                case["length"].as_u64().unwrap(),
                "{opts:?}"
            );
            for (ch, gold) in case["channels"].as_array().unwrap().iter().enumerate() {
                let data = &ir.channels[ch];
                let mut max_diff = 0f64;
                for pt in gold["points"].as_array().unwrap() {
                    let i = pt[0].as_u64().unwrap() as usize;
                    let want = pt[1].as_f64().unwrap() as f32;
                    max_diff = max_diff.max((data[i] - want).abs() as f64);
                }
                let hash_ok = fnv1a64(data) == gold["hash"].as_str().unwrap();
                println!(
                    "IR sr {sr} {opts:?} ch{ch}: bit-exact hash {hash_ok}, max sample diff {max_diff:e}"
                );
                assert!(max_diff <= 2e-7, "IR samples differ by {max_diff}");
                let sum: f64 = data.iter().map(|&x| x as f64).sum();
                let want = gold["sum"].as_f64().unwrap();
                assert!((sum - want).abs() < 1e-6, "sum {sum} vs {want}");
                assert!(hash_ok, "IR not bit-exact (sr {sr} ch {ch})");
            }
        }
    }

    #[test]
    fn crossfade_and_normalization() {
        let (a, b) = equal_power_gains(0.5);
        assert!((a - b).abs() < 1e-12 && (a * a + b * b - 1.0).abs() < 1e-12);
        assert_eq!(equal_power_gains(0.0), (1.0, 0.0));
        let ir = create_impulse_response(48000, &REVERB_DEFAULTS);
        let s = convolver_normalization_scale(&ir);
        assert!(s.is_finite() && s > 0.0);
        let silent = AudioBuffer::new(44100, 2, 10);
        // 1 / 0.000125 · 10^(−58/20) (Chromium's −58 dB calibration, f32)
        assert!((convolver_normalization_scale(&silent) - 10.071_403).abs() < 1e-5);
    }
}
