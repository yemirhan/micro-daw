//! Chromium `DynamicsCompressorNode`, ported from blink
//! `platform/audio/dynamics_compressor.cc` (`DynamicsCompressor` + the WebKit-derived kernel that
//! used to live in `dynamics_compressor_kernel.cc`) and `modules/webaudio/dynamics_compressor_node.cc`
//! (`DynamicsCompressorHandler::Process`: the k-rate `FinalValue()` of threshold / knee / ratio /
//! attack / release taken once per render quantum, clamped to the params' nominal ranges, and the
//! `reduction` attribute = the kernel's metering gain after the quantum).
//!
//! Followed exactly (same f32 / f64 choices as the C++, same libm float functions, same operation
//! order, `std::min` / `std::max` operand order for NaN / inf cases):
//! - 6 ms look-ahead (`kPreDelay`), 1024-frame power-of-two pre-delay rings per channel; the
//!   pre-delay is re-applied (rings zeroed, read 0, write = frames) when it differs from the last
//!   one (initially `kDefaultPreDelayFrames` = 256), i.e. on the first quantum;
//! - static curve: `KneeCurve`, `Saturate`, `SlopeAt` (`x * 1.001` in double), the 15-step
//!   geometric binary search of `KAtSlope`, recomputed only when threshold / knee / ratio change;
//! - makeup gain `pow(1 / Saturate(1, k), 0.6)` × post gain (0 dB), effect blend 1;
//! - `kNDivisionFrames` = 32: per division the envelope rate (attack: `1 − (0.25/maxDiff)^(1/attackFrames)`,
//!   release: 4th-order adaptive release polynomial through release zones 0.09 / 0.16 / 0.42 / 0.98
//!   of the release frames, 5 dB spacing), then per frame the detector (max |x| over channels →
//!   saturation → attenuation, 2.5 ms detector release), the compressor gain, the sin() warp, the
//!   metering (instant peaks, 0.325 s release) and the delayed output;
//! - the "gremlin" NaN / inf fixes and the denormal flush of the detector / gain at each division end;
//! - the start-up state (detector 0, gain 1, metering gain 1, max attack diff −1), which makes a
//!   fresh node duck its first ~10 ms (the Chromium behaviour).
//!
//! Deviations: a quantum whose length is not a multiple of 32 also processes its trailing partial
//! division (Chromium leaves those frames unwritten; the engine always renders 128-frame quanta).
//! `reset()` returns to the freshly constructed state (including the pending pre-delay re-apply),
//! like a new node. Chromium's audio thread runs with flush-to-zero; inputs here are not flushed
//! (a denormal input sample would differ by < 1e-38).

use std::f32::consts::FRAC_PI_2;

/// k-rate parameters of one render quantum (the AudioParams' final values).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompressorParams {
    /// dB, −100..0.
    pub threshold: f32,
    /// dB, 0..40.
    pub knee: f32,
    /// 1..20.
    pub ratio: f32,
    /// Seconds, 0..1.
    pub attack: f32,
    /// Seconds, 0..1.
    pub release: f32,
}

impl Default for CompressorParams {
    /// The Web Audio defaults (`DynamicsCompressorNode` constructor).
    fn default() -> Self {
        Self {
            threshold: -24.0,
            knee: 30.0,
            ratio: 12.0,
            attack: 0.003,
            release: 0.25,
        }
    }
}

const MAX_PRE_DELAY_FRAMES: usize = 1024;
const MAX_PRE_DELAY_MASK: usize = MAX_PRE_DELAY_FRAMES - 1;
const DEFAULT_PRE_DELAY_FRAMES: usize = 256;
const PRE_DELAY: f32 = 0.006;
const N_DIVISION_FRAMES: usize = 32;
const METERING_RELEASE_TIME_CONSTANT: f32 = 0.325;
const UNINITIALIZED: f32 = -1.0;
const RELEASE_ZONES: [f32; 4] = [0.09, 0.16, 0.42, 0.98];
const SAT_RELEASE_TIME: f32 = 0.0025;
const SPACING_DB: f32 = 5.0;

