//! Types crossing the control ↔ core boundary: instrument traits, commands, events, garbage.

use std::sync::Arc;

use mdaw_model::PadIndex;

use super::schedule::TrackSchedule;
use crate::contract::Instrument;

/// A drum instrument the core drives (SamplerKit implements it; tests use an impulse kit).
///
/// Every time is in context seconds and may lie in the future (later in the current render
/// quantum, or later still for a gate release): the instrument applies it at the exact frame,
/// like a Web Audio node. Implementations must not allocate, lock or block.
pub trait DrumInstrument: Instrument {
    /// Start a voice of `pad` at `time` (velocity 0..1).
    fn trigger(&mut self, pad: PadIndex, velocity: f64, time: f64);
    /// Gate release of `pad` at `time` (one-shot pads ignore it). Voices of the pad scheduled
    /// to start after `time` are dropped (SamplerKit.release).
    fn release(&mut self, pad: PadIndex, time: f64);
    /// Fade out every voice by `time`; voices scheduled later are cancelled (SamplerKit.stopAll).
    fn stop_all(&mut self, time: f64);
    /// Drop voices (of `pad`, or of every pad) that have not started by `time`.
    fn cancel_pending_after(&mut self, pad: Option<PadIndex>, time: f64);
    /// Whether `pad` plays in gate mode right now (pattern notes then get a release at
    /// noteStart + max(0.01, duration), checked at event time like the TS `playDrumNote`).
    fn pad_is_gate(&self, pad: PadIndex) -> bool;
}

/// A pitched instrument (phase 4: PolyInstrument and friends). Same timing rules as
/// [`DrumInstrument`].
pub trait SynthInstrument: Instrument {
    fn note_on(&mut self, note: f64, velocity: f64, time: f64);
    fn note_off(&mut self, note: f64, time: f64);
    /// Release every sounding note at `time` (Tone `releaseAll`).
    fn release_all(&mut self, time: f64);
}

/// The instrument in a track slot.
pub enum TrackInstrument {
    Drum(Box<dyn DrumInstrument>),
    Synth(Box<dyn SynthInstrument>),
}

/// Who sent a command. Commands are applied in (time, producer, seq) order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProducerId {
    Ui = 0,
    Midi = 1,
}

/// Commands to the core. Pad and transport commands take effect at the stamped time (or the
/// next render quantum for "now"), at an exact frame.
pub enum Command {
    /// Start the transport at `beat`. `at`: context time of the start (None: at the command's
    /// frame). Before `at` the transport is *pending*: its position reads `beat` (R4-B1).
    /// Starting while running re-anchors without silencing (send Stop first to silence).
    Start {
        at: Option<f64>,
        beat: f64,
        generation: u64,
    },
    /// Stop: cancels a pending start (R5-N1), position 0, drum voices faded (stop_all), synths
    /// released, count-in clicks silenced.
    Stop {
        generation: u64,
    },
    /// Move the transport. While pending, only the start beat changes (R5-N4).
    Seek {
        beat: f64,
        generation: u64,
    },
    /// Pre-schedule `beats` count-in clicks at `t0 + i·sec_per_beat` (accent on i % 4 == 0)
    /// on the count-in click synth (replaces an earlier count-in).
    CountIn {
        t0: f64,
        beats: u32,
        sec_per_beat: f64,
        generation: u64,
    },
    SetBpm {
        bpm: f64,
    },
    /// Loop range in beats (None: no loop). Kept in ticks, so tempo changes do not move it.
    SetLoop {
        range: Option<(f64, f64)>,
    },
    /// Transport metronome on/off (the control side applies `shouldClick`).
    SetMetronome {
        enabled: bool,
    },
    SetNoteRepeat {
        enabled: bool,
        rate_beats: f64,
    },
    SetInstrument {
        slot: usize,
        instrument: Option<TrackInstrument>,
    },
    SetSchedule {
        slot: usize,
        schedule: Option<Arc<TrackSchedule>>,
    },
    SetTrackGain {
        slot: usize,
        gain: f32,
    },
    SetTrackMute {
        slot: usize,
        muted: bool,
    },
    /// Which slot live pad hits play (DrumEngine sink). Switching releases the pads still held
    /// on the previous sink (R2-3).
    SetLiveSink {
        slot: Option<usize>,
    },
    /// A pad went down (live: UI or MIDI).
    PadDown {
        pad: PadIndex,
        velocity: f64,
    },
    /// A pad came up: repeats stop at once (R2-4), every slot hit since the last release gets
    /// a release (R2-3).
    PadUp {
        pad: PadIndex,
    },
    /// `stop_all` on one slot's drum instrument / `release_all` on its synth.
    StopVoices {
        slot: usize,
    },
    /// Skip the pattern's next play of drum `note` at absolute straight `beat` once, if it
    /// fires by `until` (LoopRecorder.consumeSuppressedHit).
    SuppressPatternHit {
        note: f64,
        beat: f64,
        until: f64,
    },
}

