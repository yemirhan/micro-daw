//! Split-complex radix-2 FFT for the partitioned convolver (no dependencies).
//!
//! - Forward = decimation in frequency: natural-order input, **bit-reversed** output.
//! - Inverse = decimation in time: bit-reversed input, natural-order output, unnormalized
//!   (conjugate twiddles, no 1/N; the convolver folds 1/N into the IR spectra).
//!
//! The convolver only multiplies spectra point-wise, so it never reorders: it reads / writes bins
//! through [`Plan::rev`]. Data is split (separate `re` / `im` slices) and every butterfly pass is
//! a loop over contiguous equal-length slices, which LLVM vectorizes. The two smallest passes
//! (half sizes 2 and 1) are fused into one radix-4 pass with trivial twiddles (1, ∓i). Every pass
//! takes a butterfly (or radix-4 group) range so the convolver can spread one transform over
//! several render quanta.
//!
//! Twiddles are computed in f64 and rounded once. Generic over [`Real`]: the convolver runs in
//! f32 and builds its IR spectra with the f64 instance.

use std::ops::{Add, Mul, Neg, Sub};

/// f32 / f64.
pub trait Real:
    Copy + Default + Add<Output = Self> + Sub<Output = Self> + Mul<Output = Self> + Neg<Output = Self>
{
    fn from_f64(x: f64) -> Self;
}

impl Real for f32 {
    fn from_f64(x: f64) -> Self {
        x as f32
    }
}

impl Real for f64 {
    fn from_f64(x: f64) -> Self {
        x
    }
}

/// Tables of one transform size `n` (power of two, ≥ 4).
pub struct Plan<T> {
    pub n: usize,
    /// `tw[h + k] = exp(−2πi·k / 2h)` for every pass half size `h` (power of two < n), `k < h`.
    pub tw_re: Vec<T>,
    pub tw_im: Vec<T>,
    /// Bit reversal: bin `k` lives at index `rev[k]` of a forward output / inverse input.
    pub rev: Vec<u32>,
}

impl<T: Real> Plan<T> {
    pub fn new(n: usize) -> Self {
        assert!(n.is_power_of_two() && n >= 4, "FFT size {n}");
        let mut tw_re = vec![T::default(); n];
        let mut tw_im = vec![T::default(); n];
        let mut h = 1;
        while h < n {
            for k in 0..h {
                let a = -std::f64::consts::PI * k as f64 / h as f64;
                tw_re[h + k] = T::from_f64(a.cos());
                tw_im[h + k] = T::from_f64(a.sin());
            }
            h *= 2;
        }
        let bits = n.trailing_zeros();
        let rev = (0..n as u32)
            .map(|k| k.reverse_bits() >> (32 - bits))
            .collect();
        Self {
            n,
            tw_re,
            tw_im,
            rev,
        }
    }

    /// Whole forward transform in place (natural → bit-reversed).
    pub fn forward(&self, re: &mut [T], im: &mut [T]) {
        let mut h = self.n / 2;
        while h >= 4 {
            dif_pass(re, im, &self.tw_re, &self.tw_im, h, 0, self.n / 2);
            h /= 2;
        }
        dif_radix4_last(re, im, 0, self.n / 4);
    }

    /// Whole unnormalized inverse transform in place (bit-reversed → natural). The convolver runs
    /// it in steps; the whole transform is for the tests.
    #[cfg(test)]
    pub fn inverse(&self, re: &mut [T], im: &mut [T]) {
        dit_radix4_first(re, im, 0, self.n / 4);
        let mut h = 4;
        while h < self.n {
            dit_pass(re, im, &self.tw_re, &self.tw_im, h, 0, self.n / 2);
            h *= 2;
        }
    }
}

/// Calls `f(base, k0, k1)` for the pieces of butterfly range `[j0, j1)` of a pass with half size
/// `h`: butterflies `k0..k1` of the group starting at index `base` (pairs `base+k`, `base+h+k`).
#[inline(always)]
fn for_groups(h: usize, j0: usize, j1: usize, mut f: impl FnMut(usize, usize, usize)) {
    let mut j = j0;
    while j < j1 {
        let (g, k0) = (j / h, j % h);
        let k1 = h.min(k0 + (j1 - j));
        f(g * 2 * h, k0, k1);
        j += k1 - k0;
    }
}

/// DIF pass, half size `h`, butterflies `[j0, j1)` of `n/2`:
/// `a' = a + b`, `b' = (a − b)·w_k`.
pub fn dif_pass<T: Real>(
    re: &mut [T],
    im: &mut [T],
    tw_re: &[T],
    tw_im: &[T],
    h: usize,
    j0: usize,
    j1: usize,
) {
    for_groups(h, j0, j1, |base, k0, k1| {
        let len = k1 - k0;
        let (lo_re, hi_re) = re[base..base + 2 * h].split_at_mut(h);
        let (lo_im, hi_im) = im[base..base + 2 * h].split_at_mut(h);
        let (ar, br) = (&mut lo_re[k0..k1], &mut hi_re[k0..k1]);
        let (ai, bi) = (&mut lo_im[k0..k1], &mut hi_im[k0..k1]);
        let (wr, wi) = (&tw_re[h + k0..h + k1], &tw_im[h + k0..h + k1]);
        for i in 0..len {
            let (xr, xi, yr, yi) = (ar[i], ai[i], br[i], bi[i]);
            ar[i] = xr + yr;
            ai[i] = xi + yi;
            let (dr, di) = (xr - yr, xi - yi);
            br[i] = dr * wr[i] - di * wi[i];
            bi[i] = dr * wi[i] + di * wr[i];
        }
    });
}

