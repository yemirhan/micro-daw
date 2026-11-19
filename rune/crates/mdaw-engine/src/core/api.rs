//! Types crossing the control ↔ core boundary: instrument traits, commands, events, garbage.

use std::sync::Arc;

use mdaw_dsp::buffer::SharedBuffer;
use mdaw_model::PadIndex;

use super::schedule::TrackSchedule;
use crate::contract::Instrument;
use crate::fx::live::{LiveFx, LiveOp};
use crate::fx::track::{StripOp, TrackStrip};
use crate::io::{InputSource, RecordTap};
use crate::player::SynthPatch;
use crate::plugin::InsertChain;
use crate::sampler::SamplerCommand;

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
    /// [`Self::trigger`], returning a handle of the voice it started (0: none, or an instrument
    /// that cannot tell its voices apart).
    fn trigger_voice(&mut self, pad: PadIndex, velocity: f64, time: f64) -> u64 {
        self.trigger(pad, velocity, time);
        0
    }
    /// Gate release of the one voice `voice` of `pad` ([`Self::trigger_voice`]) at `time`: a
    /// pattern note removed or shortened while it sounds. The pad's other voices are untouched.
    fn release_voice(&mut self, _pad: PadIndex, _voice: u64, _time: f64) {}
    /// Fade out every voice by `time`; voices scheduled later are cancelled (SamplerKit.stopAll).
    fn stop_all(&mut self, time: f64);
    /// Drop voices (of `pad`, or of every pad) that have not started by `time`.
    fn cancel_pending_after(&mut self, pad: Option<PadIndex>, time: f64);
    /// Whether `pad` plays in gate mode right now (pattern notes then get a release at
    /// noteStart + max(0.01, duration), checked at event time like the TS `playDrumNote`).
    fn pad_is_gate(&self, pad: PadIndex) -> bool;
    /// A control-side kit change (`SamplerKit.setKit` / a finished pad load), applied at the
    /// command's frame. Instruments without a kit ignore it.
    fn apply_kit(&mut self, _cmd: SamplerCommand) {}
}

/// An audio track's region players (≙ one `Tone.Player` per audio region, scheduled with
/// `transport.schedule(start(time, offset, dur), regionStart)`). Its schedule has one event per
/// region whose `pad` is the region index.
pub trait AudioInstrument: Instrument {
    /// Start region `region` at `time` (a restart of a region still playing cuts the old
    /// playback at `time`, like `Player.start` on a started player).
    fn start_region(&mut self, region: usize, time: f64);
    /// Stop everything at `time` (`player.stop()` on transport stop).
    fn stop_all(&mut self, time: f64);
    /// B3: a region's sample was reloaded in place; its next start plays `buffer` (a playback
    /// already sounding keeps its old buffer).
    fn set_region_buffer(&mut self, _region: usize, _buffer: Option<SharedBuffer>) {}
    /// An edit during playback: start `region` at `time` as if it had started `elapsed`
    /// seconds earlier (it covers the playhead after the edit), with a short fade-in.
    fn resume_region(&mut self, _region: usize, _time: f64, _elapsed: f64) {}
    /// An edit during playback removed (or changed) the region whose playback has identity
    /// `key` and started at `started`: fade it out quickly from `time` (no click).
    fn fade_out(&mut self, _key: u64, _started: f64, _time: f64) {}
    /// This instrument replaces `old` during playback ([`Command::SetAudioTrack`]): take over its
    /// sounding playbacks (allocation-free), so they continue sample-continuously.
    fn adopt(&mut self, _old: &mut dyn AudioInstrument) {}
    /// For [`Self::adopt`] between instruments of the same type.
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        None
    }
}

