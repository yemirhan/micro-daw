//! The metronome / count-in click: an emulation of the Electron app's click synth
//! (`Metronome.ts` `createClickSynth`):
//!
//! ```text
//! new Tone.Synth({ oscillator: { type: 'sine' },
//!                  envelope: { attack: 0.001, decay: 0.05, sustain: 0, release: 0.01 },
//!                  volume: -15 })      // triggerAttackRelease(note, '32n', time)
//! ```
//!
//! Tone's Envelope keeps its default curves: **linear attack, exponential decay, exponential
//! release**. Its automation (Envelope.triggerAttack / triggerRelease, Param.setRampPoint /
//! exponentialApproachValueAtTime) evaluates to, for a note at `t0` released at `tr`:
//!
//! - attack: `setRampPoint(t0)` (value 0 is replaced by Param `_minOutput` = 1e-7), then a linear
//!   ramp to the velocity (1) over the attack (shortened to `(1 − v)·A` when retriggered at a
//!   current value `v > 0`; set instantly when shorter than a sample);
//! - decay from `ds = t0 + A`: `setTargetAtTime(0, ds, τ)` with `τ = ln(D + 1)/ln 200`, held at
//!   `ds + 0.9·D`, then a linear ramp to 0 at `ds + D`;
//! - release at `tr` (only if the value there is > 0): hold the value, the same approach to 0
//!   with `τ = ln(R + 1)/ln 200`, held at `tr + 0.9·R`, linear to 0 at `tr + R`.
//!
//! The sine starts at phase 0 at each note (the monophonic Synth restarts its oscillator), at
//! −15 dB (×0.17783), mono to both channels. A newer note cancels the older note's pending
//! release (Tone's `cancelAndHoldAtTime` in the new attack).

const ATTACK: f64 = 0.001;
const DECAY: f64 = 0.05;
const RELEASE: f64 = 0.01;
const MIN_OUTPUT: f64 = 1e-7;
/// `CLICK_VOLUME_DB` (−15 dB).
pub const CLICK_GAIN: f64 = 0.177_827_941_003_892_3;
/// C6 (accent) and G5 (beat), A4 = 440 Hz.
pub const ACCENT_HZ: f64 = 1_046.502_261_202_394_6;
pub const BEAT_HZ: f64 = 783.990_871_963_498_4;
const QUEUE: usize = 32;

/// `Math.log(rampTime + 1) / Math.log(200)`.
fn approach_tc(ramp: f64) -> f64 {
    (ramp + 1.0).ln() / 200f64.ln()
}

/// `exponentialApproachValueAtTime(0, start, ramp)` from value `v` at `start`, evaluated `dt` in.
fn approach(v: f64, dt: f64, ramp: f64) -> f64 {
    let tc = approach_tc(ramp);
    if dt < 0.9 * ramp {
        v * (-dt / tc).exp()
    } else if dt < ramp {
        let held = v * (-0.9 * ramp / tc).exp();
        held * (1.0 - (dt - 0.9 * ramp) / (0.1 * ramp))
    } else {
        0.0
    }
}

#[derive(Debug, Clone, Copy)]
struct Note {
    time: f64,
    dur: f64,
    freq: f64,
}

#[derive(Debug, Clone, Copy)]
struct Voice {
    t0: f64,
    freq: f64,
    /// Attack start value and (possibly shortened) attack time.
    v0: f64,
    attack: f64,
    release_at: f64,
    /// Disposed (silenced) from this time on.
    cut: f64,
}

impl Voice {
    /// Attack + decay (no release).
    fn attack_decay(&self, t: f64) -> f64 {
        let dt = t - self.t0;
        if dt < 0.0 {
            return 0.0;
        }
        if dt < self.attack {
            return self.v0 + (1.0 - self.v0) * dt / self.attack;
        }
        approach(1.0, dt - self.attack, DECAY)
    }

    fn env(&self, t: f64) -> f64 {
        if t < self.release_at {
            return self.attack_decay(t);
        }
        let v = self.attack_decay(self.release_at);
        if v > 0.0 {
            approach(v, t - self.release_at, RELEASE)
        } else {
            self.attack_decay(t)
        }
    }

    fn done(&self, t: f64) -> bool {
        t >= self.release_at + RELEASE && t >= self.t0 + self.attack + DECAY
    }
}

/// One monophonic click synth with a queue of scheduled notes.
pub(crate) struct ClickSynth {
    sr: f64,
    queue: [Option<Note>; QUEUE],
    voice: Option<Voice>,
    /// Latest dispose time: notes scheduled before it are cut there.
    cut_at: f64,
}

impl ClickSynth {
    pub fn new(sample_rate: f64) -> Self {
        Self {
            sr: sample_rate,
            queue: [None; QUEUE],
            voice: None,
            cut_at: f64::NEG_INFINITY,
        }
    }

    /// `triggerAttackRelease(freq, dur, time)`. False when the queue is full.
    pub fn schedule(&mut self, time: f64, dur: f64, freq: f64) -> bool {
        match self.queue.iter_mut().find(|n| n.is_none()) {
            Some(slot) => {
                *slot = Some(Note { time, dur, freq });
                true
            }
            None => false,
        }
    }

    /// Dispose: silence from `t` on (sounding voice cut, scheduled notes dropped).
    pub fn silence_from(&mut self, t: f64) {
        for n in &mut self.queue {
            if n.is_some_and(|n| n.time >= t) {
                *n = None;
            }
        }
        self.cut_at = t;
        if let Some(v) = &mut self.voice {
            v.cut = v.cut.min(t);
        }
    }

