//! The audio core: the only thing the audio thread runs (docs/rune-port-plan.md §3).
//!
//! [`Core`] owns the sample clock, the tick transport, per-track schedules, the instruments, the
//! metronome / count-in clicks and note repeat, and fires every event at its exact time inside
//! 128-frame render quanta. The same `Core` runs in the cpal callback ([`Core::process`], any
//! buffer size) and offline ([`Core::render_offline`]: export and the timing tests).
//!
//! The control side talks to it through [`CoreHandle`] (UI thread) and [`LiveInput`] (MIDI
//! thread): one lock-free SPSC ring each. At the start of every quantum the core drains both
//! rings and applies commands in `(time, producer, seq)` order at frame `ceil(time·sr)` (never
//! before the quantum: a hit "now" plays at the next frame boundary). Back to the control side:
//! one ring of [`CoreEvent`]s (captures, transport, clicks, pad flashes), atomics in [`Shared`]
//! (playhead, phase, generation), and a garbage ring for replaced schedules / instruments, which
//! are freed on the control side.
//!
//! **Audio-thread discipline.** `process` does not allocate, lock or make syscalls: every
//! buffer is allocated in [`Core::new`]; commands carry their `Arc`/`Box` payloads; old ones
//! leave through the garbage ring (proved by `tests/core_alloc_guard.rs`).
//!
//! **Mix** (`core/render.rs`): instrument → track strip (`core/strip.rs`) → gain / mute / meter
//! per slot, then the clicks, then the master bus (fader, +2 dB drive, soft clip).
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
//! - Everything else (event times on the tick grid, gate lengths from the tempo at event time,
//!   loop in ticks, metronome accents from ticks, count-in times) is the TS arithmetic.

mod api;
mod click;
mod handle;
mod render;
mod repeat;
mod ring;
mod schedule;
mod strip;
mod suppress;
mod transport;

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests_capture;
#[cfg(test)]
mod tests_probes;
#[cfg(test)]
mod tests_repeat;
#[cfg(test)]
mod tests_transport;

use std::sync::Arc;

use mdaw_model::{PAD_COUNT, PadIndex};

pub use api::{
    AudioInstrument, CaptureKind, Command, CoreConfig, CoreEvent, DrumInstrument, Garbage,
    PadCapture, Phase, ProducerId, SynthInstrument, TrackInstrument,
};
pub use click::{ACCENT_HZ, BEAT_HZ, CLICK_GAIN};
pub use handle::{CoreHandle, LiveInput, Shared};
pub use repeat::{next_free_line, next_grid_tick, repeat_interval_ticks, repeat_start_tick};
pub use ring::{Consumer, Producer, ring};
pub use schedule::{ScheduledNote, TrackSchedule, playing_tracks};
pub use strip::{
    CHORUS_DELAY_SEC, DELAY_FEEDBACK, DELAY_SEC, EQ_HIGH_HZ, EQ_LOW_HZ, Strip, TRACK_FILTER_HZ,
    disabled_crossfade_gains, pan_gains,
};

use crate::contract::{PPQ, RENDER_QUANTUM};
use crate::sampler::SamplerCommand;
use click::ClickSynth;
use handle::Stamped;
pub(crate) use render::js_round;
use render::trash_to;
use repeat::NoteRepeat;
use suppress::Suppressed;
use transport::{Transport, Wrap, first_multiple, tick_range};

const Q: usize = RENDER_QUANTUM;
/// Most repeat pads fired on one grid line (all pads).
const MAX_PADS: usize = PAD_COUNT;

