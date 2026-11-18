//! Sample-rate conversion.
//!
//! The default ([`ResampleQuality::Chromium`]) is a port of Blink's `SincResampler`
//! (`third_party/blink/renderer/platform/audio/sinc_resampler.cc`), the resampler behind
//! `AudioBus::CreateBySampleRateConverting`, which `BaseAudioContext.decodeAudioData` uses to
//! convert a decoded file to the context rate: 32-tap Blackman-windowed sinc, 32 sub-sample kernel
//! offsets with linear interpolation between neighbouring kernels, cutoff `0.9 × min(1, dst/src)`
//! of Nyquist, zero-latency alignment (output frame `n` is centered on source position
//! `n × src/dst`), zeros beyond both ends, output length `floor(len × dst / src)` (truncated).
//! Measured against headless Chromium (`rune/parity/fixtures/decode_reference.json`,
//! `chromium_decode_parity`): 48k→44.1k, 96k→44.1k and 22.05k→44.1k conversions are
//! bit-identical to `decodeAudioData` (max |err| 0, same length). 1 kHz sine SNR: ~89 dB
//! (48k↔44.1k), ~105 dB (22.05k→44.1k), ~92 dB (96k→44.1k).
//!
//! [`ResampleQuality::High`] runs the same engine with a 128-tap kernel, 256 offsets and a 0.95
//! cutoff: lower aliasing and a flatter passband, for offline work where parity is not needed.

use crate::buffer::AudioBuffer;

/// Kernel configuration of the windowed-sinc resampler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResampleQuality {
    /// Blink `SincResampler` (32 taps, 32 offsets, cutoff 0.9): what `decodeAudioData` does.
    #[default]
    Chromium,
    /// 128 taps, 256 offsets, cutoff 0.95.
    High,
}

impl ResampleQuality {
    fn params(self) -> (usize, usize, f64) {
        match self {
            ResampleQuality::Chromium => (32, 32, 0.9),
            ResampleQuality::High => (128, 256, 0.95),
        }
    }
}

/// Number of output frames for `frames` input frames: Blink's
/// `static_cast<int>(source_length / sample_rate_ratio)`.
pub fn resampled_len(frames: usize, from_rate: u32, to_rate: u32) -> usize {
    let ratio = f64::from(from_rate) / f64::from(to_rate);
    (frames as f64 / ratio) as usize
}

/// Resample every channel to `target_rate` with the Chromium-compatible kernel. Returns a clone
/// when the rates already match (Blink copies the bus unchanged in that case).
pub fn resample(buffer: &AudioBuffer, target_rate: u32) -> AudioBuffer {
    resample_with(buffer, target_rate, ResampleQuality::Chromium)
}

/// [`resample`] with an explicit kernel quality.
pub fn resample_with(
    buffer: &AudioBuffer,
    target_rate: u32,
    quality: ResampleQuality,
) -> AudioBuffer {
    if buffer.sample_rate == target_rate || target_rate == 0 || buffer.sample_rate == 0 {
        return AudioBuffer {
            sample_rate: if target_rate == 0 {
                buffer.sample_rate
            } else {
                target_rate
            },
            ..buffer.clone()
        };
    }
    let ratio = f64::from(buffer.sample_rate) / f64::from(target_rate);
    let kernel = Kernel::new(ratio, quality);
    let channels = buffer
        .channels
        .iter()
        .map(|ch| kernel.process(ch))
        .collect();
    AudioBuffer {
        sample_rate: target_rate,
        channels,
    }
}

const BLOCK_SIZE: usize = 512;

struct Kernel {
    scale_factor: f64,
    size: usize,
    offsets: usize,
    /// `(offsets + 1)` kernels of `size` taps, sub-sample offset `i / offsets`.
    storage: Vec<f32>,
}

