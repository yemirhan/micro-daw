//! Control-side handles: [`CoreHandle`] (UI thread) and [`LiveInput`] (MIDI thread), plus the
//! atomics the core publishes for the UI.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};

use mdaw_model::PadIndex;

use super::api::{Command, CoreEvent, Garbage, Phase, ProducerId, TrackInstrument};
use super::ring::{Consumer, Producer};
use super::schedule::TrackSchedule;

/// A command with its target time, producer and sequence number.
pub(crate) struct Stamped {
    /// Context seconds; None = at the next render quantum.
    pub time: Option<f64>,
    pub producer: ProducerId,
    pub seq: u64,
    pub cmd: Command,
}

impl Stamped {
    /// Merge order `(time, producer, seq)`; "now" sorts first.
    pub fn key(&self) -> (f64, ProducerId, u64) {
        (
            self.time.unwrap_or(f64::NEG_INFINITY),
            self.producer,
            self.seq,
        )
    }
}

/// Published by the core after every render quantum.
pub struct Shared {
    pub sample_rate: u32,
    /// Frames rendered (the core's "now" = frames / sample_rate).
    frames: AtomicU64,
    beat_bits: AtomicU64,
    bpm_bits: AtomicU64,
    phase: AtomicU8,
    generation: AtomicU64,
    wrap_count: AtomicU64,
    repeating_pads: AtomicU64,
    dropped_events: AtomicU64,
    leaked_garbage: AtomicU64,
}

impl Shared {
    pub(crate) fn new(sample_rate: u32, bpm: f64) -> Self {
        Self {
            sample_rate,
            frames: AtomicU64::new(0),
            beat_bits: AtomicU64::new(0f64.to_bits()),
            bpm_bits: AtomicU64::new(bpm.to_bits()),
            phase: AtomicU8::new(Phase::Stopped as u8),
            generation: AtomicU64::new(0),
            wrap_count: AtomicU64::new(0),
            repeating_pads: AtomicU64::new(0),
            dropped_events: AtomicU64::new(0),
            leaked_garbage: AtomicU64::new(0),
        }
    }

    pub(crate) fn publish(
        &self,
        frames: u64,
        beat: f64,
        bpm: f64,
        phase: Phase,
        generation: u64,
        wraps: u64,
    ) {
        self.beat_bits.store(beat.to_bits(), Ordering::Relaxed);
        self.bpm_bits.store(bpm.to_bits(), Ordering::Relaxed);
        self.phase.store(phase as u8, Ordering::Relaxed);
        self.generation.store(generation, Ordering::Relaxed);
        self.wrap_count.store(wraps, Ordering::Relaxed);
        self.frames.store(frames, Ordering::Release);
    }

    pub(crate) fn publish_repeating(&self, mask: u64) {
        self.repeating_pads.store(mask, Ordering::Relaxed);
    }

    pub(crate) fn count_dropped_event(&self) {
        self.dropped_events.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn count_leak(&self) {
        self.leaked_garbage.fetch_add(1, Ordering::Relaxed);
    }

    pub fn frames(&self) -> u64 {
        self.frames.load(Ordering::Acquire)
    }
    /// Core time in context seconds (start of the next render quantum).
    pub fn now(&self) -> f64 {
        self.frames() as f64 / self.sample_rate as f64
    }
    /// Playhead (`getPosition`): the transport beat at [`Shared::now`].
    pub fn playhead_beat(&self) -> f64 {
        f64::from_bits(self.beat_bits.load(Ordering::Relaxed))
    }
    pub fn bpm(&self) -> f64 {
        f64::from_bits(self.bpm_bits.load(Ordering::Relaxed))
    }
    pub fn phase(&self) -> Phase {
        Phase::from_u8(self.phase.load(Ordering::Relaxed))
    }
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }
    pub fn wrap_count(&self) -> u64 {
        self.wrap_count.load(Ordering::Relaxed)
    }
    /// Bit per pad: held and repeating (`NoteRepeat.isRepeating`).
    pub fn repeating_pads(&self) -> u64 {
        self.repeating_pads.load(Ordering::Relaxed)
    }
    /// Events lost because the event ring was full.
    pub fn dropped_events(&self) -> u64 {
        self.dropped_events.load(Ordering::Relaxed)
    }
    /// Garbage the core had to leak (garbage ring and overflow full).
    pub fn leaked_garbage(&self) -> u64 {
        self.leaked_garbage.load(Ordering::Relaxed)
    }
}

