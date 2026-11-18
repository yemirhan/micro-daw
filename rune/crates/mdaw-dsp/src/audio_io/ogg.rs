//! Ogg page scan for end trimming and truncation detection.
//!
//! symphonia 0.5 does not apply the final-page granule end trim to short Vorbis streams (it
//! returns every decoded frame of the last packet), while the Vorbis spec — and Chromium for
//! regular multi-page files — ends the stream at the last page's granule position.
//!
//! Chromium (FFmpeg) rejects an Ogg file whose last page is cut off (`decodeAudioData` fails;
//! see `decode_reference.json` `edge/trunc_ogg_*`), while damage in the middle is skipped.

/// One page header: `(granule, serial, total page length)`; `None` when the header or segment
/// table runs past the end.
fn page_at(bytes: &[u8], at: usize) -> Option<(i64, u32, usize)> {
    let h = bytes.get(at..at.checked_add(27)?)?;
    if &h[0..4] != b"OggS" {
        return None;
    }
    let gp = i64::from_le_bytes(h[6..14].try_into().ok()?);
    let s = u32::from_le_bytes(h[14..18].try_into().ok()?);
    let nsegs = usize::from(h[26]);
    let table = bytes.get(at + 27..at + 27 + nsegs)?;
    let body: usize = table.iter().map(|&b| usize::from(b)).sum();
    Some((gp, s, 27 + nsegs + body))
}

/// Next `OggS` capture pattern at or after `from`.
fn resync(bytes: &[u8], from: usize) -> Option<usize> {
    bytes
        .get(from..)?
        .windows(4)
        .position(|w| w == b"OggS")
        .map(|p| from + p)
}

/// Walk the pages (resynchronising over damaged bytes): `(final granule of the first logical
/// stream, whether the last page found is complete)`.
fn scan(bytes: &[u8]) -> (Option<u64>, bool) {
    let mut at = 0usize;
    let mut serial = None;
    let mut granule = None;
    let mut complete = true;
    while let Some(start) = resync(bytes, at) {
        let Some((gp, s, len)) = page_at(bytes, start) else {
            // A capture pattern whose header does not fit: cut-off page (or stray bytes at EOF).
            complete = false;
            break;
        };
        if start + len > bytes.len() {
            complete = false;
            break;
        }
        let first = *serial.get_or_insert(s);
        if s == first && gp >= 0 {
            granule = Some(gp as u64);
        }
        at = start + len;
    }
    (granule.filter(|&g| g > 0), complete)
}

/// Granule position of the last page of the first logical stream (Vorbis: total frames).
pub fn ogg_final_granule(bytes: &[u8]) -> Option<u64> {
    scan(bytes).0
}

/// Ogg CRC-32 (polynomial 0x04c11db7, MSB first, init 0, no final xor).
fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, e) in t.iter_mut().enumerate() {
            let mut r = (i as u32) << 24;
            for _ in 0..8 {
                r = if r & 0x8000_0000 != 0 {
                    (r << 1) ^ 0x04c1_1db7
                } else {
                    r << 1
                };
            }
            *e = r;
        }
        t
    });
    data.iter().fold(0u32, |crc, &b| {
        (crc << 8) ^ table[((crc >> 24) as u8 ^ b) as usize]
    })
}

fn page_crc_ok(page: &[u8]) -> bool {
    let stored = u32::from_le_bytes(page[22..26].try_into().unwrap());
    let mut copy = page.to_vec();
    copy[22..26].fill(0);
    crc32(&copy) == stored
}

/// Drop pages whose CRC does not match (and bytes between pages), like FFmpeg's Ogg demuxer
/// resynchronises over damage. symphonia 0.5 instead rejects the whole file on the first CRC
/// mismatch (it scans every page while opening). `None` when nothing needed removing.
pub fn drop_damaged_pages(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0usize;
    let mut changed = false;
    while let Some(start) = resync(bytes, at) {
        changed |= start != at;
        match page_at(bytes, start) {
            Some((_, _, len)) if start + len <= bytes.len() => {
                let page = &bytes[start..start + len];
                if page_crc_ok(page) {
                    out.extend_from_slice(page);
                    at = start + len;
                } else {
                    changed = true;
                    at = start + 4;
                }
            }
            // Cut-off last page: kept as is (the caller rejects truncated files).
            _ => {
                out.extend_from_slice(&bytes[start..]);
                at = bytes.len();
                break;
            }
        }
    }
    changed |= at < bytes.len();
    changed.then_some(out)
}

/// `true` when the file's last Ogg page is cut off.
pub fn ogg_truncated(bytes: &[u8]) -> bool {
    !scan(bytes).1
}