/// First DIF pass (half size `h` = n/2) reading its input from two `h`-long halves held
/// elsewhere (`prev` = samples `0..h`, `cur` = samples `h..2h`, real/imag split), butterflies
/// `[j0, j1)`: writes `out[j]` and `out[j + h]`. The input slices are indexed from `j0`.
#[allow(clippy::too_many_arguments)]
pub fn dif_first_from<T: Real>(
    prev: (&[T], &[T]),
    cur: (&[T], &[T]),
    tw_re: &[T],
    tw_im: &[T],
    h: usize,
    j0: usize,
    j1: usize,
    re: &mut [T],
    im: &mut [T],
) {
    let len = j1 - j0;
    let (xr, xi) = (&prev.0[..len], &prev.1[..len]);
    let (yr, yi) = (&cur.0[..len], &cur.1[..len]);
    let (lo_re, hi_re) = re.split_at_mut(h);
    let (lo_im, hi_im) = im.split_at_mut(h);
    let (ar, br) = (&mut lo_re[j0..j1], &mut hi_re[j0..j1]);
    let (ai, bi) = (&mut lo_im[j0..j1], &mut hi_im[j0..j1]);
    let (wr, wi) = (&tw_re[h + j0..h + j1], &tw_im[h + j0..h + j1]);
    for i in 0..len {
        ar[i] = xr[i] + yr[i];
        ai[i] = xi[i] + yi[i];
        let (dr, di) = (xr[i] - yr[i], xi[i] - yi[i]);
        br[i] = dr * wr[i] - di * wi[i];
        bi[i] = dr * wi[i] + di * wr[i];
    }
}

/// Last two DIF passes (half sizes 2 and 1) fused, radix-4 groups `[g0, g1)` of `n/4`.
pub fn dif_radix4_last<T: Real>(re: &mut [T], im: &mut [T], g0: usize, g1: usize) {
    let (re, im) = (&mut re[4 * g0..4 * g1], &mut im[4 * g0..4 * g1]);
    let (re, im) = (re.as_chunks_mut::<4>().0, im.as_chunks_mut::<4>().0);
    for (r, i) in re.iter_mut().zip(im.iter_mut()) {
        // half 2: y0 = x0 + x2, y2 = x0 − x2, y1 = x1 + x3, y3 = (x1 − x3)·(−i)
        let (y0r, y0i) = (r[0] + r[2], i[0] + i[2]);
        let (y2r, y2i) = (r[0] - r[2], i[0] - i[2]);
        let (y1r, y1i) = (r[1] + r[3], i[1] + i[3]);
        let (y3r, y3i) = (i[1] - i[3], r[3] - r[1]);
        // half 1: z0 = y0 + y1, z1 = y0 − y1, z2 = y2 + y3, z3 = y2 − y3
        r[0] = y0r + y1r;
        i[0] = y0i + y1i;
        r[1] = y0r - y1r;
        i[1] = y0i - y1i;
        r[2] = y2r + y3r;
        i[2] = y2i + y3i;
        r[3] = y2r - y3r;
        i[3] = y2i - y3i;
    }
}

/// First two DIT passes (half sizes 1 and 2) fused, conjugate twiddles, groups `[g0, g1)`.
pub fn dit_radix4_first<T: Real>(re: &mut [T], im: &mut [T], g0: usize, g1: usize) {
    let (re, im) = (&mut re[4 * g0..4 * g1], &mut im[4 * g0..4 * g1]);
    let (re, im) = (re.as_chunks_mut::<4>().0, im.as_chunks_mut::<4>().0);
    for (r, i) in re.iter_mut().zip(im.iter_mut()) {
        let (y0r, y0i) = (r[0] + r[1], i[0] + i[1]);
        let (y1r, y1i) = (r[0] - r[1], i[0] - i[1]);
        let (y2r, y2i) = (r[2] + r[3], i[2] + i[3]);
        // y3·(+i)
        let (tr, ti) = (i[3] - i[2], r[2] - r[3]);
        r[0] = y0r + y2r;
        i[0] = y0i + y2i;
        r[2] = y0r - y2r;
        i[2] = y0i - y2i;
        r[1] = y1r + tr;
        i[1] = y1i + ti;
        r[3] = y1r - tr;
        i[3] = y1i - ti;
    }
}

