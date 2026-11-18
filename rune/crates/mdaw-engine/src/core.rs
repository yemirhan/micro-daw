//! The audio core: the only thing the audio thread runs (docs/rune-port-plan.md §3).
//!
//! [`Core`] owns the sample clock, the tick transport, per-track schedules, the instruments, the
//! metronome / count-in clicks and note repeat, and fires every event at its exact time inside
//! 128-frame render quanta. The same `Core` runs in the cpal callback ([`Core::process`] /
//! [`Core::process_timed`], any buffer size) and offline ([`Core::render_offline`]: export and
//! the timing tests).
//!
//! The control side talks to it through [`CoreHandle`] (UI thread) and [`LiveInput`] (MIDI
//! thread): one lock-free SPSC ring each, fed from a control-side outbox so nothing is ever
//! dropped (`core/handle.rs`). Each producer's commands carry non-decreasing times; at the start
//! of every quantum the core drains both rings into per-producer FIFOs and applies the commands
//! merged by `(time, producer, seq)` at frame `ceil(time·sr)` (never before the quantum).
//! Back to the control side: a **critical** event ring (captures, loop wraps, punch closes,
//! transport; an overrun is latched for the recorders), a **visual** one (pad flashes, clicks),
//! a seqlock clock snapshot in [`Shared`] (frames, playhead, phase, generation, wraps and the
//! device callback's wall-clock anchor), and a garbage ring for replaced schedules / instruments
//! / buffers, which are freed on the control side.
//!
//! **Same-time order** (Tone's): at one transport tick, pattern notes (track by track) fire
//! before the metronome and before note-repeat lines (Tone's timeline invokes events in
//! insertion order; the Part's events are inserted at playback start, each repeat occurrence
//! when the previous one fires), and ticks fire in time order across tracks and repeats. A live
//! pad command applies after every transport event at or before its frame time: Tone invokes
//! transport callbacks ahead of time (look-ahead), so a live hit at time T always comes after the
//! pattern events at T (choke and steal decisions follow from that order).
//!
//! **Audio-thread discipline.** `process` does not allocate, lock, make syscalls or free: every
//! buffer is allocated in [`Core::new`]; commands carry their `Arc`/`Box` payloads; old ones
//! leave through the garbage ring. Instruments hand over the buffers they let go of after every
//! command / pattern tick that touched them and after every quantum (proved by
//! `tests/core_alloc_guard.rs`, with the real `SamplerKit` under stress, and synth tracks / the
//! live synth with the real `SynthPlayer`s, live `setSynthParam` edits included: `Command::
//! SynthSet` carries a patch prepared on the control side). Replaced live synths ring out from
//! the slot's retire list (`Command::ReplaceSynth`) and leave through the garbage ring.
//!
//! **Output guard.** Every instrument's output (current and retiring) and every slot's output
//! after its effect chain are checked for non-finite samples: such a quantum is replaced with
//! silence, the instrument clears the state that produced it (`Instrument::clear_non_finite`)
//! and `Shared::non_finite_outputs` counts it, so one unstable voice can never poison the
//! strip, the master bus or the export.
//!
//! **Mix** (`core/render.rs`): instrument → effect chain (a track strip, `fx/track.rs`, or the
//! live synth bus, `fx/live.rs`) → gain / mute / meter per slot, then the clicks, then the master
//! bus (fader `Volume`, +2 dB drive, soft clip) and its meter. Track automation (`fx/automation.rs`)
//! rides in the slot's schedule and fires on transport ticks like the pattern notes.
//!
//! **Timing deviations from the Electron app (Tone.js)** — what the user perceives:
//! - Tone schedules 100 ms ahead (`Tone.now()` = immediate + 0.1 s); Play, a seek or a tempo
//!   change therefore take effect 100 ms after the click in the Electron app. Here they take
//!   effect at the next render quantum (≤ one device buffer + 2.7 ms): Play starts sounding
//!   ~100 ms sooner, and there is no window in which the position reads a stale value
//!   (R4-B1, R5-N4 cannot happen). The count-in keeps its 50 ms start margin (control side).
//! - A seek before a pending start just moves the start beat; the TS restarts playback
//!   (stop + play at now + 0.1 s), so its audible start is ~100 ms later than here.
//! - Note repeat needs no look-ahead, catch-up or late-skip (see `core/repeat.rs`).
//! - A pattern note up to 100 ms after a live hit is invoked before it in Tone (look-ahead), so
//!   a choke between them goes the "wrong" way there; here they apply in time order.
//! - Everything else (event times on the tick grid, gate lengths from the tempo at event time,
//!   loop in ticks, metronome accents from ticks, count-in times) is the TS arithmetic.