/// A pitched instrument (`SynthPlayer`: PolyInstrument / SampledPiano and their chains, see
/// `crate::player`). Same timing rules as [`DrumInstrument`].
pub trait SynthInstrument: Instrument {
    fn note_on(&mut self, note: f64, velocity: f64, time: f64);
    fn note_off(&mut self, note: f64, time: f64);
    /// Release every sounding note at `time` (Tone `releaseAll`).
    fn release_all(&mut self, time: f64);
    /// `triggerAttackRelease(note, duration, time, velocity)`: how pattern notes play (the
    /// instrument's one-shot allocation, gate ≥ 24 ms). Default: a note-on and its note-off.
    fn trigger_attack_release(&mut self, note: f64, duration: f64, time: f64, velocity: f64) {
        self.note_on(note, velocity, time);
        self.note_off(note, time + duration);
    }
    /// End the [`Self::trigger_attack_release`] note `note` struck at `started` at `time`
    /// instead of at its scheduled release: its pattern note left the schedule (or got shorter)
    /// while sounding (a clip deleted, moved or trimmed during playback). One instance only (a
    /// voice is identified by its note and onset): held (live) keys, other strikes of the same
    /// note and a voice re-struck since are untouched. `time` may lie in the future.
    fn cut_scheduled(&mut self, _note: f64, _started: f64, _time: f64) {}
    /// `set(params)` live, prepared on the control side ([`SynthPatch::prepare`]), at context
    /// time `time` (allocation-free; the patch keeps what it replaced and returns through the
    /// garbage ring). False when the instrument cannot take it (another engine).
    fn apply_patch(&mut self, _patch: &mut SynthPatch, _time: f64) -> bool {
        false
    }
    /// The instrument was replaced and only rings out from now on (it stops publishing its
    /// status: the slot's new instrument owns it).
    fn retire(&mut self) {}
}

/// A live note command on a slot's synth ([`Command::SynthNote`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SynthNoteOp {
    On { note: f64, velocity: f64 },
    Off { note: f64 },
    ReleaseAll,
}

/// The instrument in a track slot.
pub enum TrackInstrument {
    Drum(Box<dyn DrumInstrument>),
    Synth(Box<dyn SynthInstrument>),
    Audio(Box<dyn AudioInstrument>),
}

