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
//! Audio thread: every method except [`PolyInstrument::set`] / [`PolyInstrument::new`] is
//! allocation-free (voices are built up front for the preset's polyphony).

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
    fn make_voice(&self, sr: f64, cx: Clock) -> Self::V;
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
    preset: P,
    sr: f64,
    voices: Vec<P::V>,
    created: usize,
    allocator: PolyVoiceAllocator,
    mono: Option<MonoNoteStack>,
    mono_end: f64,
    mono_scheduled_end: f64,
    mono_scheduled_note: Option<i32>,
    lfo: Box<Lfo>,
    rng: Mulberry32,
    now_frame: u64,
    bus_l: Box<[f32; Q]>,
    bus_r: Box<[f32; Q]>,
}

fn lfo_of<P: PolyPreset>(p: &P) -> LfoParams {
    p.lfo()
}

impl<P: PolyPreset> PolyInstrument<P> {
    /// `new PolyInstrument(preset, factory)` at context time `now` (the LFO starts then).
    /// `noise_seed`: the mulberry32 seed of the noise start offsets.
    pub fn new(preset: P, sr: f64, now: f64, noise_seed: u32) -> Self {
        let cx = Clock { now, sr };
        let mono = preset.mono();
        let poly = if mono.is_some() {
            1
        } else {
            (preset.polyphony().round().max(1.0) as usize).min(MAX_VOICES)
        };
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
        let voices = (0..poly).map(|_| preset.make_voice(sr, cx)).collect();
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
            bus_l: Box::new([0.0; Q]),
            bus_r: Box::new([0.0; Q]),
        }
    }

    fn cx(&self) -> Clock {
        Clock {
            now: self.now_frame as f64 / self.sr,
            sr: self.sr,
        }
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
            let (f, sr) = (self.now_frame, self.sr);
            self.voices[self.created].born(f, sr);
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
        let l = lfo_of(&self.preset);
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

    /// `set(params)` with an already merged preset (control side: may allocate — new voices
    /// when the polyphony grows, wave tables).
    pub fn set(&mut self, next: P) {
        let cx = self.cx();
        {
            let rng = &mut self.rng;
            for v in &mut self.voices[..self.created] {
                let mut ctx = Ctx { cx, rng: &mut *rng };
                v.update(&next, &mut ctx);
            }
        }
        // Voices not used yet are built from the new preset, like Tone's lazy voices.
        for v in &mut self.voices[self.created..] {
            *v = next.make_voice(self.sr, cx);
        }
        match (&mut self.mono, next.mono()) {
            (Some(stack), Some(legato)) => stack.legato = legato,
            (None, _) => {
                let n = (next.polyphony().round().max(1.0) as usize).min(MAX_VOICES);
                self.allocator.set_max_voices(n);
                while self.voices.len() < n {
                    self.voices.push(next.make_voice(self.sr, cx));
                }
            }
            _ => {}
        }
        let l = lfo_of(&next);
        let lfo = &mut *self.lfo;
        lfo.freq
            .exponential_ramp_to(clamp(l.rate, 0.05, 30.0), 0.05, cx.now, cx);
        lfo.vibrato.linear_ramp_to(l.vibrato, 0.05, cx.now, cx);
        lfo.filter.linear_ramp_to(l.filter, 0.05, cx.now, cx);
        if (l.vibrato > 0.0 || l.filter > 0.0) && !lfo.osc.is_started(cx.now) {
            lfo.stopped.set_value_at_time(0.0, cx.now, cx);
            lfo.osc.start(cx.now, cx);
        }
        self.preset = next;
    }

    /// Render one quantum (`out` already holds other signals: ADD into it).
    pub fn process(&mut self, frame: u64, left: &mut [f32], right: &mut [f32]) {
        let n = left.len();
        self.now_frame = frame;
        if self.created == 0 {
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
