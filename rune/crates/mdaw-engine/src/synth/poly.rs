//! `PolyInstrument` (`src/audio/PolyInstrument.ts`): a polyphonic (or monophonic) instrument
//! made of voices, plus one LFO for all of them.
//!
//! - Poly: [`PolyVoiceAllocator`] decides which voice plays which note; a stolen voice gets a
//!   `hardStop` and the new attack starts [`STEAL_GAP`] (9 ms) later.
//! - Mono (subtractive `mono: true`): [`MonoNoteStack`] (last-note priority, legato glides,
//!   falling back to a held key glides).
//! - LFO (`Tone.LFO`, sine, −1..1 at `clamp(rate, .05, 30)` Hz, started at construction when
//!   vibrato or filter depth > 0): sine oscillator → `AudioToGain` WaveShaper (1024 points,
//!   `(x+1)/2`) → ×2 → −1; × vibrato depth (cents) → every oscillator's detune, × filter depth →
//!   every filter's detune. Delayed vibrato: from silence, the vibrato depth ramps 0 → depth over
//!   `lfo.delay`. Chromium only renders what the destination pulls, and the LFO is only connected
//!   to the output once the first voice exists: its phase starts advancing then (from 0).
//! - `triggerAttackRelease` gates are ≥ 24 ms.
//!
//! **Offline (WAV export) clock.** `Tone.Offline` runs Tone's whole clock BEFORE Chromium renders
//! a frame: every Part event at `t` is handed out by the tick whose `currentTime` is the first
//! accumulated block time `k·(128/sr)` (summed in f64, like `OfflineContext._currentTime`) after
//! `t`, and Tone clamps source starts / stops and `value =` writes to that time, while the native
//! timeline sees nothing in the past (`currentTime` 0). Every node already exists when rendering
//! starts, so the LFO runs from frame 0; a new voice's signals start at the block of that clock
//! time (late first frame by the usual rule of that block).
//! [`PolyInstrument::set_offline`] switches an instrument to that model (the export only).
//!
//! Audio thread: every method except [`PolyInstrument::new`] and [`PolyPatch::prepare`] is
//! allocation-free. Voices are built up front for the
//! preset's polyphony, in storage sized for [`MAX_VOICES`]. A live `set()` is split in two:
//! [`PolyPatch::prepare`] (control side) builds everything that allocates or locks: the shared
//! preset, its wave tables / noise buffers ([`Voice::Prep`]) and fresh voices for the slots not
//! used yet; [`PolyInstrument::apply`] (audio thread) swaps them in and leaves what it replaced
//! (the previous preset, the unused voices) in the patch, which goes back to the control side.

use std::sync::Arc;

use mdaw_dsp::factory::Mulberry32;
use mdaw_dsp::shaping::{tone_curve, waveshape};

use super::alloc::{MAX_VOICES, MonoAction, MonoNoteStack, PolyVoiceAllocator};
use super::common::{Ctx, HARD_STOP_FADE, MIN_GATE, ModBus, Voice, clamp, norm_velocity};
use super::presets::LfoParams;
use super::sources::{OscScratch, ToneOsc};
use super::wa::param::{AutoParam, Clock};
use super::wa::wave::BasicShape;

/// Gap between a stolen voice's fade-out and the new note.
pub const STEAL_GAP: f64 = HARD_STOP_FADE * 0.6;
const Q: usize = 128;

/// What the instrument needs to know about its preset, per engine.
pub(crate) trait PolyPreset: Clone + Send {
    type V: Voice<Preset = Self>;
    fn polyphony(&self) -> f64;
    fn lfo(&self) -> LfoParams;
    /// `Some(legato)` for a mono subtractive preset.
    fn mono(&self) -> Option<bool>;
    /// `amp.release` (or `release` for pluck), the release tail before any voice exists.
    fn release_tail(&self) -> f64;
    /// The preset's live-update resources (control side).
    fn prepare(&self, sr: f64) -> <Self::V as Voice>::Prep;
    /// A voice of `preset` (control side: allocates).
    fn make_voice(
        preset: &Arc<Self>,
        prep: &<Self::V as Voice>::Prep,
        sr: f64,
        cx: Clock,
    ) -> Self::V;
}