struct Slot {
    instrument: Option<TrackInstrument>,
    schedule: Option<Arc<TrackSchedule>>,
    gain: f32,
    muted: bool,
    pan: f32,
    strip: Option<Box<Strip>>,
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
    inputs: [Consumer<Stamped>; 2],
    staged: Vec<Stamped>,
    events: Producer<CoreEvent>,
    garbage: Producer<Garbage>,
    overflow: Vec<Garbage>,
    shared: Arc<Shared>,
    tr: Transport,
    generation: u64,
    tracks: Vec<Slot>,
    sink: Option<usize>,
    /// Per pad: slots hit live since its last release (bit per slot).
    pad_targets: [u64; PAD_COUNT],
    /// Pads whose hits a sink switch already released: their next PadUp goes to that old sink in
    /// the TS (`padTargets` still names it), which is a no-op; here it is skipped.
    switch_released: u32,
    repeat: NoteRepeat,
    metronome_on: bool,
    metro: ClickSynth,
    last_click: f64,
    count_in: ClickSynth,
    ci: Option<CountInPlan>,
    suppress: Suppressed,
    /// Master bus fader gain (None: no master processing).
    master: Option<f32>,
    /// Offline only: the tracks' reverb sends, mixed per output channel (see
    /// [`Core::render_offline_with_reverb_send`]).
    aux: Option<Box<[[f32; Q]; 4]>>,
    clip_curve: &'static [f32],
    q_l: [f32; Q],
    q_r: [f32; Q],
    s_l: [f32; Q],
    s_r: [f32; Q],
    /// Frames of the current quantum already handed out by `process`.
    carry: usize,
}

impl Core {
    /// A core with its UI handle and MIDI input. Default tempo 120 BPM.
    pub fn new(config: CoreConfig) -> (Core, CoreHandle, LiveInput) {
        let sr = config.sample_rate.max(1);
        let shared = Arc::new(Shared::new(sr, 120.0));
        let (ui_tx, ui_rx) = ring(config.command_capacity);
        let (midi_tx, midi_rx) = ring(config.command_capacity);
        let (ev_tx, ev_rx) = ring(config.event_capacity);
        let (g_tx, g_rx) = ring(config.garbage_capacity);
        let max_tracks = config.max_tracks.clamp(1, 64);
        let core = Core {
            sr: sr as f64,
            frame: 0,
            inputs: [ui_rx, midi_rx],
            staged: Vec::with_capacity(2 * config.command_capacity),
            events: ev_tx,
            garbage: g_tx,
            overflow: Vec::with_capacity(64),
            shared: shared.clone(),
            tr: Transport::new(120.0),
            generation: 0,
            tracks: (0..max_tracks)
                .map(|_| Slot {
                    instrument: None,
                    schedule: None,
                    gain: 1.0,
                    muted: false,
                    pan: 0.0,
                    strip: None,
                    live_only: false,
                })
                .collect(),
            sink: None,
            pad_targets: [0; PAD_COUNT],
            switch_released: 0,
            repeat: NoteRepeat::new(),
            metronome_on: false,
            metro: ClickSynth::new(sr as f64),
            last_click: f64::NEG_INFINITY,
            count_in: ClickSynth::new(sr as f64),
            ci: None,
            suppress: Suppressed::new(),
            master: None,
            aux: None,
            clip_curve: mdaw_dsp::shaping::master_clip_curve_shared(),
            q_l: [0.0; Q],
            q_r: [0.0; Q],
            s_l: [0.0; Q],
            s_r: [0.0; Q],
            carry: Q,
        };
        let handle = CoreHandle::new(ui_tx, ev_rx, g_rx, shared.clone());
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
        for i in 0..self.inputs.len() {
            while self.staged.len() < self.staged.capacity() {
                let Some(s) = self.inputs[i].pop() else { break };
                let key = s.key();
                let at = self.staged.partition_point(|x| x.key() <= key);
                self.staged.insert(at, s);
            }
        }
    }

    /// The frame a staged command applies at (never before `cursor`).
    fn effective_frame(&self, s: &Stamped, cursor: u64) -> u64 {
        match s.time {
            None => cursor,
            Some(t) => ((t * self.sr - 1e-6).ceil().max(0.0) as u64).max(cursor),
        }
    }