mod api;
mod apply;
mod click;
mod fire;
mod handle;
mod render;
mod repeat;
mod ring;
mod schedule;
mod shared;
mod slots;
mod suppress;
mod transport;

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests_capture;
#[cfg(test)]
mod tests_order;
#[cfg(test)]
mod tests_probes;
#[cfg(test)]
mod tests_repeat;
#[cfg(test)]
mod tests_transport;

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use mdaw_model::{PAD_COUNT, PadIndex};

pub use api::{
    AudioInstrument, CaptureKind, Command, CoreConfig, CoreEvent, DrumInstrument, Garbage,
    MAX_SLOTS, PadCapture, Phase, ProducerId, SlotFx, SynthInstrument, SynthNoteOp,
    TrackInstrument,
};
pub use click::{ACCENT_HZ, BEAT_HZ, CLICK_GAIN};
pub use handle::{CoreHandle, LiveInput};
pub use repeat::{next_free_line, next_grid_tick, repeat_interval_ticks, repeat_start_tick};
pub use ring::{Consumer, Producer, ring};
pub use schedule::{ScheduledNote, TrackSchedule, playing_tracks};
pub use shared::{ClockAnchor, ClockSnapshot, EventTime, Shared};

use crate::contract::{PPQ, RENDER_QUANTUM};
use crate::fx::meter::MeterWindow;
use crate::fx::tone::ToneParam;
use click::ClickSynth;
use handle::Stamped;
pub(crate) use render::js_round;
use render::trash_to;
use repeat::NoteRepeat;
use shared::TransportPub;
use slots::SlotSet;
use suppress::Suppressed;
use transport::{Transport, Wrap};

const Q: usize = RENDER_QUANTUM;
/// Most repeat pads fired on one grid line (all pads).
const MAX_PADS: usize = PAD_COUNT;
/// Garbage the core holds when the return ring is full (then it leaks, never frees).
const OVERFLOW_CAPACITY: usize = 256;
/// The furthest ahead of the render position a live (MIDI producer) command may be stamped
/// while the core runs on a device. A legitimate live stamp is at most ~3 device blocks +
/// 2 quanta ahead (an event during a stall is capped by `ClockSnapshot::map_event`); this is the
/// safety net behind it: the core applies a MIDI command no later than this after the quantum
/// that drains it, and the producer rebases its pending stamps past it (`core/handle.rs`), so
/// one bad stamp can never hold back the later live commands in the producer's FIFO for more
/// than this. Offline renders (no device anchor: export, the engine's test rigs) are not
/// clamped: they may schedule live commands any distance ahead.
pub const MAX_LIVE_LOOKAHEAD: f64 = 0.5;

/// Timing of one device callback, for the wall-clock anchor (see `core/shared.rs`).
#[derive(Debug, Clone, Copy)]
pub struct BlockClock {
    /// When this block's first frame was requested (the callback's `Instant`, plus the frames
    /// of the same callback already rendered).
    pub instant: Instant,
    /// Callback → speaker time of this block's first frame.
    pub output_latency: Duration,
}

/// Replaced synths ringing out per slot ([`Command::ReplaceSynth`]), each kept until its retire
/// time like the TS (`RETIRE_MS`). Documented limit: more switches than this within one retire
/// period first reclaim retirees that are already silent, then fade the oldest one out over
/// [`FADE_QUANTA`] quanta (≈5 ms) instead of cutting it.
pub(crate) const RETIRE_SLOTS: usize = 16;
/// Extra entries for evicted retirees while they fade out.
const FADE_SLOTS: usize = 2;
const FADE_QUANTA: u32 = 2;
/// A retiree whose output peak stayed below this (−120 dBFS) for [`SILENT_QUANTA`] quanta after
/// its release time is silent (reclaimable at no audible cost).
const SILENT_PEAK: f32 = 1e-6;
const SILENT_QUANTA: u32 = 16;

/// A replaced instrument ringing out (see [`RETIRE_SLOTS`]).
struct Retiree {
    inst: TrackInstrument,
    /// Retire time (context seconds).
    until: f64,
    /// Its `releaseAll` time.
    release_at: f64,
    /// Consecutive silent quanta since `release_at`.
    quiet: u32,
    /// Evicted: quanta of its fade-out already rendered.
    fade: Option<u32>,
}

