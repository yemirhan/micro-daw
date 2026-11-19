//! `Tone.Sampler` (Tone 15.1.22 `instrument/Sampler.js`) as the SampledPiano drives it:
//! `{ urls, attack: 0, release, curve: 'exponential' }`, output volume 0 dB.
//!
//! - **Nearest sample** (`_findClosest`): search outward from the note, `midi + i` before
//!   `midi − i`, so of two equidistant roots the upper one wins (searches 96 semitones).
//! - **Rate**: `2^((difference + remainder) / 12)` with `midiFloat = ftomf(mtof(note))` (the
//!   remainder is the f64 round-off of the note-name round trip), stored by Chromium as f32.
//! - **Source**: one `ToneBufferSource` per attack: a native `AudioBufferSourceNode`
//!   ([`BufferPlayer`], Chromium-exact) into a gain whose param gets `setValueAtTime(0, 0)` and
//!   `setValueAtTime(velocity, time)`, then `stop(time + duration / rate)` (the natural end).
//! - **Stop** (`OneShotSource._stopGain`, fade-out = the `release` the source was created with,
//!   exponential curve): cancels the previous stop (`cancelScheduledValues(start + 1/sr)`), then
//!   `targetRampTo(0, r, t)` = hold the value at `t`, `setTargetAtTime(0, t, τ = ln(r+1)/ln 200)`,
//!   at `t + 0.9r` Tone's own value of that curve, then a linear ramp to 0 at `t + r`. Chromium
//!   outputs the target (0) once the curve is within e^−10 of it, checked per render quantum.
//!   A release before the source starts silences it.
//! - **Bookkeeping**: `_activeSources[midi]` lists the sources not yet released;
//!   `triggerRelease(midi)` stops all of them and empties the list, `releaseAll` stops every
//!   listed source. A released source is never stopped again (so a triggerAttackRelease's
//!   pre-scheduled release is the only one it gets).
//!
//! The native node is never stopped (the capture's offline Tone clock never fires the
//! `setTimeout` that would): the gain envelope does the cutting. Sources live in a fixed pool of
//! [`MAX_SOURCES`]; a source is recycled once its buffer has ended or its fade-out is over. When
//! every slot is busy the quietest released (else the oldest) source is replaced (Tone has no
//! limit).

use mdaw_dsp::abs_node::{BufferPlayer, StartParams};
use mdaw_dsp::buffer::SharedBuffer;

use super::samples::PianoSamples;

/// Size of the source pool.
pub const MAX_SOURCES: usize = 160;
/// Chromium `kSetTargetZeroThreshold`-like snap (e^−10, see `mdaw_dsp::factory::automation`).
const SET_TARGET_THRESHOLD: f64 = 4.539_992_976_248_485e-5;
const Q: usize = crate::contract::RENDER_QUANTUM;

/// Tone `mtof` / `ftomf` (A4 = 440).
fn mtof(midi: f64) -> f64 {
    440.0 * 2f64.powf((midi - 69.0) / 12.0)
}

fn ftomf(f: f64) -> f64 {
    69.0 + 12.0 * (f / 440.0).log2()
}

/// JS `Math.round`.
pub(crate) fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// `_findClosest`: the signed distance from `midi` to the nearest root (`midi - root`), or None
/// when no root is within 96 semitones (Tone throws).
pub fn find_closest(samples: &PianoSamples, midi: i32) -> Option<i32> {
    (0..96).find_map(|i| {
        if samples.get(midi + i).is_some() {
            Some(-i)
        } else if samples.get(midi - i).is_some() {
            Some(i)
        } else {
            None
        }
    })
}

/// Root note and playback rate (as the f32 playbackRate) `triggerAttack(name)` picks for `midi`.
pub fn pick_sample(samples: &PianoSamples, midi: i32) -> Option<(i32, f32)> {
    let midi_float = ftomf(mtof(midi as f64));
    let rounded = js_round(midi_float);
    let remainder = midi_float - rounded;
    let rounded = rounded as i32;
    let difference = find_closest(samples, rounded)?;
    let rate = 2f64.powf((difference as f64 + remainder) / 12.0);
    Some((rounded - difference, rate as f32))
}

