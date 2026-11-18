//! Audio file decoding (symphonia) to a planar f32 [`AudioBuffer`] at the file's own rate.
//!
//! Formats: the extensions the Electron app accepts (`wav aif aiff mp3 flac ogg m4a`).
//! - WAV: 8-bit unsigned, 16/24/32-bit int, 32/64-bit float, WAVE_FORMAT_EXTENSIBLE.
//! - AIFF / AIFF-C (`NONE`, `sowt`, `twos`, `fl32`, `fl64`).
//! - FLAC, MP3, Ogg Vorbis, M4A (AAC-LC, ALAC).
//!
//! Chromium parity (see `rune/parity/fixtures/decode_reference.json`):
//! - Integer PCM is converted like Chromium (`media::FixedSampleTypeTraits`): u8 and s16
//!   (incl. 16-bit FLAC) are asymmetric (negative / 128 or 32768, positive / 127 or 32767);
//!   24/32-bit are / 2^31. WAV, FLAC and MP3 decode bit-identically to Chromium.
//! - Encoder delay: MP3 decodes gaplessly (symphonia `enable_gapless`: LAME/Xing encoder delay +
//!   529-frame decoder delay trimmed at the start, LAME padding at the end). For M4A, symphonia
//!   ignores edit lists, so [`super::mp4`] reads `iTunSMPB` / `elst` and trims priming and padding
//!   here like Chromium (FFmpeg) does (same length, max |err| ~4e-7). Ogg Vorbis is cut at the
//!   final granule ([`super::ogg`]); symphonia does not do it for short streams.
//!   Every channel of every file is compared (`chromium_tests.rs`). Vorbis decodes through the
//!   vendored, patched `rune/vendor/symphonia-codec-vorbis` (FFmpeg's inverse coupling for a
//!   +0.0 magnitude; no ±1 clamp — Chromium keeps overs).
//! - Fragmented MP4 (`mvex`, `mdhd` duration 0): the length is the sum of the `trun` durations.
//! - Robustness: see [`decode_vec`] (header pre-validation, `catch_unwind` boundary, frame and
//!   packet budget, Chromium's rate/channel limits). symphonia-metadata and
//!   symphonia-format-isomp4 are vendored with bounded allocations (`rune/vendor`). The
//!   `catch_unwind` boundary needs `panic = "unwind"` (the default; never set `panic = "abort"`).
//! - Deviations: Chromium cannot decode AIFF, ALAC or 64-bit float WAV (Rust can). For a Vorbis
//!   stream whose audio fits in a single Ogg page Chromium ends early (FFmpeg demuxer quirk);
//!   Rust ends at the granule. A corrupt MP3 frame: FFmpeg conceals a few frames before it stops,
//!   Rust stops at the frame (its output is a bit-exact prefix of Chromium's). Ogg Opus is not
//!   supported by symphonia (Chromium decodes it).
//!
//! [`decode_file_at_rate`] adds the context-rate conversion `decodeAudioData` performs.

use std::fmt;
use std::io::Cursor;
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;
use std::sync::Arc;

use symphonia::core::audio::{AudioBuffer as SymBuffer, AudioBufferRef, Signal};
use symphonia::core::codecs::{CODEC_TYPE_ALAC, CODEC_TYPE_FLAC, CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use super::guard;
use super::mp4::{mp4_trim, repair_truncated_tail};
use super::ogg::{drop_damaged_pages, ogg_final_granule, ogg_truncated};
use super::resample::resample;
use crate::buffer::AudioBuffer;

/// File extensions the app's sample browser accepts (`src/main` fs filter), lower case.
pub const SUPPORTED_EXTENSIONS: [&str; 7] = ["wav", "aif", "aiff", "mp3", "flac", "ogg", "m4a"];

/// Decoding failure (≙ `decodeAudioData` rejecting with "Unable to decode audio data").
#[derive(Debug)]
pub enum DecodeError {
    Io(std::io::Error),
    /// Container or codec not recognised / not supported.
    Unsupported(String),
    /// The container has no decodable audio track.
    NoAudioTrack,
    /// Corrupt or truncated stream.
    Malformed(String),
    /// Decoded to zero frames.
    Empty,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::Io(e) => write!(f, "i/o error: {e}"),
            DecodeError::Unsupported(m) => write!(f, "unsupported audio: {m}"),
            DecodeError::NoAudioTrack => write!(f, "no audio track"),
            DecodeError::Malformed(m) => write!(f, "malformed audio: {m}"),
            DecodeError::Empty => write!(f, "decoded audio is empty"),
        }
    }
}

