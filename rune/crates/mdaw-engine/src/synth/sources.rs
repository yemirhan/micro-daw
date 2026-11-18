//! The Tone sources the voices play, as Chromium renders them:
//!
//! - [`ToneOsc`]: `Tone.OmniOscillator` (and a plain `Tone.Oscillator`) in the shapes
//!   `omniOptions` produces: a single oscillator (native type at phase 0, or a periodic wave for
//!   custom partials), a `FatOscillator` (`count` oscillators at phases `i/count·360°`, each
//!   `−6 − 1.1·count` dB, detuned `−spread/2 + i·spread/(count − 1)` cents), or a
//!   `PulseOscillator` (a triangle plus a `width` offset through a 1024-point step WaveShaper
//!   `x ≤ 0 ? −1 : 1`, the width behind its own gate);
//! - [`NoiseSource`]: `Tone.Noise` (a new random offset on every start and restart).
//!
//! Signal path of an oscillator (each step an f32 GainNode multiply): native oscillator × gate ×
//! the inner oscillator's volume (fat: −6 − 1.1·count dB, else 1) → (fat: summed) × the Omni
//! volume. Frequency = the voice's frequency signal; detune = the Omni detune signal (its value +
//! the vibrato input) plus, for a fat voice, its spread offset.
//!
//! Native nodes: a start from the stopped state creates new native node(s) with their own gate
//! while the previous node(s) play out their gate (Tone's state timeline compares with a 1e-6
//! tolerance, the gates switch on exact frames, so an old stop a hair after the new start still
//! sounds for a frame). Each source keeps two node instances: the current one and the previous one
//! until its gate has closed for good.

use std::sync::Arc;

use mdaw_dsp::factory::Mulberry32;
use mdaw_dsp::shaping::{tone_curve, waveshape};

use super::presets::{OscParams, OscWave};
use super::wa::noise::{NoiseBuffer, NoisePlayer};
use super::wa::osc::OscNode;
use super::wa::param::{AutoParam, Clock};
use super::wa::source::{Gate, SourceState, Started, start_source};
use super::wa::wave::{BasicShape, WaveTables, basic_wave, tone_wave};
use super::wa::{db_to_gain, volume_gain_f64};

const MAX_FAT: usize = 7;
const Q: usize = 128;

/// `omniOptions(p)` resolved: the oscillator layout of an OscParams block.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum OmniShape {
    /// Plain oscillator: native type (`partials` None) or custom partials.
    Single(BasicShape, Option<Vec<f64>>),
    /// `fat<type>` / `fatcustom`: count, spread.
    Fat(BasicShape, Option<Vec<f64>>, usize, f64),
    Pulse(f64),
}

fn basic(w: OscWave) -> BasicShape {
    match w {
        OscWave::Sine => BasicShape::Sine,
        OscWave::Triangle => BasicShape::Triangle,
        OscWave::Square => BasicShape::Square,
        // 'pulse' never reaches here (handled first).
        OscWave::Sawtooth | OscWave::Pulse => BasicShape::Sawtooth,
    }
}

/// `omniOptions`.
pub(crate) fn omni_shape(p: &OscParams) -> OmniShape {
    let unison = p.unison.clamp(1.0, 7.0).round() as usize;
    if let Some(partials) = p.partials.as_ref().filter(|v| !v.is_empty()) {
        return if unison > 1 {
            OmniShape::Fat(BasicShape::Sine, Some(partials.clone()), unison, p.spread)
        } else {
            OmniShape::Single(BasicShape::Sine, Some(partials.clone()))
        };
    }
    if p.wave == OscWave::Pulse {
        return OmniShape::Pulse(p.width.unwrap_or(0.4).clamp(-0.95, 0.95));
    }
    if unison > 1 {
        OmniShape::Fat(basic(p.wave), None, unison, p.spread)
    } else {
        OmniShape::Single(basic(p.wave), None)
    }
}

