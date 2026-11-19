//! Port of `src/audio/sampler/classic/voices.ts`: the voice recipes of the built-in classic kits
//! (808-, 909-, 707-, 606-, LinnDrum-, CR-78-style). Each recipe returns the raw render;
//! [`super::synthesize_classic_voice`] finishes it (DC block, -1 dBFS peak, trimmed tail, capped at
//! `max_sec`). Kept in the same order and with the same literals as the TS file.
//!
//! One file per machine; the shared generators live in `common`. The `b!` / `e!` option
//! macros are defined in `common` and visible to every machine module.

#[macro_use]
mod common;
mod cr78;
mod linn;
mod tr606;
mod tr707;
mod tr808;
mod tr909;

use super::dsp::samples;
use mdaw_model::ClassicMachineId;

type Render = Box<dyn Fn(usize) -> Vec<f32> + Send + Sync>;

/// `VoiceRecipe`.
pub struct VoiceRecipe {
    /// Render length cap, seconds (also the maximum sample length).
    pub max_sec: f64,
    render: Render,
}

impl VoiceRecipe {
    /// The raw (unfinished) render.
    pub fn render(&self) -> Vec<f32> {
        (self.render)(samples(self.max_sec))
    }
}

/// `CLASSIC_RECIPES[machine][voice]`.
pub fn classic_recipe(machine: ClassicMachineId, voice: &str) -> Option<VoiceRecipe> {
    match machine {
        ClassicMachineId::Tr808 => tr808::tr808(voice),
        ClassicMachineId::Tr909 => tr909::tr909(voice),
        ClassicMachineId::Tr707 => tr707::tr707(voice),
        ClassicMachineId::Tr606 => tr606::tr606(voice),
        ClassicMachineId::Linn => linn::linn(voice),
        ClassicMachineId::Cr78 => cr78::cr78(voice),
    }
}

/// `Object.keys(CLASSIC_RECIPES[machine])`, in definition order.
pub fn classic_recipe_ids(machine: ClassicMachineId) -> &'static [&'static str] {
    match machine {
        ClassicMachineId::Tr808 => tr808::TR808_IDS,
        ClassicMachineId::Tr909 => tr909::TR909_IDS,
        ClassicMachineId::Tr707 => tr707::TR707_IDS,
        ClassicMachineId::Tr606 => tr606::TR606_IDS,
        ClassicMachineId::Linn => linn::LINN_IDS,
        ClassicMachineId::Cr78 => cr78::CR78_IDS,
    }
}