struct Slot {
    instrument: Option<TrackInstrument>,
    /// Replaced instruments playing their release until their retire time.
    retiring: [Option<Retiree>; RETIRE_SLOTS + FADE_SLOTS],
    schedule: Option<Arc<TrackSchedule>>,
    gain: f32,
    muted: bool,
    strip: Option<SlotFx>,
    meter: MeterWindow,
    live_only: bool,
}

#[derive(Clone, Copy)]
struct CountInPlan {
    t0: f64,
    spb: f64,
    beats: u32,
    next: u32,
}

/// The realtime core. See the module docs.
pub struct Core {
    sr: f64,
    /// Start frame of the next quantum to render.
    frame: u64,
    /// Frames handed out by `process` so far.
    delivered: u64,
    /// (first frame, instant, latency, frames) of the last timed device callback.
    anchor: Option<(u64, Instant, Duration, u32)>,
    /// Largest device block seen (the anchor's latency budget).
    max_block: u32,
    inputs: [Consumer<Stamped>; 2],
    /// Per producer: drained commands in arrival (= time) order.
    staged: [VecDeque<Stamped>; 2],
    events: Producer<CoreEvent>,
    visual: Producer<CoreEvent>,
    garbage: Producer<Garbage>,
    overflow: Vec<Garbage>,
    shared: Arc<Shared>,
    tr: Transport,
    generation: u64,
    tracks: Vec<Slot>,
    sink: Option<usize>,
    /// Per pad: slots hit live since its last release.
    pad_targets: [SlotSet; PAD_COUNT],
    /// Pads whose hits a sink switch already released: their next PadUp goes to that old sink in
    /// the TS (`padTargets` still names it), which is a no-op; here it is skipped.
    switch_released: u32,
    repeat: NoteRepeat,
    /// `useMidiNotes`' `muted`: live (and repeat) pad hits are captured but not played.
    live_pad_muted: bool,
    metronome_on: bool,
    metro: ClickSynth,
    last_click: f64,
    count_in: ClickSynth,
    ci: Option<CountInPlan>,
    suppress: Suppressed,
    /// Punch recording's close tick (`CoreEvent::PunchClose`).
    punch_tick: Option<i64>,
    /// Master bus `Volume` (None: no master processing).
    master: Option<ToneParam>,
    master_gain: [f32; Q],
    master_meter: MeterWindow,
    clip_curve: &'static [f32],
    q_l: [f32; Q],
    q_r: [f32; Q],
    s_l: [f32; Q],
    s_r: [f32; Q],
    /// One instrument's output (before it joins the slot bus).
    i_l: [f32; Q],
    i_r: [f32; Q],
    /// Frames of the current quantum already handed out by `process`.
    carry: usize,
}

impl Core {
    /// A core with its UI handle and MIDI input. Default tempo 120 BPM.
    pub fn new(config: CoreConfig) -> (Core, CoreHandle, LiveInput) {
        let sr = config.sample_rate.max(1);
        let max_tracks = config.max_tracks.clamp(1, MAX_SLOTS);
        let shared = Arc::new(Shared::new(sr, 120.0, max_tracks));
        let cmd_cap = config.command_capacity.max(1);
        let (ui_tx, ui_rx) = ring(cmd_cap);
        let (midi_tx, midi_rx) = ring(cmd_cap);
        let (ev_tx, ev_rx) = ring(config.event_capacity);
        let (vis_tx, vis_rx) = ring(config.visual_capacity);
        let garbage_cap = config.garbage_capacity.max(1);
        let (g_tx, g_rx) = ring(garbage_cap);
        let core = Core {
            sr: sr as f64,
            frame: 0,
            delivered: 0,
            anchor: None,
            max_block: 0,
            inputs: [ui_rx, midi_rx],
            staged: [
                VecDeque::with_capacity(cmd_cap),
                VecDeque::with_capacity(cmd_cap),
            ],
            events: ev_tx,
            visual: vis_tx,
            garbage: g_tx,
            overflow: Vec::with_capacity(OVERFLOW_CAPACITY),
            shared: shared.clone(),
            tr: Transport::new(120.0),
            generation: 0,
            tracks: (0..max_tracks)
                .map(|_| Slot {
                    instrument: None,
                    retiring: Default::default(),
                    schedule: None,
                    gain: 1.0,
                    muted: false,
                    strip: None,
                    meter: MeterWindow::default(),
                    live_only: false,
                })
                .collect(),
            sink: None,
            pad_targets: [SlotSet::EMPTY; PAD_COUNT],
            switch_released: 0,
            repeat: NoteRepeat::new(),
            live_pad_muted: false,
            metronome_on: false,
            metro: ClickSynth::new(sr as f64),
            last_click: f64::NEG_INFINITY,
            count_in: ClickSynth::new(sr as f64),
            ci: None,
            suppress: Suppressed::new(),
            punch_tick: None,
            master: None,
            master_gain: [1.0; Q],
            master_meter: MeterWindow::default(),
            clip_curve: mdaw_dsp::shaping::master_clip_curve_shared(),
            q_l: [0.0; Q],
            q_r: [0.0; Q],
            s_l: [0.0; Q],
            s_r: [0.0; Q],
            i_l: [0.0; Q],
            i_r: [0.0; Q],
            carry: Q,
        };
        let handle = CoreHandle::new(ui_tx, (ev_rx, vis_rx), (g_rx, garbage_cap), shared.clone());
        let live = LiveInput::new(midi_tx, shared);
        (core, handle, live)
    }

