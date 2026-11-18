//! The bundled Salamander piano set (port of `src/audio/pianoSamples.ts`).
//!
//! 30 MP3s, one every minor third from A0 (21) to C8 (108), embedded with `include_bytes!` from
//! `assets/piano/` (CC-BY 3.0, see the README there). Loading = what the app does per file:
//! `decodeAudioData` at the context rate (here [`mdaw_dsp::audio_io::decode_vec`] + the Chromium
//! `SincResampler` port, bit-exact vs Chromium), then [`trim_leading_silence`].
//!
//! The decoded set is memoised per context rate ([`piano_samples`]); the app memoises one set for
//! the first context it decodes in. Decoding runs on worker threads only: [`PianoSlot::load`]
//! spawns one and the audio thread just polls the slot (`OnceLock::get`, wait-free).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use mdaw_dsp::audio_io::{DecodeError, decode_vec, resample, sanitize_non_finite};
use mdaw_dsp::buffer::{AudioBuffer, SharedBuffer};

macro_rules! samples {
    ($($name:literal),* $(,)?) => {
        [$(($name, include_bytes!(concat!("../../assets/piano/", $name, ".mp3")) as &[u8])),*]
    };
}

/// `(file stem, MP3 bytes)` of every bundled sample.
pub static SAMPLE_FILES: [(&str, &[u8]); 30] = samples![
    "A0", "C1", "Ds1", "Fs1", "A1", "C2", "Ds2", "Fs2", "A2", "C3", "Ds3", "Fs3", "A3", "C4",
    "Ds4", "Fs4", "A4", "C5", "Ds5", "Fs5", "A5", "C6", "Ds6", "Fs6", "A6", "C7", "Ds7", "Fs7",
    "A7", "C8",
];

/// Number of bundled samples (`PIANO_SAMPLE_COUNT`).
pub const PIANO_SAMPLE_COUNT: usize = SAMPLE_FILES.len();

/// `sampleFileToMidi`: `'./samples/piano/Fs3.mp3'` → 54; anything else → −1.
pub fn sample_file_to_midi(path: &str) -> i32 {
    let Some(stem) = path.strip_suffix(".mp3") else {
        return -1;
    };
    let bytes = stem.as_bytes();
    let Some((&oct, head)) = bytes.split_last() else {
        return -1;
    };
    if !oct.is_ascii_digit() {
        return -1;
    }
    // The regexp `([A-G]s?)(\d)\.mp3$` takes the longest match ending at the digit.
    let pitch = if head.len() >= 2 && head[head.len() - 1] == b's' {
        &head[head.len() - 2..]
    } else if let Some(last) = head.last() {
        std::slice::from_ref(last)
    } else {
        return -1;
    };
    if !pitch[0].is_ascii_uppercase() || pitch[0] > b'G' {
        return -1;
    }
    let pc = match pitch {
        b"C" => 0,
        b"Ds" => 3,
        b"Fs" => 6,
        b"A" => 9,
        _ => return -1,
    };
    (i32::from(oct - b'0') + 1) * 12 + pc
}

/// `trimLeadingSilence`: drop the MP3 encoder delay before the onset (first |x| > 0.002 in any
/// channel), keeping 1 ms. Buffers that would lose fewer than 16 frames are left alone.
pub fn trim_leading_silence(buf: AudioBuffer) -> AudioBuffer {
    const THRESHOLD: f32 = 0.002;
    let len = buf.frames();
    let mut onset = len;
    for ch in &buf.channels {
        if let Some(i) = ch[..onset.min(ch.len())]
            .iter()
            .position(|x| x.abs() > THRESHOLD)
        {
            onset = i;
        }
    }
    // Math.round(sampleRate * 0.001)
    let keep = (buf.sample_rate as f64 * 0.001 + 0.5).floor() as usize;
    let start = onset.saturating_sub(keep);
    if start < 16 || start + 1 >= len {
        return buf;
    }
    AudioBuffer {
        sample_rate: buf.sample_rate,
        channels: buf
            .channels
            .into_iter()
            .map(|c| c[start..].to_vec())
            .collect(),
    }
}

/// The decoded set at one context rate: sample root MIDI note → buffer.
#[derive(Debug)]
pub struct PianoSamples {
    sample_rate: u32,
    by_note: Vec<Option<SharedBuffer>>,
}

impl PianoSamples {
    /// A set from explicit buffers (tests / custom sample sets, the `buffers` constructor
    /// argument of the TS `SampledPiano`). Notes outside 0..=127 are ignored.
    pub fn from_buffers(
        sample_rate: u32,
        buffers: impl IntoIterator<Item = (u8, SharedBuffer)>,
    ) -> Self {
        let mut by_note = vec![None; 128];
        for (m, b) in buffers {
            if let Some(slot) = by_note.get_mut(m as usize) {
                *slot = Some(b);
            }
        }
        Self {
            sample_rate,
            by_note,
        }
    }

    /// The context rate the buffers were decoded for.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// The buffer of sample root `midi`.
    pub fn get(&self, midi: i32) -> Option<&SharedBuffer> {
        usize::try_from(midi)
            .ok()
            .and_then(|m| self.by_note.get(m))
            .and_then(Option::as_ref)
    }