/// Voices an instrument builds for `p` (1 for mono).
fn voice_count<P: PolyPreset>(p: &P) -> usize {
    if p.mono().is_some() {
        1
    } else {
        (p.polyphony().round().max(1.0) as usize).min(MAX_VOICES)
    }
}

/// A prepared `set(params)` (see the module docs): built on the control side, applied on the
/// audio thread by [`PolyInstrument::apply`], then returned (holding what it replaced).
pub(crate) struct PolyPatch<P: PolyPreset> {
    preset: Arc<P>,
    prep: <P::V as Voice>::Prep,
    /// Fresh voices for the slots `fresh_from..` (a slot used by the time the patch applies
    /// gets `update` instead, and its fresh voice goes back unused).
    fresh_from: usize,
    fresh: Vec<Option<Box<P::V>>>,
}

impl<P: PolyPreset> PolyPatch<P> {
    /// Prepare `next` for an instrument at `sr` whose first `used` voices may already be in
    /// use (a lower bound is fine; 0 when unknown): fresh voices are built for the slots
    /// `used..polyphony`, like Tone's lazily created voices. `now`: context time (the voices'
    /// construction events lie before the render, so any time up to the apply works).
    pub fn prepare(next: P, sr: f64, now: f64, used: usize) -> Self {
        let preset = Arc::new(next);
        let prep = preset.prepare(sr);
        let n = voice_count(&*preset);
        let fresh_from = used.min(n);
        let cx = Clock::at(now, sr);
        let fresh = (fresh_from..n)
            .map(|_| Some(Box::new(P::make_voice(&preset, &prep, sr, cx))))
            .collect();
        Self {
            preset,
            prep,
            fresh_from,
            fresh,
        }
    }
}

struct Lfo {
    osc: ToneOsc,
    freq: AutoParam,
    stopped: AutoParam,
    vibrato: AutoParam,
    filter: AutoParam,
    curve: Vec<f32>,
    scratch: OscScratch,
    raw: [f32; Q],
    freqs: [f32; Q],
    zero: [f32; Q],
    stop: [f32; Q],
    depth: [f32; Q],
    vib: [f32; Q],
    filt: [f32; Q],
}

pub(crate) struct PolyInstrument<P: PolyPreset> {
    preset: Arc<P>,
    sr: f64,
    /// Capacity [`MAX_VOICES`] from construction on (a polyphony change never reallocates).
    voices: Vec<Box<P::V>>,
    created: usize,
    allocator: PolyVoiceAllocator,
    mono: Option<MonoNoteStack>,
    mono_end: f64,
    mono_scheduled_end: f64,
    mono_scheduled_note: Option<i32>,
    lfo: Box<Lfo>,
    rng: Mulberry32,
    now_frame: u64,
    /// The offline (export) Tone clock: the last accumulated block time handed out.
    offline: Option<f64>,
    /// Tone's `now()` for the scheduling call in progress (offline mode).
    call_now: Option<f64>,
    /// Offline mode: `(event time, the transport tick time Tone's clock fires it at)` for the
    /// events Tone fires a tick early (see [`PolyInstrument::set_offline_dispatch`]).
    dispatch: Vec<(f64, f64)>,
    bus_l: Box<[f32; Q]>,
    bus_r: Box<[f32; Q]>,
    /// Quanta whose voice output was non-finite and got cleared.
    non_finite: u64,
}

fn lfo_of<P: PolyPreset>(p: &P) -> LfoParams {
    p.lfo()
}

