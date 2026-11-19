//! `Tone.Noise({ type: 'white' })`, made deterministic.
//!
//! Tone draws ONE cached white-noise buffer per page: 2 channels × 220500 frames
//! (`44100·5`, at the context sample rate), `Math.random()·2 − 1` stored as f32, channel 0 first.
//! Every `start()` plays it through a looping `AudioBufferSourceNode` from a random offset
//! `Math.random()·(duration − 0.001)` seconds, which Chromium snaps to the nearest frame (no
//! sub-sample interpolation at playback rate 1). The noise is stereo, so every noise path of the
//! kit is stereo and the 1-channel offline render down-mixes `0.5·(L + R)`.
//!
//! In Rust both draws come from mulberry32 (the same generator the Chromium capture installs as
//! `Math.random`), so a seeded render equals the seeded Chromium capture sample-for-sample.

/// mulberry32 PRNG returning `[0, 1)` like the JS replacement for `Math.random`.
#[derive(Debug, Clone)]
pub struct Mulberry32(u32);

impl Mulberry32 {
    pub fn new(seed: u32) -> Self {
        Self(seed)
    }

    pub fn next_f64(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x6d2b_79f5);
        let a = self.0;
        let mut t = (a ^ (a >> 15)).wrapping_mul(1 | a);
        t = (t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t))) ^ t;
        ((t ^ (t >> 14)) as f64) / 4_294_967_296.0
    }
}

/// Frames per channel of Tone's noise buffer.
pub const NOISE_BUFFER_FRAMES: usize = 44100 * 5;

/// Tone's cached stereo white-noise buffer.
#[derive(Debug, Clone)]
pub struct NoiseBuffer {
    pub channels: [Vec<f32>; 2],
}

impl NoiseBuffer {
    /// Draw the buffer from mulberry32(`seed`) exactly like `_noiseBuffers.white`.
    pub fn white(seed: u32) -> Self {
        let mut rng = Mulberry32::new(seed);
        let mut ch = || {
            (0..NOISE_BUFFER_FRAMES)
                .map(|_| (rng.next_f64() * 2.0 - 1.0) as f32)
                .collect::<Vec<f32>>()
        };
        let l = ch();
        let r = ch();
        Self { channels: [l, r] }
    }

    /// Start frame for a `Math.random()` draw `r` at `sample_rate` (Tone `Noise._start`).
    pub fn offset_frames(r: f64, sample_rate: u32) -> usize {
        let sr = sample_rate as f64;
        let duration = NOISE_BUFFER_FRAMES as f64 / sr;
        let offset = r * (duration - 0.001);
        ((offset * sr).round() as usize) % NOISE_BUFFER_FRAMES
    }

    /// `frames` frames of both channels, looping, starting at `offset`.
    pub fn play(&self, offset: usize, frames: usize) -> [Vec<f32>; 2] {
        let take = |c: &Vec<f32>| {
            (0..frames)
                .map(|i| c[(offset + i) % NOISE_BUFFER_FRAMES])
                .collect::<Vec<f32>>()
        };
        [take(&self.channels[0]), take(&self.channels[1])]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mulberry32_matches_js() {
        // mulberry32(1) first outputs (computed with the JS reference in bufgen.ts).
        let mut r = Mulberry32::new(1);
        let a = r.next_f64();
        assert_eq!(a, 0.627_073_940_588_161_3);
        assert_eq!(r.next_f64(), 0.002_735_721_180_215_478);
    }

    #[test]
    fn buffer_is_white() {
        let b = NoiseBuffer::white(7);
        let mean: f64 =
            b.channels[0].iter().map(|&x| x as f64).sum::<f64>() / NOISE_BUFFER_FRAMES as f64;
        let var: f64 = b.channels[1]
            .iter()
            .map(|&x| (x as f64).powi(2))
            .sum::<f64>()
            / NOISE_BUFFER_FRAMES as f64;
        assert!(mean.abs() < 0.01);
        assert!((var - 1.0 / 3.0).abs() < 0.01);
        assert!(NoiseBuffer::offset_frames(0.999_999, 48000) < NOISE_BUFFER_FRAMES);
    }
}