/// The gain envelope of one source (its `_gainNode.gain` timeline).
#[derive(Debug, Clone, Copy)]
struct Envelope {
    start: f64,
    velocity: f32,
    /// When the fade-out begins: the release time, else the natural end.
    stop: f64,
    /// Origin of the setTarget curve: `stop`, or, for a stop scheduled at least one frame in the
    /// past (late), the first frame rendered after the call: Chromium starts the curve there
    /// from the held value (measured: `overflow` / `grand_piano` captures, −98 → −136 dB).
    origin: f64,
    /// Fade-out duration (the sampler's `release` when the source was made).
    fade: f64,
    tau: f64,
    /// Tone's value of the setTarget curve at `stop + 0.9·fade`.
    held: f32,
    /// The setTarget curve snapped to 0 (Chromium), until the next event.
    snapped: bool,
}

impl Envelope {
    fn new(start: f64, velocity: f32, natural_end: f64, fade: f64) -> Self {
        let mut e = Self {
            start,
            velocity,
            stop: natural_end,
            origin: natural_end,
            fade,
            tau: (fade + 1.0).ln() / 200f64.ln(),
            held: 0.0,
            snapped: false,
        };
        e.schedule_stop(natural_end, 0, 1.0);
        e
    }

    /// Stop at `t`, called when `next_frame` is the next frame to render.
    fn schedule_stop(&mut self, t: f64, next_frame: u64, sr: f64) {
        self.stop = t;
        self.origin = if next_frame as f64 >= t * sr + 1.0 {
            next_frame as f64 / sr
        } else {
            t
        };
        self.snapped = false;
        let v = self.velocity as f64;
        self.held = (v * (-(0.9 * self.fade) / self.tau).exp()) as f32;
    }

    /// Gain at time `t` (frame `k` of a quantum starting at a multiple of 128 when `k == 0`).
    #[inline]
    fn value(&mut self, t: f64, quantum_start: bool) -> f32 {
        if t < self.start || self.stop < self.start {
            return 0.0;
        }
        if t < self.stop {
            return self.velocity;
        }
        let t09 = self.stop + 0.9 * self.fade;
        if t < t09 {
            if self.snapped {
                return 0.0;
            }
            let v = self.velocity as f64 * (-(t - self.origin).max(0.0) / self.tau).exp();
            if quantum_start && v.abs() < SET_TARGET_THRESHOLD {
                self.snapped = true;
                return 0.0;
            }
            return v as f32;
        }
        let t1 = self.stop + self.fade;
        if t < t1 {
            let h = self.held as f64;
            return (h + (0.0 - h) * ((t - t09) / (t1 - t09))) as f32;
        }
        0.0
    }

    /// Silent from `t` on.
    fn done_at(&self, t: f64) -> bool {
        self.stop < self.start || t >= self.stop + self.fade
    }
}

#[derive(Debug, Clone)]
struct Source {
    player: BufferPlayer,
    env: Envelope,
    midi: i32,
    /// In `_activeSources[midi]` (not released yet).
    listed: bool,
    /// Attack order (for stealing).
    serial: u64,
    /// Onset (context seconds): with `midi`, the instance `cut_released` targets.
    start: f64,
}

/// The sampler: a fixed pool of sources.
pub struct ToneSampler {
    sr: u32,
    sources: Vec<Option<Source>>,
    serial: u64,
    /// The next frame `process` renders (calls in between happen "at" this frame).
    next_frame: u64,
    /// Fade-out of new sources (`sampler.release`).
    pub release: f64,
    tmp: [[f32; Q]; 2],
}

impl ToneSampler {
    pub fn new(sr: u32, release: f64) -> Self {
        Self {
            sr,
            sources: vec![None; MAX_SOURCES],
            serial: 0,
            next_frame: 0,
            release,
            tmp: [[0.0; Q]; 2],
        }
    }

    /// Sources currently allocated (sounding or scheduled).
    pub fn source_count(&self) -> usize {
        self.sources.iter().filter(|s| s.is_some()).count()
    }

