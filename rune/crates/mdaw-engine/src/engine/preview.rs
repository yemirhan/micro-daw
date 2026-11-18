//! The pack store's sound preview player (`PackService.play` / `stopPreview`,
//! `src/services/PackService.ts:742-777`).
//!
//! The app plays a preview as a `Tone.ToneBufferSource` started at `Tone.immediate()` with
//! gain [`PREVIEW_GAIN`] into a `Tone.Volume(-6)` connected to the destination. `play` calls
//! `ensureMasterBus()` first, and `installMasterBus` chains master volume → drive → soft
//! clipper onto the Destination node itself, so a preview goes INTO the master bus like every
//! other source: the master fader scales it and the clipper bounds it. Here the preview slot is
//! summed with the other slots before the master stage, which is the same routing.
//!
//! It stops with a [`PREVIEW_FADE`] linear fade-out. A new preview replaces the previous one:
//! the TS stops the old source (`src.stop(Tone.immediate())`, so it fades over 10 ms) and starts
//! the new one in the same tick. Here the source is a [`PreviewVoice`] on a slot of its own
//! (live only: transport stop never touches it); a new preview swaps the slot's instrument with
//! [`CoreHandle::replace_synth`]: the old voice gets `release_all` at the swap time (its 10 ms
//! fade) and rings out on the slot's retire list until [`PREVIEW_RETIRE`]. A stop is a note-off.

use mdaw_dsp::abs_node::{BufferPlayer, StartParams};
use mdaw_dsp::buffer::SharedBuffer;

use super::ArrangementEngine;
use super::audio_regions::db_gain;
use crate::contract::{Instrument, RENDER_QUANTUM};
use crate::core::{CoreHandle, SynthInstrument, SynthNoteOp, TrackInstrument};

const Q: usize = RENDER_QUANTUM;

/// `PREVIEW_GAIN`: the source's start gain.
pub const PREVIEW_GAIN: f64 = 0.9;
/// `PREVIEW_FADE`: seconds of the linear fade-out on stop.
pub const PREVIEW_FADE: f64 = 0.01;
/// `new Tone.Volume(-6)`.
pub const PREVIEW_VOLUME_DB: f64 = -6.0;
/// How long a replaced preview stays on the retire list: its fade plus 5 ms of margin.
pub const PREVIEW_RETIRE: f64 = PREVIEW_FADE + 0.005;

/// One preview playback (see the module docs). Built on the control side, played on the audio
/// thread; never allocates or frees there (the buffer leaves with the instrument).
pub struct PreviewVoice {
    sr: f64,
    player: BufferPlayer,
    gain: f32,
    /// Linear fade to 0 over frames [from, to) and silence after it (`_stopGain`).
    fade: Option<(u64, u64)>,
    done: bool,
    tmp: Box<[[f32; Q]; 2]>,
}

impl PreviewVoice {
    /// A voice that starts `buffer` at context time `start` (the buffer is at the context rate).
    pub fn new(buffer: SharedBuffer, sample_rate: u32, start: f64) -> Self {
        Self {
            sr: sample_rate as f64,
            player: BufferPlayer::new(
                buffer,
                sample_rate,
                StartParams::sampler(start, 0.0, 1.0, None),
            ),
            gain: (PREVIEW_GAIN * db_gain(PREVIEW_VOLUME_DB) as f64) as f32,
            fade: None,
            done: false,
            tmp: Box::new([[0.0; Q]; 2]),
        }
    }

    /// `src.stop(time)`: linear fade to silence over [`PREVIEW_FADE`] (a later stop while fading
    /// is ignored, like Tone's assert on a source that is already stopping).
    pub fn stop(&mut self, time: f64) {
        if self.fade.is_some() || self.done {
            return;
        }
        let from = (time * self.sr - 1e-9).ceil().max(0.0) as u64;
        let to = ((time + PREVIEW_FADE) * self.sr - 1e-9).ceil().max(0.0) as u64;
        self.fade = Some((from, to.max(from + 1)));
    }

    /// The buffer's end or the fade's end was reached.
    pub fn is_done(&self) -> bool {
        self.done
    }
}

impl Instrument for PreviewVoice {
    fn process(&mut self, frame: u64, left: &mut [f32], right: &mut [f32]) {
        if self.done {
            return;
        }
        let n = left.len().min(right.len()).min(Q);
        if let Some((_, to)) = self.fade
            && frame >= to
        {
            self.done = true;
            return;
        }
        let nch = self.player.buffer().channel_count().clamp(1, 2);
        {
            let [a, b] = &mut *self.tmp;
            let mut outs: [&mut [f32]; 2] = [&mut a[..n], &mut b[..n]];
            self.player.process(&mut outs[..nch], frame);
        }
        for k in 0..n {
            let f = frame + k as u64;
            let mut g = self.gain;
            if let Some((from, to)) = self.fade
                && f >= from
            {
                g = if f >= to {
                    0.0
                } else {
                    self.gain * (1.0 - (f - from) as f32 / (to - from) as f32)
                };
            }
            let l = self.tmp[0][k] * g;
            let r = if nch == 2 { self.tmp[1][k] * g } else { l };
            left[k] += l;
            right[k] += r;
        }
        if self.player.is_finished() {
            self.done = true;
        }
    }
}

