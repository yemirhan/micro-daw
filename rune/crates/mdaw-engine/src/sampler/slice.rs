//! Pure helpers of SamplerKit.ts (`computeSlice`, `voiceEnvelope`) and of
//! `src/audio/voices/common.ts` (`clamp`, `normVelocity`, `velocityGain`).

use super::PadParams;

/// `clamp` of `src/audio/voices/common.ts` (`Math.min(hi, Math.max(lo, v))`).
pub fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    if v.is_nan() {
        return v;
    }
    hi.min(lo.max(v))
}

/// `normVelocity`: non-finite → 0.8, > 1 → /127, clamped to 0..1.
pub fn norm_velocity(velocity: f64) -> f64 {
    if !velocity.is_finite() {
        return 0.8;
    }
    if velocity > 1.0 {
        return clamp(velocity / 127.0, 0.0, 1.0);
    }
    clamp(velocity, 0.0, 1.0)
}

/// `velocityGain(v, s) = 1 − s + s·v^1.7`.
pub fn velocity_gain(velocity: f64, sensitivity: f64) -> f64 {
    let v = clamp(velocity, 0.0, 1.0);
    let s = clamp(sensitivity, 0.0, 1.0);
    1.0 - s + s * v.powf(1.7)
}

pub(super) fn db_to_gain(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

/// The part of a buffer a pad plays (`computeSlice`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slice {
    /// Buffer seconds (into the reversed copy for reverse pads).
    pub offset: f64,
    pub length: f64,
    pub rate: f64,
    pub real_duration: f64,
}

/// `computeSlice`: None when the slice is 0.5 ms or shorter.
pub fn compute_slice(pad: &PadParams, buffer_duration: f64) -> Option<Slice> {
    let or0 = |v: f64| if v.is_nan() { 0.0 } else { v };
    let start = clamp(or0(pad.start_sec), 0.0, buffer_duration);
    let end = match pad.end_sec {
        None => buffer_duration,
        Some(e) => clamp(e, 0.0, buffer_duration),
    };
    let length = end - start;
    #[allow(clippy::neg_cmp_op_on_partial_ord)] // NaN → silent, as in the TS
    if !(length > 0.0005) {
        return None;
    }
    let rate = 2f64.powf(clamp(or0(pad.pitch), -24.0, 24.0) / 12.0);
    Some(Slice {
        offset: if pad.reverse {
            buffer_duration - end
        } else {
            start
        },
        length,
        rate,
        real_duration: length / rate,
    })
}

/// One automation step of a voice envelope.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EnvelopePoint {
    /// `setValueAtTime(value, time)`
    Set { value: f64, time: f64 },
    /// `linearRampToValueAtTime(value, time)`
    Ramp { value: f64, time: f64 },
}

/// `voiceEnvelope`: optional fade-in, hold at `gain`, linear fade to silence over the last
/// `fade_out` seconds (never before the fade-in is done). At most 4 points.
pub fn voice_envelope(
    start: f64,
    duration: f64,
    gain: f64,
    fade_in: f64,
    fade_out: f64,
) -> ([EnvelopePoint; 4], usize) {
    let mut pts = [EnvelopePoint::Set {
        value: 0.0,
        time: 0.0,
    }; 4];
    let mut n = 0;
    let mut push = |p| {
        pts[n] = p;
        n += 1;
    };
    let end = start + duration.max(0.0);
    let in_end = end.min(start + fade_in.max(0.0));
    if in_end > start {
        push(EnvelopePoint::Set {
            value: 0.0,
            time: start,
        });
        push(EnvelopePoint::Ramp {
            value: gain,
            time: in_end,
        });
    } else {
        push(EnvelopePoint::Set {
            value: gain,
            time: start,
        });
    }
    let fade_start = in_end.max(end - fade_out.max(0.0));
    if fade_start > in_end {
        push(EnvelopePoint::Set {
            value: gain,
            time: fade_start,
        });
    }
    if end > fade_start {
        push(EnvelopePoint::Ramp {
            value: 0.0,
            time: end,
        });
    } else {
        push(EnvelopePoint::Set {
            value: 0.0,
            time: end,
        });
    }
    (pts, n)
}