/// What a capture event reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureKind {
    Hit,
    Release,
    RepeatHit,
}

/// Transport phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Stopped = 0,
    /// Start scheduled (count-in or a future `at`), not reached yet.
    Pending = 1,
    Started = 2,
}

impl Phase {
    pub fn from_u8(v: u8) -> Phase {
        match v {
            1 => Phase::Pending,
            2 => Phase::Started,
            _ => Phase::Stopped,
        }
    }
}

/// A live pad event, stamped by the core at the frame it sounded.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PadCapture {
    pub kind: CaptureKind,
    /// None for repeat hits (generated by the core).
    pub producer: Option<ProducerId>,
    pub pad: PadIndex,
    pub velocity: f64,
    /// Context time it sounded (a frame boundary for live hits, the grid time for repeats).
    pub time: f64,
    /// Transport beat at `time`, from ticks (pending: the start beat; stopped: the position).
    pub beat: f64,
    pub phase: Phase,
    /// Loop wraps so far (running counter) and the sum of the wrapped loop lengths in beats,
    /// both including a wrap at exactly `time`. Wrapped beats between two captures =
    /// the difference (LoopRecorder's "loop events with onTime < w.time ≤ off").
    pub wrap_count: u64,
    pub wrapped_beats: f64,
    /// Repeat hits: the repeat step in beats (`hintBeats`).
    pub grid_beats: Option<f64>,
    /// Transport generation current at `time`.
    pub generation: u64,
}

/// Core → control events (one SPSC ring, read on the UI thread).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CoreEvent {
    Capture(PadCapture),
    Started {
        time: f64,
        beat: f64,
        generation: u64,
    },
    Stopped {
        time: f64,
        generation: u64,
    },
    Seeked {
        time: f64,
        beat: f64,
        generation: u64,
    },
    /// Transport loop ('loop' event): jumped from `from_beat` (loop end) to `to_beat`.
    LoopWrap {
        time: f64,
        from_beat: f64,
        to_beat: f64,
        wrap_count: u64,
        wrapped_beats: f64,
        generation: u64,
    },
    /// A count-in click sounded; `remaining` beats left including this one.
    CountInClick {
        time: f64,
        index: u32,
        remaining: u32,
        generation: u64,
    },
    MetronomeClick {
        time: f64,
        beat: f64,
        accent: bool,
    },
    /// A pattern drum note fired (pad flash).
    PatternHit {
        slot: usize,
        pad: PadIndex,
        time: f64,
    },
}

/// Things the core no longer needs; freed on the control side (never on the audio thread).
pub enum Garbage {
    Schedule(Arc<TrackSchedule>),
    Instrument(TrackInstrument),
}

/// Core sizes. Everything is allocated in `Core::new`.
#[derive(Debug, Clone, Copy)]
pub struct CoreConfig {
    pub sample_rate: u32,
    pub max_tracks: usize,
    pub command_capacity: usize,
    pub event_capacity: usize,
    pub garbage_capacity: usize,
}

impl CoreConfig {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            max_tracks: 64,
            command_capacity: 1024,
            event_capacity: 16384,
            garbage_capacity: 1024,
        }
    }
}