impl std::error::Error for DecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DecodeError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for DecodeError {
    fn from(e: std::io::Error) -> Self {
        DecodeError::Io(e)
    }
}

impl From<SymError> for DecodeError {
    fn from(e: SymError) -> Self {
        match e {
            SymError::IoError(e) => DecodeError::Io(e),
            SymError::Unsupported(m) => DecodeError::Unsupported(m.to_string()),
            SymError::DecodeError(m) => DecodeError::Malformed(m.to_string()),
            other => DecodeError::Malformed(other.to_string()),
        }
    }
}

/// Decode a file at its own sample rate. The extension is used as a probe hint only; the
/// container is detected from the content. The file is read once and not copied.
pub fn decode_file(path: impl AsRef<Path>) -> Result<AudioBuffer, DecodeError> {
    let path = path.as_ref();
    let bytes = std::fs::read(path)?;
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    decode_vec(bytes, ext)
}

/// Decode a file and convert it to `context_rate` the way `decodeAudioData` does (Blink
/// `SincResampler`, truncated length) — what the Electron app's `SampleManager.loadFromPath`
/// stores.
pub fn decode_file_at_rate(
    path: impl AsRef<Path>,
    context_rate: u32,
) -> Result<AudioBuffer, DecodeError> {
    Ok(resample(&decode_file(path)?, context_rate))
}

/// Decode in-memory file bytes. `ext_hint` is a file extension without the dot (may be empty).
/// Copies `bytes` once (symphonia needs an owned source); use [`decode_vec`] to avoid that.
pub fn decode_bytes(bytes: &[u8], ext_hint: &str) -> Result<AudioBuffer, DecodeError> {
    decode_vec(bytes.to_vec(), ext_hint)
}

/// [`decode_bytes`] taking ownership of the bytes (no copy).
///
/// Never panics and never allocates without bound on corrupt input: headers symphonia cannot
/// survive are rejected first ([`guard::validate_header`]), symphonia runs inside a
/// `catch_unwind` boundary (a panic becomes [`DecodeError::Malformed`]), and the decoded length
/// is bounded by [`guard::FrameBudget`] (file size, declared duration, 30 min at 192 kHz).
///
/// Damaged streams follow Chromium (`media::AudioFileReader::Read`, captured in
/// `decode_reference.json` `edge`): decoding stops at the first container or codec error and the
/// frames decoded so far are kept (truncated WAV / MP3 / fragmented M4A, corrupt MP3 or Ogg);
/// FLAC with a damaged or missing frame and Ogg with a cut-off last page are rejected. Non-finite
/// float samples are passed through like Chromium does (see [`sanitize_non_finite`]).
pub fn decode_vec(bytes: Vec<u8>, ext_hint: &str) -> Result<AudioBuffer, DecodeError> {
    guard::validate_header(&bytes)?;
    let mut bytes = bytes;
    if is_ogg(&bytes)
        && let Some(repaired) = drop_damaged_pages(&bytes)
    {
        bytes = repaired;
    }
    if is_mp4(&bytes) {
        repair_truncated_tail(&mut bytes);
    }
    let bytes = Shared(Arc::new(bytes));
    let result = panic::catch_unwind(AssertUnwindSafe(|| decode_shared(&bytes, ext_hint)));
    result.unwrap_or_else(|payload| {
        let msg = payload
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown".to_owned());
        Err(DecodeError::Malformed(format!("decoder panicked: {msg}")))
    })
}

