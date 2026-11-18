//! Rendering: the block driver (`process`, offline renders) and the per-quantum mix (tracks
//! through their strips, gain and mute, the clicks, the master bus).

use mdaw_dsp::buffer::AudioBuffer;

use super::api::Garbage;
use super::ring::Producer;
use super::shared::Shared;
use super::strip::pan_gains;
use super::{BlockClock, Core, Q};

impl Core {
    /// [`Core::process`] for a device callback: also records the wall-clock anchor (this block's
    /// first frame is requested at `clock.instant` and heard `clock.output_latency` later) and
    /// publishes it with the clock snapshot (`Shared::snapshot`).
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

    /// Offline render that also returns the tracks' reverb sends (the RoomReverb wet leak of the
    /// track strips, see `core/strip.rs`), as 4 buses: `[A_L, B_L, A_R, B_R]` where output L
    /// gets `IR_L ⊛ A_L + IR_R ⊛ B_L` (normalized IR) and output R `IR_L ⊛ A_R + IR_R ⊛ B_R`.
    /// Renders whole quanta from a quantum boundary (allocates: never on the audio thread).
    pub fn render_offline_with_reverb_send(
        &mut self,
        frames: usize,
    ) -> (AudioBuffer, [Vec<f32>; 4]) {
        self.aux = Some(Box::new([[0.0; Q]; 4]));
        let mut l = Vec::with_capacity(frames + Q);
        let mut r = Vec::with_capacity(frames + Q);
        let mut aux: [Vec<f32>; 4] = Default::default();
        if self.carry < Q {
            // finish the current quantum without sends
            l.extend_from_slice(&self.q_l[self.carry..]);
            r.extend_from_slice(&self.q_r[self.carry..]);
            for a in &mut aux {
                a.resize(l.len(), 0.0);
            }
            self.carry = Q;
        }
        while l.len() < frames {
            self.render_quantum();
            l.extend_from_slice(&self.q_l);
            r.extend_from_slice(&self.q_r);
            if let Some(x) = self.aux.as_ref() {
                for (a, s) in aux.iter_mut().zip(x.iter()) {
                    a.extend_from_slice(s);
                }
            }
        }
        // the rest of the last quantum is handed out by the next `process`
        self.carry = Q - (l.len() - frames);
        self.delivered += frames as u64;
        l.truncate(frames);
        r.truncate(frames);
        for a in &mut aux {
            a.truncate(frames);
        }
        self.aux = None;
        let buf = AudioBuffer {
            sample_rate: self.sr as u32,
            channels: vec![l, r],
        };
        (buf, aux)
    }

    // --- Rendering ------------------------------------------------------------------------

    pub(super) fn render(&mut self, q0: u64) {
        self.q_l = [0.0; Q];
        self.q_r = [0.0; Q];
        if let Some(aux) = self.aux.as_mut() {
            **aux = [[0.0; Q]; 4];
        }
        for (i, slot) in self.tracks.iter_mut().enumerate() {
            let Some(inst) = slot.instrument.as_mut() else {
                continue;
            };
            self.s_l = [0.0; Q];
            self.s_r = [0.0; Q];
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
            let gain = if slot.muted { 0.0 } else { slot.gain };
            if let Some(strip) = slot.strip.as_mut() {
                strip.process(&mut self.s_l, &mut self.s_r, slot.pan);
                if let Some(aux) = self.aux.as_mut() {
                    let (gl, gr) = pan_gains(slot.pan);
                    let (gl, gr) = ((gl * 0.5) as f32 * gain, (gr * 0.5) as f32 * gain);
                    for k in 0..Q {
                        let (sl, sr) = (strip.send[0][k], strip.send[1][k]);
                        aux[0][k] += gl * sl;
                        aux[1][k] += gl * sr;
                        aux[2][k] += gr * sl;
                        aux[3][k] += gr * sr;
                    }
                }
            }
            let mut energy = 0f32;
            for k in 0..Q {
                let (l, r) = (self.s_l[k] * gain, self.s_r[k] * gain);
                energy += l * l + r * r;
                self.q_l[k] += l;
                self.q_r[k] += r;
            }
            self.shared.publish_level(i, energy / (2 * Q) as f32);
        }
        self.metro.render(q0, &mut self.q_l, &mut self.q_r);
        self.count_in.render(q0, &mut self.q_l, &mut self.q_r);
        if let Some(g) = self.master {
            let curve = self.clip_curve;
            for x in self.q_l.iter_mut().chain(self.q_r.iter_mut()) {
                *x = mdaw_dsp::shaping::master_bus_sample(curve, *x * g);
            }
        }
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
