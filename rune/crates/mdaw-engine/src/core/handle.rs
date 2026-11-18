//! Control-side handles: [`CoreHandle`] (UI thread) and [`LiveInput`] (MIDI thread).
//!
//! **Never dropped.** A command that does not fit in its producer's ring waits in that producer's
//! **outbox** (a `VecDeque` on the control side) and is pushed, in order, by the next send or
//! [`CoreHandle::flush`] (the engine flushes on every `pump` / `tick`; the MIDI thread on every
//! send and should also call [`LiveInput::flush`] periodically). Every command is therefore
//! either submitted or queued: the transport generation a `start` / `stop` / `seek` returns is
//! always the one the core will see, in the order it was sent.
//!
//! - **Coalescing.** While queued, a latest-value setter (track gain / pan / mute / schedule per
//!   slot, tempo, loop, metronome, master bus, note-repeat settings) replaces an older queued
//!   setter with the same key, unless a transport or instrument command lies between them (the
//!   older value must then still apply in its place). A fader drag during a stall therefore
//!   queues one command, not thousands.
//! - **Atomic submissions.** A count-in carries its transport start (`Command::CountIn`), so the
//!   two can never be split.
//! - **Per-producer order.** Each producer stamps its commands with a non-decreasing time: "now"
//!   is `max(previous time, core now)` and a timed command is clamped to at least the previous
//!   one, so a producer's commands apply in the order they were sent (a `pad_up` "now" after a
//!   `pad_down` 1 ms ahead is not applied before it). The core merges the producers by
//!   `(time, producer, seq)`.
//! - **Garbage backpressure.** A command whose payload the core will retire (a schedule, an
//!   instrument, a strip, a kit change or a buffer) is only pushed while the garbage return ring
//!   has room for its worst case plus a reserve for voices still playing replaced buffers;
//!   otherwise it waits in the outbox (with everything after it) until the control side has
//!   collected garbage. So the audio thread never has to free (or leak) a retired payload.

use std::collections::VecDeque;
use std::sync::Arc;

use mdaw_model::{PAD_COUNT, PadIndex};

use super::api::{Command, CoreEvent, Garbage, ProducerId, SlotFx, SynthNoteOp, TrackInstrument};
use super::ring::{Consumer, Producer};
use super::schedule::TrackSchedule;
use super::shared::Shared;
use crate::fx::live::LiveOp;
use crate::fx::state::FxState;
use crate::fx::track::{StripOp, TrackStrip};
use crate::sampler::{SamplerCommand, VOICE_POOL};
use crate::synth::presets::SynthPreset;

/// A command with its target time, producer and sequence number.
pub(crate) struct Stamped {
    /// Context seconds (non-decreasing per producer).
    pub time: f64,
    pub producer: ProducerId,
    pub seq: u64,
    pub cmd: Command,
}

impl Stamped {
    /// Merge order `(time, producer, seq)`.
    pub fn key(&self) -> (f64, ProducerId, u64) {
        (self.time, self.producer, self.seq)
    }
}

/// Garbage-ring room kept free for buffers released later by voices that still play a replaced
/// buffer (one per voice of a kit or region player).
pub(crate) const GARBAGE_RESERVE: usize = 2 * VOICE_POOL;

/// Worst-case number of garbage items the core retires when applying `cmd`.
fn garbage_cost(cmd: &Command) -> usize {
    match cmd {
        Command::SetInstrument { .. }
        | Command::SetSchedule { .. }
        | Command::SetTrackStrip { .. }
        | Command::StripOp { .. }
        | Command::SetRegionBuffer { .. }
        | Command::SynthSet { .. } => 1,
        // a full retire list evicts one now; the retired one leaves later
        Command::ReplaceSynth { .. } => 2,
        Command::KitCommand { cmd, .. } => {
            1 + match **cmd {
                SamplerCommand::SetKit { .. } => 2 * PAD_COUNT,
                SamplerCommand::SetPadBuffer { .. } => 2,
            }
        }
        _ => 0,
    }
}

/// Key of a latest-value setter (see the module docs), None for every other command.
fn coalesce_key(cmd: &Command) -> Option<(u8, usize)> {
    Some(match cmd {
        Command::SetTrackGain { slot, .. } => (0, *slot),
        Command::SetTrackPan { slot, .. } => (1, *slot),
        Command::SetTrackMute { slot, .. } => (2, *slot),
        Command::SetSchedule { slot, .. } => (3, *slot),
        Command::SetBpm { .. } => (4, 0),
        Command::SetLoop { .. } => (5, 0),
        Command::SetMetronome { .. } => (6, 0),
        Command::SetMasterBus { .. } => (7, 0),
        Command::SetNoteRepeat { .. } => (8, 0),
        _ => return None,
    })
}

