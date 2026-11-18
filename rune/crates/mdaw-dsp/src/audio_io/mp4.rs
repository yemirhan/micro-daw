//! Minimal MP4/M4A box walker for encoder-delay ("priming") trimming.
//!
//! symphonia's isomp4 demuxer ignores edit lists and iTunes gapless metadata, so an AAC `.m4a`
//! decodes with its priming frames (1024 for ffmpeg/FDK, 2112 for iTunes/afconvert) in front and
//! the last frame's padding behind. Chromium (FFmpeg's mov demuxer) honours both, so to match it
//! we read, for the first sound track:
//! - `udta/meta/ilst/----` **iTunSMPB** (`priming`, `padding`, `original length`) — preferred;
//! - else `edts/elst` (first non-empty edit's `media_time` = frames to skip) with the media
//!   duration from `mdhd` (in the track timescale), cross-checked against the edit's
//!   `segment_duration` (movie timescale, often only millisecond precision).

/// Frames to drop at the start and the exact number of frames to keep afterwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mp4Trim {
    pub skip: u64,
    pub frames: Option<u64>,
}

struct Box<'a> {
    kind: [u8; 4],
    body: &'a [u8],
}

fn boxes(mut data: &[u8]) -> Vec<Box<'_>> {
    let mut out = Vec::new();
    while data.len() >= 8 {
        let size32 = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as u64;
        let kind = [data[4], data[5], data[6], data[7]];
        let (header, size) = match size32 {
            0 => (8, data.len() as u64),
            1 if data.len() >= 16 => (16, u64::from_be_bytes(data[8..16].try_into().unwrap())),
            _ => (8, size32),
        };
        if size < header as u64 || size > data.len() as u64 {
            break;
        }
        out.push(Box {
            kind,
            body: &data[header..size as usize],
        });
        data = &data[size as usize..];
    }
    out
}

fn child<'a>(data: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
    boxes(data)
        .into_iter()
        .find(|b| &b.kind == kind)
        .map(|b| b.body)
}

fn be_u32(b: &[u8], at: usize) -> Option<u64> {
    b.get(at..at + 4)
        .map(|s| u64::from(u32::from_be_bytes(s.try_into().unwrap())))
}

fn be_u64(b: &[u8], at: usize) -> Option<u64> {
    b.get(at..at + 8)
        .map(|s| u64::from_be_bytes(s.try_into().unwrap()))
}

/// `(timescale, duration)` of an `mvhd`/`mdhd` full box.
fn header_times(body: &[u8]) -> Option<(u64, u64)> {
    match body.first()? {
        1 => Some((be_u32(body, 20)?, be_u64(body, 24)?)),
        _ => Some((be_u32(body, 12)?, be_u32(body, 16)?)),
    }
}

/// First non-empty edit `(segment_duration [movie ts], media_time [media ts])`.
fn first_edit(elst: &[u8]) -> Option<(u64, i64)> {
    let version = *elst.first()?;
    let count = be_u32(elst, 4)?;
    let mut at = 8;
    for _ in 0..count {
        let (dur, time) = if version == 1 {
            (be_u64(elst, at)?, be_u64(elst, at + 8)? as i64)
        } else {
            (
                be_u32(elst, at)?,
                i64::from(be_u32(elst, at + 4)? as u32 as i32),
            )
        };
        at += if version == 1 { 20 } else { 12 };
        if time >= 0 {
            return Some((dur, time));
        }
    }
    None
}

/// iTunSMPB: `" 00000000 00000840 000001CC 0000000000046A0C ..."` → (priming, padding, length).
fn itunsmpb(moov: &[u8]) -> Option<(u64, u64, u64)> {
    let udta = child(moov, b"udta")?;
    let meta = child(udta, b"meta")?;
    // `meta` is a full box (4 bytes version/flags) in iTunes files; QuickTime-style files omit it.
    let ilst = child(meta.get(4..)?, b"ilst").or_else(|| child(meta, b"ilst"))?;
    for item in boxes(ilst) {
        if &item.kind != b"----" {
            continue;
        }
        let name = child(item.body, b"name").and_then(|n| n.get(4..));
        if name != Some(b"iTunSMPB".as_slice()) {
            continue;
        }
        let data = child(item.body, b"data")?.get(8..)?;
        let text = std::str::from_utf8(data).ok()?;
        let fields: Vec<u64> = text
            .split_whitespace()
            .filter_map(|f| u64::from_str_radix(f, 16).ok())
            .collect();
        if fields.len() >= 4 {
            return Some((fields[1], fields[2], fields[3]));
        }
    }
    None
}

/// Trim for the first `soun` track of an MP4 file, converted to frames at `sample_rate`.
pub fn mp4_trim(file: &[u8], sample_rate: u32) -> Option<Mp4Trim> {
    let moov = child(file, b"moov")?;
    if let Some((priming, _padding, length)) = itunsmpb(moov) {
        return Some(Mp4Trim {
            skip: priming,
            frames: Some(length),
        });
    }
    let (movie_ts, _) = header_times(child(moov, b"mvhd")?)?;
    for trak in boxes(moov).into_iter().filter(|b| &b.kind == b"trak") {
        let Some(mdia) = child(trak.body, b"mdia") else {
            continue;
        };
        let is_sound = child(mdia, b"hdlr").and_then(|h| h.get(8..12)) == Some(b"soun".as_slice());
        if !is_sound {
            continue;
        }
        let (media_ts, media_dur) = header_times(child(mdia, b"mdhd")?)?;
        if media_ts == 0 {
            return None;
        }
        let to_frames = |t: u64, ts: u64| -> u64 {
            ((t as f64) * f64::from(sample_rate) / ts as f64).round() as u64
        };
        let edit = child(trak.body, b"edts")
            .and_then(|e| child(e, b"elst"))
            .and_then(first_edit);
        let (seg_dur, media_time) = edit.unwrap_or((0, 0));
        let skip = to_frames(media_time as u64, media_ts);
        let from_mdhd = to_frames(media_dur, media_ts).saturating_sub(skip);
        let frames = if seg_dur > 0 && movie_ts > 0 {
            let from_elst = to_frames(seg_dur, movie_ts);
            // The edit is the authority; mdhd is only used when it agrees within the edit's
            // (coarser) timescale precision.
            let precision = (f64::from(sample_rate) / movie_ts as f64).ceil() as u64;
            if from_mdhd.abs_diff(from_elst) <= precision {
                from_mdhd
            } else {
                from_elst
            }
        } else {
            from_mdhd
        };
        return Some(Mp4Trim {
            skip,
            frames: Some(frames),
        });
    }
    None
}
