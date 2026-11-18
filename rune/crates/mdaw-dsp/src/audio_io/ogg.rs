//! Ogg page scan for end trimming.
//!
//! symphonia 0.5 does not apply the final-page granule end trim to short Vorbis streams (it
//! returns every decoded frame of the last packet), while the Vorbis spec — and Chromium for
//! regular multi-page files — ends the stream at the last page's granule position.

/// Granule position of the last page of the first logical stream (Vorbis: total frames).
pub fn ogg_final_granule(bytes: &[u8]) -> Option<u64> {
    let mut at = 0usize;
    let mut serial = None;
    let mut granule = None;
    while let Some(h) = bytes.get(at..at + 27) {
        if &h[0..4] != b"OggS" {
            break;
        }
        let gp = i64::from_le_bytes(h[6..14].try_into().unwrap());
        let s = u32::from_le_bytes(h[14..18].try_into().unwrap());
        let nsegs = usize::from(h[26]);
        let table = bytes.get(at + 27..at + 27 + nsegs)?;
        let body: usize = table.iter().map(|&b| usize::from(b)).sum();
        let first = *serial.get_or_insert(s);
        if s == first && gp >= 0 {
            granule = Some(gp as u64);
        }
        at += 27 + nsegs + body;
    }
    granule.filter(|&g| g > 0)
}
