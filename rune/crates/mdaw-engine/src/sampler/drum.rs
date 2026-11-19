//! [`SamplerKit`] as the core's [`DrumInstrument`] (≙ `playDrumNote` + the live sink of
//! `ArrangementEngine.createLiveSink` + `drumEngine`'s fallback kit, all on one `SamplerKit`).
//!
//! Every time the core passes may lie in the future (later in the quantum, or a gate release at
//! `noteStart + max(0.01, dur)`); the kit applies them at their exact frame like Web Audio, and
//! drops voices of a gate pad that would start after its release (`SamplerKit.release`).

use mdaw_dsp::buffer::SharedBuffer;
use mdaw_model::PadIndex;

use super::{SamplerCommand, SamplerKit};
use crate::core::DrumInstrument;

impl DrumInstrument for SamplerKit {
    fn trigger(&mut self, pad: PadIndex, velocity: f64, time: f64) {
        SamplerKit::trigger(self, pad, velocity, time);
    }

    fn release(&mut self, pad: PadIndex, time: f64) {
        SamplerKit::release(self, pad, time);
    }

    fn trigger_voice(&mut self, pad: PadIndex, velocity: f64, time: f64) -> u64 {
        SamplerKit::trigger(self, pad, velocity, time).map_or(0, |h| h.serial())
    }

    fn release_voice(&mut self, pad: PadIndex, voice: u64, time: f64) {
        SamplerKit::release_voice(self, pad, voice, time);
    }

    fn stop_all(&mut self, time: f64) {
        SamplerKit::stop_all(self, Some(time));
    }

    fn cancel_pending_after(&mut self, pad: Option<PadIndex>, time: f64) {
        SamplerKit::cancel_pending_after(self, pad, time);
    }

    fn pad_is_gate(&self, pad: PadIndex) -> bool {
        self.pads().get(pad).is_some_and(|p| p.gate)
    }

    fn apply_kit(&mut self, cmd: SamplerCommand) {
        self.apply(cmd);
    }
}

/// The `Instrument` impl lives in `sampler.rs`; this adds the garbage hand-over the core calls
/// after each quantum.
pub(crate) fn drain_into(kit: &mut SamplerKit, sink: &mut dyn FnMut(SharedBuffer)) {
    kit.drain_garbage(sink);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::Instrument;
    use crate::sampler::{PadBuffer, PadParams};
    use mdaw_dsp::buffer::AudioBuffer;
    use mdaw_model::PAD_COUNT;
    use std::sync::Arc;

    fn kit(gate: bool) -> SamplerKit {
        let mut pads = [PadParams::default(); PAD_COUNT];
        pads[0].gate = gate;
        pads[0].decay = 0.01;
        let mut k = SamplerKit::new(48_000, pads, 0);
        let buf = Arc::new(AudioBuffer::mono(48_000, vec![0.5; 48_000]));
        k.set_pad_buffer(
            0,
            Some(PadBuffer {
                forward: buf,
                reversed: None,
            }),
        );
        k
    }

    fn render(k: &mut SamplerKit, frames: usize) -> Vec<f32> {
        let mut out = Vec::new();
        let mut f = 0u64;
        while out.len() < frames {
            let (mut l, mut r) = ([0f32; 128], [0f32; 128]);
            Instrument::process(k, f, &mut l, &mut r);
            out.extend_from_slice(&l);
            f += 128;
        }
        out
    }

    #[test]
    fn future_gate_release_and_cancel_pending() {
        // gate pad: voice at 0.01 s, release scheduled at 0.02 s → silent from ~0.03 s
        let mut k = kit(true);
        DrumInstrument::trigger(&mut k, 0, 1.0, 0.01);
        DrumInstrument::release(&mut k, 0, 0.02);
        // a voice starting after the release is cancelled by it
        DrumInstrument::trigger(&mut k, 0, 1.0, 0.05);
        DrumInstrument::release(&mut k, 0, 0.04);
        let out = render(&mut k, 4800);
        assert!(
            out[700..900].iter().any(|x| x.abs() > 0.01),
            "sounding at 15 ms"
        );
        assert!(
            out[1500..].iter().all(|x| x.abs() < 1e-6),
            "released, nothing after"
        );
        assert!(DrumInstrument::pad_is_gate(&k, 0));

        // one-shot: release ignored, but cancel_pending_after drops a future voice
        let mut k = kit(false);
        DrumInstrument::trigger(&mut k, 0, 1.0, 0.01);
        DrumInstrument::trigger(&mut k, 0, 1.0, 0.08);
        DrumInstrument::cancel_pending_after(&mut k, Some(0), 0.05);
        let out = render(&mut k, 4800);
        let late = out[3840..4800].iter().map(|x| x.abs()).fold(0f32, f32::max);
        let early = out[480..1000].iter().map(|x| x.abs()).fold(0f32, f32::max);
        assert!(
            (late - early).abs() < 1e-3,
            "one voice only (no doubling at 80 ms)"
        );
    }

    #[test]
    fn release_voice_ends_only_that_voice_of_a_gate_pad() {
        // two overlapping gate hits on pad 0 (0.5 each), both held to 0.5 s; releasing the
        // first by its handle at 0.1 s leaves the second sounding
        let mut k = kit(true);
        let a = DrumInstrument::trigger_voice(&mut k, 0, 1.0, 0.01);
        let b = DrumInstrument::trigger_voice(&mut k, 0, 1.0, 0.02);
        assert!(a != 0 && b != 0 && a != b);
        DrumInstrument::release(&mut k, 0, 0.5);
        DrumInstrument::release_voice(&mut k, 0, a, 0.1);
        let out = render(&mut k, 30_000);
        let level = |x: &[f32]| x.iter().map(|v| v.abs()).fold(0f32, f32::max);
        let both = level(&out[2400..4800]);
        let one = level(&out[6000..20_000]);
        assert!(both > 0.1, "two voices: {both}");
        assert!(
            (one / both - 0.5).abs() < 0.02,
            "one voice left: {one} of {both}"
        );
        assert!(
            level(&out[26_000..]) < 1e-6,
            "the gate release still ends it"
        );
    }
}