/// `audio_utilities::DecibelsToLinear` (float pow).
fn db_to_linear(db: f32) -> f32 {
    10f32.powf(0.05 * db)
}

/// `audio_utilities::LinearToDecibels` (float log10).
fn linear_to_db(linear: f32) -> f32 {
    20.0 * linear.log10()
}

/// `std::min(a, b)`: `b < a ? b : a`.
fn std_min(a: f32, b: f32) -> f32 {
    if b < a { b } else { a }
}

/// `std::max(a, b)`: `a < b ? b : a`.
fn std_max(a: f32, b: f32) -> f32 {
    if a < b { b } else { a }
}

/// blink `ClampTo` for floats (NaN-free inputs here).
fn clamp_to(x: f32, lo: f32, hi: f32) -> f32 {
    if x >= hi {
        hi
    } else if x <= lo {
        lo
    } else {
        x
    }
}

/// `DenormalDisabler::FlushDenormalFloatToZero`.
/// `sin`, `asin` and `exp` (and the attack's `powf`) are computed in f64 and rounded to f32: that
/// matches Chromium's results where the f32 functions differ by an ulp (16 of the 18 captured
/// cases are then bit-exact, see `compressor_tests.rs`).
#[inline]
fn f64_sin(x: f32) -> f32 {
    (x as f64).sin() as f32
}

#[inline]
fn f64_asin(x: f32) -> f32 {
    (x as f64).asin() as f32
}

#[inline]
fn f64_exp(x: f32) -> f32 {
    (x as f64).exp() as f32
}

fn flush_denormal(x: f32) -> f32 {
    if x.abs() < f32::MIN_POSITIVE { 0.0 } else { x }
}

/// The static compression curve (the `UpdateStaticCurveParameters` cache).
#[derive(Debug, Clone, Copy)]
struct Curve {
    ratio: f32,
    slope: f32,
    linear_threshold: f32,
    db_threshold: f32,
    db_knee: f32,
    knee_threshold: f32,
    knee_threshold_db: f32,
    yknee_threshold_db: f32,
    k: f32,
}

impl Curve {
    const UNSET: Self = Self {
        ratio: UNINITIALIZED,
        slope: UNINITIALIZED,
        linear_threshold: UNINITIALIZED,
        db_threshold: UNINITIALIZED,
        db_knee: UNINITIALIZED,
        knee_threshold: UNINITIALIZED,
        knee_threshold_db: UNINITIALIZED,
        yknee_threshold_db: UNINITIALIZED,
        k: UNINITIALIZED,
    };

    /// Exponential knee: 1st-derivative matched at the threshold, asymptote threshold + 1/k.
    fn knee_curve(&self, x: f32, k: f32) -> f32 {
        if x < self.linear_threshold {
            return x;
        }
        self.linear_threshold + (1.0 - f64_exp(-k * (x - self.linear_threshold))) / k
    }

    /// Knee, then constant ratio past the knee.
    fn saturate(&self, x: f32, k: f32) -> f32 {
        if x < self.knee_threshold {
            self.knee_curve(x, k)
        } else {
            let x_db = linear_to_db(x);
            let y_db = self.yknee_threshold_db + self.slope * (x_db - self.knee_threshold_db);
            db_to_linear(y_db)
        }
    }

    /// dB-domain slope of the knee curve at `x`.
    fn slope_at(&self, x: f32, k: f32) -> f32 {
        if x < self.linear_threshold {
            return 1.0;
        }
        let x2 = (x as f64 * 1.001) as f32;
        let x_db = linear_to_db(x);
        let x2_db = linear_to_db(x2);
        let y_db = linear_to_db(self.knee_curve(x, k));
        let y2_db = linear_to_db(self.knee_curve(x2, k));
        (y2_db - y_db) / (x2_db - x_db)
    }