impl SynthInstrument for PreviewVoice {
    fn note_on(&mut self, _note: f64, _velocity: f64, _time: f64) {}
    fn note_off(&mut self, _note: f64, time: f64) {
        self.stop(time);
    }
    fn release_all(&mut self, time: f64) {
        self.stop(time);
    }
}

/// The engine side: the slot and when the current preview ends.
pub(super) struct PreviewSlot {
    slot: usize,
    /// Context time the current preview's buffer ends (None: nothing playing / stopped).
    end: Option<f64>,
}

impl PreviewSlot {
    pub fn new(h: &mut CoreHandle, slot: usize) -> Self {
        h.set_track_gain(slot, 1.0);
        h.set_live_only(slot, true);
        Self { slot, end: None }
    }
}

impl ArrangementEngine {
    /// Plays a decoded preview buffer (at the context rate) now, replacing any preview still
    /// playing. Returns the context time its buffer ends.
    pub fn play_preview(&mut self, buffer: SharedBuffer) -> f64 {
        let t = self.now();
        let duration = buffer.frames() as f64 / self.sr as f64;
        let voice = PreviewVoice::new(buffer, self.sr, t);
        let slot = self.preview.slot;
        // The old voice (if any) fades out over PREVIEW_FADE from the swap, like the TS
        // `stopPreview()` before a new start; it is never cut.
        self.h.replace_synth(
            slot,
            Some(TrackInstrument::Synth(Box::new(voice))),
            0.0,
            PREVIEW_RETIRE,
        );
        self.h.flush();
        let end = t + duration;
        self.preview.end = Some(end);
        end
    }

    /// `stopPreview()`: fades the current preview out ([`PREVIEW_FADE`]).
    pub fn stop_preview(&mut self) {
        if self.preview.end.take().is_some() {
            let slot = self.preview.slot;
            self.h.synth_note(slot, SynthNoteOp::Off { note: 0.0 }, 0.0);
            self.h.flush();
        }
    }

