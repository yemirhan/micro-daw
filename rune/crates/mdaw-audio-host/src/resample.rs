//! Streaming stereo sample-rate converter: context rate → device rate.
//!
//! Polyphase windowed-sinc (Kaiser, β = 9, ≈ 90 dB stopband) with [`PHASES`] tabulated phases
//! and linear interpolation between neighbouring phases. The position is tracked with exact
//! rational arithmetic: output frame `n` sits at input time `n · M / L` (`M / L` =
//! context / device rate, reduced), kept as an integer index plus a numerator in units of `1/L`,
//! so the long-run ratio of input frames consumed to output frames produced is exactly
//! `context_rate / device_rate` (no drift, ever).
//!
//! The filter is zero-phase in index terms (output `n` is centred on input time `n·M/L`); the
//! cost is look-ahead: the input is pulled [`Resampler::latency_frames`] frames ahead of the
//! output. Everything is allocated in [`Resampler::new`]; [`Resampler::process`] never
//! allocates.

/// Tabulated filter phases between two input samples (linearly interpolated).
pub const PHASES: usize = 512;
/// Taps per phase when upsampling; downsampling widens the kernel by `in / out`.
const BASE_TAPS: usize = 128;
const MAX_TAPS: usize = 1024;
/// Kaiser β ≈ 0.1102 · (90 − 8.7): ~90 dB stopband.
const KAISER_BETA: f64 = 9.0;
/// Cutoff as a fraction of the lower Nyquist frequency (the transition band ends just below it).
const ROLLOFF: f64 = 0.91;

pub struct Resampler {
    context_rate: u32,
    device_rate: u32,
    /// Output step per frame is `m / l` input frames (`m / l` = context / device, reduced).
    l: u64,
    m: u64,
    taps: usize,
    half: usize,
    /// `(PHASES + 1) × taps`: row `p` is the kernel for fractional position `p / PHASES`.
    table: Box<[f32]>,
    hist_l: Box<[f32]>,
    hist_r: Box<[f32]>,
    /// Valid input frames in `hist_*`.
    len: usize,
    /// `hist` index of `floor(t)` for the next output frame (`t` its input time).
    pos: usize,
    /// `t - floor(t)` in units of `1 / l`.
    frac: u64,
    /// Max input frames per `pull`.
    chunk: usize,
    consumed: u64,
    produced: u64,
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

fn bessel_i0(x: f64) -> f64 {
    let y = x * x / 4.0;
    let (mut sum, mut term) = (1.0f64, 1.0f64);
    for k in 1..64 {
        term *= y / (k * k) as f64;
        sum += term;
        if term < sum * 1e-17 {
            break;
        }
    }
    sum
}

impl Resampler {
    /// A converter from `context_rate` to `device_rate` that pulls at most `max_chunk` input
    /// frames per `pull` call.
    pub fn new(context_rate: u32, device_rate: u32, max_chunk: usize) -> Self {
        let (cr, dr) = (context_rate.max(1) as u64, device_rate.max(1) as u64);
        let g = gcd(cr, dr);
        let (l, m) = (dr / g, cr / g);
        let ratio = (dr as f64 / cr as f64).min(1.0);
        let taps = (((BASE_TAPS as f64 / ratio).ceil() as usize + 1) & !1).min(MAX_TAPS);
        let half = taps / 2;
        // Cutoff in cycles per input sample.
        let fc = 0.5 * ratio * ROLLOFF;
        let i0b = bessel_i0(KAISER_BETA);
        let kernel = |d: f64| -> f64 {
            let x = d / half as f64;
            if x.abs() >= 1.0 {
                return 0.0;
            }
            let w = bessel_i0(KAISER_BETA * (1.0 - x * x).sqrt()) / i0b;
            let a = 2.0 * fc * d;
            let sinc = if a.abs() < 1e-12 {
                1.0
            } else {
                (std::f64::consts::PI * a).sin() / (std::f64::consts::PI * a)
            };
            2.0 * fc * sinc * w
        };
        let mut table = vec![0f32; (PHASES + 1) * taps];
        let mut row = vec![0f64; taps];
        for p in 0..=PHASES {
            let mu = p as f64 / PHASES as f64;
            // Tap `k` multiplies input `floor(t) - half + 1 + k`: distance `mu + half - 1 - k`.
            for (k, c) in row.iter_mut().enumerate() {
                *c = kernel(mu + half as f64 - 1.0 - k as f64);
            }
            let sum: f64 = row.iter().sum();
            for (dst, c) in table[p * taps..(p + 1) * taps].iter_mut().zip(&row) {
                *dst = (c / sum) as f32; // unity DC gain at every phase
            }
        }
        let chunk = max_chunk.max(1);
        let cap = taps + chunk;
        Self {
            context_rate: cr as u32,
            device_rate: dr as u32,
            l,
            m,
            taps,
            half,
            table: table.into_boxed_slice(),
            hist_l: vec![0.0; cap].into_boxed_slice(),
            hist_r: vec![0.0; cap].into_boxed_slice(),
            // Prime with `half - 1` zeros so output 0 is centred on input 0.
            len: half - 1,
            pos: half - 1,
            frac: 0,
            chunk,
            consumed: 0,
            produced: 0,
        }
    }