/// Tables of one Tone oscillator: native type at phase 0, else Tone's periodic wave.
fn wave_for(
    shape: BasicShape,
    partials: Option<&[f64]>,
    phase_deg: f64,
    sr: f64,
) -> Arc<WaveTables> {
    let phase = phase_deg * std::f64::consts::PI / 180.0;
    match partials {
        Some(p) => tone_wave(None, p, phase, sr),
        None if phase == 0.0 => basic_wave(shape, sr),
        None => tone_wave(Some(shape), &[], phase, sr),
    }
}

/// The native oscillator node(s) of one start: one node, or the fat stack (node, its spread
/// detune).
#[derive(Debug, Clone)]
struct Nodes {
    nodes: Vec<(OscNode, f32)>,
    /// Per-node volume (fat: −6 − 1.1·count dB; else 1).
    vol: f32,
}

#[derive(Debug, Clone)]
struct Instance {
    nodes: Nodes,
    gate: Gate,
    live: bool,
}

fn build_nodes(shape: &OmniShape, sr: f64) -> Nodes {
    match shape {
        OmniShape::Single(s, partials) => Nodes {
            nodes: vec![(
                OscNode::new(wave_for(*s, partials.as_deref(), 0.0, sr), sr),
                0.0,
            )],
            vol: 1.0,
        },
        OmniShape::Fat(s, partials, count, spread) => {
            let count = (*count).clamp(1, MAX_FAT);
            let mut nodes = Vec::with_capacity(MAX_FAT);
            for i in 0..count {
                let phase = (i as f64 / count as f64) * 360.0;
                nodes.push((
                    OscNode::new(wave_for(*s, partials.as_deref(), phase, sr), sr),
                    0.0,
                ));
            }
            let mut n = Nodes {
                nodes,
                vol: db_to_gain(-6.0 - count as f64 * 1.1),
            };
            set_spread(&mut n, *spread);
            n
        }
        OmniShape::Pulse(_) => Nodes {
            nodes: vec![(OscNode::new(basic_wave(BasicShape::Triangle, sr), sr), 0.0)],
            vol: 1.0,
        },
    }
}

/// `FatOscillator.spread = s`.
fn set_spread(n: &mut Nodes, spread: f64) {
    let count = n.nodes.len();
    if count > 1 {
        let start = -spread / 2.0;
        let step = spread / (count - 1) as f64;
        for (i, o) in n.nodes.iter_mut().enumerate() {
            o.1 = (start + step * i as f64) as f32;
        }
    }
}

fn pulse_curve() -> &'static [f32] {
    static CURVE: std::sync::OnceLock<Vec<f32>> = std::sync::OnceLock::new();
    CURVE.get_or_init(|| tone_curve(1024, |v| if v <= 0.0 { -1.0 } else { 1.0 }))
}

/// Per-quantum scratch for oscillator rendering.
#[derive(Debug, Clone)]
pub(crate) struct OscScratch {
    gate: [f32; Q],
    vol: [f32; Q],
    freq: [f32; Q],
    det: [f32; Q],
    det2: [f32; Q],
    buf: [f32; Q],
    acc: [f32; Q],
    width: [f32; Q],
}

impl Default for OscScratch {
    fn default() -> Self {
        Self {
            gate: [0.0; Q],
            vol: [0.0; Q],
            freq: [0.0; Q],
            det: [0.0; Q],
            det2: [0.0; Q],
            buf: [0.0; Q],
            acc: [0.0; Q],
            width: [0.0; Q],
        }
    }
}

/// A `Tone.OmniOscillator` / `Tone.Oscillator` with its start state, gates and volume.
#[derive(Debug, Clone)]
pub(crate) struct ToneOsc {
    shape: OmniShape,
    state: SourceState,
    inst: [Instance; 2],
    cur: usize,
    /// PulseOscillator: the width signal and its gate.
    width: AutoParam,
    width_gate: AutoParam,
    /// The Omni / Oscillator Source volume (a GainNode, `volume.value = dB` sets it at now).
    volume: AutoParam,
    /// The Omni detune signal (its own value; the vibrato input is added at render).
    detune: AutoParam,
    /// Quantum in which oscillators rebuilt by a live update have dead signals in their first
    /// frame (see `common::signals_skip_first_frame`).
    dead_frame: Option<u64>,
    /// First frame of a fat stack's spread detunes (export: see [`ToneOsc::offline_built`]).
    spread_from: u64,
    sr: f64,
}

