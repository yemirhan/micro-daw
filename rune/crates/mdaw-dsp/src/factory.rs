//! The 16 factory drum voices: `src/audio/drumKit.ts` rendered offline exactly like
//! `bufferCache.ts` `renderFactoryVoice` does (Tone.Offline, 1 channel, velocity 1, length from
//! `FACTORY_VOICE_RENDER`, then `trimTail`).
//!
//! The Tone.js / Web Audio graph is reproduced as sample-domain DSP with Web Audio semantics:
//! - [`automation`] — AudioParam timelines (setValue / linear & exponential ramps / setTarget /
//!   cancelScheduledValues and Tone's cancelAndHoldAtTime), a-rate, value at `k/sr`;
//! - [`osc`] — Chromium's band-limited PeriodicWave oscillators (sine / square / triangle);
//! - [`biquad`] — Web Audio biquads (lowpass/highpass Q in dB), Tone.Filter rolloff cascades;
//! - [`crate::shaping`] — the tanh WaveShaper curves (oversample none);
//! - [`noise`] — Tone's stereo white-noise buffer + random start offset, drawn from a seeded
//!   mulberry32 instead of `Math.random`;
//! - [`trim`] — `trimTail`.
//!
//! Parity evidence: `rune/parity/fixtures/factory_reference.json`, captured from the real app
//! code in Chromium (`rune/parity/chromium/`), compared in `parity_tests`.
//!
//! Determinism: the app's factory voices are NOT deterministic (Tone.Noise uses `Math.random`
//! for the buffer and for every start offset). Rust renders with fixed seeds
//! ([`FACTORY_NOISE_SEED`], [`FACTORY_OFFSET_SEED_BASE`] + voice index): the same seeds the
//! Chromium capture installs, so `render_factory_voice` equals the seeded Chromium render.

pub mod automation;
pub mod biquad;
pub mod kit;
pub mod noise;
pub mod osc;
pub mod trim;

#[cfg(test)]
mod parity_tests;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use crate::buffer::{AudioBuffer, SharedBuffer};
use mdaw_model::types::FactoryVoiceId;

pub use kit::DrumVoice;
pub use noise::{Mulberry32, NoiseBuffer};
pub use osc::prewarm_wavetables;
pub use trim::{TrimOptions, trim_tail};

/// Seed of the noise buffer (both channels, 2 × 220500 draws).
pub const FACTORY_NOISE_SEED: u32 = 0x5eed_0001;
/// Seed of a voice's noise start offset = base + index in `FactoryVoiceId::ALL`.
pub const FACTORY_OFFSET_SEED_BASE: u32 = 0x5eed_1000;

/// `FACTORY_VOICE_RENDER` entry: kit voice, tom note, render length before trimming.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FactoryRender {
    pub voice: DrumVoice,
    pub note: Option<u8>,
    pub seconds: f64,
}

/// `FACTORY_VOICE_RENDER[id]`.
pub fn factory_voice_render(id: FactoryVoiceId) -> FactoryRender {
    use FactoryVoiceId as F;
    let (voice, note, seconds) = match id {
        F::Kick => (DrumVoice::Kick, None, 1.0),
        F::KickSub => (DrumVoice::KickSub, None, 2.0),
        F::Snare => (DrumVoice::Snare, None, 0.7),
        F::Rim => (DrumVoice::Rim, None, 0.5),
        F::Clap => (DrumVoice::Clap, None, 0.8),
        F::Chh => (DrumVoice::Chh, None, 0.35),
        F::Phh => (DrumVoice::Phh, None, 0.4),
        F::Ohh => (DrumVoice::Ohh, None, 1.2),
        F::TomLo => (DrumVoice::Tom, Some(41), 1.4),
        F::TomMid => (DrumVoice::Tom, Some(45), 1.2),
        F::TomHi => (DrumVoice::Tom, Some(48), 1.1),
        F::Crash => (DrumVoice::Crash, None, 2.6),
        F::Ride => (DrumVoice::Ride, None, 2.0),
        F::Shaker => (DrumVoice::Shaker, None, 0.4),
        F::Perc => (DrumVoice::Perc, None, 0.7),
        F::Snap => (DrumVoice::Snap, None, 0.5),
    };
    FactoryRender {
        voice,
        note,
        seconds,
    }
}

/// Seeds for the two `Math.random` uses of Tone.Noise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoiseSeeds {
    /// mulberry32 seed of the cached noise buffer.
    pub buffer: u32,
    /// mulberry32 seed whose first draw is the start offset.
    pub offset: u32,
}

impl NoiseSeeds {
    /// The default (and Chromium-capture) seeds of a factory voice.
    pub fn factory(id: FactoryVoiceId) -> Self {
        let index = FactoryVoiceId::ALL
            .iter()
            .position(|v| *v == id)
            .unwrap_or(0) as u32;
        Self {
            buffer: FACTORY_NOISE_SEED,
            offset: FACTORY_OFFSET_SEED_BASE + index,
        }
    }
}

/// Offline context length: `OfflineAudioContext(1, seconds·sr, sr)` truncates to an integer.
pub fn render_frames(seconds: f64, sample_rate: u32) -> usize {
    (seconds * sample_rate as f64) as usize
}

