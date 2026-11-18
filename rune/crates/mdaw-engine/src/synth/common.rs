//! Shared voice helpers (`src/audio/voices/common.ts`, `voices/types.ts`).

use mdaw_dsp::factory::Mulberry32;

use super::wa::param::Clock;

pub fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    hi.min(lo.max(v))
}

pub fn midi_to_freq(midi: f64) -> f64 {
    440.0 * 2f64.powf((midi - 69.0) / 12.0)
}

/// Minimum time between a voice's attack and its release (a release is never dropped).
pub const MIN_GATE: f64 = 0.012;
/// Fade used by hardStop (releaseAll / voice steal).
pub const HARD_STOP_FADE: f64 = 0.015;

/// Velocity (0..1) → linear gain; `sensitivity` 0 = constant, 1 = full range (`v^1.7`).
pub fn velocity_gain(velocity: f64, sensitivity: f64) -> f64 {
    let v = clamp(velocity, 0.0, 1.0);
    let s = clamp(sensitivity, 0.0, 1.0);
    1.0 - s + s * v.powf(1.7)
}

/// Velocities that arrive as 0..127 → 0..1; missing / non-finite → 0.8.
pub fn norm_velocity(velocity: Option<f64>) -> f64 {
    match velocity {
        Some(v) if v.is_finite() => {
            if v > 1.0 {
                clamp(v / 127.0, 0.0, 1.0)
            } else {
                clamp(v, 0.0, 1.0)
            }
        }
        _ => 0.8,
    }
}

pub fn gain_to_db(gain: f64) -> f64 {
    if gain <= 0.0 {
        f64::NEG_INFINITY
    } else {
        20.0 * gain.log10()
    }
}

/// A per-voice monotonically increasing start time (Tone sources reject a restart at the same
/// time as the previous start).
#[derive(Debug, Clone, Copy)]
pub struct MonotonicClock {
    last: f64,
}

impl Default for MonotonicClock {
    fn default() -> Self {
        Self {
            last: f64::NEG_INFINITY,
        }
    }
}

impl MonotonicClock {
    pub fn next(&mut self, time: f64) -> f64 {
        let t = if time <= self.last {
            self.last + 0.0005
        } else {
            time
        };
        self.last = t;
        t
    }

    pub fn last_time(&self) -> f64 {
        self.last
    }
}

/// Note name or number → MIDI (`noteToMidi`): 'C4', 'F#3', 'Bb2', 'Cs4'; -1 when unparsable.
pub fn note_name_to_midi(note: &str) -> i32 {
    let s = note.trim().as_bytes();
    if s.is_empty() {
        return -1;
    }
    let pc = match s[0].to_ascii_uppercase() {
        b'C' => 0,
        b'D' => 2,
        b'E' => 4,
        b'F' => 5,
        b'G' => 7,
        b'A' => 9,
        b'B' => 11,
        _ => return -1,
    };
    let mut i = 1;
    let mut acc = 0;
    if let Some(&c) = s.get(1) {
        match c {
            b'#' | b's' => {
                acc = 1;
                i = 2;
            }
            b'b' => {
                acc = -1;
                i = 2;
            }
            _ => {}
        }
    }
    let rest = &note.trim()[i..];
    let digits = rest.strip_prefix('-').unwrap_or(rest);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return -1;
    }
    let Ok(oct) = rest.parse::<i32>() else {
        return -1;
    };
    (oct + 1) * 12 + pc + acc
}

const NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// `midiToName`.
pub fn midi_to_name(midi: f64) -> String {
    let m = midi.round() as i64;
    format!(
        "{}{}",
        NAMES[m.rem_euclid(12) as usize],
        m.div_euclid(12) - 1
    )
}

/// What a voice gets when it schedules: the clock and the instrument's noise-offset generator
/// (Tone draws every noise start offset from the page-wide `Math.random`).
pub(crate) struct Ctx<'a> {
    pub cx: Clock,
    pub rng: &'a mut Mulberry32,
}

/// Per-quantum modulation from the instrument's LFO (cents): vibrato → oscillator detune,
/// filter → filter detune.
pub(crate) struct ModBus<'a> {
    pub vibrato: &'a [f32],
    pub filter: &'a [f32],
}

/// One monophonic voice (`voices/types.ts` `Voice`). Scheduling methods take context seconds;
/// `render` ADDS one quantum into the bus (`l` and `r`; a mono voice adds the same signal to
/// both, like Chromium's up-mix into the stereo bus).
pub(crate) trait Voice: Send {
    type Preset;
    /// What [`Voice::update`] needs that would allocate or lock (wave tables, noise buffers),
    /// prepared once per preset on the control side and shared by the instrument's voices.
    type Prep: Send;
    fn attack(
        &mut self,
        midi: i32,
        time: f64,
        velocity: f64,
        glide_from: Option<i32>,
        cx: &mut Ctx,
    );
    fn glide(&mut self, midi: i32, time: f64, cx: &mut Ctx);
    /// Key up; returns the time the voice will be silent.
    fn release(&mut self, time: f64, cx: &mut Ctx) -> f64;
    /// Fast fade-out (voice steal / panic); returns the time the voice will be silent.
    fn hard_stop(&mut self, time: f64, cx: &mut Ctx) -> f64;
    fn release_duration(&self) -> f64;
    /// Apply new parameters live (audio thread: allocation-free; `prep` is the preset's
    /// prepared resources, `preset` is shared with the instrument, which keeps the previous one
    /// alive until it leaves through the garbage ring).
    fn update(&mut self, preset: &std::sync::Arc<Self::Preset>, prep: &Self::Prep, cx: &mut Ctx);
    /// Clear DSP state that went non-finite (the instrument's output guard).
    fn clear_non_finite(&mut self) {}
    /// Nothing sounds and nothing is scheduled: rendering can be skipped.
    fn is_idle(&self) -> bool;
    fn render(&mut self, frame: u64, m: &ModBus, l: &mut [f32], r: &mut [f32]);
    /// Drop scheduling history no query at or after `now` can see.
    fn prune(&mut self, now: f64);
    /// The voice is first used at quantum `frame` (the Tone voice is constructed then).
    fn born(&mut self, frame: u64, sr: f64);
    /// Export (`Tone.Offline`): the voice's nodes exist from frame 0 and it starts rendering at
    /// quantum `frame` (state that ran on silence until then is brought up to date).
    fn offline_start(&mut self, _frame: u64) {}
    /// Export (`Tone.Offline`): Tone constructed the voice at its clock time `t`, ahead of the
    /// native render, so the constructor's `value =` writes are native events at `t` (effective
    /// from frame `ceil(t·sr)`, one frame after the quantum start when `t`, an accumulated block
    /// time, lands a hair above it), while the voice's signals have run since frame 0.
    fn offline_built(&mut self, _t: f64, _cx: Clock) {}
}

/// Whether Tone signals (ConstantSourceNodes, `start(0)` → clamped to `currentTime`) created at
/// quantum `frame` start one frame late: Chromium's `start_frame_offset = currentTime·sr −
/// start_frame` is positive when `(frame/sr)·sr` rounds above `frame` (e.g. frame 96512 at
/// 48 kHz), and the node then skips its first frame. A fresh voice's signals output 0 there; the
/// oscillators starting in that quantum read that frame's (zero) frequency as their first phase
/// increment (see `wa::osc`).
pub(crate) fn signals_skip_first_frame(frame: u64, sr: f64) -> bool {
    (frame as f64 / sr) * sr > frame as f64
}
