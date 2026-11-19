//! Export helpers of `src/services/ExportService.ts` that do not render audio:
//!
//! - [`midi_export`]: `exportMidi`, byte-identical to the file the app writes through
//!   midi-writer-js 3.2.1 (its quirks included, see [`midi`]);
//! - [`arrangement_end_seconds`]: `getArrangementEndSeconds` (how long the WAV render runs before
//!   [`EXPORT_TAIL_SECONDS`] are added), with the pad buffer durations injected;
//! - [`drum_play_beat`] / [`effects_tail_seconds`]: the shared helpers.
//!
//! Parity: `rune/parity/ts/midi_export.golden.ts` runs the real `exportMidi` /
//! `getArrangementEndSeconds` over every template and synthetic projects
//! (`rune/parity/fixtures/midi_export.json`); `export/tests.rs` replays it byte for byte.

mod end_seconds;
mod midi;

pub use end_seconds::{
    EXPORT_TAIL_SECONDS, MAX_DELAY_TAIL_SECONDS, arrangement_end_seconds, drum_play_beat,
    effects_tail_seconds, export_duration_seconds,
};
pub use midi::{MIDI_TICKS_PER_BEAT, midi_export};

use std::fmt;

/// Why an export did not produce a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportError {
    /// No unmuted MIDI track (the TS `throw new Error('Nothing to export')`).
    NothingToExport,
    /// A value midi-writer-js cannot encode: the TS throws (`TNaN is not a valid duration.`) or
    /// never returns (negative ticks loop forever in `numberToVariableLength`; ticks past 2^28
    /// overflow its 32-bit buffer). Rust reports these instead.
    InvalidValue(String),
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NothingToExport => f.write_str("Nothing to export"),
            Self::InvalidValue(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for ExportError {}

#[cfg(test)]
mod tests;