    fn k_at_slope(&self, desired_slope: f32) -> f32 {
        let x_db = self.db_threshold + self.db_knee;
        let x = db_to_linear(x_db);
        let (mut min_k, mut max_k, mut k) = (0.1f32, 10000f32, 5f32);
        for _ in 0..15 {
            if self.slope_at(x, k) < desired_slope {
                max_k = k;
            } else {
                min_k = k;
            }
            k = (min_k * max_k).sqrt();
        }
        k
    }

    fn update(&mut self, db_threshold: f32, db_knee: f32, ratio: f32) -> f32 {
        if db_threshold != self.db_threshold || db_knee != self.db_knee || ratio != self.ratio {
            self.db_threshold = db_threshold;
            self.linear_threshold = db_to_linear(db_threshold);
            self.db_knee = db_knee;
            self.ratio = ratio;
            self.slope = 1.0 / self.ratio;
            let k = self.k_at_slope(1.0 / self.ratio);
            self.knee_threshold_db = db_threshold + db_knee;
            self.knee_threshold = db_to_linear(self.knee_threshold_db);
            self.yknee_threshold_db = linear_to_db(self.knee_curve(self.knee_threshold, k));
            self.k = k;
        }
        self.k
    }
}

/// Stereo DynamicsCompressor (state + pre-delay buffers). Built on the control side.
pub struct Compressor {
    sample_rate: f32,
    curve: Curve,
    pre_delay: [Box<[f32; MAX_PRE_DELAY_FRAMES]>; 2],
    read_index: usize,
    write_index: usize,
    last_pre_delay_frames: usize,
    detector_average: f32,
    compressor_gain: f32,
    max_attack_compression_diff_db: f32,
    metering_gain: f32,
    metering_release_k: f32,
    reduction: f32,
}

impl Compressor {
    pub fn new(sample_rate: f32) -> Self {
        // audio_utilities::DiscreteTimeConstantForSampleRate (double), stored as float.
        let metering_release_k = (1.0
            - (-1.0 / (sample_rate as f64 * METERING_RELEASE_TIME_CONSTANT as f64)).exp())
            as f32;
        let mut c = Self {
            sample_rate,
            curve: Curve::UNSET,
            pre_delay: [
                Box::new([0.0; MAX_PRE_DELAY_FRAMES]),
                Box::new([0.0; MAX_PRE_DELAY_FRAMES]),
            ],
            read_index: 0,
            write_index: DEFAULT_PRE_DELAY_FRAMES,
            last_pre_delay_frames: DEFAULT_PRE_DELAY_FRAMES,
            detector_average: 0.0,
            compressor_gain: 1.0,
            max_attack_compression_diff_db: -1.0,
            metering_gain: 1.0,
            metering_release_k,
            reduction: 0.0,
        };
        c.reset();
        c
    }

    /// Back to the state of a freshly constructed node (keeps the static-curve cache, which is a
    /// pure function of the parameters).
    pub fn reset(&mut self) {
        self.detector_average = 0.0;
        self.compressor_gain = 1.0;
        self.metering_gain = 1.0;
        for b in &mut self.pre_delay {
            b.fill(0.0);
        }
        self.read_index = 0;
        self.write_index = DEFAULT_PRE_DELAY_FRAMES;
        self.last_pre_delay_frames = DEFAULT_PRE_DELAY_FRAMES;
        self.max_attack_compression_diff_db = -1.0;
        self.reduction = 0.0;
    }

    /// `DynamicsCompressorNode.reduction` (dB, ≤ 0) after the last quantum.
    pub fn reduction(&self) -> f32 {
        self.reduction
    }

