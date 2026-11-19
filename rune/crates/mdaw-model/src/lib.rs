//! micro-daw data model: the `.mdaw` project format (field-for-field compatible with the
//! Electron app), migrations, and the pure helpers the engine and UI share.
//!
//! Ported from `src/types/**`, `src/utils/**`, `src/shared/**` and the pure parts of
//! `src/services/**` of the Electron app. Each module names its TypeScript source.

pub mod arrangement_helpers;
pub mod automation;
pub mod beat_helpers;
pub mod chords;
pub mod classic_meta;
pub mod controller;
pub mod doc;
pub mod fs_entries;
pub mod fsio;
pub mod js;
pub mod kit;
pub mod knobs;
pub mod loop_settings;
pub mod migrate;
pub mod notes;
pub mod open;
pub mod pack_source;
pub mod packs;
pub mod paths;
pub mod piano_roll;
pub mod settings;
pub mod slicer;
pub mod templates_data;
pub mod types;
pub mod undo;

#[cfg(test)]
pub(crate) mod test_support;

pub use types::*;
