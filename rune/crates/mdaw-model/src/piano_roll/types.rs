//! Constants and editor types (`src/types/pianoRollEditor.ts` + the constant block of
//! `pianoRollHelpers.ts`).

use crate::types::RegionNote;

pub const BEATS_PER_BAR: f64 = 4.0;

/// Full MIDI range, top row = `PR_NOTE_MAX`.
pub const PR_NOTE_MIN: i32 = 0;
pub const PR_NOTE_MAX: i32 = 127;
pub const PR_NOTE_COUNT: i32 = PR_NOTE_MAX - PR_NOTE_MIN + 1;

/// One GM drum pad (port of the fields of `DRUM_SOUNDS` that `DRUM_ROWS` keeps).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrumRow {
    pub midi_note: f64,
    /// `shortName` in `DRUM_SOUNDS`.
    pub name: &'static str,
    /// `name` in `DRUM_SOUNDS`.
    pub full_name: &'static str,
    pub color: &'static str,
}

/// Drum lane rows (GM pads, in pad order) — `DRUM_SOUNDS` of `src/utils/constants.ts`.
pub const DRUM_ROWS: [DrumRow; 8] = [
    DrumRow {
        midi_note: 36.0,
        name: "KCK",
        full_name: "Kick",
        color: "oklch(0.62 0.22 25)",
    },
    DrumRow {
        midi_note: 38.0,
        name: "SNR",
        full_name: "Snare",
        color: "oklch(0.70 0.18 65)",
    },
    DrumRow {
        midi_note: 42.0,
        name: "CHH",
        full_name: "Closed HH",
        color: "oklch(0.70 0.20 150)",
    },
    DrumRow {
        midi_note: 46.0,
        name: "OHH",
        full_name: "Open HH",
        color: "oklch(0.65 0.18 180)",
    },
    DrumRow {
        midi_note: 39.0,
        name: "CLP",
        full_name: "Clap",
        color: "oklch(0.62 0.22 310)",
    },
    DrumRow {
        midi_note: 45.0,
        name: "TOM",
        full_name: "Tom",
        color: "oklch(0.65 0.20 265)",
    },
    DrumRow {
        midi_note: 49.0,
        name: "CRS",
        full_name: "Crash",
        color: "oklch(0.75 0.17 90)",
    },
    DrumRow {
        midi_note: 51.0,
        name: "RDE",
        full_name: "Ride",
        color: "oklch(0.65 0.18 210)",
    },
];
pub const PR_DRUM_COUNT: usize = DRUM_ROWS.len();

pub const DEFAULT_NOTE_VELOCITY: f64 = 0.8;
/// 1 beat.
pub const DEFAULT_NOTE_DURATION: f64 = 1.0;
pub const DEFAULT_DRUM_DURATION: f64 = 0.25;
/// Shortest note the editor will create/resize to when snap is off.
pub const MIN_NOTE_LENGTH: f64 = 1.0 / 64.0;
/// Internal resolution; all computed beat values are rounded to this.
pub const TICKS_PER_BEAT: f64 = 960.0;
pub const MIN_VELOCITY: f64 = 1.0 / 127.0;

pub const PR_MIN_PX_PER_BEAT: f64 = 6.0;
pub const PR_MAX_PX_PER_BEAT: f64 = 640.0;
pub const PR_DEFAULT_PX_PER_BEAT: f64 = 64.0;
pub const PR_MIN_ROW_HEIGHT: f64 = 7.0;
pub const PR_MAX_ROW_HEIGHT: f64 = 40.0;
pub const PR_DEFAULT_ROW_HEIGHT: f64 = 16.0;
pub const PR_DEFAULT_DRUM_ROW_HEIGHT: f64 = 30.0;

pub(crate) const EPS: f64 = 1e-6;

/// A grid/snap choice. `beats == 0` means snapping is off.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SnapOption {
    pub id: &'static str,
    pub label: &'static str,
    pub beats: f64,
}

const fn opt(id: &'static str, label: &'static str, beats: f64) -> SnapOption {
    SnapOption { id, label, beats }
}

pub const SNAP_OPTIONS: [SnapOption; 11] = [
    opt("1/1", "1/1", 4.0),
    opt("1/2", "1/2", 2.0),
    opt("1/4", "1/4", 1.0),
    opt("1/8", "1/8", 0.5),
    opt("1/16", "1/16", 0.25),
    opt("1/32", "1/32", 0.125),
    opt("1/4T", "1/4 T", 2.0 / 3.0),
    opt("1/8T", "1/8 T", 1.0 / 3.0),
    opt("1/16T", "1/16 T", 1.0 / 6.0),
    opt("1/32T", "1/32 T", 1.0 / 12.0),
    opt("off", "Off", 0.0),
];

/// Note lengths offered in the toolbar (in beats).
pub const NOTE_LENGTH_OPTIONS: [SnapOption; 11] = [
    opt("1/1", "1/1", 4.0),
    opt("1/2", "1/2", 2.0),
    opt("1/4.", "1/4 .", 1.5),
    opt("1/4", "1/4", 1.0),
    opt("1/8.", "1/8 .", 0.75),
    opt("1/8", "1/8", 0.5),
    opt("1/16", "1/16", 0.25),
    opt("1/32", "1/32", 0.125),
    opt("1/4T", "1/4 T", 2.0 / 3.0),
    opt("1/8T", "1/8 T", 1.0 / 3.0),
    opt("1/16T", "1/16 T", 1.0 / 6.0),
];

/// `PianoRollTool`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PianoRollTool {
    #[default]
    Pointer,
    Draw,
    Eraser,
}

/// Which part of a note the pointer is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteEdge {
    Body,
    Start,
    End,
}

impl NoteEdge {
    pub fn as_str(self) -> &'static str {
        match self {
            NoteEdge::Body => "body",
            NoteEdge::Start => "start",
            NoteEdge::End => "end",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoteHit {
    pub index: usize,
    pub edge: NoteEdge,
}

/// A vertical line in the note grid / ruler. level 0 = bar, 1 = beat, 2 = subdivision.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridLine {
    pub beat: f64,
    pub level: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RulerLabel {
    pub beat: f64,
    pub label: String,
    /// true for bar starts
    pub major: bool,
}

/// Result of an edit that also changes the selection.
#[derive(Debug, Clone, PartialEq)]
pub struct NotesEdit {
    pub notes: Vec<RegionNote>,
    pub selection: Vec<usize>,
}

/// Last pointer position over the grid, for status display + key highlight.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridHover {
    pub beat: f64,
    pub row: f64,
    pub pitch: f64,
}

/// Zoom + scroll state shared by every piano roll pane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PianoRollViewport {
    pub px_per_beat: f64,
    pub row_height: f64,
    pub scroll_left: f64,
    pub scroll_top: f64,
    /// Visible size of the note grid (px).
    pub width: f64,
    pub height: f64,
}