    pub fn context_rate(&self) -> u32 {
        self.context_rate
    }

    pub fn device_rate(&self) -> u32 {
        self.device_rate
    }

    /// Filter taps per output frame.
    pub fn taps(&self) -> usize {
        self.taps
    }

    /// Look-ahead in input frames: after producing `n` frames exactly
    /// `floor((n - 1) · M / L) + latency_frames() + 1` input frames have been pulled.
    pub fn latency_frames(&self) -> usize {
        self.half
    }

    /// Input (context) frames pulled so far.
    pub fn consumed(&self) -> u64 {
        self.consumed
    }

    /// Output (device) frames produced so far.
    pub fn produced(&self) -> u64 {
        self.produced
    }

    /// How far the pulled input runs ahead of the output, in ns: `consumed / context_rate −
    /// produced / device_rate`. A frame rendered now at the input's end reaches the output
    /// this much later than one rendered at the output's current position.
    pub fn lead_ns(&self) -> u64 {
        let a = self.consumed as u128 * 1_000_000_000 / self.context_rate as u128;
        let b = self.produced as u128 * 1_000_000_000 / self.device_rate as u128;
        a.saturating_sub(b).min(u64::MAX as u128) as u64
    }

    /// Outputs that can be produced from the buffered input.
    fn available(&self) -> usize {
        // Output k needs hist[pos_k + half] with pos_k = pos + (frac + k·m) / l.
        if self.pos + self.half >= self.len {
            return 0;
        }
        let max_pos = (self.len - 1 - self.half - self.pos) as u64; // pos_k - pos <= max_pos
        // Largest k with (frac + k·m) / l <= max_pos  ⇔  frac + k·m < (max_pos + 1)·l.
        (((max_pos + 1) * self.l - self.frac - 1) / self.m + 1) as usize
    }

    /// Fill `out_l` / `out_r` (same length) with the next output frames, calling `pull` with
    /// zeroed slices (≤ the max chunk) to fetch exactly the input those frames need.
    pub fn process<F>(&mut self, out_l: &mut [f32], out_r: &mut [f32], pull: &mut F)
    where
        F: FnMut(&mut [f32], &mut [f32]),
    {
        let want = out_l.len().min(out_r.len());
        let mut done = 0;
        while done < want {
            let avail = self.available();
            if avail == 0 {
                self.refill(want - done, pull);
                continue;
            }
            let n = avail.min(want - done);
            for i in done..done + n {
                let (yl, yr) = self.output();
                out_l[i] = yl;
                out_r[i] = yr;
                self.frac += self.m;
                self.pos += (self.frac / self.l) as usize;
                self.frac %= self.l;
            }
            done += n;
            self.produced += n as u64;
        }
    }

    fn output(&self) -> (f32, f32) {
        let pnum = self.frac * PHASES as u64;
        let p = (pnum / self.l) as usize;
        let w = (pnum % self.l) as f32 / self.l as f32;
        let a = &self.table[p * self.taps..(p + 1) * self.taps];
        let b = &self.table[(p + 1) * self.taps..(p + 2) * self.taps];
        let start = self.pos + 1 - self.half;
        let xl = &self.hist_l[start..start + self.taps];
        let xr = &self.hist_r[start..start + self.taps];
        let (mut yl, mut yr) = (0f32, 0f32);
        for k in 0..self.taps {
            let c = a[k] + w * (b[k] - a[k]);
            yl += c * xl[k];
            yr += c * xr[k];
        }
        (yl, yr)
    }

    /// Pull the input needed for the next `remaining` outputs (at most one chunk).
    fn refill<F>(&mut self, remaining: usize, pull: &mut F)
    where
        F: FnMut(&mut [f32], &mut [f32]),
    {
        // Drop history no future output reads (everything before pos + 1 - half).
        let drop = self.pos + 1 - self.half;
        if drop > 0 {
            self.hist_l.copy_within(drop..self.len, 0);
            self.hist_r.copy_within(drop..self.len, 0);
            self.len -= drop;
            self.pos -= drop;
        }
        let last_pos = self.pos as u64 + (self.frac + (remaining as u64 - 1) * self.m) / self.l;
        let needed = (last_pos + self.half as u64 + 1).saturating_sub(self.len as u64);
        let space = self.hist_l.len() - self.len;
        let n = (needed.min(self.chunk as u64) as usize).min(space).max(1);
        let (l, r) = (
            &mut self.hist_l[self.len..self.len + n],
            &mut self.hist_r[self.len..self.len + n],
        );
        l.fill(0.0);
        r.fill(0.0);
        pull(l, r);
        self.len += n;
        self.consumed += n as u64;
    }
}

#[cfg(test)]
#[path = "resample_tests.rs"]
mod tests;