impl ToneOsc {
    fn with_shape(shape: OmniShape, sr: f64, cx: Clock) -> Self {
        let inst = || Instance {
            nodes: build_nodes(&shape, sr),
            gate: Gate::new(cx),
            live: false,
        };
        let width = match shape {
            OmniShape::Pulse(w) => {
                // Build the shared step curve here, never on the audio thread.
                pulse_curve();
                w
            }
            _ => 0.0,
        };
        Self {
            inst: [inst(), inst()],
            shape,
            state: SourceState::default(),
            cur: 0,
            width: AutoParam::signal(width, cx),
            width_gate: AutoParam::gain(0.0, cx),
            volume: AutoParam::gain(1.0, cx),
            detune: AutoParam::signal(0.0, cx),
            dead_frame: None,
            spread_from: 0,
            sr,
        }
    }

    /// `new Tone.OmniOscillator(omniOptions(p))` (volume 0 dB, detune 0).
    pub fn omni(p: &OscParams, sr: f64, cx: Clock) -> Self {
        Self::with_shape(omni_shape(p), sr, cx)
    }

    /// `new Tone.Oscillator({ type })` (a native type at phase 0).
    pub fn basic(shape: BasicShape, sr: f64, cx: Clock) -> Self {
        Self::with_shape(OmniShape::Single(shape, None), sr, cx)
    }

    fn is_pulse(&self) -> bool {
        matches!(self.shape, OmniShape::Pulse(_))
    }

    /// `osc.type = <native type>` on a plain oscillator (the running node changes waveform).
    pub fn set_basic(&mut self, shape: BasicShape) {
        let w = basic_wave(shape, self.sr);
        for i in &mut self.inst {
            for n in &mut i.nodes.nodes {
                n.0.set_wave(w.clone());
            }
        }
        self.shape = OmniShape::Single(shape, None);
    }

    /// `osc.volume.value = gainToDb(g)` (at now).
    pub fn set_level(&mut self, g: f64, cx: Clock) {
        self.volume.set_value(volume_gain_f64(g), cx);
    }

    /// `osc.detune.value = cents` (at now).
    pub fn set_detune(&mut self, cents: f64, cx: Clock) {
        self.detune.set_value(cents, cx);
    }

    /// Export (`Tone.Offline`): Tone built this oscillator at its clock time `t`, so its
    /// construction writes (the detune signal's `value =`, a fat stack's spread on the inner
    /// oscillators' detune signals) are native events at `t`: until frame `ceil(t·sr)` the
    /// signals, which have run since frame 0, still hold 0 cents. A node starting in that quantum
    /// reads its first phase increment there (see `wa::osc`).
    pub fn offline_built(&mut self, t: f64, cx: Clock) {
        let cents = self.detune.get_value_at_time(t);
        if cents != 0.0 {
            self.detune.set_value_at_time(0.0, 0.0, cx);
            self.detune.set_value_at_time(cents, t, cx);
        }
        self.spread_from = (t * cx.sr).ceil() as u64;
    }