impl<P: PolyPreset> PolyInstrument<P> {
    /// `new PolyInstrument(preset, factory)` at context time `now` (the LFO starts then).
    /// `noise_seed`: the mulberry32 seed of the noise start offsets.
    pub fn new(preset: P, sr: f64, now: f64, noise_seed: u32) -> Self {
        let cx = Clock::at(now, sr);
        let mono = preset.mono();
        let poly = voice_count(&preset);
        let l = lfo_of(&preset);
        let mut lfo = Box::new(Lfo {
            osc: ToneOsc::basic(BasicShape::Sine, sr, cx),
            freq: AutoParam::signal(clamp(l.rate, 0.05, 30.0), cx),
            stopped: AutoParam::signal(0.0, cx),
            vibrato: AutoParam::gain(l.vibrato, cx),
            filter: AutoParam::gain(l.filter, cx),
            curve: tone_curve(1024, |x| (x + 1.0) / 2.0),
            scratch: OscScratch::default(),
            raw: [0.0; Q],
            freqs: [0.0; Q],
            zero: [0.0; Q],
            stop: [0.0; Q],
            depth: [0.0; Q],
            vib: [0.0; Q],
            filt: [0.0; Q],
        });
        if l.vibrato > 0.0 || l.filter > 0.0 {
            lfo.stopped.set_value_at_time(0.0, now, cx);
            lfo.osc.start(now, cx);
        }
        let preset = Arc::new(preset);
        let prep = preset.prepare(sr);
        let mut voices = Vec::with_capacity(MAX_VOICES);
        voices.extend((0..poly).map(|_| Box::new(P::make_voice(&preset, &prep, sr, cx))));
        Self {
            allocator: PolyVoiceAllocator::new(poly),
            mono: mono.map(MonoNoteStack::new),
            preset,
            sr,
            voices,
            created: 0,
            mono_end: f64::NEG_INFINITY,
            mono_scheduled_end: f64::NEG_INFINITY,
            mono_scheduled_note: None,
            lfo,
            rng: Mulberry32::new(noise_seed),
            now_frame: (now * sr).round() as u64,
            offline: None,
            call_now: None,
            dispatch: Vec::new(),
            bus_l: Box::new([0.0; Q]),
            bus_r: Box::new([0.0; Q]),
            non_finite: 0,
        }
    }

    /// Clear voice state that went non-finite.
    pub fn clear_non_finite(&mut self) {
        for v in &mut self.voices[..self.created] {
            v.clear_non_finite();
        }
    }

    /// Quanta whose voice output went non-finite and was cleared (the output guard).
    pub fn non_finite_quanta(&self) -> u64 {
        self.non_finite
    }

    fn cx(&self) -> Clock {
        let native = self.now_frame as f64 / self.sr;
        Clock {
            now: self.call_now.unwrap_or(native),
            sr: self.sr,
            native,
        }
    }

    /// The voices used so far (export pre-pass).
    pub fn used_voices(&self) -> impl Iterator<Item = &P::V> {
        self.voices[..self.created].iter().map(|v| &**v)
    }

    /// Every voice (export pre-pass).
    pub fn voices_mut(&mut self) -> impl Iterator<Item = &mut P::V> {
        self.voices.iter_mut().map(|v| &mut **v)
    }

    pub fn preset(&self) -> &P {
        &self.preset
    }

    pub fn sample_rate(&self) -> f64 {
        self.sr
    }

    /// The instrument's "now" (the start of the next quantum to render).
    pub fn now(&self) -> f64 {
        self.cx().now
    }

    /// Render like `Tone.Offline` (the WAV export; see the module docs).
    pub fn set_offline(&mut self) {
        self.offline = Some(0.0);
    }

    /// Export: the events Tone's transport fires one tick early, as `(event time, time of the
    /// tick that fires it)`. A Part event at tick `n` is stored as `floor(toTicks(Ticks(n)))`
    /// plus a remainder that only shifts the time it passes on: when the seconds round trip
    /// lands a hair below `n` (tick 1440 at 112 BPM → 1439.9999999999998), the clock fires it
    /// with tick `n − 1`, so Tone's `now()` is the first block time after THAT tick, which can
    /// be a block before the event's time (its sources are then not clamped).
    pub fn set_offline_dispatch(&mut self, dispatch: Vec<(f64, f64)>) {
        self.dispatch = dispatch;
    }

    /// Offline mode: Tone's `now()` while it hands out an event at `t` (the first accumulated
    /// block time after the tick that fires it: `t`, or see `set_offline_dispatch`).
    fn enter(&mut self, t: f64) {
        let Some(mut k) = self.offline else { return };
        let t = self
            .dispatch
            .iter()
            .find(|d| (d.0 - t).abs() < 1e-9)
            .map_or(t, |d| d.1);
        let step = Q as f64 / self.sr;
        if k > t + step {
            k = 0.0; // an earlier time than the last call: count again
        }
        while k <= t {
            k += step;
        }
        self.offline = Some(k);
        self.call_now = Some(k);
    }

    /// Number of voices used so far (`getPoolSize`).
    pub fn pool_size(&self) -> usize {
        self.created
    }