/// Commands a setter must not be moved across.
fn is_barrier(cmd: &Command) -> bool {
    matches!(
        cmd,
        Command::Start { .. }
            | Command::Stop { .. }
            | Command::Seek { .. }
            | Command::CountIn { .. }
            | Command::SetInstrument { .. }
            | Command::ReplaceSynth { .. }
            | Command::StopVoices { .. }
            | Command::SetLiveSink { .. }
    )
}

/// Sends commands from one producer thread (see the module docs).
pub(crate) struct Sender {
    tx: Producer<Stamped>,
    producer: ProducerId,
    seq: u64,
    last_time: f64,
    outbox: VecDeque<Stamped>,
    /// (seq, cost) of pushed commands that retire garbage, until the core applied them.
    inflight: VecDeque<(u64, usize)>,
    inflight_cost: usize,
    shared: Arc<Shared>,
}

impl Sender {
    pub fn new(tx: Producer<Stamped>, producer: ProducerId, shared: Arc<Shared>) -> Self {
        Self {
            tx,
            producer,
            seq: 0,
            last_time: 0.0,
            outbox: VecDeque::new(),
            inflight: VecDeque::new(),
            inflight_cost: 0,
            shared,
        }
    }

    fn stamp(&mut self, time: Option<f64>) -> f64 {
        let t = match time.filter(|t| t.is_finite()) {
            Some(t) => t,
            None => self.shared.now(),
        };
        self.last_time = t.max(self.last_time);
        self.last_time
    }

    /// Queue `cmd` (coalescing it into a queued setter when allowed), then flush.
    fn send(&mut self, time: Option<f64>, cmd: Command, garbage: Option<(usize, usize)>) {
        let time = self.stamp(time);
        if let Some(key) = coalesce_key(&cmd) {
            for s in self.outbox.iter_mut().rev() {
                if is_barrier(&s.cmd) {
                    break;
                }
                if coalesce_key(&s.cmd) == Some(key) {
                    s.cmd = cmd; // the older value is dropped here, on the control side
                    return;
                }
            }
        }
        self.seq += 1;
        self.outbox.push_back(Stamped {
            time,
            producer: self.producer,
            seq: self.seq,
            cmd,
        });
        self.flush(garbage);
    }

    /// Push queued commands while the ring (and, for garbage-producing commands, the garbage
    /// ring: `(waiting items, capacity)`) has room.
    fn flush(&mut self, garbage: Option<(usize, usize)>) {
        let applied = self.shared.applied(self.producer as usize);
        while let Some(&(seq, cost)) = self.inflight.front() {
            if seq > applied {
                break;
            }
            self.inflight.pop_front();
            self.inflight_cost -= cost;
        }
        while let Some(front) = self.outbox.front() {
            let cost = garbage_cost(&front.cmd);
            if cost > 0
                && let Some((waiting, cap)) = garbage
                && waiting + self.inflight_cost + cost + GARBAGE_RESERVE > cap
                && self.inflight_cost + waiting > 0
            {
                break;
            }
            let Some(s) = self.outbox.pop_front() else {
                break;
            };
            let seq = s.seq;
            match self.tx.push(s) {
                Ok(()) => {
                    if cost > 0 {
                        self.inflight.push_back((seq, cost));
                        self.inflight_cost += cost;
                    }
                }
                Err(s) => {
                    self.outbox.push_front(s);
                    break;
                }
            }
        }
    }

    fn queued(&self) -> usize {
        self.outbox.len()
    }
}

/// The MIDI thread's input: pad hits straight to the core (no UI frame in between).
pub struct LiveInput {
    sender: Sender,
    shared: Arc<Shared>,
}

impl LiveInput {
    pub(crate) fn new(tx: Producer<Stamped>, shared: Arc<Shared>) -> Self {
        Self {
            sender: Sender::new(tx, ProducerId::Midi, shared.clone()),
            shared,
        }
    }
    /// `time`: context seconds the hit should sound (None: as soon as possible). It sounds at
    /// the first frame ≥ that time, never earlier than the next render quantum, and never
    /// before an earlier command of this producer.
    pub fn pad_down(&mut self, pad: PadIndex, velocity: f64, time: Option<f64>) {
        self.sender
            .send(time, Command::PadDown { pad, velocity }, None);
    }
    pub fn pad_up(&mut self, pad: PadIndex, time: Option<f64>) {
        self.sender.send(time, Command::PadUp { pad }, None);
    }
    /// A live note on a slot's synth (the live keyboard) at the next render quantum + `delay`.
    pub fn synth_note(&mut self, slot: usize, op: SynthNoteOp, delay: f64) {
        self.sender
            .send(None, Command::SynthNote { slot, op, delay }, None);
    }
    /// Push commands still waiting for room in the ring.
    pub fn flush(&mut self) {
        self.sender.flush(None);
    }
    /// Commands waiting in the outbox.
    pub fn queued(&self) -> usize {
        self.sender.queued()
    }
    pub fn shared(&self) -> &Shared {
        &self.shared
    }
}

