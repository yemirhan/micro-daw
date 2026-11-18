//! Rendering: the block driver (`process`, offline renders) and the per-quantum mix (tracks
//! through their effect chains, gain and mute, meters, the clicks, the master bus).

use mdaw_dsp::buffer::AudioBuffer;

use super::api::{Garbage, SlotFx};
use super::ring::Producer;
use super::shared::Shared;
use super::{BlockClock, Core, Q};

impl Core {
    /// [`Core::process`] for a device callback: also records the wall-clock anchor (this block's
    /// first frame is requested at `clock.instant` and heard `clock.output_latency` later) and
    /// publishes it with the clock snapshot (`Shared::snapshot`).
    ///
    /// The app's glue for `mdaw-audio-host` (whose render callback receives a `BlockTiming` per
    /// chunk; `tests/host_rates.rs` runs exactly this):
    /// `core.process_timed(l, r, BlockClock { instant: t.chunk_instant(), output_latency:
    /// t.output_latency })`. Offline renders use [`Core::process`] (no anchor).
    pub fn process_timed(&mut self, out_l: &mut [f32], out_r: &mut [f32], clock: BlockClock) {
        let n = out_l.len().min(out_r.len());
        self.anchor = Some((
            self.delivered,
            clock.instant,
            clock.output_latency,
            n.min(u32::MAX as usize) as u32,
        ));
        self.process(out_l, out_r);
        self.publish_state();
    }

    /// Fill `out_l` / `out_r` (any length; the shorter one wins). Renders whole quanta and
    /// carries the rest of the last one to the next call.
    pub fn process(&mut self, out_l: &mut [f32], out_r: &mut [f32]) {
        let n = out_l.len().min(out_r.len());
        let mut done = 0;
        while done < n {
            if self.carry == Q {
                self.render_quantum();
                self.carry = 0;
            }
            let take = (Q - self.carry).min(n - done);
            out_l[done..done + take].copy_from_slice(&self.q_l[self.carry..self.carry + take]);
            out_r[done..done + take].copy_from_slice(&self.q_r[self.carry..self.carry + take]);
            self.carry += take;
            done += take;
        }
        self.delivered += n as u64;
    }

    /// Render `seconds` of audio offline (export, tests) through the same code path.
    pub fn render_offline(&mut self, seconds: f64) -> AudioBuffer {
        let frames = (seconds * self.sr).round().max(0.0) as usize;
        let mut l = vec![0f32; frames];
        let mut r = vec![0f32; frames];
        self.process(&mut l, &mut r);
        AudioBuffer {
            sample_rate: self.sr as u32,
            channels: vec![l, r],
        }
    }

    // --- Rendering ------------------------------------------------------------------------

    pub(super) fn render(&mut self, q0: u64) {
        self.q_l = [0.0; Q];
        self.q_r = [0.0; Q];
        for (i, slot) in self.tracks.iter_mut().enumerate() {
            if slot.instrument.is_none() && slot.strip.is_none() {
                continue;
            }
            self.s_l = [0.0; Q];
            self.s_r = [0.0; Q];
            if let Some(inst) = slot.instrument.as_mut() {
                let inst = inst.as_instrument();
                inst.process(q0, &mut self.s_l, &mut self.s_r);
                inst.take_garbage(&mut |b| {
                    trash_to(
                        &mut self.garbage,
                        &mut self.overflow,
                        &self.shared,
                        Garbage::Buffer(b),
                    )
                });
                self.shared.count_leaks(inst.take_leaks());
            }
            // The effect chain keeps running without an instrument (tails, automation).
            match slot.strip.as_mut() {
                Some(SlotFx::Track(s)) => s.process(q0, &mut self.s_l, &mut self.s_r),
                Some(SlotFx::Live(l)) => l.process(q0, &mut self.s_l, &mut self.s_r),
                None => {}
            }
            let gain = if slot.muted { 0.0 } else { slot.gain };
            for k in 0..Q {
                self.s_l[k] *= gain;
                self.s_r[k] *= gain;
                self.q_l[k] += self.s_l[k];
                self.q_r[k] += self.s_r[k];
            }
            slot.meter.push(&self.s_l, &self.s_r);
            self.shared.publish_level(i, slot.meter.mean_square());
        }
        self.metro.render(q0, &mut self.q_l, &mut self.q_r);
        self.count_in.render(q0, &mut self.q_l, &mut self.q_r);
        if let Some(m) = self.master.as_mut() {
            m.render(q0, self.sr, &mut self.master_gain);
            let curve = self.clip_curve;
            for k in 0..Q {
                let g = self.master_gain[k];
                self.q_l[k] = mdaw_dsp::shaping::master_bus_sample(curve, self.q_l[k] * g);
                self.q_r[k] = mdaw_dsp::shaping::master_bus_sample(curve, self.q_r[k] * g);
            }
        }
        self.master_meter.push(&self.q_l, &self.q_r);
        self.shared
            .publish_master_level(self.master_meter.mean_square());
    }
}

/// JS `Math.round` (half up).
pub(crate) fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// Send `g` to the control side (overflow list when the ring is full; leak when both are).
pub(super) fn trash_to(
    garbage: &mut Producer<Garbage>,
    overflow: &mut Vec<Garbage>,
    shared: &Shared,
    g: Garbage,
) {
    if let Err(g) = garbage.push(g) {
        if overflow.len() < overflow.capacity() {
            overflow.push(g);
        } else {
            // Never free on the audio thread: leak (counted) rather than drop.
            std::mem::forget(g);
            shared.count_leaks(1);
        }
    }
}