impl TrackInstrument {
    pub(crate) fn as_instrument(&mut self) -> &mut dyn Instrument {
        match self {
            TrackInstrument::Drum(d) => &mut **d,
            TrackInstrument::Synth(s) => &mut **s,
            TrackInstrument::Audio(a) => &mut **a,
        }
    }
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
    /// on the count-in click synth (replaces an earlier count-in) AND start the transport at
    /// `start_beat` at their end (`t0 + beats·sec_per_beat`): one command, so the two can never
    /// be split.
    CountIn {
        t0: f64,
        beats: u32,
        sec_per_beat: f64,
        start_beat: f64,
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
    /// An audio track's region players and schedule, swapped together at one frame (the event
    /// `pad`s index the new instrument's regions). While the transport runs, the new instrument
    /// adopts the old one's sounding playbacks ([`AudioInstrument::adopt`]); then the schedule
    /// swap reconciles them like [`Command::SetSchedule`] does (removed regions fade out,
    /// regions covering the playhead resume).
    SetAudioTrack {
        slot: usize,
        instrument: Option<TrackInstrument>,
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
    /// Track pan (−1..1): `panner.pan.value = pan` on the slot's track strip (no strip: ignored).
    SetTrackPan {
        slot: usize,
        pan: f32,
    },
    /// Run the slot through an effect chain before its gain and mute: a track strip
    /// (`createTrackAudioNodes`, `fx/track.rs`) or the live synth bus (`fx/live.rs`). None
    /// (default): instrument × gain, as the core tests expect.
    SetTrackStrip {
        slot: usize,
        strip: Option<SlotFx>,
    },
    /// A change to the slot's track strip at the command's time (boxed: the ring slots stay
    /// small; the box goes back through the garbage ring).
    StripOp {
        slot: usize,
        op: Box<StripOp>,
    },
    /// An `AudioEngine` setter on the slot's live synth bus.
    LiveOp {
        slot: usize,
        op: LiveOp,
    },
    /// A slot only for live hits (drumEngine's fallback kit): transport Stop leaves it alone.
    SetLiveOnly {
        slot: usize,
        live_only: bool,
    },
    /// Master bus (`installMasterBus`): `Some(db)` = the master `Volume` at `db` (fader − the
    /// −12 dB reference), +2 dB drive and the soft clipper on the summed output; None (default) =
    /// no master processing.
    SetMasterBus {
        db: Option<f64>,
    },
    /// `setMasterVolumeDb`: the master `Volume` ramps to `db` in 30 ms (`rampTo`).
    MasterVolume {
        db: f64,
    },
    /// A kit change for a slot's drum instrument (boxed: the ring slots stay small; the empty
    /// box goes back through the garbage ring).
    KitCommand {
        slot: usize,
        cmd: Box<SamplerCommand>,
    },
    /// B3: a reloaded buffer for one region of a slot's audio instrument.
    SetRegionBuffer {
        slot: usize,
        region: usize,
        buffer: Option<SharedBuffer>,
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
    /// A one-shot pad hit (`drumHit`: pad previews): played and captured like
    /// [`Command::PadDown`], never note-repeated.
    PadHit {
        pad: PadIndex,
        velocity: f64,
    },
    /// A pad came up: repeats stop at once (R2-4), every slot hit since the last release gets
    /// a release (R2-3).
    PadUp {
        pad: PadIndex,
    },
    /// `useMidiNotes`' `muted` for pads: while set, live and note-repeat pad hits are still
    /// captured (recorded, flashed) and repeated, but not played (`hitPad` skips
    /// `drumEngine.hit`). Releases are unaffected.
    SetLivePadMute {
        muted: bool,
    },
    /// `noteRepeat.releaseAll()` (panic): every held pad stops repeating; no pad is released.
    RepeatReleaseAll,
    /// `stop_all` on one slot's drum instrument / `release_all` on its synth.
    StopVoices {
        slot: usize,
    },
    /// `playDrumNote(sampler, midi, vel, now, dur)` on a drum track's own kit (the piano roll's
    /// `previewNote`): trigger `pad` now; a gate pad gets its release `max(0.01, duration)` later.
    DrumNote {
        slot: usize,
        pad: PadIndex,
        velocity: f64,
        duration: f64,
    },
    /// A live note on a slot's synth (`AudioEngine.noteOn` / `noteOff` / `releaseAll`), at the
    /// command's time + `delay` (the live path's `LIVE_OFFSET`).
    SynthNote {
        slot: usize,
        op: SynthNoteOp,
        delay: f64,
    },
    /// `instrument.set(patch)` on a slot's synth (`setSynthParam`): everything that allocates
    /// (the merged preset, wave tables, noise buffers, fresh voices for the unused slots) is
    /// prepared on the control side ([`SynthPatch::prepare`]); the core swaps it in without
    /// allocating and returns the box (holding what it replaced) through the garbage ring.
    /// Queued edits of one slot coalesce: only the latest applies.
    SynthSet {
        slot: usize,
        patch: Box<SynthPatch>,
    },
    /// Replace a slot's synth and retire the old one (`AudioEngine.createInstrument`): the old
    /// instrument gets `release_all` at `t + release_delay` and keeps rendering into the slot
    /// until `t + retire_after`, then goes to the garbage ring.
    ReplaceSynth {
        slot: usize,
        instrument: Option<TrackInstrument>,
        release_delay: f64,
        retire_after: f64,
    },
    /// Skip the pattern's next play of drum `note` at absolute straight `beat` once, if it
    /// fires by `until` (LoopRecorder.consumeSuppressedHit). Scoped to the transport run
    /// `generation` (the take's): ignored if stale, and the table is cleared by every Stop and
    /// Start (the TS clears `suppressedHits` when the take ends).
    SuppressPatternHit {
        note: f64,
        beat: f64,
        until: f64,
        generation: u64,
    },
    /// A live synth note for Song recording (`captureNoteOn(note, v, false)` /
    /// `captureNoteOff(note)` of `useMidiNotes`): the core emits [`CoreEvent::SynthCapture`] at
    /// the command's frame with the transport beat and generation there, in order with the pad
    /// captures and punch closes. The UI decides whether a recording takes it.
    CaptureSynthNote {
        note: f64,
        velocity: f64,
        on: bool,
    },
    /// Punch recording: emit [`CoreEvent::PunchClose`] each time the running transport reaches
    /// integer `tick` (`transport.schedule(closeOpenNotes, loopEnd − 50 ms)`); None: off.
    /// Cleared by Stop.
    SetPunchClose {
        tick: Option<i64>,
    },
    /// The device input the core reads every quantum (`crate::io`); None: no input (every
    /// input channel reads silence).
    SetInput {
        source: Option<Box<InputSource>>,
    },
    /// Which input channels feed a slot (an armed audio track): added to the slot's signal
    /// before its insert chain and track strip when `monitor` is set. None: no input.
    SetSlotInput {
        slot: usize,
        input: Option<SlotInput>,
    },
    /// Record a slot's dry input (its [`SlotInput`] channels; silence without one) into a take,
    /// one quantum per quantum from the command's quantum on. None: stop.
    SetSlotRecord {
        slot: usize,
        tap: Option<Box<RecordTap>>,
    },
    /// A slot's insert effects (hosted plugins), between its instrument / input and its track
    /// strip. None: no inserts.
    SetInserts {
        slot: usize,
        chain: Option<Box<InsertChain>>,
    },
}

/// A slot's live input ([`Command::SetSlotInput`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotInput {
    /// Input channel on the left (and on the right too when `right` is None: a mono input is
    /// heard in the centre).
    pub left: usize,
    pub right: Option<usize>,
    /// Hear the input through the slot (its inserts, strip, gain and mute).
    pub monitor: bool,
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

/// Core → control events, read on the UI thread. Two rings: **critical** (captures, loop
/// wraps, punch closes, transport) — an overrun is latched in `Shared::critical_overruns` and
/// must be handled by the recorders — and **visual** (pattern hits, clicks), which may drop.
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
    /// A live synth note to record (see [`Command::CaptureSynthNote`]).
    SynthCapture {
        note: f64,
        velocity: f64,
        on: bool,
        /// Context time of the note (its frame).
        time: f64,
        /// Transport beat at `time` (pending: the start beat; stopped: the position).
        beat: f64,
        /// Transport generation current at `time`.
        generation: u64,
    },
    /// The punch close tick was reached (see [`Command::SetPunchClose`]).
    PunchClose {
        time: f64,
        generation: u64,
    },
}

impl CoreEvent {
    /// Whether losing this event would corrupt a recording or the transport state.
    pub fn is_critical(&self) -> bool {
        !matches!(
            self,
            CoreEvent::PatternHit { .. }
                | CoreEvent::MetronomeClick { .. }
                | CoreEvent::CountInClick { .. }
        )
    }
}

/// Things the core no longer needs; freed on the control side (never on the audio thread).
pub enum Garbage {
    Schedule(Arc<TrackSchedule>),
    Instrument(TrackInstrument),
    /// A buffer an instrument let go of (SamplerKit / audio regions).
    Buffer(SharedBuffer),
    /// An applied kit command's box.
    KitCommand(Box<SamplerCommand>),
    /// A replaced effect chain.
    Strip(SlotFx),
    /// An applied strip op's box.
    StripOp(Box<StripOp>),
    /// An applied `SynthSet`'s patch (the replaced preset and voices).
    Patch(Box<SynthPatch>),
    /// A replaced device input.
    Input(Box<InputSource>),
    /// A finished take's core end.
    RecordTap(Box<RecordTap>),
    /// A replaced insert chain (its plugins' render halves).
    Inserts(Box<InsertChain>),
}

/// A slot's effect chain (see [`Command::SetTrackStrip`]).
pub enum SlotFx {
    Track(Box<TrackStrip>),
    Live(Box<LiveFx>),
}

/// Most track slots a core has (documented limit: [`crate::engine::MAX_TRACKS`] document
/// tracks plus drumEngine's fallback kit, the live synth, the pack store's sound preview and
/// the Samples view's audition).
pub const MAX_SLOTS: usize = 260;

/// Core sizes. Everything is allocated in `Core::new`.
#[derive(Debug, Clone, Copy)]
pub struct CoreConfig {
    pub sample_rate: u32,
    /// Track slots (1..=[`MAX_SLOTS`]).
    pub max_tracks: usize,
    /// Per producer.
    pub command_capacity: usize,
    /// Critical events (captures, wraps, punch closes, transport).
    pub event_capacity: usize,
    /// Visual events (pattern hits, clicks).
    pub visual_capacity: usize,
    pub garbage_capacity: usize,
}

impl CoreConfig {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            max_tracks: MAX_SLOTS,
            command_capacity: 1024,
            event_capacity: 16384,
            visual_capacity: 16384,
            garbage_capacity: 8192,
        }
    }
}