    /// `triggerAttack(midiToName(midi), time, gain)`.
    pub fn trigger_attack(&mut self, samples: &PianoSamples, midi: i32, time: f64, gain: f64) {
        let Some((root, rate)) = pick_sample(samples, midi) else {
            return;
        };
        let Some(buffer) = samples.get(root) else {
            return;
        };
        let buffer: SharedBuffer = buffer.clone();
        let duration = buffer.frames() as f64 / buffer.sample_rate as f32 as f64;
        let natural_end = time + duration / rate as f64;
        let player = BufferPlayer::new(
            buffer,
            self.sr,
            StartParams::sampler(time, 0.0, rate as f64, None),
        );
        let env = Envelope::new(time, gain as f32, natural_end, self.release);
        self.serial += 1;
        let src = Source {
            player,
            env,
            midi,
            listed: true,
            serial: self.serial,
            start: time,
        };
        let slot = self.free_slot(time);
        self.sources[slot] = Some(src);
    }

    /// `triggerRelease(midiToName(midi), time)`.
    pub fn trigger_release(&mut self, midi: i32, time: f64) {
        for s in self.sources.iter_mut().flatten() {
            if s.listed && s.midi == midi {
                s.listed = false;
                s.env.schedule_stop(time, self.next_frame, self.sr as f64);
            }
        }
    }

    /// Bring forward to `time` the stop of `midi`'s source struck at `started` if it is already
    /// released for later than `time` (an attack-release note cut short).
    pub fn cut_released(&mut self, midi: i32, started: f64, time: f64) {
        for s in self.sources.iter_mut().flatten() {
            if !s.listed && s.midi == midi && (s.start - started).abs() <= 1e-6 && s.env.stop > time
            {
                s.env.schedule_stop(time, self.next_frame, self.sr as f64);
            }
        }
    }

    /// `releaseAll(time)`.
    pub fn release_all(&mut self, time: f64) {
        for s in self.sources.iter_mut().flatten() {
            if s.listed {
                s.listed = false;
                s.env.schedule_stop(time, self.next_frame, self.sr as f64);
            }
        }
    }

    fn free_slot(&self, time: f64) -> usize {
        if let Some(i) = self.sources.iter().position(Option::is_none) {
            return i;
        }
        // Pool full: the released source closest to the end of its fade, else the oldest.
        let mut best = 0;
        let mut best_key = (true, f64::INFINITY, u64::MAX);
        for (i, s) in self.sources.iter().enumerate() {
            let Some(s) = s else { continue };
            let key = (s.listed, s.env.stop + s.env.fade - time, s.serial);
            if key.partial_cmp(&best_key) == Some(std::cmp::Ordering::Less) {
                best = i;
                best_key = key;
            }
        }
        best
    }

    /// Add every source into `l` / `r` (`frames ≤ 128`, starting at context frame `frame`).
    pub fn process(&mut self, frame: u64, l: &mut [f32], r: &mut [f32]) {
        let n = l.len().min(r.len()).min(Q);
        let sr = self.sr as f64;
        let quantum_start = frame.is_multiple_of(Q as u64);
        self.next_frame = frame + n as u64;
        for slot in self.sources.iter_mut() {
            let Some(s) = slot else { continue };
            let t_end = (frame + n as u64) as f64 / sr;
            if s.player.is_finished() || s.env.done_at(frame as f64 / sr) {
                *slot = None;
                continue;
            }
            if t_end <= s.env.start {
                continue;
            }
            let channels = s.player.buffer().channels.len();
            let [a, b] = &mut self.tmp;
            let played = {
                let mut outs: [&mut [f32]; 2] = [&mut a[..n], &mut b[..n]];
                let used = channels.clamp(1, 2);
                s.player.process(&mut outs[..used], frame)
            };
            if !played {
                continue;
            }
            for i in 0..n {
                let t = (frame + i as u64) as f64 / sr;
                let g = s.env.value(t, quantum_start && i == 0);
                let xl = a[i];
                let xr = if channels >= 2 { b[i] } else { xl };
                l[i] += xl * g;
                r[i] += xr * g;
            }
        }
    }
}