    /// A preview is playing (its buffer has not ended and it was not stopped): the
    /// ToneBufferSource `onended` that clears `previewing`.
    pub fn is_preview_playing(&self) -> bool {
        self.preview.end.is_some_and(|end| self.now() < end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdaw_dsp::buffer::AudioBuffer;
    use std::sync::Arc;

    fn ones(frames: usize) -> SharedBuffer {
        Arc::new(AudioBuffer::mono(48_000, vec![1.0; frames]))
    }

    fn render(v: &mut PreviewVoice, frames: usize) -> (Vec<f32>, Vec<f32>) {
        let (mut l, mut r) = (Vec::new(), Vec::new());
        let mut f = 0u64;
        while l.len() < frames {
            let (mut a, mut b) = ([0f32; Q], [0f32; Q]);
            v.process(f, &mut a, &mut b);
            l.extend_from_slice(&a);
            r.extend_from_slice(&b);
            f += Q as u64;
        }
        (l, r)
    }

    #[test]
    fn plays_at_gain_0_9_times_minus_6_db_on_both_channels_until_the_buffer_ends() {
        let mut v = PreviewVoice::new(ones(1000), 48_000, 0.0);
        let (l, r) = render(&mut v, 1280);
        let g = (0.9 * 10f64.powf(-6.0 / 20.0)) as f32;
        assert!((l[0] - g).abs() < 1e-6 && (l[999] - g).abs() < 1e-6);
        assert_eq!(l[1000], 0.0);
        assert_eq!(l, r);
        assert!(v.is_done());
    }

    #[test]
    fn stop_fades_out_linearly_over_10_ms() {
        let mut v = PreviewVoice::new(ones(48_000), 48_000, 0.0);
        v.stop(0.01);
        let (l, _) = render(&mut v, 2048);
        let g = l[0];
        assert!((l[479] - g).abs() < 1e-6, "full gain before the stop");
        assert!((l[480 + 240] - g * 0.5).abs() < 1e-3, "half way down");
        assert!(l[960..].iter().all(|x| *x == 0.0), "silent after the fade");
        assert!(v.is_done());
    }

    #[test]
    fn the_engine_plays_a_preview_through_its_own_slot_and_stop_fades_it() {
        let mut rig = super::super::test_rig::Rig::new();
        rig.run(0.01);
        let end = rig.e.play_preview(ones(48_000));
        assert!(rig.e.is_preview_playing());
        let t0 = rig.now();
        rig.run(0.05);
        assert!(rig.rms(t0, rig.now()) > 0.1, "the preview sounds");
        // Transport stop leaves the preview alone (live only).
        rig.e.stop();
        rig.run(0.02);
        let t1 = rig.now();
        rig.run(0.02);
        assert!(
            rig.rms(t1, rig.now()) > 0.1,
            "transport stop does not cut it"
        );
        rig.e.stop_preview();
        assert!(!rig.e.is_preview_playing());
        rig.run(0.03);
        let t2 = rig.now();
        rig.run(0.05);
        assert_eq!(rig.rms(t2, rig.now()), 0.0, "silent after the fade");
        assert!(end > t0);
        // A preview whose buffer ends stops reporting as playing.
        rig.e.play_preview(ones(480));
        rig.run(0.05);
        assert!(!rig.e.is_preview_playing());
    }

    /// Largest sample-to-sample step of the left channel over frames [a, b).
    fn max_step(x: &[f32], a: usize, b: usize) -> f32 {
        (a.max(1)..b.min(x.len()))
            .map(|i| (x[i] - x[i - 1]).abs())
            .fold(0.0, f32::max)
    }

    /// Finding 9: a new preview fades the sounding one out over 10 ms (the TS `stopPreview()`
    /// before the new start) instead of cutting it. The new preview is silent here so the
    /// output is the old voice alone.
    #[test]
    fn replacing_a_sounding_preview_fades_the_old_one_out() {
        let mut rig = super::super::test_rig::Rig::new();
        rig.run(0.01);
        rig.e.play_preview(ones(48_000));
        rig.run(0.05);
        let switch = rig.left.len();
        let level = rig.left[switch - 1]; // g after the master bus (+2 dB drive)
        rig.e
            .play_preview(Arc::new(AudioBuffer::mono(48_000, vec![0.0; 48_000])));
        rig.run(0.05);
        let l = &rig.left;
        let fade = (PREVIEW_FADE * 48_000.0) as usize;
        let step = max_step(l, switch - 256, l.len());
        let slope = level / fade as f32;
        eprintln!("level {level}, max step {step}, fade slope {slope}");
        assert!(
            step <= slope * 1.5,
            "no discontinuity: max step {step} (fade slope {slope})"
        );
        assert!(
            l[switch + fade + 256..].iter().all(|x| *x == 0.0),
            "the old voice is silent after the fade"
        );
    }

    /// The new preview plays at full gain from its start, and `stop_preview` after a
    /// replacement fades it.
    #[test]
    fn the_replacing_preview_plays_at_full_gain_and_stops_with_a_fade() {
        let mut rig = super::super::test_rig::Rig::new();
        rig.run(0.01);
        rig.e.play_preview(ones(48_000));
        rig.run(0.05);
        let full = *rig.left.last().unwrap();
        rig.e.play_preview(ones(48_000));
        rig.run(0.05);
        assert!((rig.left.last().unwrap() - full).abs() < 1e-6);
        let stop = rig.left.len();
        rig.e.stop_preview();
        rig.run(0.03);
        let fade = (PREVIEW_FADE * 48_000.0) as usize;
        let step = max_step(&rig.left, stop, rig.left.len());
        assert!(step <= full / fade as f32 * 1.5, "stop fades: {step}");
        assert!(rig.left[stop + fade + 256..].iter().all(|x| *x == 0.0));
    }

    /// 20 replacements in one quantum fill the retire list: the sounding voice is evicted with
    /// the core's short fade (2 quanta), never cut.
    #[test]
    fn rapid_preview_switching_never_cuts() {
        let mut rig = super::super::test_rig::Rig::new();
        rig.run(0.01);
        rig.e.play_preview(ones(48_000));
        rig.run(0.05);
        let start = rig.left.len();
        let level = rig.left[start - 1];
        for _ in 0..20 {
            rig.e
                .play_preview(Arc::new(AudioBuffer::mono(48_000, vec![0.0; 48_000])));
        }
        rig.run(0.05);
        let step = max_step(&rig.left, start - 128, rig.left.len());
        // Steepest legal ramp: the eviction fade (level over 2 quanta) on top of the voice's
        // own 10 ms fade.
        let bound = level * (1.0 / 256.0 + 1.0 / 480.0) * 1.1;
        eprintln!("level {level}, max step {step}, bound {bound}");
        assert!(step <= bound, "max step {step} > {bound}");
        assert!(rig.left[start + 1024..].iter().all(|x| *x == 0.0));
    }

    /// Finding 8: previews go through the master bus like the TS (`toDestination()` after
    /// `ensureMasterBus()`): the master fader scales them and the clipper bounds them.
    #[test]
    fn previews_go_through_the_master_fader_and_clipper() {
        let level = |master_db: f64, amp: f32| -> f32 {
            let mut rig = super::super::test_rig::Rig::new();
            rig.e.set_master_volume_db(master_db);
            rig.run(0.1); // past the 30 ms master ramp
            rig.e
                .play_preview(Arc::new(AudioBuffer::mono(48_000, vec![amp; 48_000])));
            rig.run(0.05);
            *rig.left.last().unwrap()
        };
        let at_ref = level(-12.0, 0.5);
        let at_minus_24 = level(-24.0, 0.5);
        let ratio_db = 20.0 * (at_minus_24 / at_ref).log10();
        assert!(
            (ratio_db + 12.0).abs() < 0.05,
            "−12 dB fader → {ratio_db} dB"
        );
        // Loud preview at full master: bounded by the soft clipper's ceiling (0.98).
        let loud = level(0.0, 8.0);
        assert!(loud <= 0.98 + 1e-6 && loud > 0.9, "clipped to {loud}");
    }
}
