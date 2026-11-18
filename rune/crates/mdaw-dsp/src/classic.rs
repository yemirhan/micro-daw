//! Built-in classic kits: original syntheses in the style of classic drum machines (808-, 909-,
//! 707-, 606-, LinnDrum-, CR-78-style). Literal port of the pure parts of
//! `src/audio/sampler/classic/**`:
//!
//! - `dsp.ts` → [`dsp`] (PRNG, envelopes, oscillators, RBJ biquads, drive, lofi, finalize)
//! - `voices.ts` → [`voices`] (every recipe of every machine)
//! - this module: `synth.ts` and the pure part of `render.ts`. The Web Worker / main-thread
//!   scheduling of `render.ts` becomes a plain function ([`render_classic_voice`]) plus a
//!   thread-safe memoizing cache ([`cached_classic_voice`]); callers run it on whatever thread
//!   they like.
//! - `analysis.ts` → [`analysis`] (offline spectral sanity metrics, used by the tests)
//!
//! Machine metadata (names, voice labels, pads) lives in `mdaw_model::classic_meta`.

pub mod analysis;
pub mod dsp;
pub mod voices;

#[cfg(test)]
mod golden_tests;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use crate::buffer::AudioBuffer;
use mdaw_model::ClassicMachineId;

pub use analysis::{VoiceAnalysis, analyze_voice};
pub use voices::{VoiceRecipe, classic_recipe, classic_recipe_ids};

/// Sample rate of every rendered classic voice (mono).
pub const CLASSIC_SAMPLE_RATE: u32 = 44100;

/// `Unknown classic voice: <machine>/<voice>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownClassicVoice {
    pub machine: ClassicMachineId,
    pub voice: String,
}

impl std::fmt::Display for UnknownClassicVoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Unknown classic voice: {}/{}",
            self.machine.id(),
            self.voice
        )
    }
}

impl std::error::Error for UnknownClassicVoice {}

/// `hasClassicRecipe`.
pub fn has_classic_recipe(machine: ClassicMachineId, voice: &str) -> bool {
    classic_recipe_ids(machine).contains(&voice)
}

/// `synthesizeClassicVoice`: mono 44.1 kHz samples, normalized to -1 dBFS with the silent tail
/// trimmed. Deterministic (seeded noise).
pub fn synthesize_classic_voice(
    machine: ClassicMachineId,
    voice: &str,
) -> Result<Vec<f32>, UnknownClassicVoice> {
    let recipe = classic_recipe(machine, voice).ok_or_else(|| UnknownClassicVoice {
        machine,
        voice: voice.to_owned(),
    })?;
    Ok(dsp::finalize(
        &recipe.render(),
        dsp::FinalizeOpts {
            max_sec: recipe.max_sec,
            ..dsp::FinalizeOpts::D
        },
    ))
}

/// `renderClassicVoice` without the worker: the voice as a mono 44.1 kHz [`AudioBuffer`]. Not
/// cached; see [`cached_classic_voice`].
pub fn render_classic_voice(
    machine: ClassicMachineId,
    voice: &str,
) -> Result<AudioBuffer, UnknownClassicVoice> {
    Ok(AudioBuffer::mono(
        CLASSIC_SAMPLE_RATE,
        synthesize_classic_voice(machine, voice)?,
    ))
}

type Cell = Arc<OnceLock<Arc<AudioBuffer>>>;

fn cache() -> &'static Mutex<HashMap<(ClassicMachineId, String), Cell>> {
    static CACHE: OnceLock<Mutex<HashMap<(ClassicMachineId, String), Cell>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// Memoized [`render_classic_voice`], safe to call from any thread: concurrent requests for the
/// same voice render it once and share the buffer (≙ the in-flight dedup of the TS buffer cache).
/// Unknown voices are an error and are not cached.
pub fn cached_classic_voice(
    machine: ClassicMachineId,
    voice: &str,
) -> Result<Arc<AudioBuffer>, UnknownClassicVoice> {
    if !has_classic_recipe(machine, voice) {
        return Err(UnknownClassicVoice {
            machine,
            voice: voice.to_owned(),
        });
    }
    let cell = {
        let mut map = cache().lock().unwrap_or_else(|e| e.into_inner());
        map.entry((machine, voice.to_owned())).or_default().clone()
    };
    // Rendered outside the map lock: other voices render in parallel.
    Ok(cell
        .get_or_init(|| Arc::new(render_classic_voice(machine, voice).expect("recipe exists")))
        .clone())
}

/// Drop every memoized voice.
pub fn clear_classic_cache() {
    cache().lock().unwrap_or_else(|e| e.into_inner()).clear();
}