    fn set_pre_delay_time(&mut self, pre_delay_time: f32) {
        // `unsigned pre_delay_frames = pre_delay_time * SampleRate();`
        let frames = ((pre_delay_time * self.sample_rate) as usize).min(MAX_PRE_DELAY_FRAMES - 1);
        if self.last_pre_delay_frames != frames {
            self.last_pre_delay_frames = frames;
            for b in &mut self.pre_delay {
                b.fill(0.0);
            }
            self.read_index = 0;
            self.write_index = frames;
        }
    }

    /// One render quantum (`l.len() == r.len() ≤ 128`) in place.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32], p: &CompressorParams) {
        debug_assert_eq!(l.len(), r.len());
        // AudioParamHandler::FinalValue clamps to the nominal ranges.
        let db_threshold = p.threshold.clamp(-100.0, 0.0);
        let db_knee = p.knee.clamp(0.0, 40.0);
        let ratio = p.ratio.clamp(1.0, 20.0);
        let attack_time = p.attack.clamp(0.0, 1.0);
        let release_time = p.release.clamp(0.0, 1.0);
        let sample_rate = self.sample_rate;

        let (dry_mix, wet_mix) = (0.0f32, 1.0f32); // effect blend 1
        let k = self.curve.update(db_threshold, db_knee, ratio);

        // Makeup gain.
        let full_range_gain = self.curve.saturate(1.0, k);
        let full_range_makeup_gain = (1.0 / full_range_gain).powf(0.6);
        let master_linear_gain = db_to_linear(0.0) * full_range_makeup_gain;

        let attack_time = std_max(0.001, attack_time);
        let attack_frames = attack_time * sample_rate;
        let release_frames = sample_rate * release_time;
        let sat_release_frames = SAT_RELEASE_TIME * sample_rate;

        // 4th-order polynomial through the release zones (x = 0, 1, 2, 3).
        let y1 = release_frames * RELEASE_ZONES[0];
        let y2 = release_frames * RELEASE_ZONES[1];
        let y3 = release_frames * RELEASE_ZONES[2];
        let y4 = release_frames * RELEASE_ZONES[3];
        #[allow(clippy::excessive_precision)]
        let (ka, kb, kc, kd, ke) = (
            0.9999999999999998f32 * y1 + 1.8432219684323923e-16f32 * y2
                - 1.9373394351676423e-16f32 * y3
                + 8.824516011816245e-18f32 * y4,
            -1.5788320352845888f32 * y1 + 2.3305837032074286f32 * y2 - 0.9141194204840429f32 * y3
                + 0.1623677525612032f32 * y4,
            0.5334142869106424f32 * y1 - 1.272736789213631f32 * y2 + 0.9258856042207512f32 * y3
                - 0.18656310191776226f32 * y4,
            0.08783463138207234f32 * y1 - 0.1694162967925622f32 * y2 + 0.08588057951595272f32 * y3
                - 0.00429891410546283f32 * y4,
            -0.042416883008123074f32 * y1 + 0.1115693827987602f32 * y2
                - 0.09764676325265872f32 * y3
                + 0.028494263462021576f32 * y4,
        );

        self.set_pre_delay_time(PRE_DELAY);

