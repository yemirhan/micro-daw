//! `Tone.Envelope` with its default curves (attack linear, decay / release "exponential"): an
//! ADSR scheduled on a Signal (ConstantSource offset, value 0). Tone 15.1.22 `Envelope.ts`:
//!
//! - attack at `t` with `velocity`: if the current value `c > 0` the attack is shortened to
//!   `(1 − c)·attack`; an attack shorter than one sample is a `cancelScheduledValues` + `set`,
//!   otherwise `linearRampTo(velocity, attack, t)` (`setRampPoint` first); then, if `decay ≠ 0`
//!   and `sustain < 1`, `exponentialApproachValueAtTime(velocity·sustain, t + attack, decay)`;
//! - release at `t`, only when the current value is > 0: `targetRampTo(0, release, t)` (or a set
//!   to 0 when shorter than one sample).
//!
//! `AmplitudeEnvelope` multiplies its input by these values (its gain param, set to 0 by
//! `connectSignal`, plus the envelope).

use super::param::{AutoParam, Clock};

#[derive(Debug, Clone)]
pub(crate) struct Envelope {
    pub sig: AutoParam,
    pub attack: f64,
    pub decay: f64,
    pub sustain: f64,
    pub release: f64,
}

impl Envelope {
    pub fn new(attack: f64, decay: f64, sustain: f64, release: f64, cx: Clock) -> Self {
        Self {
            sig: AutoParam::signal(0.0, cx),
            attack,
            decay,
            sustain,
            release,
        }
    }

    pub fn set(&mut self, attack: f64, decay: f64, sustain: f64, release: f64) {
        self.attack = attack;
        self.decay = decay;
        self.sustain = sustain;
        self.release = release;
    }

    pub fn trigger_attack(&mut self, time: f64, velocity: f64, cx: Clock) {
        let mut attack = self.attack;
        let decay = self.decay;
        let current = self.sig.get_value_at_time(time);
        if current > 0.0 {
            let attack_rate = 1.0 / attack;
            let remaining = 1.0 - current;
            attack = remaining / attack_rate;
        }
        if attack < 1.0 / cx.sr {
            self.sig.cancel_scheduled_values(time, cx);
            self.sig.set_value_at_time(velocity, time, cx);
        } else {
            self.sig.linear_ramp_to(velocity, attack, time, cx);
        }
        if decay != 0.0 && self.sustain < 1.0 {
            let decay_value = velocity * self.sustain;
            let decay_start = time + attack;
            self.sig
                .exponential_approach_value_at_time(decay_value, decay_start, decay, cx);
        }
    }

    pub fn trigger_release(&mut self, time: f64, cx: Clock) {
        let current = self.sig.get_value_at_time(time);
        if current > 0.0 {
            let release = self.release;
            if release < 1.0 / cx.sr {
                self.sig.set_value_at_time(0.0, time, cx);
            } else {
                self.sig.target_ramp_to(0.0, release, time, cx);
            }
        }
    }

    /// Envelope values for one quantum (ConstantSource offset, not connected).
    pub fn render(&mut self, frame: u64, sr: f64, out: &mut [f32]) {
        self.sig.render(frame, sr, false, out);
    }

    pub fn prune(&mut self, now: f64) {
        self.sig.prune(now);
    }
}
