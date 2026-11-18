//! micro-daw data model: the `.mdaw` project format (field-for-field compatible with the
//! Electron app), migrations, and the pure helpers the engine and UI share.
//!
//! Ported from `src/types/**`, `src/utils/**`, `src/shared/**` and the pure parts of
//! `src/services/**` of the Electron app. Each module names its TypeScript source.

pub mod loop_settings;
pub mod types;

pub use types::*;