    /// `getVoice(index)`: voices are built up front; this marks them in use (a voice renders
    /// from its first use on, like the lazily created Tone voice).
    fn voice(&mut self, index: usize) -> usize {
        let i = index.min(self.voices.len() - 1);
        while self.created <= i {
            // offline: Tone builds the voice at its clock time (its signals start at the block
            // that time names, with Chromium's late-first-frame rule of that block)
            let f = match self.call_now {
                Some(t) if self.offline.is_some() => (t * self.sr).round() as u64,
                _ => self.now_frame,
            };
            let sr = self.sr;
            self.voices[self.created].born(f, sr);
            if self.offline.is_some() {
                let now = self.now_frame;
                self.voices[self.created].offline_start(now);
                if let Some(t) = self.call_now {
                    let cx = self.cx();
                    self.voices[self.created].offline_built(t, cx);
                }
            }
            self.created += 1;
        }
        i
    }

    fn release_duration(&self) -> f64 {
        if self.created > 0 {
            self.voices[0].release_duration()
        } else {
            self.preset.release_tail()
        }
    }

    /// Delayed vibrato: fade the vibrato in after `lfo.delay` when playing from silence.
    fn on_first_note(&mut self, t: f64, from_silence: bool) {
        let l = lfo_of(&*self.preset);
        if !from_silence || l.vibrato <= 0.0 || l.delay <= 0.0 {
            return;
        }
        let cx = self.cx();
        let g = &mut self.lfo.vibrato;
        g.cancel_scheduled_values(t, cx);
        g.set_value_at_time(0.0, t, cx);
        g.linear_ramp_to_value_at_time(l.vibrato, t + l.delay, cx);
    }

    fn with_voice<R>(&mut self, i: usize, f: impl FnOnce(&mut P::V, &mut Ctx) -> R) -> R {
        let cx = self.cx();
        let mut ctx = Ctx {
            cx,
            rng: &mut self.rng,
        };
        f(&mut self.voices[i], &mut ctx)
    }

    pub fn note_on(&mut self, midi: i32, velocity: f64, time: f64) {
        self.enter(time);
        if !(0..=127).contains(&midi) {
            return;
        }
        let t = time;
        let vel = norm_velocity(Some(velocity));
        if let Some(stack) = self.mono.as_mut() {
            let was_held = stack.held_count() > 0;
            let action = stack.note_on(midi, vel);
            let v = self.voice(0);
            let from_silence = !was_held && self.mono_end <= t;
            self.on_first_note(t, from_silence);
            match action {
                MonoAction::Legato { .. } => {
                    self.with_voice(v, |voice, ctx| voice.glide(midi, t, ctx))
                }
                MonoAction::Attack { from, .. } => {
                    let from = if was_held { from } else { None };
                    self.with_voice(v, |voice, ctx| voice.attack(midi, t, vel, from, ctx));
                }
                _ => {}
            }
            self.mono_end = f64::INFINITY;
            return;
        }
        let silent = self.allocator.active_voice_count(t) == 0;
        self.on_first_note(t, silent);
        let alloc = self.allocator.note_on(midi, t, vel);
        let v = self.voice(alloc.voice);
        self.with_voice(v, |voice, ctx| {
            let mut at = t;
            if alloc.stolen {
                voice.hard_stop(t, ctx);
                at = t + STEAL_GAP;
            }
            voice.attack(midi, at, vel, None, ctx);
        });
    }

    pub fn note_off(&mut self, midi: i32, time: f64) {
        self.enter(time);
        let t = time;
        if let Some(stack) = self.mono.as_mut() {
            let action = stack.note_off(midi);
            if self.created == 0 {
                return;
            }
            match action {
                MonoAction::Release { .. } => {
                    self.mono_end = self.with_voice(0, |v, ctx| v.release(t, ctx))
                }
                MonoAction::Legato { note, .. } => {
                    self.with_voice(0, |v, ctx| v.glide(note, t, ctx))
                }
                _ => {}
            }
            return;
        }
        let rel = self.release_duration();
        let cx = self.cx();
        let (voices, rng, created) = (&mut self.voices, &mut self.rng, self.created);
        self.allocator.note_off(midi, t, rel, |i| {
            if i < created {
                let mut ctx = Ctx { cx, rng: &mut *rng };
                voices[i].release(t, &mut ctx);
            }
        });
    }