        let frames = l.len();
        let mut frame_index = 0;
        while frame_index < frames {
            let division_frames = N_DIVISION_FRAMES.min(frames - frame_index);
            if self.detector_average.is_nan() || self.detector_average.is_infinite() {
                self.detector_average = 1.0;
            }
            let desired_gain = self.detector_average;
            // Pre-warp so we get desired_gain after the sin() warp below.
            let scaled_desired_gain = f64_asin(desired_gain) / FRAC_PI_2;

            let is_releasing = scaled_desired_gain > self.compressor_gain;
            let mut compression_diff_db = linear_to_db(self.compressor_gain / scaled_desired_gain);
            let envelope_rate = if is_releasing {
                self.max_attack_compression_diff_db = -1.0;
                if compression_diff_db.is_nan() || compression_diff_db.is_infinite() {
                    compression_diff_db = -1.0;
                }
                // Adaptive release: -12..0 dB → 0..3.
                let x = clamp_to(compression_diff_db, -12.0, 0.0);
                let x = 0.25 * (x + 12.0);
                let x2 = x * x;
                let x3 = x2 * x;
                let x4 = x2 * x2;
                let release_frames = ka + kb * x + kc * x2 + kd * x3 + ke * x4;
                let db_per_frame = SPACING_DB / release_frames;
                db_to_linear(db_per_frame)
            } else {
                if compression_diff_db.is_nan() || compression_diff_db.is_infinite() {
                    compression_diff_db = 1.0;
                }
                if self.max_attack_compression_diff_db == -1.0
                    || self.max_attack_compression_diff_db < compression_diff_db
                {
                    self.max_attack_compression_diff_db = compression_diff_db;
                }
                let eff_atten_diff_db = std_max(0.5, self.max_attack_compression_diff_db);
                let x = 0.25 / eff_atten_diff_db;
                1.0 - ((x as f64).powf((1.0 / attack_frames) as f64) as f32)
            };

            let mut read = self.read_index;
            let mut write = self.write_index;
            let mut detector_average = self.detector_average;
            let mut compressor_gain = self.compressor_gain;
            for _ in 0..division_frames {
                let (sl, sr) = (l[frame_index], r[frame_index]);
                self.pre_delay[0][write] = sl;
                self.pre_delay[1][write] = sr;
                let mut compressor_input = 0.0f32;
                for s in [sl, sr] {
                    let abs = if s > 0.0 { s } else { -s };
                    if compressor_input < abs {
                        compressor_input = abs;
                    }
                }
                let abs_input = if compressor_input > 0.0 {
                    compressor_input
                } else {
                    -compressor_input
                };
                let shaped_input = self.curve.saturate(abs_input, k);
                let attenuation = if abs_input <= 0.0001 {
                    1.0
                } else {
                    shaped_input / abs_input
                };
                let attenuation_db = std_max(2.0, -linear_to_db(attenuation));
                let db_per_frame = attenuation_db / sat_release_frames;
                let sat_release_rate = db_to_linear(db_per_frame) - 1.0;
                let rate = if attenuation > detector_average {
                    sat_release_rate
                } else {
                    1.0
                };
                detector_average += (attenuation - detector_average) * rate;
                detector_average = std_min(1.0, detector_average);
                if detector_average.is_nan() || detector_average.is_infinite() {
                    detector_average = 1.0;
                }

                if envelope_rate < 1.0 {
                    // Attack: reduce gain towards the desired gain.
                    compressor_gain += (scaled_desired_gain - compressor_gain) * envelope_rate;
                } else {
                    // Release: exponentially increase gain towards 1.
                    compressor_gain *= envelope_rate;
                    compressor_gain = std_min(1.0, compressor_gain);
                }

                let post_warp_compressor_gain = f64_sin(FRAC_PI_2 * compressor_gain);
                let total_gain = dry_mix + wet_mix * master_linear_gain * post_warp_compressor_gain;

                let db_real_gain = 20.0 * post_warp_compressor_gain.log10();
                if db_real_gain < self.metering_gain {
                    self.metering_gain = db_real_gain;
                } else {
                    self.metering_gain +=
                        (db_real_gain - self.metering_gain) * self.metering_release_k;
                }

                l[frame_index] = self.pre_delay[0][read] * total_gain;
                r[frame_index] = self.pre_delay[1][read] * total_gain;

                frame_index += 1;
                read = (read + 1) & MAX_PRE_DELAY_MASK;
                write = (write + 1) & MAX_PRE_DELAY_MASK;
            }
            self.read_index = read;
            self.write_index = write;
            self.detector_average = flush_denormal(detector_average);
            self.compressor_gain = flush_denormal(compressor_gain);
        }
        self.reduction = self.metering_gain;
    }
}

#[cfg(test)]
#[path = "compressor_tests.rs"]
mod tests;