/// The UI thread's handle on the core.
pub struct CoreHandle {
    sender: Sender,
    generation: u64,
    events: Consumer<CoreEvent>,
    visual: Consumer<CoreEvent>,
    garbage: Consumer<Garbage>,
    garbage_capacity: usize,
    shared: Arc<Shared>,
}

impl CoreHandle {
    pub(crate) fn new(
        tx: Producer<Stamped>,
        (events, visual): (Consumer<CoreEvent>, Consumer<CoreEvent>),
        (garbage, garbage_capacity): (Consumer<Garbage>, usize),
        shared: Arc<Shared>,
    ) -> Self {
        Self {
            sender: Sender::new(tx, ProducerId::Ui, shared.clone()),
            generation: 0,
            events,
            visual,
            garbage,
            garbage_capacity,
            shared,
        }
    }

    pub fn shared(&self) -> &Shared {
        &self.shared
    }
    /// The shared atomics (for helpers that outlive a borrow of the handle).
    pub fn shared_arc(&self) -> Arc<Shared> {
        self.shared.clone()
    }
    pub fn now(&self) -> f64 {
        self.shared.now()
    }
    /// The generation of the last transport command sent (or queued).
    pub fn generation(&self) -> u64 {
        self.generation
    }

    fn garbage_state(&self) -> Option<(usize, usize)> {
        Some((self.garbage.len(), self.garbage_capacity))
    }

    /// Send a raw command (timed or "now"). Never dropped: queued while the ring is full.
    pub fn send(&mut self, time: Option<f64>, cmd: Command) {
        let g = self.garbage_state();
        self.sender.send(time, cmd, g);
    }

    /// Push queued commands (call every UI frame; the engine's `pump` does).
    pub fn flush(&mut self) {
        let g = self.garbage_state();
        self.sender.flush(g);
    }

    /// Commands waiting in the outbox.
    pub fn queued(&self) -> usize {
        self.sender.queued()
    }

    fn next_gen(&mut self) -> u64 {
        self.generation += 1;
        self.generation
    }

    /// Start at `beat`, at context time `at` (None: the next render quantum). Returns the
    /// transport generation of this run.
    pub fn start(&mut self, at: Option<f64>, beat: f64) -> u64 {
        let generation = self.next_gen();
        self.send(
            None,
            Command::Start {
                at,
                beat,
                generation,
            },
        );
        generation
    }

    /// Loop-record start: `beats` count-in clicks from `t0`, transport start at
    /// `t0 + beats·sec_per_beat` at `beat`, as ONE command. Returns (generation, count-in end).
    pub fn start_with_count_in(
        &mut self,
        beat: f64,
        t0: f64,
        beats: u32,
        sec_per_beat: f64,
    ) -> (u64, f64) {
        let generation = self.next_gen();
        let end = t0 + beats as f64 * sec_per_beat;
        self.send(
            None,
            Command::CountIn {
                t0,
                beats,
                sec_per_beat,
                start_beat: beat,
                generation,
            },
        );
        (generation, end)
    }

    pub fn stop(&mut self) -> u64 {
        let generation = self.next_gen();
        self.send(None, Command::Stop { generation });
        generation
    }

    pub fn seek(&mut self, beat: f64) -> u64 {
        let generation = self.next_gen();
        self.send(None, Command::Seek { beat, generation });
        generation
    }