    /// `updateOmni(osc, p)` (control side: may build tables). A different source type (plain /
    /// fat / pulse) or fat count makes Tone stop the old oscillators now and build new ones,
    /// started now when the source is started; otherwise the waves change in place.
    pub fn update(&mut self, p: &OscParams, cx: Clock) {
        let shape = omni_shape(p);
        let rebuild = match (&self.shape, &shape) {
            (OmniShape::Single(..), OmniShape::Single(..))
            | (OmniShape::Pulse(_), OmniShape::Pulse(_)) => false,
            (OmniShape::Fat(_, _, a, _), OmniShape::Fat(_, _, b, _)) => a != b,
            _ => true,
        };
        if rebuild {
            let started = self.state.started_at(cx.now);
            self.inst[self.cur].gate.stop(cx.now, cx);
            if self.is_pulse() {
                self.width_gate.cancel_scheduled_values(cx.now, cx);
                self.width_gate.set_value_at_time(0.0, cx.now, cx);
            }
            let sr = self.sr;
            let fresh = Self::with_shape(shape.clone(), sr, cx);
            self.inst = fresh.inst;
            self.cur = fresh.cur;
            self.shape = fresh.shape;
            // The new Tone oscillators' signals start one frame late on some quanta.
            let frame = (cx.now * cx.sr).round() as u64;
            self.dead_frame =
                super::common::signals_skip_first_frame(frame, cx.sr).then_some(frame);
            if started {
                self.start_new(cx.now, cx);
            }
        } else {
            for inst in &mut self.inst {
                match &shape {
                    OmniShape::Fat(s, partials, _, spread) => {
                        let n = inst.nodes.nodes.len();
                        for (i, o) in inst.nodes.nodes.iter_mut().enumerate() {
                            let phase = (i as f64 / n as f64) * 360.0;
                            o.0.set_wave(wave_for(*s, partials.as_deref(), phase, self.sr));
                        }
                        set_spread(&mut inst.nodes, *spread);
                    }
                    OmniShape::Single(s, partials) => {
                        let w = wave_for(*s, partials.as_deref(), 0.0, self.sr);
                        inst.nodes.nodes[0].0.set_wave(w);
                    }
                    OmniShape::Pulse(_) => {}
                }
            }
            if let OmniShape::Pulse(w) = shape {
                self.width.set_value(w, cx);
            }
        }
        self.shape = shape;
        self.detune.set_value(p.semi * 100.0 + p.fine, cx);
    }

    /// New native node(s) at `t` (the previous ones keep playing out their gate).
    fn start_new(&mut self, t: f64, cx: Clock) {
        let next = self.cur ^ 1;
        let inst = &mut self.inst[next];
        for n in &mut inst.nodes.nodes {
            n.0.restart_at(t, cx.sr);
        }
        inst.gate = Gate::new(cx);
        inst.gate.start(t, cx);
        inst.live = true;
        self.cur = next;
        if self.is_pulse() {
            self.width_gate.set_value_at_time(1.0, t, cx);
        }
    }

    /// `startSource(osc, t)`.
    pub fn start(&mut self, t: f64, cx: Clock) {
        match start_source(&mut self.state, t, cx.now) {
            Some(Started::New(t)) => self.start_new(t, cx),
            Some(Started::Restart(t)) => {
                self.inst[self.cur].gate.cancel_stop(cx);
                if self.is_pulse() {
                    self.width_gate.cancel_scheduled_values(t, cx);
                    self.width_gate.set_value_at_time(1.0, t, cx);
                }
            }
            None => {}
        }
    }

    /// `stopSource(osc, t)`.
    pub fn stop(&mut self, t: f64, cx: Clock) {
        if let Some(t) = self.state.stop(t, cx.now) {
            self.inst[self.cur].gate.stop(t, cx);
            if self.is_pulse() {
                self.width_gate.cancel_scheduled_values(t, cx);
                self.width_gate.set_value_at_time(0.0, t, cx);
            }
        }
    }

    /// `state === 'started'` at `t`.
    pub fn is_started(&self, t: f64) -> bool {
        self.state.started_at(t)
    }

    /// Whether the source is silent for good (gates closed, nothing scheduled).
    pub fn is_idle(&self) -> bool {
        let gates = self
            .inst
            .iter()
            .all(|i| !i.live || i.gate.param.idle_value() == Some(0.0));
        gates && (!self.is_pulse() || self.width_gate.idle_value() == Some(0.0))
    }