/// The kit output before `trimTail` (1 channel, `render_frames` long).
pub fn render_factory_voice_untrimmed(
    id: FactoryVoiceId,
    sample_rate: u32,
    seeds: NoiseSeeds,
) -> Vec<f32> {
    let spec = factory_voice_render(id);
    let frames = render_frames(spec.seconds, sample_rate);
    let buffer = NoiseBuffer::white(seeds.buffer);
    let offset = NoiseBuffer::offset_frames(Mulberry32::new(seeds.offset).next_f64(), sample_rate);
    let noise = buffer.play(offset, frames);
    kit::render_voice(spec.voice, 1.0, spec.note, sample_rate, frames, &noise)
}

/// Factory voice with explicit noise seeds, trimmed like `renderFactoryVoice`.
pub fn render_factory_voice_seeded(
    id: FactoryVoiceId,
    sample_rate: u32,
    seeds: NoiseSeeds,
) -> AudioBuffer {
    let raw = render_factory_voice_untrimmed(id, sample_rate, seeds);
    let mut trimmed = trim_tail(&[raw], sample_rate, TrimOptions::default());
    AudioBuffer::mono(sample_rate, trimmed.remove(0))
}

/// The factory pad sample of `id` at `sample_rate` (mono, trimmed, peak ≤ 0.98), with the
/// default deterministic noise ([`NoiseSeeds::factory`]).
pub fn render_factory_voice(id: FactoryVoiceId, sample_rate: u32) -> AudioBuffer {
    render_factory_voice_seeded(id, sample_rate, NoiseSeeds::factory(id))
}

type VoiceCell = Arc<OnceLock<SharedBuffer>>;
type VoiceCache = Mutex<HashMap<(FactoryVoiceId, u32), VoiceCell>>;

fn voice_cache() -> &'static VoiceCache {
    static CACHE: OnceLock<VoiceCache> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// Memoized [`render_factory_voice`], keyed by (voice, sample rate), safe to call from any
/// thread: concurrent requests for the same key render it once and share the buffer; different
/// keys render in parallel (rendering happens outside the map lock). ≙ the factory side of the
/// TS `bufferCache`.
pub fn cached_factory_voice(id: FactoryVoiceId, sample_rate: u32) -> SharedBuffer {
    let cell = {
        let mut map = voice_cache().lock().unwrap_or_else(PoisonError::into_inner);
        map.entry((id, sample_rate)).or_default().clone()
    };
    cell.get_or_init(|| Arc::new(render_factory_voice(id, sample_rate)))
        .clone()
}

/// Drop every memoized factory voice (e.g. after an output sample-rate change).
pub fn clear_factory_cache() {
    voice_cache()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factory_voice_cache_memoizes_per_voice_and_rate() {
        let sr = 22050;
        let ids = [FactoryVoiceId::Kick, FactoryVoiceId::Chh];
        let got: Vec<SharedBuffer> = std::thread::scope(|s| {
            let hs: Vec<_> = (0..4)
                .map(|i| s.spawn(move || cached_factory_voice(ids[i % 2], sr)))
                .collect();
            hs.into_iter().map(|h| h.join().unwrap()).collect()
        });
        assert!(Arc::ptr_eq(&got[0], &got[2]));
        assert!(Arc::ptr_eq(&got[1], &got[3]));
        assert!(!Arc::ptr_eq(&got[0], &got[1]));
        // Bit-identical to an uncached render.
        assert_eq!(*got[0], render_factory_voice(FactoryVoiceId::Kick, sr));
        assert_eq!(*got[1], render_factory_voice(FactoryVoiceId::Chh, sr));
        // The rate is part of the key.
        let other_rate = cached_factory_voice(FactoryVoiceId::Kick, 16000);
        assert_eq!(other_rate.sample_rate, 16000);
        assert!(!Arc::ptr_eq(&other_rate, &got[0]));

        clear_factory_cache();
        let again = cached_factory_voice(FactoryVoiceId::Kick, sr);
        assert!(!Arc::ptr_eq(&again, &got[0]), "cleared");
        assert_eq!(again, got[0]);
    }

    #[test]
    fn render_spec_and_frames() {
        assert_eq!(factory_voice_render(FactoryVoiceId::TomLo).note, Some(41));
        assert_eq!(factory_voice_render(FactoryVoiceId::Crash).seconds, 2.6);
        // OfflineAudioContext length truncates: 0.35 · 44100 = 15434.999…
        assert_eq!(render_frames(0.35, 44100), 15434);
        assert_eq!(render_frames(2.6, 48000), 124800);
        assert_eq!(
            NoiseSeeds::factory(FactoryVoiceId::Snap).offset,
            FACTORY_OFFSET_SEED_BASE + 15
        );
    }

    #[test]
    fn every_voice_renders_deterministically_at_other_rates() {
        for id in FactoryVoiceId::ALL {
            let a = render_factory_voice(id, 22050);
            let b = render_factory_voice(id, 22050);
            assert_eq!(a, b, "{}", id.id());
            assert_eq!(a.channel_count(), 1);
            let peak = a.channels[0].iter().fold(0f32, |m, x| m.max(x.abs()));
            assert!(peak > 0.05 && peak <= 0.98, "{} peak {peak}", id.id());
            let max = render_frames(factory_voice_render(id).seconds, 22050);
            assert!(a.frames() >= 221 && a.frames() <= max);
            assert_eq!(*a.channels[0].last().unwrap(), 0.0, "faded out");
        }
    }
}