    /// `triggerAttackRelease(midi, duration, time, velocity)` (gate ≥ 2·MIN_GATE).
    pub fn trigger_attack_release(&mut self, midi: i32, duration: f64, time: f64, velocity: f64) {
        self.enter(time);
        if !(0..=127).contains(&midi) {
            return;
        }
        let t = time;
        let dur = duration.max(MIN_GATE * 2.0);
        let vel = norm_velocity(Some(velocity));
        if self.mono.is_some() {
            let v = self.voice(0);
            let overlapping =
                self.mono_scheduled_end > t + 1e-4 && self.mono_scheduled_note.is_some();
            let from = if overlapping {
                self.mono_scheduled_note
            } else {
                None
            };
            let end = self.with_voice(v, |voice, ctx| {
                voice.attack(midi, t, vel, from, ctx);
                voice.release(t + dur, ctx)
            });
            self.mono_scheduled_end = t + dur;
            self.mono_scheduled_note = Some(midi);
            let prev = if self.mono_end == f64::INFINITY {
                f64::NEG_INFINITY
            } else {
                self.mono_end
            };
            self.mono_end = prev.max(end);
            return;
        }
        let rel = self.release_duration();
        let alloc = self.allocator.one_shot(midi, t, dur, rel, vel);
        let v = self.voice(alloc.voice);
        self.with_voice(v, |voice, ctx| {
            let mut at = t;
            if alloc.stolen {
                voice.hard_stop(t, ctx);
                at = t + STEAL_GAP;
            }
            voice.attack(midi, at, vel, None, ctx);
            voice.release(at + dur, ctx);
        });
    }

    pub fn release_all(&mut self, time: f64) {
        self.enter(time);
        let t = time;
        if let Some(stack) = self.mono.as_mut() {
            stack.release_all();
            self.mono_scheduled_end = f64::NEG_INFINITY;
            self.mono_scheduled_note = None;
        }
        self.allocator.release_all(t, |_| {});
        let mut end = t;
        for i in 0..self.created {
            end = end.max(self.with_voice(i, |v, ctx| v.hard_stop(t, ctx)));
        }
        self.mono_end = if self.mono.is_some() {
            end
        } else {
            f64::NEG_INFINITY
        };
    }

    /// `getActiveVoiceCount(time)`.
    pub fn active_voice_count(&self, time: f64) -> usize {
        if let Some(stack) = &self.mono {
            return usize::from(stack.held_count() > 0 || self.mono_end > time);
        }
        self.allocator.active_voice_count(time)
    }

    /// `getHeldNotes()` ascending (control side: allocates).
    pub fn held_notes(&self) -> Vec<i32> {
        if let Some(stack) = &self.mono {
            let mut v: Vec<i32> = stack.held_notes().collect();
            v.sort_unstable();
            return v;
        }
        self.allocator.held_notes()
    }

    /// Whether `midi` is held (allocation-free [`Self::held_notes`] membership).
    pub fn is_held(&self, midi: i32) -> bool {
        match &self.mono {
            Some(stack) => stack.held_notes().any(|n| n == midi),
            None => self.allocator.is_held(midi),
        }
    }

