//! `Tone.Noise`: Tone's cached stereo noise buffers (2 × 220500 frames at the context rate, drawn
//! from `Math.random` once per page) played by a looping `AudioBufferSourceNode` from a random
//! offset `Math.random()·(duration − 0.001)` on every start.
//!
//! Buffers (`Noise.ts`): white `r·2 − 1`; pink = Paul Kellet's filter (f64 state, stored f32,
//! then `·0.11` on the stored value); brown `(last + 0.02·white) / 1.02` stored f32 (the stored
//! value is the next `last`), then `·3.5`. Rust draws them from mulberry32 seeds (the Chromium
//! capture installs the same generator as `Math.random`).
//!
//! Playback (Chromium `AudioBufferSourceHandler`, rate 1, loop over the whole buffer): the first
//! frame is `ceil(round(t·sr·1024)/1024)`, the read position starts at `round(offset·sr)` (the
//! playbackRate intrinsic value is exactly 1) plus the sub-sample start `start_frame − t·sr`, then
//! advances 1 frame per frame; a fractional position is read with linear interpolation (f64).
//! A restart (`Noise._restart`) switches to a new random offset at its start frame.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use mdaw_dsp::factory::Mulberry32;
use serde::{Deserialize, Serialize};

use super::osc::start_frame;

/// Frames per channel of Tone's noise buffers.
pub const NOISE_FRAMES: usize = 44100 * 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NoiseType {
    White,
    Pink,
    Brown,
}

/// mulberry32 seeds of the three buffers (what the Chromium capture used).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NoiseSeeds {
    pub white: u32,
    pub pink: u32,
    pub brown: u32,
}

impl Default for NoiseSeeds {
    fn default() -> Self {
        Self {
            white: 0x5eed_2001,
            pink: 0x5eed_2002,
            brown: 0x5eed_2003,
        }
    }
}

pub type NoiseBuffer = Arc<[Vec<f32>; 2]>;

fn draw(kind: NoiseType, seed: u32) -> [Vec<f32>; 2] {
    let mut rng = Mulberry32::new(seed);
    let mut channel = || {
        let mut out = vec![0f32; NOISE_FRAMES];
        match kind {
            NoiseType::White => {
                for x in &mut out {
                    *x = (rng.next_f64() * 2.0 - 1.0) as f32;
                }
            }
            NoiseType::Pink => {
                let (mut b0, mut b1, mut b2, mut b3, mut b4, mut b5, mut b6) =
                    (0f64, 0f64, 0f64, 0f64, 0f64, 0f64, 0f64);
                for x in &mut out {
                    let white = rng.next_f64() * 2.0 - 1.0;
                    b0 = 0.99886 * b0 + white * 0.0555179;
                    b1 = 0.99332 * b1 + white * 0.0750759;
                    b2 = 0.969 * b2 + white * 0.153852;
                    b3 = 0.8665 * b3 + white * 0.3104856;
                    b4 = 0.55 * b4 + white * 0.5329522;
                    b5 = -0.7616 * b5 - white * 0.016898;
                    let v = (b0 + b1 + b2 + b3 + b4 + b5 + b6 + white * 0.5362) as f32;
                    *x = (v as f64 * 0.11) as f32;
                    b6 = white * 0.115926;
                }
            }
            NoiseType::Brown => {
                let mut last = 0f64;
                for x in &mut out {
                    let white = rng.next_f64() * 2.0 - 1.0;
                    let v = ((last + 0.02 * white) / 1.02) as f32;
                    last = v as f64;
                    *x = (v as f64 * 3.5) as f32;
                }
            }
        }
        out
    };
    let l = channel();
    let r = channel();
    [l, r]
}

/// The shared buffer of `kind` drawn from `seed` (built once per process; control side).
pub fn noise_buffer(kind: NoiseType, seed: u32) -> NoiseBuffer {
    type Cache = Mutex<HashMap<(NoiseType, u32), NoiseBuffer>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(b) = cache
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&(kind, seed))
    {
        return b.clone();
    }
    let b: NoiseBuffer = Arc::new(draw(kind, seed));
    cache
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .entry((kind, seed))
        .or_insert(b)
        .clone()
}

