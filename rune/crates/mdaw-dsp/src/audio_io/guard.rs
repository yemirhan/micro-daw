//! Defensive limits around symphonia (0.5.5 panics, overflows or allocates gigabytes on some
//! corrupt inputs: a zero WAV/AIFF sample rate builds a `TimeBase` with denominator 0, isomp4
//! unwraps missing `esds` fields, header-declared packet sizes reach `Vec::with_capacity`, …).
//!
//! - [`validate_header`] rejects WAV `fmt ` / AIFF `COMM` headers that would reach those panics,
//!   before symphonia sees the bytes.
//! - [`check_params`] applies Chromium's own limits to the probed track (rate 3000…768000 Hz,
//!   1…32 channels — `decodeAudioData` rejects the rest, see `decode_reference.json` `edge`) and
//!   bounds the per-packet allocation.
//! - [`FrameBudget`] bounds the total decoded frames and packets by the file size, the declared
//!   duration and an absolute cap.

use super::decode::DecodeError;

/// Lowest / highest file sample rate Chromium decodes (`media::limits::kMin/kMaxSampleRate`;
/// 2000 Hz and 800000 Hz WAVs are rejected, 3000 and 768000 Hz decode).
pub const MIN_SAMPLE_RATE: u32 = 3000;
pub const MAX_SAMPLE_RATE: u32 = 768_000;
/// Most channels Chromium decodes (`media::limits::kMaxChannels`; a 33-channel WAV is rejected).
pub const MAX_CHANNELS: usize = 32;
/// Absolute cap on decoded frames: 30 minutes at 192 kHz.
pub const MAX_FRAMES: u64 = 30 * 60 * 192_000;
/// Largest packet (frames) any supported codec produces: FLAC's 65535-frame blocks; ALAC's
/// default is 4096. symphonia allocates `max_frames_per_packet × channels` up front.
pub const MAX_FRAMES_PER_PACKET: u64 = 1 << 16;
/// Frames allowed per input byte when the declared duration does not justify more. Real files
/// stay far below (PCM ≤ 1, MP3/AAC/Vorbis ≈ 2–20, FLAC of digital silence ≈ 300).
pub const FRAMES_PER_BYTE: u64 = 1024;

fn malformed(m: &str) -> DecodeError {
    DecodeError::Malformed(m.to_owned())
}

fn le_u16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn le_u32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn be_u16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn be_u32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// IEEE 754 80-bit extended (AIFF `sampleRate`) → f64; `None` for NaN/∞ encodings.
fn extended_to_f64(b: &[u8]) -> Option<f64> {
    let b: &[u8; 10] = b.try_into().ok()?;
    let sign = if b[0] & 0x80 != 0 { -1.0 } else { 1.0 };
    let exp = i32::from(u16::from_be_bytes([b[0] & 0x7f, b[1]]));
    let mant = u64::from_be_bytes(b[2..10].try_into().ok()?);
    if exp == 0x7fff {
        return None;
    }
    if exp == 0 && mant == 0 {
        return Some(0.0);
    }
    Some(sign * mant as f64 * 2f64.powi(exp - 16383 - 63))
}

fn check_rate_channels(rate: f64, channels: i64) -> Result<(), DecodeError> {
    if channels <= 0 {
        return Err(malformed("zero channels"));
    }
    if channels as usize > MAX_CHANNELS {
        return Err(DecodeError::Unsupported(format!(
            "{channels} channels (max {MAX_CHANNELS})"
        )));
    }
    if !(rate >= f64::from(MIN_SAMPLE_RATE) && rate <= f64::from(MAX_SAMPLE_RATE)) {
        return Err(DecodeError::Unsupported(format!(
            "sample rate {rate} Hz outside {MIN_SAMPLE_RATE}..={MAX_SAMPLE_RATE}"
        )));
    }
    Ok(())
}

/// Iterate `(id, body)` of RIFF (little-endian sizes, even padding) or IFF (big-endian) chunks.
fn chunks(data: &[u8], big_endian: bool) -> impl Iterator<Item = (&[u8], &[u8])> {
    let mut at = 0usize;
    std::iter::from_fn(move || {
        let id = data.get(at..at + 4)?;
        let size = if big_endian {
            be_u32(data, at + 4)?
        } else {
            le_u32(data, at + 4)?
        } as usize;
        let start = at + 8;
        let body = &data[start.min(data.len())..start.saturating_add(size).min(data.len())];
        at = start.saturating_add(size).saturating_add(size & 1);
        Some((id, body))
    })
}