    /// Apply a prepared `set(params)` (audio thread: allocation-free, nothing freed). The voices
    /// in use get `update`; the slots not used yet take the patch's fresh voices (the replaced
    /// ones go into the patch); a larger polyphony appends fresh voices within the
    /// pre-allocated storage.
    pub fn apply(&mut self, patch: &mut PolyPatch<P>) {
        let cx = self.cx();
        // The patch now holds the previous preset: the voices' drops below only decrement.
        std::mem::swap(&mut self.preset, &mut patch.preset);
        let next = &self.preset;
        {
            let rng = &mut self.rng;
            for v in &mut self.voices[..self.created] {
                let mut ctx = Ctx { cx, rng: &mut *rng };
                v.update(next, &patch.prep, &mut ctx);
            }
        }
        let n = voice_count(&**next);
        for i in self.created..n.max(self.voices.len()) {
            let Some(fresh) = i
                .checked_sub(patch.fresh_from)
                .and_then(|k| patch.fresh.get_mut(k))
            else {
                // Unused voices above the new polyphony stay unreachable (a later patch that
                // grows the polyphony rebuilds them); a slot without a fresh voice (the patch
                // assumed more voices in use) is updated instead.
                if i < self.voices.len() && i < n {
                    let mut ctx = Ctx {
                        cx,
                        rng: &mut self.rng,
                    };
                    self.voices[i].update(next, &patch.prep, &mut ctx);
                }
                continue;
            };
            if i < self.voices.len() {
                if let Some(f) = fresh.as_mut() {
                    std::mem::swap(&mut self.voices[i], f);
                }
            } else if self.voices.len() < self.voices.capacity()
                && let Some(f) = fresh.take()
            {
                self.voices.push(f);
            }
        }
        match (&mut self.mono, next.mono()) {
            (Some(stack), Some(legato)) => stack.legato = legato,
            (None, _) => self.allocator.set_max_voices(n.min(self.voices.len())),
            _ => {}
        }
        let l = lfo_of(&**next);
        let lfo = &mut *self.lfo;
        lfo.freq
            .exponential_ramp_to(clamp(l.rate, 0.05, 30.0), 0.05, cx.now, cx);
        lfo.vibrato.linear_ramp_to(l.vibrato, 0.05, cx.now, cx);
        lfo.filter.linear_ramp_to(l.filter, 0.05, cx.now, cx);
        if (l.vibrato > 0.0 || l.filter > 0.0) && !lfo.osc.is_started(cx.now) {
            lfo.stopped.set_value_at_time(0.0, cx.now, cx);
            lfo.osc.start(cx.now, cx);
        }
    }

    /// Render one quantum (`out` already holds other signals: ADD into it).
    pub fn process(&mut self, frame: u64, left: &mut [f32], right: &mut [f32]) {
        let n = left.len();
        self.now_frame = frame;
        self.call_now = None;
        if self.created == 0 && self.offline.is_none() {
            // Nothing pulls the LFO yet: Chromium only renders nodes connected to the
            // destination, so the LFO (phase 0 at its start) advances from the first voice on.
            self.now_frame = frame + n as u64;
            return;
        }
        let sr = self.sr;
        let lfo = &mut *self.lfo;
        lfo.freq.render(frame, sr, false, &mut lfo.freqs[..n]);
        lfo.raw[..n].fill(0.0);
        lfo.osc.render(
            frame,
            &lfo.freqs[..n],
            &lfo.zero[..n],
            &mut lfo.scratch,
            &mut lfo.raw[..n],
        );
        lfo.stopped.render(frame, sr, false, &mut lfo.stop[..n]);
        for k in 0..n {
            let shaped = waveshape(&lfo.curve, lfo.raw[k] + lfo.stop[k]);
            lfo.raw[k] = shaped * 2.0 + -1.0;
        }
        lfo.vibrato.render(frame, sr, false, &mut lfo.depth[..n]);
        for k in 0..n {
            lfo.vib[k] = lfo.raw[k] * lfo.depth[k];
        }
        lfo.filter.render(frame, sr, false, &mut lfo.depth[..n]);
        for k in 0..n {
            lfo.filt[k] = lfo.raw[k] * lfo.depth[k];
        }
        let m = ModBus {
            vibrato: &lfo.vib[..n],
            filter: &lfo.filt[..n],
        };
        let (bl, br) = (&mut self.bus_l[..n], &mut self.bus_r[..n]);
        bl.fill(0.0);
        br.fill(0.0);
        for v in &mut self.voices[..self.created] {
            if !v.is_idle() {
                v.render(frame, &m, bl, br);
            }
        }
        if !bl.iter().chain(br.iter()).all(|x| x.is_finite()) {
            // Non-finite guard: never pass NaN / ∞ on (it would poison the chain, the strip
            // and the master bus for good); clear the state that produced it.
            bl.fill(0.0);
            br.fill(0.0);
            for v in &mut self.voices[..self.created] {
                v.clear_non_finite();
            }
            self.non_finite += 1;
        }
        for k in 0..n {
            left[k] += bl[k];
            right[k] += br[k];
        }
        self.now_frame = frame + n as u64;
        let now = self.now_frame as f64 / sr;
        for v in &mut self.voices[..self.created] {
            v.prune(now);
        }
    }
}