/// Replace NaN and ±∞ samples by 0; returns how many were replaced.
///
/// `decode_*` keep them, like Chromium's `decodeAudioData` (a float WAV holding NaN / ±Inf
/// decodes to exactly those values, see `decode_reference.json` `edge/wav_f32nonfinite_*`). The
/// engine must sanitize at its boundary (this function) so one bad sample cannot poison filters
/// and the master bus.
pub fn sanitize_non_finite(buffer: &mut AudioBuffer) -> usize {
    let mut n = 0;
    for x in buffer.channels.iter_mut().flatten() {
        if !x.is_finite() {
            *x = 0.0;
            n += 1;
        }
    }
    n
}

/// Shared, cheaply clonable file bytes (a `'static` symphonia source without copying).
#[derive(Clone)]
struct Shared(Arc<Vec<u8>>);

impl AsRef<[u8]> for Shared {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

fn decode_shared(shared: &Shared, ext_hint: &str) -> Result<AudioBuffer, DecodeError> {
    let bytes: &[u8] = shared.as_ref();
    let mut hint = Hint::new();
    if !ext_hint.is_empty() {
        hint.with_extension(&ext_hint.to_ascii_lowercase());
    }
    let mss = MediaSourceStream::new(Box::new(Cursor::new(shared.clone())), Default::default());
    let format_opts = FormatOptions {
        enable_gapless: true,
        ..Default::default()
    };
    let probed = symphonia::default::get_probe().format(
        &hint,
        mss,
        &format_opts,
        &MetadataOptions::default(),
    )?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL && t.codec_params.sample_rate.is_some())
        .ok_or(DecodeError::NoAudioTrack)?;
    let params = &track.codec_params;
    guard::check_params(
        params.sample_rate,
        params.channels.map(|c| c.count()),
        params.max_frames_per_packet,
    )?;
    if params.codec == CODEC_TYPE_ALAC
        && let Some(cookie) = &params.extra_data
    {
        guard::check_alac_cookie(cookie)?;
    }
    let track_id = track.id;
    let mut sample_rate = params.sample_rate.unwrap_or(0);
    let is_flac = params.codec == CODEC_TYPE_FLAC;
    let declared = params.n_frames;
    // symphonia's FLAC decoder left-aligns samples in i32; FFmpeg gives <= 16-bit FLAC as s16.
    let flac16 = is_flac && params.bits_per_sample.is_some_and(|b| b <= 16);
    let budget = guard::FrameBudget::new(bytes.len(), declared, sample_rate);
    let mut decoder = symphonia::default::get_codecs().make(params, &DecoderOptions::default())?;

