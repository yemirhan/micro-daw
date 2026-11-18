//! Chromium `DelayNode` kernel (blink `delay.cc`, `cpu/arm/delay_neon.cc` / `cpu/x86/delay_sse2.cc`):
//! the input block is written into a ring buffer of `128 + ceil(maxDelay·sr)` frames first, then
//! every frame reads `delay·sr` frames back with linear interpolation. The a-rate path is Chromium's
//! vector one (every quantum is a multiple of 4 frames), all in f32: `desired = delay·sr`,
//! `pos = f32(w) + (f32(len) − desired)` (wrapped), `i1 = trunc(pos)`, `f = pos − i1`,
//! `out = s1 + f·(s2 − s1)`. Writing the whole block before reading is what makes a DelayNode
//! inside a feedback cycle work: the cycle's other nodes see the delay's PREVIOUS render quantum
//! (Chromium pulls a node already being processed from its stale output bus), so a feedback loop
//! around a delay of D frames repeats every D + 128 frames (measured: echoes at 480, 1088, 1696…
//! for D = 480 at 48 kHz).

use super::Q;

/// One channel of delay memory.
#[derive(Clone)]
pub struct DelayLine {
    buf: Box<[f32]>,
    w: usize,
    max_delay: f64,
}

impl DelayLine {
    /// Room for `max_delay` seconds at `sr` (control side).
    pub fn new(max_delay: f64, sr: f64) -> Self {
        let len = Q + (max_delay * sr).ceil() as usize;
        Self {
            buf: vec![0.0; len].into_boxed_slice(),
            w: 0,
            max_delay,
        }
    }

    pub fn reset(&mut self) {
        self.buf.fill(0.0);
        self.w = 0;
    }

    /// Delay `io` in place by `delay[k]` seconds per frame (the param's f32 values), on Chromium's
    /// a-rate path (`a_rate`: the param has an input connection or values that change in this
    /// quantum, `HasSampleAccurateValues`), else its k-rate path (`delay[0]` for the block, the
    /// position in f64).
    pub fn process(&mut self, io: &mut [f32; Q], delay: &[f32; Q], sr: f64, a_rate: bool) {
        let len = self.buf.len();
        let w0 = self.w;
        for (k, &x) in io.iter().enumerate() {
            let i = w0 + k;
            self.buf[if i >= len { i - len } else { i }] = x;
        }
        if a_rate {
            let lenf = len as f32;
            let srf = sr as f32;
            for k in 0..Q {
                // `max(delay, 0)` (the param's nominal range clamps the top)
                let d = delay[k].max(0.0).min(self.max_delay as f32);
                let wi = w0 + k;
                let wi = if wi >= len { wi - len } else { wi };
                let mut pos = wi as f32 + (lenf - d * srf);
                if pos >= lenf {
                    pos -= lenf;
                }
                let mut i1 = pos as usize;
                if i1 >= len {
                    i1 -= len;
                }
                let i2 = if i1 + 1 >= len { i1 + 1 - len } else { i1 + 1 };
                let f = pos - i1 as f32;
                let (s1, s2) = (self.buf[i1], self.buf[i2]);
                io[k] = s1 + f * (s2 - s1);
            }
        } else {
            let d = (delay[0] as f64).clamp(0.0, self.max_delay);
            let mut pos = w0 as f64 + len as f64 - d * (sr as f32) as f64;
            if pos >= len as f64 {
                pos -= len as f64;
            }
            let i1 = pos as usize;
            let f = (pos - i1 as f64) as f32;
            for (k, y) in io.iter_mut().enumerate() {
                let a = i1 + k;
                let a = if a >= len { a - len } else { a };
                let s1 = self.buf[a];
                if f == 0.0 {
                    *y = s1;
                } else {
                    let b = if a + 1 >= len { a + 1 - len } else { a + 1 };
                    *y = s1 + f * (self.buf[b] - s1);
                }
            }
        }
        self.w = (w0 + Q) % len;
    }

    /// Same with one delay for the whole quantum.
    pub fn process_const(&mut self, io: &mut [f32; Q], delay: f32, sr: f64) {
        self.process(io, &[delay; Q], sr, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_and_fractional_delays() {
        let sr = 48000.0;
        let mut d = DelayLine::new(1.0, sr);
        let mut io = [0f32; Q];
        io[0] = 1.0;
        d.process_const(&mut io, 10.0 / sr as f32, sr);
        assert!((io[10] - 1.0).abs() < 1e-4 && io[9].abs() < 1e-4);
        // 300 frames: lands in the third quantum
        let mut d = DelayLine::new(1.0, sr);
        let mut seen = None;
        for q in 0..4 {
            let mut io = [0f32; Q];
            if q == 0 {
                io[5] = 1.0;
            }
            d.process_const(&mut io, (300.5 / sr) as f32, sr);
            for (k, v) in io.iter().enumerate() {
                if *v > 0.4 && seen.is_none() {
                    seen = Some(q * Q + k);
                }
            }
        }
        assert_eq!(seen, Some(305));
    }
}
