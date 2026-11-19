//! Pure logic for the piano roll editor: layout (pitch <-> row), coordinate transforms, snapping,
//! grid/ruler generation, hit-testing and all note edit math (move / resize / draw / duplicate /
//! paste / quantize / velocity).
//!
//! Port of `src/utils/pianoRollHelpers.ts` (types from `src/types/pianoRollEditor.ts`, drum rows
//! from `DRUM_SOUNDS` in `src/utils/constants.ts`). Beats are relative to the region start unless a
//! parameter says otherwise; `offset` parameters are the region's absolute start beat so grid lines
//! and snapping line up with the song's bars.
//!
//! Porting conventions:
//! - every number stays `f64` where the TypeScript code could produce a non-integer (pitches and
//!   rows included, e.g. `pitch_to_row(60.5)` is `66.5` on the piano layout, exactly like JS);
//! - `Math.round` / `Math.max` / `Math.min` / `clamp` follow JS semantics (round half up, NaN
//!   propagates, no panics when `lo > hi`) via the helpers in [`math`];
//! - TS `Set<number>` selections are `&[usize]` / `Vec<usize>` in insertion order (JS `Set`
//!   iteration order), so callers see the same ordering as the Electron app.

mod edit;
mod grid;
mod layout;
mod math;
mod select;
mod types;
mod view;

pub use edit::*;
pub use grid::*;
pub use layout::*;
pub use math::*;
pub use select::*;
pub use types::*;
pub use view::*;

#[cfg(test)]
mod golden_tests;
#[cfg(test)]
mod tests;