    pub fn set_bpm(&mut self, bpm: f64) {
        self.send(None, Command::SetBpm { bpm })
    }
    pub fn set_loop(&mut self, range: Option<(f64, f64)>) {
        self.send(None, Command::SetLoop { range })
    }
    pub fn set_metronome(&mut self, enabled: bool) {
        self.send(None, Command::SetMetronome { enabled })
    }
    pub fn set_note_repeat(&mut self, enabled: bool, rate_beats: f64) {
        self.send(
            None,
            Command::SetNoteRepeat {
                enabled,
                rate_beats,
            },
        )
    }
    pub fn set_instrument(&mut self, slot: usize, instrument: Option<TrackInstrument>) {
        self.send(None, Command::SetInstrument { slot, instrument })
    }
    pub fn set_schedule(&mut self, slot: usize, schedule: Option<Arc<TrackSchedule>>) {
        self.send(None, Command::SetSchedule { slot, schedule })
    }
    pub fn set_track_gain(&mut self, slot: usize, gain: f32) {
        self.send(None, Command::SetTrackGain { slot, gain })
    }
    pub fn set_track_mute(&mut self, slot: usize, muted: bool) {
        self.send(None, Command::SetTrackMute { slot, muted })
    }
    pub fn set_track_pan(&mut self, slot: usize, pan: f32) {
        self.send(None, Command::SetTrackPan { slot, pan })
    }
    /// Give the slot an effect chain (built on this thread: it allocates), or none.
    pub fn set_track_strip(&mut self, slot: usize, strip: Option<SlotFx>) {
        self.send(None, Command::SetTrackStrip { slot, strip })
    }
    /// A fresh track strip for `fx` / pan / volume (`createTrackAudioNodes`), built at the core's
    /// current time.
    pub fn new_track_strip(&self, fx: &FxState, pan: f64, volume_db: f64) -> SlotFx {
        SlotFx::Track(Box::new(TrackStrip::new(
            fx,
            pan,
            volume_db,
            self.shared.sample_rate,
            self.now(),
        )))
    }
    /// A change to the slot's track strip at the next render quantum.
    pub fn strip_op(&mut self, slot: usize, op: StripOp) {
        self.send(
            None,
            Command::StripOp {
                slot,
                op: Box::new(op),
            },
        )
    }
    /// An `AudioEngine` setter on the slot's live bus at the next render quantum.
    pub fn live_op(&mut self, slot: usize, op: LiveOp) {
        self.send(None, Command::LiveOp { slot, op })
    }
    pub fn set_live_only(&mut self, slot: usize, live_only: bool) {
        self.send(None, Command::SetLiveOnly { slot, live_only })
    }
    pub fn set_master_bus(&mut self, db: Option<f64>) {
        self.send(None, Command::SetMasterBus { db })
    }
    pub fn set_master_volume(&mut self, db: f64) {
        self.send(None, Command::MasterVolume { db })
    }
    pub fn kit_command(&mut self, slot: usize, cmd: SamplerCommand) {
        self.send(
            None,
            Command::KitCommand {
                slot,
                cmd: Box::new(cmd),
            },
        )
    }
    pub fn set_region_buffer(
        &mut self,
        slot: usize,
        region: usize,
        buffer: Option<mdaw_dsp::buffer::SharedBuffer>,
    ) {
        self.send(
            None,
            Command::SetRegionBuffer {
                slot,
                region,
                buffer,
            },
        )
    }
    pub fn stop_voices(&mut self, slot: usize) {
        self.send(None, Command::StopVoices { slot })
    }
    pub fn set_live_sink(&mut self, slot: Option<usize>) {
        self.send(None, Command::SetLiveSink { slot })
    }
    /// A live note on a slot's synth "now" (the next render quantum) + `delay` seconds.
    pub fn synth_note(&mut self, slot: usize, op: SynthNoteOp, delay: f64) {
        self.send(None, Command::SynthNote { slot, op, delay })
    }
    /// `instrument.set(...)` with the merged preset (see [`Command::SynthSet`]).
    pub fn synth_set(&mut self, slot: usize, preset: SynthPreset) {
        self.send(
            None,
            Command::SynthSet {
                slot,
                preset: Box::new(preset),
            },
        )
    }
    /// Swap a slot's synth, the old one releasing and retiring (see [`Command::ReplaceSynth`]).
    pub fn replace_synth(
        &mut self,
        slot: usize,
        instrument: Option<TrackInstrument>,
        release_delay: f64,
        retire_after: f64,
    ) {
        self.send(
            None,
            Command::ReplaceSynth {
                slot,
                instrument,
                release_delay,
                retire_after,
            },
        )
    }
    pub fn pad_down(&mut self, pad: PadIndex, velocity: f64, time: Option<f64>) {
        self.send(time, Command::PadDown { pad, velocity })
    }
    pub fn pad_up(&mut self, pad: PadIndex, time: Option<f64>) {
        self.send(time, Command::PadUp { pad })
    }
    /// Skip the pattern's next play of drum `note` at `beat` once, if it fires by `until`, in
    /// the transport run `generation` only.
    pub fn suppress_pattern_hit(&mut self, note: f64, beat: f64, until: f64, generation: u64) {
        self.send(
            None,
            Command::SuppressPatternHit {
                note,
                beat,
                until,
                generation,
            },
        )
    }
    /// Emit a `PunchClose` event each time the running transport reaches `tick` (None: off).
    pub fn set_punch_close(&mut self, tick: Option<i64>) {
        self.send(None, Command::SetPunchClose { tick })
    }

    /// Next core event, if any: recording-critical events first (captures, wraps, punch
    /// closes, transport), then visual ones (pad flashes, clicks).
    pub fn poll_event(&mut self) -> Option<CoreEvent> {
        self.events.pop().or_else(|| self.visual.pop())
    }

    /// Free what the core returned (schedules, instruments, buffers). Returns how many items.
    pub fn collect_garbage(&mut self) -> usize {
        let mut n = 0;
        while let Some(g) = self.garbage.pop() {
            drop(g);
            n += 1;
        }
        n
    }
}
