//! Audio file I/O: WAV encoding (exact port of `src/utils/wavEncoder.ts`), decoding of the
//! formats the app accepts (wav / aif / aiff / flac / mp3 / ogg / m4a), Chromium-compatible
//! resampling, and waveform peaks.
//!
//! Parity evidence: `rune/parity/fixtures/wav.json` (byte-identical WAV output) and
//! `rune/parity/fixtures/decode_reference.json` (Chromium `decodeAudioData` captures of
//! `testdata/`, see `rune/parity/chromium/capture-decode.mjs`).

mod decode;
mod mp4;
mod ogg;
mod peaks;
mod resample;
mod wav;

#[cfg(test)]
mod tests;

pub use decode::{
    DecodeError, SUPPORTED_EXTENSIONS, decode_bytes, decode_file, decode_file_at_rate,
};
pub use mp4::{Mp4Trim, mp4_trim};
pub use ogg::ogg_final_granule;
pub use peaks::{abs_peaks, min_max_peaks};
pub use resample::{ResampleQuality, resample, resample_with, resampled_len};
pub use wav::{WAV_HEADER_SIZE, encode_wav};
