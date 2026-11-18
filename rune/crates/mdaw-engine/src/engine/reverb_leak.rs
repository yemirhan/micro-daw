//! Offline RoomReverb wet leak of the track strips (see `core/strip.rs`): the 4 reverb-send buses
//! of [`crate::core::Core::render_offline_with_reverb_send`] convolved with the normalized
//! RoomReverb IR (`RoomReverb` defaults, `ConvolverNode.normalize`): output L gets
//! `scale·(IR_L ⊛ A_L + IR_R ⊛ B_L)`, output R `scale·(IR_L ⊛ A_R + IR_R ⊛ B_R)`.
//!
//! One FFT convolution over the whole render (f64, radix-2; two real signals per complex FFT).

use mdaw_dsp::reverb_ir::{
    REVERB_DEFAULTS, convolver_normalization_scale, create_impulse_response,
};

#[derive(Clone, Copy, Default)]
struct C {
    re: f64,
    im: f64,
}

impl C {
    fn mul(self, o: C) -> C {
        C {
            re: self.re * o.re - self.im * o.im,
            im: self.re * o.im + self.im * o.re,
        }
    }
    fn add(self, o: C) -> C {
        C {
            re: self.re + o.re,
            im: self.im + o.im,
        }
    }
    fn conj(self) -> C {
        C {
            re: self.re,
            im: -self.im,
        }
    }
}

/// In-place iterative radix-2 FFT (`inverse`: unscaled inverse).
fn fft(a: &mut [C], inverse: bool) {
    let n = a.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            a.swap(i, j);
        }
    }
    let sign = if inverse { 1.0 } else { -1.0 };
    let mut len = 2;
    while len <= n {
        let ang = sign * 2.0 * std::f64::consts::PI / len as f64;
        let half = len / 2;
        let tw: Vec<C> = (0..half)
            .map(|k| C {
                re: (ang * k as f64).cos(),
                im: (ang * k as f64).sin(),
            })
            .collect();
        for start in (0..n).step_by(len) {
            for k in 0..half {
                let u = a[start + k];
                let v = a[start + k + half].mul(tw[k]);
                a[start + k] = u.add(v);
                a[start + k + half] = C {
                    re: u.re - v.re,
                    im: u.im - v.im,
                };
            }
        }
        len <<= 1;
    }
}

/// Split the FFT `z` of `x + i·y` (x, y real) into bin `k` of X and Y.
fn split(z: &[C], k: usize) -> (C, C) {
    let n = z.len();
    let a = z[k];
    let b = z[(n - k) % n].conj();
    let x = C {
        re: (a.re + b.re) * 0.5,
        im: (a.im + b.im) * 0.5,
    };
    // (a − b) / 2i
    let d = C {
        re: a.re - b.re,
        im: a.im - b.im,
    };
    let y = C {
        re: d.im * 0.5,
        im: -d.re * 0.5,
    };
    (x, y)
}

fn packed(x: &[f32], y: &[f32], n: usize) -> Vec<C> {
    let mut v = vec![C::default(); n];
    for (i, c) in v.iter_mut().enumerate() {
        c.re = x.get(i).copied().unwrap_or(0.0) as f64;
        c.im = y.get(i).copied().unwrap_or(0.0) as f64;
    }
    v
}

/// The reverb leak per output channel (`frames` long) at `sample_rate`.
pub(super) fn reverb_leak(sends: &[Vec<f32>; 4], frames: usize, sample_rate: u32) -> [Vec<f64>; 2] {
    let silent = sends.iter().all(|s| s.iter().all(|x| *x == 0.0));
    if silent || frames == 0 {
        return [vec![0.0; frames], vec![0.0; frames]];
    }
    let ir = create_impulse_response(sample_rate, &REVERB_DEFAULTS);
    let scale = convolver_normalization_scale(&ir);
    let n = (frames + ir.frames()).next_power_of_two();
    let mut h = packed(&ir.channels[0], &ir.channels[1], n);
    fft(&mut h, false);
    let mut l = packed(&sends[0], &sends[1], n);
    let mut r = packed(&sends[2], &sends[3], n);
    fft(&mut l, false);
    fft(&mut r, false);
    // Y = YL + i·YR with YL = A_L·HL + B_L·HR, YR = A_R·HL + B_R·HR
    let mut y: Vec<C> = (0..n)
        .map(|k| {
            let (hl, hr) = split(&h, k);
            let (al, bl) = split(&l, k);
            let (ar, br) = split(&r, k);
            let yl = al.mul(hl).add(bl.mul(hr));
            let yr = ar.mul(hl).add(br.mul(hr));
            yl.add(C {
                re: -yr.im,
                im: yr.re,
            })
        })
        .collect();
    fft(&mut y, true);
    let k = scale / n as f64;
    [
        y[..frames].iter().map(|c| c.re * k).collect(),
        y[..frames].iter().map(|c| c.im * k).collect(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_a_direct_convolution() {
        let n = 64;
        let x: Vec<f32> = (0..20).map(|i| ((i * 7 % 5) as f32 - 2.0) * 0.1).collect();
        let y: Vec<f32> = (0..20).map(|i| ((i * 3 % 7) as f32 - 3.0) * 0.05).collect();
        let hx: Vec<f32> = (0..10).map(|i| 1.0 / (i + 1) as f32).collect();
        let hy: Vec<f32> = (0..10).map(|i| (i as f32 - 4.0) * 0.1).collect();
        let mut hp = packed(&hx, &hy, n);
        fft(&mut hp, false);
        let mut z = packed(&x, &y, n);
        fft(&mut z, false);
        let mut out: Vec<C> = (0..n)
            .map(|k| {
                let (a, b) = split(&z, k);
                let (p, q) = split(&hp, k);
                a.mul(p).add(b.mul(q))
            })
            .collect();
        fft(&mut out, true);
        for (t, o) in out.iter().enumerate().take(29) {
            let mut d = 0.0;
            for i in 0..=t {
                d += x.get(i).copied().unwrap_or(0.0) as f64
                    * hx.get(t - i).copied().unwrap_or(0.0) as f64
                    + y.get(i).copied().unwrap_or(0.0) as f64
                        * hy.get(t - i).copied().unwrap_or(0.0) as f64;
            }
            assert!((o.re / n as f64 - d).abs() < 1e-12 && (o.im / n as f64).abs() < 1e-12);
        }
    }
}