    fn render_quantum(&mut self) {
        self.drain_inputs();
        let q0 = self.frame;
        let q1 = q0 + Q as u64;
        let mut cursor = q0;
        loop {
            let next = self.staged.first().map(|s| self.effective_frame(s, cursor));
            let due = next.filter(|f| *f < q1);
            self.advance(cursor, due.unwrap_or(q1));
            let Some(f) = due else { break };
            cursor = f;
            let s = self.staged.remove(0);
            self.apply(s, self.time(f));
        }
        self.render(q0);
        self.frame = q1;
        let t1 = self.time(q1);
        let repeating = (0..PAD_COUNT)
            .filter(|p| self.repeat.is_repeating(*p))
            .fold(0u64, |m, p| m | 1 << p);
        self.shared.publish_repeating(repeating);
        self.shared.publish(
            q1,
            self.tr.beat_at(t1),
            self.tr.bpm,
            self.tr.phase,
            self.generation,
            (self.tr.wrap_count, self.tr.wrapped_beats),
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

    fn apply(&mut self, s: Stamped, t: f64) {
        let producer = s.producer;
        match s.cmd {
            Command::Start {
                at,
                beat,
                generation,
            } => {
                if !self.accept_gen(generation) {
                    return;
                }
                let was_running = self.tr.phase != Phase::Stopped;
                if was_running {
                    // Hits of the old run handed out for later than `t` (fractional ticks).
                    self.for_each_drum(|d| d.cancel_pending_after(None, t));
                }
                let beat = if beat.is_finite() { beat.max(0.0) } else { 0.0 };
                self.tr
                    .start(at.filter(|a| a.is_finite()).unwrap_or(t).max(t), beat);
                if !was_running {
                    self.repeat.restart_all(self.clock(t));
                }
            }
            Command::Stop { generation } => {
                if !self.accept_gen(generation) {
                    return;
                }
                let was_running = self.tr.phase != Phase::Stopped;
                self.tr.stop();
                for slot in &mut self.tracks {
                    if slot.live_only {
                        continue;
                    }
                    match &mut slot.instrument {
                        Some(TrackInstrument::Drum(d)) => d.stop_all(t),
                        Some(TrackInstrument::Synth(s)) => s.release_all(t),
                        Some(TrackInstrument::Audio(a)) => a.stop_all(t),
                        None => {}
                    }
                }
                self.count_in.silence_from(t);
                self.ci = None;
                self.emit(CoreEvent::Stopped {
                    time: t,
                    generation,
                });
                if was_running {
                    self.repeat.restart_all(self.clock(t));
                }
            }
            Command::Seek { beat, generation } => {
                if !self.accept_gen(generation) || !beat.is_finite() {
                    return;
                }
                self.tr.seek(t, beat.max(0.0));
                self.settle_wrap(t);
                self.emit(CoreEvent::Seeked {
                    time: t,
                    beat: self.tr.beat_at(t),
                    generation,
                });
            }
            Command::CountIn {
                t0,
                beats,
                sec_per_beat,
                generation,
            } => {
                if !self.accept_gen(generation) || !(t0.is_finite() && sec_per_beat > 0.0) {
                    return;
                }
                self.count_in.silence_from(t);
                let dur = self.tr.sec_per_beat() / 8.0; // '32n'
                let beats = beats.min(32);
                for i in 0..beats {
                    let hz = if i % 4 == 0 { ACCENT_HZ } else { BEAT_HZ };
                    self.count_in
                        .schedule(t0 + i as f64 * sec_per_beat, dur, hz);
                }
                self.ci = Some(CountInPlan {
                    t0,
                    spb: sec_per_beat,
                    beats,
                    next: 0,
                });
            }
            Command::SetBpm { bpm } => {
                if bpm.is_finite() && bpm > 0.0 {
                    self.tr.set_bpm(t, bpm);
                }
            }
            Command::SetLoop { range } => self.tr.set_loop(range),
            Command::SetMetronome { enabled } => self.metronome_on = enabled,
            Command::SetNoteRepeat {
                enabled,
                rate_beats,
            } => self.repeat.configure(enabled, rate_beats, self.clock(t)),
            Command::SetInstrument { slot, instrument } => {
                let old = match self.tracks.get_mut(slot) {
                    Some(s) => {
                        // the new instrument has no live pads held yet
                        for targets in &mut self.pad_targets {
                            *targets &= !(1u64 << slot);
                        }
                        std::mem::replace(&mut s.instrument, instrument)
                    }
                    None => instrument,
                };
                if let Some(old) = old {
                    self.trash(Garbage::Instrument(old));
                }
            }
            Command::SetSchedule { slot, schedule } => {
                let old = match self.tracks.get_mut(slot) {
                    Some(s) => std::mem::replace(&mut s.schedule, schedule),
                    None => schedule,
                };
                if let Some(old) = old {
                    self.trash(Garbage::Schedule(old));
                }
            }
            Command::SetTrackGain { slot, gain } => {
                if let Some(s) = self.tracks.get_mut(slot) {
                    s.gain = if gain.is_finite() { gain } else { 0.0 };
                }
            }
            Command::SetTrackMute { slot, muted } => {
                if let Some(s) = self.tracks.get_mut(slot) {
                    s.muted = muted;
                }
            }
            Command::SetTrackPan { slot, pan } => {
                if let Some(s) = self.tracks.get_mut(slot) {
                    s.pan = if pan.is_finite() { pan } else { 0.0 };
                }
            }
            Command::SetTrackStrip { slot, strip } => {
                let old = match self.tracks.get_mut(slot) {
                    Some(s) => std::mem::replace(&mut s.strip, strip),
                    None => strip,
                };
                if let Some(old) = old {
                    self.trash(Garbage::Strip(old));
                }
            }
            Command::SetLiveOnly { slot, live_only } => {
                if let Some(s) = self.tracks.get_mut(slot) {
                    s.live_only = live_only;
                }
            }
            Command::SetMasterBus { gain } => self.master = gain.filter(|g| g.is_finite()),
            Command::KitCommand { slot, mut cmd } => {
                let c = std::mem::replace(
                    &mut *cmd,
                    SamplerCommand::SetPadBuffer {
                        pad: usize::MAX,
                        buffer: None,
                    },
                );
                match self
                    .tracks
                    .get_mut(slot)
                    .and_then(|s| s.instrument.as_mut())
                {
                    Some(TrackInstrument::Drum(d)) => d.apply_kit(c),
                    _ => *cmd = c,
                }
                self.trash(Garbage::KitCommand(cmd));
            }
            Command::SetRegionBuffer {
                slot,
                region,
                buffer,
            } => match self
                .tracks
                .get_mut(slot)
                .and_then(|s| s.instrument.as_mut())
            {
                Some(TrackInstrument::Audio(a)) => a.set_region_buffer(region, buffer),
                _ => {
                    if let Some(b) = buffer {
                        self.trash(Garbage::Buffer(b));
                    }
                }
            },
            Command::SetLiveSink { slot } => self.set_sink(slot, t),
            Command::PadDown { pad, velocity } => {
                if pad >= PAD_COUNT {
                    return;
                }
                self.settle_wrap(t);
                self.live_trigger(pad, velocity, t);
                self.capture(CaptureKind::Hit, Some(producer), pad, velocity, t, None);
                self.repeat.press(pad, velocity, self.clock(t));
            }
            Command::PadUp { pad } => {
                if pad >= PAD_COUNT {
                    return;
                }
                self.repeat.release(pad);
                let mut targets = std::mem::take(&mut self.pad_targets[pad]);
                let switched = self.switch_released & (1 << pad) != 0;
                self.switch_released &= !(1 << pad);
                if targets == 0
                    && !switched
                    && let Some(sink) = self.sink
                {
                    // drumEngine.release: no hit since the last release → the current sink.
                    targets = 1u64 << (sink & 63);
                }
                for (i, slot) in self.tracks.iter_mut().enumerate() {
                    if targets & (1u64 << i) != 0
                        && let Some(TrackInstrument::Drum(d)) = &mut slot.instrument
                    {
                        d.release(pad, t);
                    }
                }
                self.settle_wrap(t);
                self.capture(CaptureKind::Release, Some(producer), pad, 0.0, t, None);
            }
            Command::StopVoices { slot } => match self
                .tracks
                .get_mut(slot)
                .and_then(|s| s.instrument.as_mut())
            {
                Some(TrackInstrument::Drum(d)) => d.stop_all(t),
                Some(TrackInstrument::Synth(s)) => s.release_all(t),
                Some(TrackInstrument::Audio(a)) => a.stop_all(t),
                None => {}
            },
            Command::SuppressPatternHit { note, beat, until } => {
                self.suppress.insert(note, beat, until)
            }
        }
    }

    fn set_sink(&mut self, slot: Option<usize>, t: f64) {
        if slot == self.sink {
            return;
        }
        if let Some(prev) = self.sink {
            let bit = 1u64 << (prev & 63);
            for pad in 0..PAD_COUNT {
                if self.pad_targets[pad] & bit != 0 {
                    self.pad_targets[pad] &= !bit;
                    if self.pad_targets[pad] == 0 {
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

    // --- Events out ----------------------------------------------------------------------

    fn emit(&mut self, e: CoreEvent) {
        if self.events.push(e).is_err() {
            self.shared.count_dropped_event();
        }
    }

    fn trash(&mut self, g: Garbage) {
        trash_to(&mut self.garbage, &mut self.overflow, &self.shared, g);
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

    // --- Time advance --------------------------------------------------------------------

    /// Fire everything due in frames `[fa, fb)`.
    fn advance(&mut self, fa: u64, fb: u64) {
        if fa >= fb {
            return;
        }
        let (ta, tb) = (self.time(fa), self.time(fb));
        self.count_in_events(tb);
        match self.tr.phase {
            Phase::Stopped => self.free_repeats(ta, tb),
            Phase::Pending => {
                if self.tr.pending_at < tb {
                    let at = self.tr.pending_at.max(ta);
                    self.tr.pending_at = at;
                    self.tr.begin();
                    self.emit(CoreEvent::Started {
                        time: at,
                        beat: self.tr.pending_beat,
                        generation: self.generation,
                    });
                    self.run_started(at, tb);
                }
            }
            Phase::Started => self.run_started(ta, tb),
        }
    }

    fn count_in_events(&mut self, tb: f64) {
        while let Some(ci) = self.ci {
            if ci.next >= ci.beats || ci.t0 + ci.next as f64 * ci.spb >= tb {
                break;
            }
            self.emit(CoreEvent::CountInClick {
                time: ci.t0 + ci.next as f64 * ci.spb,
                index: ci.next,
                remaining: ci.beats - ci.next,
                generation: self.generation,
            });
            if let Some(c) = &mut self.ci {
                c.next += 1;
            }
        }
    }

    fn run_started(&mut self, mut t: f64, tb: f64) {
        for _ in 0..64 {
            if t >= tb {
                break;
            }
            self.settle_wrap(t);
            let (k_a, k_b, t_end, wraps) = self.tr.span(t, tb);
            let (lo, hi) = tick_range(k_a, k_b);
            if lo < hi {
                self.fire_ticks(lo, hi);
            }
            if !(wraps && t_end < tb) {
                break;
            }
            if let Some(w) = self.tr.wrap_at_end(t_end) {
                self.on_wrap(w);
            }
            t = t_end;
        }
    }

    /// Integer ticks `[lo, hi)` of the running transport (no wrap inside).
    fn fire_ticks(&mut self, lo: i64, hi: i64) {
        let spb = self.tr.sec_per_beat();
        for i in 0..self.tracks.len() {
            let Slot {
                instrument,
                schedule,
                ..
            } = &mut self.tracks[i];
            let (Some(inst), Some(sched)) = (instrument.as_mut(), schedule.as_deref()) else {
                continue;
            };
            for e in sched.in_ticks(lo, hi) {
                let time = self.tr.time_of_tick(e.tick);
                match inst {
                    TrackInstrument::Drum(d) => {
                        let Some(pad) = e.pad else { continue };
                        if self.suppress.consume(e.note, e.beat, time) {
                            continue;
                        }
                        d.trigger(pad, e.velocity, time);
                        if d.pad_is_gate(pad) {
                            // R2-11: the tempo at event time.
                            d.release(pad, time + (e.duration_beats * spb).max(0.01));
                        }
                        if self
                            .events
                            .push(CoreEvent::PatternHit { slot: i, pad, time })
                            .is_err()
                        {
                            self.shared.count_dropped_event();
                        }
                    }
                    TrackInstrument::Synth(s) => {
                        s.note_on(e.note, e.velocity, time);
                        s.note_off(e.note, time + e.duration_beats * spb);
                    }
                    TrackInstrument::Audio(a) => {
                        if let Some(region) = e.pad {
                            a.start_region(region, time);
                        }
                    }
                }
            }
        }
        if self.metronome_on {
            let mut m = first_multiple(lo, PPQ as i64);
            while m < hi {
                let time = self.tr.time_of_tick(m as f64);
                if time > self.last_click {
                    let beat = m / PPQ as i64;
                    let accent = beat % 4 == 0;
                    let hz = if accent { ACCENT_HZ } else { BEAT_HZ };
                    self.metro.schedule(time, spb / 8.0, hz);
                    self.last_click = time;
                    self.emit(CoreEvent::MetronomeClick {
                        time,
                        beat: beat as f64,
                        accent,
                    });
                }
                m += PPQ as i64;
            }
        }
        if self.repeat.enabled && self.repeat.any_transport() {
            let interval = self.repeat.interval_ticks();
            let grid = self.repeat.rate_beats;
            let mut m = first_multiple(lo, interval);
            while m < hi {
                let time = self.tr.time_of_tick(m as f64);
                let mut due = [(0usize, 0f64); MAX_PADS];
                let mut n = 0;
                for (pad, vel) in self.repeat.transport_due(time) {
                    due[n] = (pad, vel);
                    n += 1;
                }
                for &(pad, vel) in &due[..n] {
                    self.live_trigger(pad, vel, time);
                    self.capture(CaptureKind::RepeatHit, None, pad, vel, time, Some(grid));
                }
                m += interval;
            }
        }
    }

    /// Free-running repeats with the transport stopped.
    fn free_repeats(&mut self, ta: f64, tb: f64) {
        let grid = self.repeat.rate_beats;
        for _ in 0..256 {
            let Some((pad, vel, time)) = self.repeat.next_free_before(tb) else {
                break;
            };
            self.repeat.advance_free(pad, self.tr.bpm);
            // The core never runs late; the TS skip rule (> 30 ms late) is kept for safety.
            if time >= ta - 0.03 {
                let time = time.max(ta);
                self.live_trigger(pad, vel, time);
                self.capture(CaptureKind::RepeatHit, None, pad, vel, time, Some(grid));
            }
        }
    }

    /// A live (or repeat) hit on the live sink.
    fn live_trigger(&mut self, pad: PadIndex, velocity: f64, time: f64) {
        let Some(slot) = self.sink else { return };
        if let Some(TrackInstrument::Drum(d)) = self
            .tracks
            .get_mut(slot)
            .and_then(|s| s.instrument.as_mut())
        {
            d.trigger(pad, velocity, time);
            self.pad_targets[pad] |= 1u64 << slot;
            self.switch_released &= !(1 << pad);
        }
    }
}