/// Reject WAV / AIFF headers symphonia cannot survive (zero rate → `TimeBase::new(_, 0)` panic,
/// zero channels / block align / sample size → divisions by zero) and those Chromium refuses.
pub fn validate_header(bytes: &[u8]) -> Result<(), DecodeError> {
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE" {
        let fmt = chunks(&bytes[12..], false).find(|(id, _)| *id == b"fmt ");
        let Some((_, fmt)) = fmt else {
            return Err(malformed("WAV without fmt chunk"));
        };
        let (Some(channels), Some(rate), Some(align), Some(bits)) = (
            le_u16(fmt, 2),
            le_u32(fmt, 4),
            le_u16(fmt, 12),
            le_u16(fmt, 14),
        ) else {
            return Err(malformed("short WAV fmt chunk"));
        };
        check_rate_channels(f64::from(rate), i64::from(channels))?;
        if align == 0 || bits == 0 {
            return Err(malformed("WAV block align / bits per sample is zero"));
        }
    } else if bytes.len() >= 12
        && &bytes[0..4] == b"FORM"
        && matches!(&bytes[8..12], b"AIFF" | b"AIFC")
    {
        let comm = chunks(&bytes[12..], true).find(|(id, _)| *id == b"COMM");
        let Some((_, comm)) = comm else {
            return Err(malformed("AIFF without COMM chunk"));
        };
        let (Some(channels), Some(bits), Some(rate)) = (
            be_u16(comm, 0),
            be_u16(comm, 6),
            comm.get(8..18).and_then(extended_to_f64),
        ) else {
            return Err(malformed("short AIFF COMM chunk"));
        };
        check_rate_channels(rate, i64::from(channels as i16))?;
        if bits as i16 <= 0 || bits > 64 {
            return Err(malformed("AIFF sample size out of range"));
        }
    }
    Ok(())
}

/// Chromium's rate / channel limits and a bound on symphonia's per-packet allocation.
pub fn check_params(
    rate: Option<u32>,
    channels: Option<usize>,
    max_frames_per_packet: Option<u64>,
) -> Result<(), DecodeError> {
    let rate = rate.ok_or_else(|| malformed("no sample rate"))?;
    // Codecs that only know their channel layout after the first packet (AAC) report `None`.
    check_rate_channels(f64::from(rate), channels.map_or(1, |c| c as i64))?;
    if max_frames_per_packet.is_some_and(|m| m > MAX_FRAMES_PER_PACKET) {
        return Err(malformed("packet size out of range"));
    }
    Ok(())
}

/// ALAC sizes its buffers from the magic cookie's `frameLength` (first big-endian u32 of the
/// 24/48-byte `ALACSpecificConfig`) before decoding anything: bound it like other packets.
pub fn check_alac_cookie(cookie: &[u8]) -> Result<(), DecodeError> {
    match be_u32(cookie, 0) {
        Some(frames) if u64::from(frames) <= MAX_FRAMES_PER_PACKET => Ok(()),
        Some(_) => Err(malformed("ALAC frame length out of range")),
        None => Err(malformed("short ALAC cookie")),
    }
}

/// Bounds the decoding loop: total frames and packets.
#[derive(Debug, Clone, Copy)]
pub struct FrameBudget {
    pub max_frames: u64,
    pub max_packets: u64,
}

impl FrameBudget {
    /// `min(MAX_FRAMES, max(bytes · FRAMES_PER_BYTE, declared + 1/8 + 1 s))`; packets ≤ bytes + 64
    /// (every packet of the supported containers occupies at least one byte of the file).
    pub fn new(file_bytes: usize, declared_frames: Option<u64>, rate: u32) -> Self {
        let by_size = (file_bytes as u64).saturating_mul(FRAMES_PER_BYTE);
        let by_header = declared_frames
            .filter(|&d| d <= MAX_FRAMES)
            .map_or(0, |d| d + d / 8 + u64::from(rate));
        Self {
            max_frames: by_size.max(by_header).min(MAX_FRAMES),
            max_packets: (file_bytes as u64).saturating_add(64),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extended_rates() {
        // 44100 = 0x400E AC44000000000000
        let b = [0x40, 0x0e, 0xac, 0x44, 0, 0, 0, 0, 0, 0];
        assert_eq!(extended_to_f64(&b), Some(44100.0));
        assert_eq!(extended_to_f64(&[0; 10]), Some(0.0));
        assert_eq!(extended_to_f64(&[0x7f, 0xff, 0, 0, 0, 0, 0, 0, 0, 0]), None);
    }

    #[test]
    fn limits() {
        assert!(check_rate_channels(3000.0, 1).is_ok());
        assert!(check_rate_channels(768_000.0, 32).is_ok());
        assert!(check_rate_channels(2999.0, 1).is_err());
        assert!(check_rate_channels(768_001.0, 1).is_err());
        assert!(check_rate_channels(f64::NAN, 1).is_err());
        assert!(check_rate_channels(44100.0, 0).is_err());
        assert!(check_rate_channels(44100.0, 33).is_err());
        let b = FrameBudget::new(1000, None, 44100);
        assert_eq!((b.max_frames, b.max_packets), (1_024_000, 1064));
        let b = FrameBudget::new(1000, Some(8_000_000), 44100);
        assert_eq!(b.max_frames, 9_044_100);
        assert_eq!(
            FrameBudget::new(1000, Some(u64::MAX), 44100).max_frames,
            1_024_000
        );
        assert_eq!(
            FrameBudget::new(usize::MAX, None, 44100).max_frames,
            MAX_FRAMES
        );
    }
}