    pub fn sample_rate(&self) -> f64 {
        self.sr
    }

    pub fn shared(&self) -> &Arc<Shared> {
        &self.shared
    }

    fn time(&self, frame: u64) -> f64 {
        frame as f64 / self.sr
    }

    // --- Commands -------------------------------------------------------------------------

    fn drain_inputs(&mut self) {
        while let Some(g) = self.overflow.pop() {
            if let Err(g) = self.garbage.push(g) {
                self.overflow.push(g);
                break;
            }
        }
        // on a device, a live command applies no later than MAX_LIVE_LOOKAHEAD after the
        // quantum that drains it (its FIFO stays ordered: the clamp only lowers times to a
        // bound that grows with the frame)
        let live_limit = if self.anchor.is_some() {
            self.time(self.frame) + MAX_LIVE_LOOKAHEAD
        } else {
            f64::INFINITY
        };
        for i in 0..self.inputs.len() {
            while self.staged[i].len() < self.staged[i].capacity() {
                let Some(mut s) = self.inputs[i].pop() else {
                    break;
                };
                if s.producer == ProducerId::Midi && s.time > live_limit {
                    s.time = live_limit;
                }
                self.staged[i].push_back(s);
            }
        }
    }

    /// The producer whose front command applies next (merge by `(time, producer, seq)`).
    fn next_staged(&self) -> Option<usize> {
        match (self.staged[0].front(), self.staged[1].front()) {
            (Some(a), Some(b)) => Some(if a.key() <= b.key() { 0 } else { 1 }),
            (Some(_), None) => Some(0),
            (None, Some(_)) => Some(1),
            (None, None) => None,
        }
    }

    /// The frame a staged command applies at (never before `cursor`).
    fn effective_frame(&self, s: &Stamped, cursor: u64) -> u64 {
        ((s.time * self.sr - 1e-6).ceil().max(0.0) as u64).max(cursor)
    }

    fn render_quantum(&mut self) {
        self.drain_inputs();
        let q0 = self.frame;
        let q1 = q0 + Q as u64;
        let mut cursor = q0;
        loop {
            let next = self
                .next_staged()
                .and_then(|p| self.staged[p].front().map(|s| (p, s)))
                .map(|(p, s)| {
                    let live = matches!(
                        s.cmd,
                        Command::PadDown { .. }
                            | Command::PadHit { .. }
                            | Command::PadUp { .. }
                            | Command::CaptureSynthNote { .. }
                    );
                    (p, self.effective_frame(s, cursor), live)
                })
                .filter(|(_, f, _)| *f < q1);
            let Some((p, f, live)) = next else {
                self.advance(cursor, q1, false);
                break;
            };
            // a live pad applies after the transport events at its frame time (module docs)
            self.advance(cursor, f, live);
            cursor = f;
            if let Some(s) = self.staged[p].pop_front() {
                self.apply(s, self.time(f));
            }
        }
        self.render(q0);
        self.frame = q1;
        let repeating = (0..PAD_COUNT)
            .filter(|p| self.repeat.is_repeating(*p))
            .fold(0u64, |m, p| m | 1 << p);
        self.shared.publish_repeating(repeating);
        self.publish_state();
    }

    /// Publish the clock snapshot at the current frame (with the last anchor).
    fn publish_state(&self) {
        let t = self.time(self.frame);
        self.shared.publish(
            TransportPub {
                frames: self.frame,
                beat: self.tr.beat_at(t),
                bpm: self.tr.bpm,
                phase: self.tr.phase,
                generation: self.generation,
                wraps: self.tr.wrap_count,
                wrapped_beats: self.tr.wrapped_beats,
            },
            self.anchor,
        );
    }