impl Kernel {
    /// `SincResampler::InitializeKernel`.
    fn new(scale_factor: f64, quality: ResampleQuality) -> Self {
        let (size, offsets, cutoff) = quality.params();
        // Blackman window.
        let alpha = 0.16;
        let a0 = 0.5 * (1.0 - alpha);
        let a1 = 0.5;
        let a2 = 0.5 * alpha;
        let mut sinc_scale_factor = if scale_factor > 1.0 {
            1.0 / scale_factor
        } else {
            1.0
        };
        sinc_scale_factor *= cutoff;
        let half = (size / 2) as f64;
        let mut storage = vec![0f32; size * (offsets + 1)];
        for offset_index in 0..=offsets {
            let subsample_offset = offset_index as f64 / offsets as f64;
            for i in 0..size {
                let s =
                    sinc_scale_factor * std::f64::consts::PI * (i as f64 - half - subsample_offset);
                let mut sinc = if s == 0.0 { 1.0 } else { s.sin() / s };
                sinc *= sinc_scale_factor;
                let x = (i as f64 - subsample_offset) / size as f64;
                let tau = 2.0 * std::f64::consts::PI;
                let window = a0 - a1 * (tau * x).cos() + a2 * (tau * 2.0 * x).cos();
                storage[i + offset_index * size] = (sinc * window) as f32;
            }
        }
        Kernel {
            scale_factor,
            size,
            offsets,
            storage,
        }
    }

    /// `SincResampler::Process(source, destination, numberOfSourceFrames)` on one channel.
    fn process(&self, source: &[f32]) -> Vec<f32> {
        let n_out = (source.len() as f64 / self.scale_factor) as usize;
        let mut out = Vec::with_capacity(n_out);
        let k = self.size;
        let half = k / 2;
        // Input buffer layout (see sinc_resampler.cc): [r1 .. r0=r2 .. r3 .. r4 .. end].
        let mut input = vec![0f32; BLOCK_SIZE + k];
        let (r0, r1, r2, r3, r4, r5) = (half, 0, half, BLOCK_SIZE, half + BLOCK_SIZE, k);
        let mut read = 0usize;
        let mut consume = |dst: &mut [f32]| {
            let n = dst.len().min(source.len().saturating_sub(read));
            dst[..n].copy_from_slice(&source[read..read + n]);
            dst[n..].fill(0.0);
            read += n;
        };
        // Step (1): prime the input buffer.
        consume(&mut input[r0..r0 + BLOCK_SIZE + half]);
        let mut virtual_source_index = 0.0f64;
        while out.len() < n_out {
            // Step (2): produce output while the kernel fits in the buffered input.
            while virtual_source_index < BLOCK_SIZE as f64 {
                let source_index_i = virtual_source_index as usize;
                let subsample_remainder = virtual_source_index - source_index_i as f64;
                let virtual_offset_index = subsample_remainder * self.offsets as f64;
                let offset_index = virtual_offset_index as usize;
                let k1 = &self.storage[offset_index * k..offset_index * k + k];
                let k2 = &self.storage[(offset_index + 1) * k..(offset_index + 2) * k];
                let input_p = &input[r1 + source_index_i..r1 + source_index_i + k];
                let mut sum1 = 0f32;
                let mut sum2 = 0f32;
                for ((&x, &a), &b) in input_p.iter().zip(k1).zip(k2) {
                    sum1 += x * a;
                    sum2 += x * b;
                }
                let f = virtual_offset_index - offset_index as f64;
                let result = (1.0 - f) * f64::from(sum1) + f * f64::from(sum2);
                out.push(result as f32);
                virtual_source_index += self.scale_factor;
                if out.len() == n_out {
                    return out;
                }
            }
            // Wrap back around to the start.
            virtual_source_index -= BLOCK_SIZE as f64;
            // Step (3): copy r3 to r1 and r4 to r2.
            input.copy_within(r3..r3 + half, r1);
            input.copy_within(r4..r4 + half, r2);
            // Step (4): refresh the buffer with more input.
            consume(&mut input[r5..r5 + BLOCK_SIZE]);
        }
        out
    }
}
