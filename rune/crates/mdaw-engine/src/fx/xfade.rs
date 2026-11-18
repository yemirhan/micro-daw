//! `Tone.CrossFade` (the wet/dry of every Tone effect and of RoomReverb): `fade` (a Signal) goes
//! through `GainToAudio` (a 1024-point WaveShaper `x ↦ |x|·2 − 1`, oversample none) into the `pan`
//! of a mono `StereoPannerNode` fed by a constant 1; its two outputs are the gains of `a` (dry)
//! and `b` (wet): `a = f32(cos(θ))`, `b = f32(sin(θ))`, `θ = (pan·0.5 + 0.5)·π/2` (f64, per frame).
//! The curve has no point at 0, so `fade = 0` gives pan −0.998045 and a −56.3 dB wet leak (the
//! "disabled effect" leak of the track strip).

use std::sync::LazyLock;

use mdaw_dsp::shaping::{tone_curve, waveshape};

use super::Q;
use super::tone::{ToneParam, Units};
use crate::sampler::param::Clock;

static G2A: LazyLock<Vec<f32>> = LazyLock::new(|| tone_curve(1024, |x| x.abs() * 2.0 - 1.0));

/// The GainToAudio curve (resolve on the control side: the first use allocates).
pub fn g2a_curve() -> &'static [f32] {
    &G2A
}

/// Equal-power gains of a CrossFade at `fade` (one frame).
#[inline]
pub fn gains(curve: &[f32], fade: f32) -> (f32, f32) {
    let pan = (waveshape(curve, fade) as f64).clamp(-1.0, 1.0);
    let th = (pan * 0.5 + 0.5) * std::f64::consts::FRAC_PI_2;
    (th.cos() as f32, th.sin() as f32)
}

/// A CrossFade's `fade` and the per-frame gains of the current quantum.
#[derive(Clone)]
pub struct CrossFade {
    pub fade: ToneParam,
    curve: &'static [f32],
    vals: [f32; Q],
    pub a: [f32; Q],
    pub b: [f32; Q],
    last: f32,
}

impl CrossFade {
    pub fn new(fade: f64, cx: Clock) -> Self {
        let curve = g2a_curve();
        let (ga, gb) = gains(curve, fade as f32);
        Self {
            fade: ToneParam::signal(fade, Units::Linear, cx),
            curve,
            vals: [0.0; Q],
            a: [ga; Q],
            b: [gb; Q],
            last: fade as f32,
        }
    }

    /// Compute `a` / `b` for the quantum at `frame`.
    pub fn update(&mut self, frame: u64, sr: f64) {
        let varying = self.fade.render(frame, sr, &mut self.vals);
        if !varying && self.vals[0] == self.last {
            return;
        }
        if varying {
            for k in 0..Q {
                let (a, b) = gains(self.curve, self.vals[k]);
                self.a[k] = a;
                self.b[k] = b;
            }
        } else {
            let (a, b) = gains(self.curve, self.vals[0]);
            self.a = [a; Q];
            self.b = [b; Q];
        }
        self.last = if varying { f32::NAN } else { self.vals[0] };
    }

    /// `out = a·dry + b·wet` per frame (the CrossFade's output sum).
    #[inline]
    pub fn mix(&self, dry: &[f32; Q], wet: &[f32; Q], out: &mut [f32; Q]) {
        for k in 0..Q {
            out[k] = dry[k] * self.a[k] + wet[k] * self.b[k];
        }
    }

    /// Whether the wet path is silent (b == 0 everywhere): never for Tone (see module docs).
    pub fn wet_is_zero(&self) -> bool {
        self.b.iter().all(|&b| b == 0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_fade_leaks_at_minus_56_db_and_half_is_equal_power() {
        let c = g2a_curve();
        let (a, b) = gains(c, 0.0);
        assert!((20.0 * (b as f64).log10() + 56.27).abs() < 0.05 && 1.0 - a < 2e-6);
        let (a, b) = gains(c, 0.5);
        assert!((a - b).abs() < 1e-6);
        let (a, b) = gains(c, 1.0);
        assert!(a.abs() < 1e-7 && b == 1.0);
    }
}