/// Sends commands from one producer thread.
struct Sender {
    tx: Producer<Stamped>,
    producer: ProducerId,
    seq: u64,
}

impl Sender {
    fn send(&mut self, time: Option<f64>, cmd: Command) -> bool {
        self.seq += 1;
        let stamped = Stamped {
            time: time.filter(|t| t.is_finite()),
            producer: self.producer,
            seq: self.seq,
            cmd,
        };
        self.tx.push(stamped).is_ok()
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
            sender: Sender {
                tx,
                producer: ProducerId::Midi,
                seq: 0,
            },
            shared,
        }
    }
    /// `time`: context seconds the hit should sound (None: as soon as possible). It sounds at
    /// the first frame ≥ that time, never earlier than the next render quantum.
    pub fn pad_down(&mut self, pad: PadIndex, velocity: f64, time: Option<f64>) -> bool {
        self.sender.send(time, Command::PadDown { pad, velocity })
    }
    pub fn pad_up(&mut self, pad: PadIndex, time: Option<f64>) -> bool {
        self.sender.send(time, Command::PadUp { pad })
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
    garbage: Consumer<Garbage>,
    shared: Arc<Shared>,
}

impl CoreHandle {
    pub(crate) fn new(
        tx: Producer<Stamped>,
        events: Consumer<CoreEvent>,
        garbage: Consumer<Garbage>,
        shared: Arc<Shared>,
    ) -> Self {
        Self {
            sender: Sender {
                tx,
                producer: ProducerId::Ui,
                seq: 0,
            },
            generation: 0,
            events,
            garbage,
            shared,
        }
    }

    pub fn shared(&self) -> &Shared {
        &self.shared
    }
    pub fn now(&self) -> f64 {
        self.shared.now()
    }
    /// The generation of the last transport command sent.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Send a raw command (timed or "now").
    pub fn send(&mut self, time: Option<f64>, cmd: Command) -> bool {
        self.sender.send(time, cmd)
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
    /// `t0 + beats·sec_per_beat` at `beat`. Returns (generation, count-in end).
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
                generation,
            },
        );
        self.send(
            None,
            Command::Start {
                at: Some(end),
                beat,
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

    pub fn set_bpm(&mut self, bpm: f64) -> bool {
        self.send(None, Command::SetBpm { bpm })
    }
    pub fn set_loop(&mut self, range: Option<(f64, f64)>) -> bool {
        self.send(None, Command::SetLoop { range })
    }
    pub fn set_metronome(&mut self, enabled: bool) -> bool {
        self.send(None, Command::SetMetronome { enabled })
    }
    pub fn set_note_repeat(&mut self, enabled: bool, rate_beats: f64) -> bool {
        self.send(
            None,
            Command::SetNoteRepeat {
                enabled,
                rate_beats,
            },
        )
    }
    pub fn set_instrument(&mut self, slot: usize, instrument: Option<TrackInstrument>) -> bool {
        self.send(None, Command::SetInstrument { slot, instrument })
    }
    pub fn set_schedule(&mut self, slot: usize, schedule: Option<Arc<TrackSchedule>>) -> bool {
        self.send(None, Command::SetSchedule { slot, schedule })
    }
    pub fn set_track_gain(&mut self, slot: usize, gain: f32) -> bool {
        self.send(None, Command::SetTrackGain { slot, gain })
    }
    pub fn set_track_mute(&mut self, slot: usize, muted: bool) -> bool {
        self.send(None, Command::SetTrackMute { slot, muted })
    }
    pub fn set_live_sink(&mut self, slot: Option<usize>) -> bool {
        self.send(None, Command::SetLiveSink { slot })
    }
    pub fn pad_down(&mut self, pad: PadIndex, velocity: f64, time: Option<f64>) -> bool {
        self.send(time, Command::PadDown { pad, velocity })
    }
    pub fn pad_up(&mut self, pad: PadIndex, time: Option<f64>) -> bool {
        self.send(time, Command::PadUp { pad })
    }
    pub fn suppress_pattern_hit(&mut self, note: f64, beat: f64, until: f64) -> bool {
        self.send(None, Command::SuppressPatternHit { note, beat, until })
    }

    /// Next core event, if any.
    pub fn poll_event(&mut self) -> Option<CoreEvent> {
        self.events.pop()
    }

    /// Free what the core returned (schedules, instruments). Returns how many items.
    pub fn collect_garbage(&mut self) -> usize {
        let mut n = 0;
        while let Some(g) = self.garbage.pop() {
            drop(g);
            n += 1;
        }
        n
    }
}