    /// Render one quantum, ADDING into `out`. `freq`: the frequency signal; `vib`: the vibrato
    /// input of the detune signal.
    #[allow(clippy::needless_range_loop)] // parallel per-frame arrays
    /// Returns whether the source's output was silent (Chromium's silence hint: every gate k-rate
    /// 0, so the gain nodes zeroed their buses).
    pub fn render(
        &mut self,
        frame: u64,
        freq: &[f32],
        vib: &[f32],
        s: &mut OscScratch,
        out: &mut [f32],
    ) -> bool {
        let n = out.len();
        let sr = self.sr;
        let mut silent = true;
        self.detune.render(frame, sr, true, &mut s.det[..n]);
        for k in 0..n {
            s.det[k] += vib[k];
        }
        // Sum of the node instances (each: Σ nodes × gate × node volume).
        s.acc[..n].fill(0.0);
        let cur = self.cur;
        for (idx, inst) in self.inst.iter_mut().enumerate() {
            if !inst.live {
                continue;
            }
            let sa = inst.gate.param.render(frame, sr, false, &mut s.gate[..n]);
            silent &= inst.gate.param.gate_silent(sa);
            let vol = inst.nodes.vol;
            let dead = idx == cur && self.dead_frame == Some(frame);
            if dead {
                s.freq[..n].copy_from_slice(freq);
                s.freq[0] = 0.0;
            }
            // export: no spread before the fat stack's construction writes (`offline_built`)
            let unspread = (self.spread_from.saturating_sub(frame) as usize).min(n);
            for (node, spread) in &mut inst.nodes.nodes {
                s.det2[..unspread].copy_from_slice(&s.det[..unspread]);
                for k in unspread..n {
                    s.det2[k] = *spread + s.det[k];
                }
                if dead {
                    s.det2[0] = 0.0;
                    node.render(frame, &s.freq[..n], &s.det2[..n], &mut s.buf[..n]);
                } else {
                    node.render(frame, freq, &s.det2[..n], &mut s.buf[..n]);
                }
                for k in 0..n {
                    s.acc[k] += s.buf[k] * s.gate[k] * vol;
                }
            }
            if idx != cur && inst.gate.param.idle_value() == Some(0.0) {
                inst.live = false;
            }
        }
        self.volume.render(frame, sr, false, &mut s.vol[..n]);
        if self.is_pulse() {
            let sa = self.width_gate.render(frame, sr, false, &mut s.width[..n]);
            silent &= self.width_gate.gate_silent(sa);
            self.width.render(frame, sr, false, &mut s.det2[..n]);
            let curve = pulse_curve();
            for k in 0..n {
                let x = s.acc[k] + s.det2[k] * s.width[k];
                out[k] += waveshape(curve, x) * s.vol[k];
            }
        } else {
            for k in 0..n {
                out[k] += s.acc[k] * s.vol[k];
            }
        }
        silent
    }
}

/// Buffer sources of one noise that can sound at once.
const NOISE_INSTANCES: usize = 4;

/// `Tone.Noise` with its start state, gates and volume.
///
/// `Noise._stop(t)` stops the current buffer source AND forgets it (`_source = null`), even when
/// `t` is in the future (a voice's release schedules the stop at the end of the tail). A restart
/// before that stop (a re-attack during the release) then has no source to stop: the old buffer
/// source keeps playing until its own scheduled stop, under the new one.
#[derive(Debug, Clone)]
pub(crate) struct NoiseSource {
    state: SourceState,
    inst: [(NoisePlayer, Gate, bool); NOISE_INSTANCES],
    /// Tone's `_source` (the instance a stop / restart acts on).
    source: Option<usize>,
    /// The buffer new sources play (old ones keep theirs).
    buffer: NoiseBuffer,
    volume: AutoParam,
    /// Whether a buffer source has ever been connected (the noise path is stereo from then on).
    pub connected: bool,
    sr: f64,
}

impl NoiseSource {
    pub fn new(buffer: NoiseBuffer, sr: f64, cx: Clock) -> Self {
        let inst = || (NoisePlayer::new(buffer.clone()), Gate::new(cx), false);
        Self {
            state: SourceState::default(),
            inst: [inst(), inst(), inst(), inst()],
            source: None,
            buffer,
            volume: AutoParam::gain(1.0, cx),
            connected: false,
            sr,
        }
    }