    /// Root notes, ascending.
    pub fn roots(&self) -> impl Iterator<Item = u8> + '_ {
        (0u8..128).filter(|&m| self.by_note[m as usize].is_some())
    }

    pub fn len(&self) -> usize {
        self.roots().count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Decode one bundled file at `context_rate` (decodeAudioData + trim).
pub fn decode_sample(bytes: &[u8], context_rate: u32) -> Result<AudioBuffer, DecodeError> {
    let decoded = decode_vec(bytes.to_vec(), "mp3")?;
    let mut buf = if decoded.sample_rate == context_rate {
        decoded
    } else {
        resample(&decoded, context_rate)
    };
    sanitize_non_finite(&mut buf);
    Ok(trim_leading_silence(buf))
}

/// Decode the whole set at `context_rate` on scoped worker threads (not memoised; see
/// [`piano_samples`]). Fails if no sample decodes, like `loadPianoSamples`; a file that fails
/// alone is skipped (`decodeOne` → null).
pub fn decode_piano_samples(context_rate: u32) -> Result<PianoSamples, DecodeError> {
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, PIANO_SAMPLE_COUNT);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results: Vec<Vec<(u8, Result<AudioBuffer, DecodeError>)>> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                s.spawn(|| {
                    let mut out = Vec::new();
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(&(name, bytes)) = SAMPLE_FILES.get(i) else {
                            break;
                        };
                        let midi = sample_file_to_midi(&format!("./samples/piano/{name}.mp3"));
                        if midi >= 0 {
                            out.push((midi as u8, decode_sample(bytes, context_rate)));
                        }
                    }
                    out
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or_default())
            .collect()
    });
    let mut last_err = None;
    let mut ok = Vec::new();
    for (m, r) in results.into_iter().flatten() {
        match r {
            Ok(b) => ok.push((m, Arc::new(b))),
            Err(e) => last_err = Some(e),
        }
    }
    if ok.is_empty() {
        return Err(
            last_err.unwrap_or_else(|| DecodeError::Malformed("No piano samples found".into()))
        );
    }
    Ok(PianoSamples::from_buffers(context_rate, ok))
}

type Cache = Mutex<HashMap<u32, Arc<OnceLock<Arc<PianoSamples>>>>>;

fn cache() -> &'static Cache {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The decoded set at `context_rate`, decoded once per process and rate (`loadPianoSamples`).
/// Blocks while decoding: call it from a worker thread. A failed decode is not cached, so a later
/// call retries (the TS resets `loading` on failure).
pub fn piano_samples(context_rate: u32) -> Result<Arc<PianoSamples>, DecodeError> {
    let cell = cache()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .entry(context_rate)
        .or_default()
        .clone();
    if let Some(s) = cell.get() {
        return Ok(s.clone());
    }
    let decoded = Arc::new(decode_piano_samples(context_rate)?);
    Ok(cell.get_or_init(|| decoded).clone())
}

/// The already decoded set at `context_rate`, if any (`getPianoBuffers`).
pub fn cached_piano_samples(context_rate: u32) -> Option<Arc<PianoSamples>> {
    let cell = cache()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&context_rate)?
        .clone();
    cell.get().cloned()
}

/// Where a [`super::SampledPiano`] finds its samples once they are decoded. Shared between the
/// loader thread (which fills it once) and the audio thread (which polls it without locking).
#[derive(Debug, Clone, Default)]
pub struct PianoSlot(Arc<OnceLock<Arc<PianoSamples>>>);

impl PianoSlot {
    /// An empty slot (the piano stays on its fallback until [`Self::fill`]).
    pub fn empty() -> Self {
        Self::default()
    }

    /// A slot holding `samples` already.
    pub fn ready(samples: Arc<PianoSamples>) -> Self {
        let s = Self::default();
        s.fill(samples);
        s
    }

    /// The bundled set at `context_rate`: filled at once when it is already decoded
    /// (`getPianoBuffers()`), otherwise decoded on a new worker thread (`loadPianoSamples()`).
    /// A failed decode leaves the slot empty (the piano keeps its fallback) and is reported on
    /// stderr, like the app's console warning.
    pub fn load(context_rate: u32) -> Self {
        if let Some(s) = cached_piano_samples(context_rate) {
            return Self::ready(s);
        }
        let slot = Self::default();
        let fill = slot.clone();
        let spawned = std::thread::Builder::new()
            .name("piano-samples".into())
            .spawn(move || match piano_samples(context_rate) {
                Ok(s) => fill.fill(s),
                Err(e) => {
                    eprintln!("[audio] piano samples failed to load, using synthesized piano: {e}")
                }
            });
        if let Err(e) = spawned {
            eprintln!("[audio] could not start the piano sample loader: {e}");
        }
        slot
    }

    /// Hand the samples over (later calls are ignored).
    pub fn fill(&self, samples: Arc<PianoSamples>) {
        let _ = self.0.set(samples);
    }

    /// The samples, once decoded. Wait-free.
    pub fn get(&self) -> Option<&Arc<PianoSamples>> {
        self.0.get()
    }
}
