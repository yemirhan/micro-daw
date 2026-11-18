//! `trimTail` of `src/audio/sampler/bufferCache.ts` (exact: f64 arithmetic, f32 stores).

/// Silence threshold (−60 dBFS).
pub const SILENCE: f32 = 0.001;
/// Peaks above this are scaled down to it.
pub const MAX_PEAK: f64 = 0.98;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrimOptions {
    pub pad_sec: f64,
    pub fade_sec: f64,
    pub min_sec: f64,
}

impl Default for TrimOptions {
    fn default() -> Self {
        Self {
            pad_sec: 0.01,
            fade_sec: 0.005,
            min_sec: 0.01,
        }
    }
}

/// JS `Math.round` for finite values.
fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// Cut after the last sample above −60 dBFS (+`pad_sec`), at least `min_sec`, fade the last
/// `fade_sec` linearly to zero and scale the peak down to 0.98 when it is above.
pub fn trim_tail(channels: &[Vec<f32>], sample_rate: u32, opts: TrimOptions) -> Vec<Vec<f32>> {
    let sr = sample_rate as f64;
    let length = channels.first().map_or(0, Vec::len);
    let mut last: i64 = -1;
    let mut peak = 0f32;
    for ch in channels {
        for (i, &x) in ch.iter().enumerate() {
            let a = x.abs();
            if a > peak {
                peak = a;
            }
            if a > SILENCE && i as i64 > last {
                last = i as i64;
            }
        }
    }
    let min_len = (length as f64).min(1f64.max(js_round(opts.min_sec * sr))) as usize;
    let end = (length as i64)
        .min((min_len as i64).max(last + 1 + js_round(opts.pad_sec * sr) as i64))
        as usize;
    let fade_len = end.min(js_round(opts.fade_sec * sr) as usize);
    let scale = if peak as f64 > MAX_PEAK {
        MAX_PEAK / peak as f64
    } else {
        1.0
    };
    channels
        .iter()
        .map(|ch| {
            let mut out = ch[..end].to_vec();
            if scale != 1.0 {
                for x in &mut out {
                    *x = (*x as f64 * scale) as f32;
                }
            }
            for i in 0..fade_len {
                let x = &mut out[end - 1 - i];
                *x = (*x as f64 * (i as f64 / fade_len as f64)) as f32;
            }
            out
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_fades_and_scales() {
        let mut x = vec![0f32; 1000];
        x[10] = 2.0;
        x[100] = 0.5;
        let out = trim_tail(&[x], 1000, TrimOptions::default());
        // last loud sample 100 → end = 101 + 10
        assert_eq!(out[0].len(), 111);
        assert_eq!(out[0][10], 0.98);
        assert_eq!(out[0][100], 0.245);
        assert_eq!(out[0][110], 0.0);
    }

    #[test]
    fn minimum_length() {
        let out = trim_tail(&[vec![0f32; 5000]], 48000, TrimOptions::default());
        assert_eq!(out[0].len(), 480);
        let out = trim_tail(&[vec![0f32; 3]], 48000, TrimOptions::default());
        assert_eq!(out[0].len(), 3);
    }
}
