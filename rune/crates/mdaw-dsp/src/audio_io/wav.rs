//! Exact port of `src/utils/wavEncoder.ts` (`audioBufferToWav`).

use crate::buffer::AudioBuffer;

/// WAV header size in bytes (canonical 44-byte RIFF/`fmt `/`data` header, no extra chunks).
pub const WAV_HEADER_SIZE: usize = 44;

/// `audioBufferToWav(buffer)`: RIFF PCM 16-bit little-endian, channels interleaved, 44-byte
/// header. Each sample is clamped to ±1 and scaled asymmetrically (negative × 0x8000, positive
/// × 0x7fff), truncated toward zero (`DataView.setInt16` ToInt16), no dither. NaN → 0.
///
/// Header fields are written with JS `setUint32`/`setUint16` semantics (modulo 2^32 / 2^16).
pub fn encode_wav(buffer: &AudioBuffer) -> Vec<u8> {
    let num_channels = buffer.channel_count();
    let sample_rate = buffer.sample_rate;
    let bits_per_sample: u32 = 16;
    let bytes_per_sample = (bits_per_sample / 8) as usize;
    let block_align = num_channels * bytes_per_sample;
    let num_frames = buffer.frames();
    let data_size = num_frames * block_align;

    let mut out = Vec::with_capacity(WAV_HEADER_SIZE + data_size);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36u32.wrapping_add(data_size as u32)).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(num_channels as u16).to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&sample_rate.wrapping_mul(block_align as u32).to_le_bytes());
    out.extend_from_slice(&(block_align as u16).to_le_bytes());
    out.extend_from_slice(&(bits_per_sample as u16).to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_size as u32).to_le_bytes());

    for i in 0..num_frames {
        for ch in &buffer.channels {
            out.extend_from_slice(&sample_to_i16(ch[i]).to_le_bytes());
        }
    }
    out
}

/// `Math.max(-1, Math.min(1, x))` then `x < 0 ? x * 0x8000 : x * 0x7fff`, stored via ToInt16.
fn sample_to_i16(x: f32) -> i16 {
    let x = f64::from(x);
    if x.is_nan() {
        return 0;
    }
    let s = x.clamp(-1.0, 1.0);
    let v = if s < 0.0 { s * 32768.0 } else { s * 32767.0 };
    // In range [-32768, 32767]: ToInt16 is truncation toward zero.
    v.trunc() as i16
}