const MAX_PENDING: usize = 8;

/// The noise source's native playback (successive buffer sources share it: a new one only plays
/// once the previous one's gate is 0).
#[derive(Debug, Clone)]
pub(crate) struct NoisePlayer {
    buffer: NoiseBuffer,
    /// Read position (buffer frames) of the current source, None before the first start.
    pos: Option<f64>,
    pending: [(u64, f64); MAX_PENDING],
    n_pending: usize,
}

impl NoisePlayer {
    pub fn new(buffer: NoiseBuffer) -> Self {
        Self {
            buffer,
            pos: None,
            pending: [(0, 0.0); MAX_PENDING],
            n_pending: 0,
        }
    }

    pub fn set_buffer(&mut self, buffer: NoiseBuffer) {
        self.buffer = buffer;
    }

    /// A new buffer source at `time` from the random draw `r` (`Noise._start`).
    pub fn start(&mut self, time: f64, r: f64, sr: f64) {
        let duration = NOISE_FRAMES as f64 / sr;
        let offset = r * (duration - 0.001);
        let offset_frames = ((offset * sr).round() as usize % NOISE_FRAMES) as f64;
        let (frame, _) = start_frame(time, sr);
        let pos = offset_frames + (frame as f64 - time * sr).abs();
        if self.n_pending == MAX_PENDING {
            self.pending.copy_within(1.., 0);
            self.n_pending -= 1;
        }
        let at = self.pending[..self.n_pending].partition_point(|p| p.0 <= frame);
        self.pending.copy_within(at..self.n_pending, at + 1);
        self.pending[at] = (frame, pos);
        self.n_pending += 1;
    }

    /// A new buffer source replacing this one (`restart` of a node instance).
    pub fn restart(&mut self, time: f64, r: f64, sr: f64) {
        self.pos = None;
        self.n_pending = 0;
        self.start(time, r, sr);
    }

    /// Render both channels (0 before the first start).
    pub fn render(&mut self, frame: u64, l: &mut [f32], r: &mut [f32]) {
        let n = l.len();
        let len = NOISE_FRAMES as f64;
        let [bl, br] = &*self.buffer;
        for k in 0..n {
            while self.n_pending > 0 && self.pending[0].0 <= frame + k as u64 {
                self.pos = Some(self.pending[0].1);
                self.pending.copy_within(1..self.n_pending, 0);
                self.n_pending -= 1;
            }
            match self.pos.as_mut() {
                None => {
                    l[k] = 0.0;
                    r[k] = 0.0;
                }
                Some(pos) => {
                    if *pos >= len {
                        *pos -= len;
                    }
                    let i = *pos as usize;
                    let f = *pos - i as f64;
                    let j = if i + 1 >= NOISE_FRAMES {
                        i + 1 - NOISE_FRAMES
                    } else {
                        i + 1
                    };
                    l[k] = ((1.0 - f) * bl[i] as f64 + f * bl[j] as f64) as f32;
                    r[k] = ((1.0 - f) * br[i] as f64 + f * br[j] as f64) as f32;
                    *pos += 1.0;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffers_are_deterministic_and_scaled() {
        let w = noise_buffer(NoiseType::White, 1);
        assert_eq!(w[0][0], (0.627_073_940_588_161_3f64 * 2.0 - 1.0) as f32);
        let p = noise_buffer(NoiseType::Pink, 2);
        let b = noise_buffer(NoiseType::Brown, 3);
        let rms = |x: &[f32]| {
            (x.iter().map(|&v| (v as f64).powi(2)).sum::<f64>() / x.len() as f64).sqrt()
        };
        assert!(rms(&p[0]) > 0.05 && rms(&p[0]) < 0.5, "{}", rms(&p[0]));
        assert!(rms(&b[1]) > 0.05 && rms(&b[1]) < 1.0, "{}", rms(&b[1]));
    }
}