    /// A transport command from an older generation than the current one is stale.
    fn accept_gen(&mut self, gen_: u64) -> bool {
        if gen_ < self.generation {
            return false;
        }
        self.generation = gen_;
        true
    }

    fn set_sink(&mut self, slot: Option<usize>, t: f64) {
        if slot == self.sink {
            return;
        }
        if let Some(prev) = self.sink {
            for pad in 0..PAD_COUNT {
                if self.pad_targets[pad].contains(prev) {
                    self.pad_targets[pad].remove(prev);
                    if self.pad_targets[pad].is_empty() {
                        self.switch_released |= 1 << pad;
                    }
                    if let Some(TrackInstrument::Drum(d)) = self
                        .tracks
                        .get_mut(prev)
                        .and_then(|s| s.instrument.as_mut())
                    {
                        d.release(pad, t);
                    }
                }
            }
            self.drain_slot(prev);
        }
        self.sink = slot;
    }

    fn for_each_drum(&mut self, mut f: impl FnMut(&mut Box<dyn DrumInstrument>)) {
        for slot in &mut self.tracks {
            if let Some(TrackInstrument::Drum(d)) = &mut slot.instrument {
                f(d);
            }
        }
    }

    fn clock(&self, t: f64) -> repeat::Clock {
        repeat::Clock {
            running: self.tr.phase != Phase::Stopped,
            time: t,
            ticks: self.tr.beat_at(t) * PPQ as f64,
            bpm: self.tr.bpm,
        }
    }

    // --- Events and garbage out -----------------------------------------------------------

    /// A recording-critical event: an overrun is latched for the control side.
    fn emit(&mut self, e: CoreEvent) {
        if self.events.push(e).is_err() {
            self.shared.count_critical_overrun();
        }
    }

    /// A visual event (pad flash, click): may be dropped (counted).
    fn emit_visual(&mut self, e: CoreEvent) {
        if self.visual.push(e).is_err() {
            self.shared.count_dropped_event();
        }
    }

    fn trash(&mut self, g: Garbage) {
        trash_to(&mut self.garbage, &mut self.overflow, &self.shared, g);
    }

    /// Hand the buffers one slot's instrument let go of to the garbage ring.
    fn drain_slot(&mut self, slot: usize) {
        let Some(inst) = self
            .tracks
            .get_mut(slot)
            .and_then(|s| s.instrument.as_mut())
        else {
            return;
        };
        let inst = inst.as_instrument();
        let (garbage, overflow, shared) = (&mut self.garbage, &mut self.overflow, &self.shared);
        inst.take_garbage(&mut |b| trash_to(garbage, overflow, shared, Garbage::Buffer(b)));
        shared.count_leaks(inst.take_leaks());
    }

    fn drain_all_slots(&mut self) {
        for i in 0..self.tracks.len() {
            self.drain_slot(i);
        }
    }

    fn capture(
        &mut self,
        kind: CaptureKind,
        producer: Option<ProducerId>,
        pad: PadIndex,
        velocity: f64,
        time: f64,
        grid_beats: Option<f64>,
    ) {
        let c = PadCapture {
            kind,
            producer,
            pad,
            velocity,
            time,
            beat: self.tr.beat_at(time),
            phase: self.tr.phase,
            wrap_count: self.tr.wrap_count,
            wrapped_beats: self.tr.wrapped_beats,
            grid_beats,
            generation: self.generation,
        };
        self.emit(CoreEvent::Capture(c));
    }

    fn on_wrap(&mut self, w: Wrap) {
        self.emit(CoreEvent::LoopWrap {
            time: w.time,
            from_beat: w.from_beat,
            to_beat: w.to_beat,
            wrap_count: self.tr.wrap_count,
            wrapped_beats: self.tr.wrapped_beats,
            generation: self.generation,
        });
    }

    /// Apply a loop wrap due at exactly `t` before reading the position there.
    fn settle_wrap(&mut self, t: f64) {
        if let Some(w) = self.tr.wrap_if_due(t) {
            self.on_wrap(w);
        }
    }

    /// A live (or repeat) hit on the live sink.
    fn live_trigger(&mut self, pad: PadIndex, velocity: f64, time: f64) {
        if self.live_pad_muted {
            return;
        }
        let Some(slot) = self.sink else { return };
        if let Some(TrackInstrument::Drum(d)) = self
            .tracks
            .get_mut(slot)
            .and_then(|s| s.instrument.as_mut())
        {
            d.trigger(pad, velocity, time);
            self.pad_targets[pad].insert(slot);
            self.switch_released &= !(1 << pad);
            self.drain_slot(slot);
        }
    }
}