    let mut channels: Vec<Vec<f32>> = Vec::new();
    let mut spec: Option<(u32, usize)> = None;
    let mut packets = 0u64;
    let mut stopped_by: Option<DecodeError> = None;
    loop {
        packets += 1;
        if packets > budget.max_packets {
            return Err(DecodeError::Malformed("too many packets".into()));
        }
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymError::ResetRequired) => break,
            Err(e) => {
                stopped_by = Some(e.into());
                break;
            }
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            // Like Chromium's AudioFileReader: stop at the first failing packet, keep the rest.
            Err(e) => {
                stopped_by = Some(e.into());
                break;
            }
        };
        let this = (decoded.spec().rate, decoded.spec().channels.count());
        // A mid-stream rate / channel change ends decoding (Chromium: `on_output_error_`).
        if *spec.get_or_insert(this) != this {
            break;
        }
        guard::check_params(Some(this.0), Some(this.1), None)?;
        let frames = channels.first().map_or(0, Vec::len) as u64 + decoded.frames() as u64;
        if frames > budget.max_frames {
            return Err(DecodeError::Malformed(format!(
                "more than {} frames decoded",
                budget.max_frames
            )));
        }
        sample_rate = this.0;
        append_planar(&mut channels, decoded, flac16);
    }

    let decoded_frames = channels.first().map_or(0, Vec::len) as u64;
    if is_flac {
        // Chromium rejects FLAC with a damaged or missing frame instead of salvaging it.
        if let Some(e) = stopped_by {
            return Err(e);
        }
        if declared.is_some_and(|d| d > 0 && decoded_frames < d) {
            return Err(DecodeError::Malformed(format!(
                "truncated FLAC: {decoded_frames} of {} frames",
                declared.unwrap_or(0)
            )));
        }
    }
    if is_ogg(bytes) && ogg_truncated(bytes) {
        return Err(DecodeError::Malformed("truncated Ogg page".into()));
    }

    if is_mp4(bytes)
        && let Some(trim) = mp4_trim(bytes, sample_rate)
    {
        for ch in &mut channels {
            let skip = (trim.skip as usize).min(ch.len());
            ch.drain(..skip);
            if let Some(frames) = trim.frames {
                ch.truncate(frames as usize);
            }
        }
    }

    if is_ogg(bytes)
        && let Some(granule) = ogg_final_granule(bytes)
    {
        for ch in &mut channels {
            ch.truncate(granule as usize);
        }
    }

    let buffer = AudioBuffer {
        sample_rate,
        channels,
    };
    if buffer.frames() == 0 || buffer.channel_count() == 0 {
        return Err(stopped_by.unwrap_or(DecodeError::Empty));
    }
    Ok(buffer)
}

fn is_mp4(bytes: &[u8]) -> bool {
    bytes.get(4..8) == Some(b"ftyp".as_slice())
}

fn is_ogg(bytes: &[u8]) -> bool {
    bytes.starts_with(b"OggS")
}

/// Chromium's integer→float conversion (`media::FixedSampleTypeTraits::ToFloat`, applied to
/// FFmpeg's u8 / s16 output): asymmetric, negative values over `|min|`, positive over `max`.
/// FFmpeg hands 24- and 32-bit sources over as s32, where the asymmetry vanishes in f32
/// (`2147483647.0f == 2^31`), so symphonia's symmetric conversion already matches for those.
fn u8_to_f32(v: u8) -> f32 {
    let x = f32::from(v) - 128.0;
    if x < 0.0 {
        x * (1.0 / 128.0)
    } else {
        x * (1.0 / 127.0)
    }
}

fn s16_to_f32(v: i16) -> f32 {
    let x = f32::from(v);
    if x < 0.0 {
        x * (1.0 / 32768.0)
    } else {
        x * (1.0 / 32767.0)
    }
}

fn append_planar(channels: &mut Vec<Vec<f32>>, decoded: AudioBufferRef<'_>, flac16: bool) {
    let spec = *decoded.spec();
    let frames = decoded.frames();
    let n = spec.channels.count();
    if channels.len() < n {
        let len = channels.first().map_or(0, Vec::len);
        channels.resize(n, vec![0.0; len]);
    }
    for ch in channels.iter_mut().skip(n) {
        ch.resize(ch.len() + frames, 0.0);
    }
    match &decoded {
        AudioBufferRef::U8(b) => {
            for (c, dst) in channels.iter_mut().take(n).enumerate() {
                dst.extend(b.chan(c).iter().map(|&v| u8_to_f32(v)));
            }
        }
        AudioBufferRef::S16(b) => {
            for (c, dst) in channels.iter_mut().take(n).enumerate() {
                dst.extend(b.chan(c).iter().map(|&v| s16_to_f32(v)));
            }
        }
        AudioBufferRef::S32(b) if flac16 => {
            for (c, dst) in channels.iter_mut().take(n).enumerate() {
                dst.extend(b.chan(c).iter().map(|&v| s16_to_f32((v >> 16) as i16)));
            }
        }
        _ => {
            let mut buf = SymBuffer::<f32>::new(decoded.capacity() as u64, spec);
            decoded.convert(&mut buf);
            for (c, dst) in channels.iter_mut().take(n).enumerate() {
                dst.extend_from_slice(buf.chan(c));
            }
        }
    }
}