    /// `noise.type = …`: the new buffer; a started noise switches now (`_stop(now)` +
    /// `_start(now)`: a new buffer source with a new random offset).
    pub fn set_buffer(&mut self, buffer: NoiseBuffer, rng: &mut Mulberry32, cx: Clock) {
        self.buffer = buffer;
        if self.state.started_at(cx.now) {
            self.stop_source(cx.now, cx);
            self.start_new(cx.now, rng, cx);
        }
    }

    /// `noise.volume.value = gainToDb(g)` (at now).
    pub fn set_level(&mut self, g: f64, cx: Clock) {
        self.volume.set_value(volume_gain_f64(g), cx);
    }

    /// `Noise._start`: a new buffer source with a new random offset.
    fn start_new(&mut self, t: f64, rng: &mut Mulberry32, cx: Clock) {
        // A free slot, else the one that is not the current source and started first.
        let slot = (0..NOISE_INSTANCES)
            .find(|&i| !self.inst[i].2)
            .unwrap_or_else(|| {
                (0..NOISE_INSTANCES)
                    .find(|&i| Some(i) != self.source)
                    .unwrap_or(0)
            });
        let inst = &mut self.inst[slot];
        inst.0.set_buffer(self.buffer.clone());
        inst.0.restart(t, rng.next_f64(), self.sr);
        inst.1 = Gate::new(cx);
        inst.1.start(t, cx);
        inst.2 = true;
        self.source = Some(slot);
        self.connected = true;
    }

    /// `Noise._stop(t)`.
    fn stop_source(&mut self, t: f64, cx: Clock) {
        if let Some(i) = self.source.take() {
            self.inst[i].1.stop(t, cx);
        }
    }

    /// `startSource(noise, t)`; every (re)start draws a new offset from `rng`.
    pub fn start(&mut self, t: f64, rng: &mut Mulberry32, cx: Clock) {
        match start_source(&mut self.state, t, cx.now) {
            Some(Started::New(t)) => self.start_new(t, rng, cx),
            Some(Started::Restart(t)) => {
                // Noise._restart: `_stop(t)` then `_start(t)`.
                self.stop_source(t, cx);
                self.start_new(t, rng, cx);
            }
            None => {}
        }
    }

    pub fn stop(&mut self, t: f64, cx: Clock) {
        if let Some(t) = self.state.stop(t, cx.now) {
            self.stop_source(t, cx);
        }
    }

    pub fn is_idle(&self) -> bool {
        self.inst
            .iter()
            .all(|i| !i.2 || i.1.param.idle_value() == Some(0.0))
    }

    /// Render both channels (Σ sources × gate × volume) into `l` / `r` (overwriting); returns
    /// whether the output was silent.
    pub fn render(
        &mut self,
        frame: u64,
        s: &mut NoiseScratch,
        l: &mut [f32],
        r: &mut [f32],
    ) -> bool {
        let n = l.len();
        l.fill(0.0);
        r.fill(0.0);
        let mut silent = true;
        let current = self.source;
        for (idx, (player, gate, live)) in self.inst.iter_mut().enumerate() {
            if !*live {
                continue;
            }
            let sa = gate.param.render(frame, self.sr, false, &mut s.gate[..n]);
            silent &= gate.param.gate_silent(sa);
            player.render(frame, &mut s.l[..n], &mut s.r[..n]);
            for k in 0..n {
                l[k] += s.l[k] * s.gate[k];
                r[k] += s.r[k] * s.gate[k];
            }
            if Some(idx) != current && gate.param.idle_value() == Some(0.0) {
                *live = false;
            }
        }
        self.volume.render(frame, self.sr, false, &mut s.vol[..n]);
        for k in 0..n {
            l[k] *= s.vol[k];
            r[k] *= s.vol[k];
        }
        silent
    }
}

/// Scratch for [`NoiseSource::render`].
#[derive(Debug, Clone)]
pub(crate) struct NoiseScratch {
    gate: [f32; Q],
    vol: [f32; Q],
    l: [f32; Q],
    r: [f32; Q],
}

impl Default for NoiseScratch {
    fn default() -> Self {
        Self {
            gate: [0.0; Q],
            vol: [0.0; Q],
            l: [0.0; Q],
            r: [0.0; Q],
        }
    }
}
