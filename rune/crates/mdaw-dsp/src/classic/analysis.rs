//! Port of `src/audio/sampler/classic/analysis.ts`: offline sanity metrics for rendered drum
//! voices (peak, length, spectral centroid, low/high energy split).

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoiceAnalysis {
    pub peak_db: f64,
    pub length_sec: f64,
    /// Energy-weighted mean frequency, Hz.
    pub centroid_hz: f64,
    /// Fraction of spectral energy below 150 Hz.
    pub low_frac: f64,
    /// Fraction of spectral energy above 5 kHz.
    pub high_frac: f64,
    /// Frequency of the strongest FFT bin, Hz.
    pub peak_hz: f64,
}

/// In-place iterative radix-2 FFT (re/im, length = power of two).
fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = (-2.0 * std::f64::consts::PI) / len as f64;
        let (wr, wi) = (ang.cos(), ang.sin());
        for i in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f64, 0.0f64);
            for k in 0..len / 2 {
                let a = i + k;
                let b = a + len / 2;
                let xr = re[b] * cr - im[b] * ci;
                let xi = re[b] * ci + im[b] * cr;
                re[b] = re[a] - xr;
                im[b] = im[a] - xi;
                re[a] += xr;
                im[a] += xi;
                let t = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = t;
            }
        }
        len <<= 1;
    }
}

pub fn analyze_voice(data: &[f32], sample_rate: f64) -> VoiceAnalysis {
    let peak = data.iter().fold(0.0f64, |p, &v| p.max((v as f64).abs()));
    // Whole sound (zero-padded), capped at 2^18 samples (~6 s at 44.1 kHz).
    let mut size = 1usize;
    while size < data.len() && size < 1 << 18 {
        size <<= 1;
    }
    let mut re = vec![0.0f64; size];
    let mut im = vec![0.0f64; size];
    for (r, &v) in re.iter_mut().zip(data) {
        *r = v as f64;
    }
    fft(&mut re, &mut im);
    let (mut total, mut low, mut high, mut weighted, mut best, mut best_bin) =
        (0.0, 0.0, 0.0, 0.0, 0.0, 0usize);
    for k in 1..size / 2 {
        let f = (k as f64 * sample_rate) / size as f64;
        let e = re[k] * re[k] + im[k] * im[k];
        total += e;
        weighted += e * f;
        if f < 150.0 {
            low += e;
        }
        if f > 5000.0 {
            high += e;
        }
        if e > best {
            best = e;
            best_bin = k;
        }
    }
    let t = if total == 0.0 { 1.0 } else { total };
    VoiceAnalysis {
        peak_db: if peak > 0.0 {
            20.0 * peak.log10()
        } else {
            f64::NEG_INFINITY
        },
        length_sec: data.len() as f64 / sample_rate,
        centroid_hz: weighted / t,
        low_frac: low / t,
        high_frac: high / t,
        peak_hz: (best_bin as f64 * sample_rate) / size as f64,
    }
}
