//! Rendering: the block driver (`process`, offline renders) and the per-quantum mix (tracks
//! through their effect chains, gain and mute, meters, the clicks, the master bus).

use mdaw_dsp::buffer::AudioBuffer;

use super::api::{Garbage, Phase, SlotFx};
use super::ring::Producer;
use super::shared::Shared;
use super::{BlockClock, Core, FADE_QUANTA, Q, SILENT_PEAK};
use crate::plugin::PluginTransport;

/// What a take records while its slot has no input channel.
static SILENCE: [f32; Q] = [0.0; Q];

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
        // the latency budget uses the largest block seen: a smaller callback must not shrink it
        // (events mapped against it would then run late)
        self.max_block = self.max_block.max(n.min(u32::MAX as usize) as u32);
        self.anchor = Some((
            self.delivered,
            clock.instant,
            clock.output_latency,
            self.max_block,
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
        let q_end = (q0 + Q as u64) as f64 / self.sr;
        // The device input of this quantum (silence without a source).
        self.in_channels = match self.input.as_mut() {
            Some(src) => {
                src.read_quantum(&mut self.in_bufs);
                src.channels().min(self.in_bufs.len())
            }
            None => 0,
        };
        let transport = PluginTransport {
            bpm: self.tr.bpm,
            beat: self.tr.beat_at(q0 as f64 / self.sr),
            playing: self.tr.phase == Phase::Started,
        };
        for (i, slot) in self.tracks.iter_mut().enumerate() {
            if slot.instrument.is_none()
                && slot.strip.is_none()
                && slot.input.is_none()
                && slot.record.is_none()
                && slot.inserts.is_none()
            {
                continue;
            }
            self.s_l = [0.0; Q];
            self.s_r = [0.0; Q];
            let q_start = q0 as f64 / self.sr;
            for r in &mut slot.retiring {
                let Some(ret) = r.as_mut() else { continue };
                self.i_l = [0.0; Q];
                self.i_r = [0.0; Q];
                let inst = ret.inst.as_instrument();
                inst.process(q0, &mut self.i_l, &mut self.i_r);
                if guard(&mut self.i_l, &mut self.i_r) {
                    inst.clear_non_finite();
                    self.shared.count_non_finite();
                }
                let mut done = ret.until <= q_end;
                if let Some(k) = ret.fade.as_mut() {
                    // evicted: linear fade to 0 over FADE_QUANTA quanta, then out
                    let span = (FADE_QUANTA as usize * Q) as f32;
                    for i in 0..Q {
                        let g = 1.0 - ((*k as usize * Q + i + 1) as f32 / span);
                        self.i_l[i] *= g.max(0.0);
                        self.i_r[i] *= g.max(0.0);
                    }
                    *k += 1;
                    done |= *k >= FADE_QUANTA;
                }
                let peak = self
                    .i_l
                    .iter()
                    .chain(self.i_r.iter())
                    .fold(0f32, |m, x| m.max(x.abs()));
                if q_start >= ret.release_at && peak < SILENT_PEAK {
                    ret.quiet = ret.quiet.saturating_add(1);
                } else {
                    ret.quiet = 0;
                }
                for k in 0..Q {
                    self.s_l[k] += self.i_l[k];
                    self.s_r[k] += self.i_r[k];
                }
                if done && let Some(old) = r.take() {
                    trash_to(
                        &mut self.garbage,
                        &mut self.overflow,
                        &self.shared,
                        Garbage::Instrument(old.inst),
                    );
                }
            }
            if let Some(inst) = slot.instrument.as_mut() {
                let inst = inst.as_instrument();
                self.i_l = [0.0; Q];
                self.i_r = [0.0; Q];
                inst.set_transport(&transport);
                inst.process(q0, &mut self.i_l, &mut self.i_r);
                if guard(&mut self.i_l, &mut self.i_r) {
                    inst.clear_non_finite();
                    self.shared.count_non_finite();
                }
                for k in 0..Q {
                    self.s_l[k] += self.i_l[k];
                    self.s_r[k] += self.i_r[k];
                }
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
            // Live input: recorded dry, heard through the slot when monitored.
            if slot.input.is_some() || slot.record.is_some() {
                let channels = self.in_channels;
                let (left, right, monitor) = match slot.input {
                    Some(inp) if inp.left < channels => (
                        Some(&self.in_bufs[inp.left]),
                        inp.right
                            .filter(|r| *r < channels)
                            .map(|r| &self.in_bufs[r]),
                        inp.monitor,
                    ),
                    _ => (None, None, false),
                };
                if let Some(tap) = slot.record.as_mut() {
                    // A take stays on the clock: a missing input records silence.
                    tap.write(q0, left.unwrap_or(&SILENCE), right);
                }
                if let (Some(left), true) = (left, monitor) {
                    let right = right.unwrap_or(left);
                    for k in 0..Q {
                        self.s_l[k] += left[k];
                        self.s_r[k] += right[k];
                    }
                }
            }
            // The effect chain keeps running without an instrument (tails, automation).
            match slot.strip.as_mut() {
                // A track's chain: its plugins and built-in effects in the order the strip
                // holds, then pan and volume. A plugin's non-finite output is silenced before
                // it reaches the next stage (a filter would hold it for good).
                Some(SlotFx::Track(s)) => {
                    let order = s.order();
                    let mut done = 0u64;
                    let mut bad = false;
                    for stage in order.stages() {
                        match stage {
                            crate::fx::track::Stage::Effect(e) => {
                                s.process_effect(e, q0, &mut self.s_l, &mut self.s_r)
                            }
                            crate::fx::track::Stage::Plugin(tag) => {
                                done |= 1 << tag;
                                if let Some(chain) = slot.inserts.as_mut()
                                    && chain.process_tag(
                                        tag,
                                        &transport,
                                        &mut self.s_l,
                                        &mut self.s_r,
                                    )
                                {
                                    bad |= guard(&mut self.s_l, &mut self.s_r);
                                }
                            }
                        }
                    }
                    if let Some(chain) = slot.inserts.as_mut() {
                        if chain.process_rest(done, &transport, &mut self.s_l, &mut self.s_r) {
                            bad |= guard(&mut self.s_l, &mut self.s_r);
                        }
                        if bad {
                            chain.reset();
                            self.shared.count_non_finite();
                        }
                    }
                    s.finish(q0, &mut self.s_l, &mut self.s_r);
                }
                other => {
                    if let Some(chain) = slot.inserts.as_mut() {
                        chain.process(&transport, &mut self.s_l, &mut self.s_r);
                        if guard(&mut self.s_l, &mut self.s_r) {
                            chain.reset();
                            self.shared.count_non_finite();
                        }
                    }
                    if let Some(SlotFx::Live(l)) = other {
                        l.process(q0, &mut self.s_l, &mut self.s_r);
                    }
                }
            }
            // Backstop: an effect chain that produced non-finite samples never reaches the mix.
            if guard(&mut self.s_l, &mut self.s_r) {
                self.shared.count_non_finite();
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

/// The output guard: replace a quantum holding a non-finite sample with silence. True when it
/// did.
fn guard(l: &mut [f32; Q], r: &mut [f32; Q]) -> bool {
    if l.iter().chain(r.iter()).all(|x| x.is_finite()) {
        return false;
    }
    l.fill(0.0);
    r.fill(0.0);
    true
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    use crate::contract::Instrument;
    use crate::core::{Core, CoreConfig, SynthInstrument, TrackInstrument};

    /// Emits NaN until its state is cleared.
    struct Unstable(Arc<AtomicU32>, bool);

    impl Instrument for Unstable {
        fn process(&mut self, _: u64, l: &mut [f32], r: &mut [f32]) {
            let v = if self.1 { f32::NAN } else { 0.25 };
            l.iter_mut().chain(r.iter_mut()).for_each(|x| *x += v);
        }
        fn clear_non_finite(&mut self) {
            self.0.fetch_add(1, Ordering::Relaxed);
            self.1 = false;
        }
    }

    impl SynthInstrument for Unstable {
        fn note_on(&mut self, _: f64, _: f64, _: f64) {}
        fn note_off(&mut self, _: f64, _: f64) {}
        fn release_all(&mut self, _: f64) {}
    }

    #[test]
    fn a_non_finite_instrument_output_is_silenced_and_its_state_cleared() {
        let (mut core, mut h, _) = Core::new(CoreConfig::new(48000));
        let cleared = Arc::new(AtomicU32::new(0));
        h.set_instrument(
            0,
            Some(TrackInstrument::Synth(Box::new(Unstable(
                cleared.clone(),
                true,
            )))),
        );
        h.set_instrument(
            1,
            Some(TrackInstrument::Synth(Box::new(Unstable(
                Arc::default(),
                false,
            )))),
        );
        h.set_master_bus(Some(0.0));
        let out = core.render_offline(0.1);
        assert!(out.channels.iter().flatten().all(|x| x.is_finite()));
        assert_eq!(
            cleared.load(Ordering::Relaxed),
            1,
            "cleared once, then finite"
        );
        assert_eq!(h.shared().non_finite_outputs(), 1);
        // the healthy track and, after the clear, the unstable one are heard
        assert!(out.channels[0][4000] > 0.1);
    }
}