/// DIT pass (inverse), half size `h`, butterflies `[j0, j1)`:
/// `t = b·conj(w_k)`, `a' = a + t`, `b' = a − t`.
pub fn dit_pass<T: Real>(
    re: &mut [T],
    im: &mut [T],
    tw_re: &[T],
    tw_im: &[T],
    h: usize,
    j0: usize,
    j1: usize,
) {
    for_groups(h, j0, j1, |base, k0, k1| {
        let len = k1 - k0;
        let (lo_re, hi_re) = re[base..base + 2 * h].split_at_mut(h);
        let (lo_im, hi_im) = im[base..base + 2 * h].split_at_mut(h);
        let (ar, br) = (&mut lo_re[k0..k1], &mut hi_re[k0..k1]);
        let (ai, bi) = (&mut lo_im[k0..k1], &mut hi_im[k0..k1]);
        let (wr, wi) = (&tw_re[h + k0..h + k1], &tw_im[h + k0..h + k1]);
        for i in 0..len {
            let (xr, xi, yr, yi) = (ar[i], ai[i], br[i], bi[i]);
            let tr = yr * wr[i] + yi * wi[i];
            let ti = yi * wr[i] - yr * wi[i];
            ar[i] = xr + tr;
            ai[i] = xi + ti;
            br[i] = xr - tr;
            bi[i] = xi - ti;
        }
    });
}

/// Upper half of the last DIT pass (half size `h` = n/2), butterflies `[j0, j1)`, ADDED into
/// `out` (indexed from `j0`): `out[j − j0] += a_j − b_j·conj(w_j)` = inverse output sample `h + j`
/// (the overlap-save half; the lower half is never computed).
#[allow(clippy::too_many_arguments)]
pub fn dit_last_upper_add<T: Real>(
    re: &[T],
    im: &[T],
    tw_re: &[T],
    tw_im: &[T],
    h: usize,
    j0: usize,
    j1: usize,
    out_re: &mut [T],
    out_im: &mut [T],
) {
    let len = j1 - j0;
    let (lo_re, hi_re) = re.split_at(h);
    let (lo_im, hi_im) = im.split_at(h);
    let (ar, br) = (&lo_re[j0..j1], &hi_re[j0..j1]);
    let (ai, bi) = (&lo_im[j0..j1], &hi_im[j0..j1]);
    let (wr, wi) = (&tw_re[h + j0..h + j1], &tw_im[h + j0..h + j1]);
    let (or, oi) = (&mut out_re[..len], &mut out_im[..len]);
    for i in 0..len {
        let tr = br[i] * wr[i] + bi[i] * wi[i];
        let ti = bi[i] * wr[i] - br[i] * wi[i];
        or[i] = or[i] + (ar[i] - tr);
        oi[i] = oi[i] + (ai[i] - ti);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn naive_dft(re: &[f64], im: &[f64]) -> (Vec<f64>, Vec<f64>) {
        let n = re.len();
        let mut or = vec![0.0; n];
        let mut oi = vec![0.0; n];
        for k in 0..n {
            for t in 0..n {
                let a = -2.0 * std::f64::consts::PI * ((k * t) % n) as f64 / n as f64;
                or[k] += re[t] * a.cos() - im[t] * a.sin();
                oi[k] += re[t] * a.sin() + im[t] * a.cos();
            }
        }
        (or, oi)
    }

    #[test]
    fn forward_matches_dft_and_inverse_round_trips() {
        for n in [4usize, 8, 16, 64, 256, 512] {
            let re: Vec<f64> = (0..n)
                .map(|i| ((i * 7919) % 101) as f64 / 50.0 - 1.0)
                .collect();
            let im: Vec<f64> = (0..n)
                .map(|i| ((i * 104_729) % 97) as f64 / 48.0 - 1.0)
                .collect();
            let plan = Plan::<f64>::new(n);
            let (mut r, mut i) = (re.clone(), im.clone());
            plan.forward(&mut r, &mut i);
            let (dr, di) = naive_dft(&re, &im);
            for k in 0..n {
                let p = plan.rev[k] as usize;
                assert!(
                    (r[p] - dr[k]).abs() < 1e-9 && (i[p] - di[k]).abs() < 1e-9,
                    "n {n} k {k}"
                );
            }
            plan.inverse(&mut r, &mut i);
            for t in 0..n {
                assert!((r[t] / n as f64 - re[t]).abs() < 1e-12, "n {n}");
                assert!((i[t] / n as f64 - im[t]).abs() < 1e-12, "n {n}");
            }
            // Chunked passes == whole passes.
            let (mut r2, mut i2) = (re.clone(), im.clone());
            let mut h = n / 2;
            while h >= 4 {
                let mut j = 0;
                while j < n / 2 {
                    let e = (j + 3).min(n / 2);
                    dif_pass(&mut r2, &mut i2, &plan.tw_re, &plan.tw_im, h, j, e);
                    j = e;
                }
                h /= 2;
            }
            dif_radix4_last(&mut r2, &mut i2, 0, n / 4);
            let (mut r3, mut i3) = (re.clone(), im.clone());
            plan.forward(&mut r3, &mut i3);
            assert_eq!((r2, i2), (r3, i3));
        }
    }
}