    fn next_note(&self) -> Option<(usize, Note)> {
        self.queue
            .iter()
            .enumerate()
            .filter_map(|(i, n)| n.map(|n| (i, n)))
            .min_by(|a, b| a.1.time.total_cmp(&b.1.time))
    }

    fn start(&mut self, n: Note) {
        let current = self.voice.map_or(0.0, |v| v.env(n.time));
        let (v0, attack) = if current > 0.0 {
            (current, (1.0 - current) * ATTACK)
        } else {
            (MIN_OUTPUT, ATTACK)
        };
        let (v0, attack) = if attack < 1.0 / self.sr {
            (1.0, 0.0)
        } else {
            (v0, attack)
        };
        self.voice = Some(Voice {
            t0: n.time,
            freq: n.freq,
            v0,
            attack,
            release_at: n.time + n.dur,
            cut: if n.time < self.cut_at {
                self.cut_at
            } else {
                f64::INFINITY
            },
        });
    }

    /// Add `left.len()` frames starting at absolute frame `frame0`.
    pub fn render(&mut self, frame0: u64, left: &mut [f32], right: &mut [f32]) {
        if self.voice.is_none() && self.queue.iter().all(Option::is_none) {
            return;
        }
        for i in 0..left.len() {
            let t = (frame0 + i as u64) as f64 / self.sr;
            while let Some((idx, n)) = self.next_note() {
                if n.time > t {
                    break;
                }
                self.queue[idx] = None;
                self.start(n);
            }
            let Some(v) = self.voice else { continue };
            if v.done(t) || t >= v.cut {
                self.voice = None;
                continue;
            }
            let s = (std::f64::consts::TAU * v.freq * (t - v.t0)).sin() * v.env(t) * CLICK_GAIN;
            left[i] += s as f32;
            right[i] += s as f32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voice(dur: f64) -> Voice {
        Voice {
            t0: 1.0,
            freq: 784.0,
            v0: MIN_OUTPUT,
            attack: ATTACK,
            release_at: 1.0 + dur,
            cut: f64::INFINITY,
        }
    }

    #[test]
    fn envelope_shape_matches_tone_automation() {
        let v = voice(0.0625); // '32n' at 120 BPM: released after the decay ended
        assert!(v.env(1.0) < 1e-6);
        assert!((v.env(1.0005) - 0.5).abs() < 1e-6, "linear attack");
        assert!((v.env(1.001) - 1.0).abs() < 1e-9);
        let tc = approach_tc(DECAY);
        assert!((tc - 0.009_209).abs() < 1e-5);
        assert!(
            (v.env(1.011) - (-0.01 / tc).exp()).abs() < 1e-9,
            "setTargetAtTime decay"
        );
        let held = (-0.045 / tc).exp();
        assert!(
            (v.env(1.001 + 0.0475) - held * 0.5).abs() < 1e-9,
            "linear tail"
        );
        assert_eq!(v.env(1.052), 0.0);
        assert_eq!(v.env(1.07), 0.0, "release of a finished decay does nothing");
        // '32n' at 300 BPM (25 ms): the release starts during the decay
        let short = voice(0.025);
        let at = short.attack_decay(1.025);
        assert!(at > 0.0);
        assert!((short.env(1.026) - at * (-0.001 / approach_tc(RELEASE)).exp()).abs() < 1e-9);
        assert_eq!(short.env(1.0351), 0.0);
    }

    #[test]
    fn renders_a_click_at_the_scheduled_frame_and_silences() {
        let sr = 48000.0;
        let mut c = ClickSynth::new(sr);
        c.schedule(0.01, 0.0625, ACCENT_HZ);
        let (mut l, mut r) = (vec![0f32; 4800], vec![0f32; 4800]);
        for q in 0..(4800 / 128) {
            let s = q * 128;
            c.render(s as u64, &mut l[s..s + 128], &mut r[s..s + 128]);
        }
        let first = l.iter().position(|x| x.abs() > 1e-9).unwrap();
        assert_eq!(
            first, 481,
            "first frame after t = 0.01 s (phase 0 at frame 480)"
        );
        let peak = l.iter().fold(0f32, |m, x| m.max(x.abs()));
        assert!((peak as f64 - CLICK_GAIN).abs() < 0.01, "peak {peak}");
        assert!(
            l[480 + 2500..].iter().all(|x| *x == 0.0),
            "silent after the decay"
        );
        // a fresh synth: one click now, one later; disposed half way through the first quantum
        let mut c = ClickSynth::new(sr);
        c.schedule(0.0, 0.0625, BEAT_HZ);
        c.schedule(0.004, 0.0625, BEAT_HZ);
        let (mut l1, mut r1) = (vec![0f32; 128], vec![0f32; 128]);
        let (mut l2, mut r2) = (vec![0f32; 128], vec![0f32; 128]);
        c.silence_from(64.0 / sr);
        c.render(0, &mut l1, &mut r1);
        c.render(128, &mut l2, &mut r2);
        assert!(l1[10] != 0.0);
        assert!(l1[64..].iter().all(|x| *x == 0.0), "cut at the stop frame");
        assert!(
            l2.iter().all(|x| *x == 0.0),
            "disposed: the scheduled click never sounds"
        );
    }
}
