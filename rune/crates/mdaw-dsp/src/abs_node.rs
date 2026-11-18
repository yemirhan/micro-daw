//! Chromium `AudioBufferSourceNode` playback, reproduced sample for sample: how the Electron app
//! plays every pad voice.
//!
//! `SamplerKit.trigger` (`src/audio/sampler/SamplerKit.ts`) plays the pad buffer (or
//! `getReversedBuffer(buffer)` for reverse pads) through one `Tone.ToneBufferSource` per hit.
//! Chromium receives:
//!
//! - `playbackRate.setValueAtTime(2^(pitch/12), 0)`, from Tone's `Param` constructor. This is an
//!   automation event, so the param's intrinsic value is still the default 1.0 when `start()` runs
//!   (see the offset rounding below). Rate 1 sets nothing.
//! - `start(time, offset)`: Tone's `start(time, offset, undefined, 1)` passes no duration, so the
//!   node is in grain mode without a grain duration.
//! - `stop(t)` at a Tone clock tick on or after `time + realDuration`. The real cut is Tone's
//!   source gain (`setValueAtTime(0, end)`) and SamplerKit's per-voice envelope, which reaches 0 at
//!   `end`. So the native stop time only matters past the voice end.
//!
//! The buffers stay at their own rate: classic voices at 44.1 kHz, files at their decoded rate.
//! The node converts them to the context rate itself, so the engine must not pre-resample them.
//! Pre-resampling with the `SincResampler` and playing at rate 1 gives a different signal: the
//! `pre_resampling_does_not_match` test measures the error at −13.9 dB vs Chromium on the 808
//! closed hat at 48 kHz: sinc vs linear interpolation, plus a different length.
//!
//! What Chromium does (Blink `audio_buffer_source_handler.cc` and
//! `audio_scheduled_source_handler.cc`, checked against the M153 headless capture in
//! `rune/parity/fixtures/playback_reference.json`):
//!
//! - **Rate**: per render quantum, `(buffer_rate_f32 as f64 / context_rate_f32 as f64) ×
//!   playbackRate_f32`. playbackRate is a k-rate f32 param. Detune is 0. The result is clamped to
//!   ±1024.
//! - **Start**:
//!   - The first frame is `start_frame = ceil(round(when × sr × 1024) / 1024)`, which rounds up.
//!   - The read position in that first quantum is advanced by `|when × sr − start_frame| × rate`
//!     buffer frames. This is the sub-sample start: the playhead begins between buffer samples.
//!   - Frames before `start_frame` are silent.
//! - **Offset**: `virtual_read_index = offset × buffer_rate`. If the playbackRate param's intrinsic
//!   value is exactly 1 when `start()` runs, Chromium instead rounds the offset to the nearest
//!   buffer frame (`TimeToSampleFrame`). That is always the case on the app's Tone path, at every
//!   pitch.
//! - **Interpolation**:
//!   - Linear, in f64: `(1 − f)·s[i] + f·s[i+1]` with `i = trunc(pos)`, `f = pos − i`. The result
//!     is clamped to the f32 range and rounded to f32.
//!   - At the last buffer frame (`i + 1 == len`), Chromium extrapolates from the last two samples:
//!     `s[i] + (s[i] − s[i−1])·f`. It does not interpolate toward zero or wrap.
//!   - The position advances by `rate` (an f64 add) per output frame.
//! - **Fast path**: when `rate == 1` and the read position, the buffer end and the loop length
//!   are all integral, samples are copied as-is. The same samples come out, but the sign of zero
//!   is preserved.
//! - **End**:
//!   - Playback finishes once `pos ≥ (len / sr_f32) × sr_f32`, the buffer duration in frames;
//!     the rest of the quantum is silent.
//!   - A quantum that begins with `pos ≥ len` renders nothing.
//!   - If one rate step is longer than the whole buffer (`rate > len`), every quantum is silent.
//! - **Grain duration** (`start(when, offset, duration)`):
//!   - Chromium counts the buffer frames played, `played`, accumulated per quantum: the initial
//!     sub-sample skip, plus `frames × |rate|`.
//!   - A quantum renders `ceil((duration × buffer_rate − played) / |rate|)` frames at most, then
//!     the node finishes.
//! - **Stop**: frames from `ceil(round(stop × sr × 1024) / 1024)` on are silent, and the node
//!   finishes.
//!
//! Exactness: on the full-length capture (63 raw cases at 44.1k/48k/96k, plus 8 SamplerKit
//! renders), every case is bit-identical when [`BufferPlayer::process`] is called with 128-frame
//! quanta aligned to multiples of 128. Other block sizes give the same samples, except in grain
//! mode, where Chromium's per-quantum `played` accumulation rounds differently.
//!
//! Not modelled, because the app never uses them: looping, negative playback rates, detune,
//! playbackRate automation during playback, and setting the buffer after `start()`.

use std::sync::Arc;

use crate::buffer::AudioBuffer;

/// Chromium's `kMaxRate`.
const MAX_RATE: f64 = 1024.0;

/// How a node was started: the arguments of `start()` / `stop()` plus the playbackRate value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StartParams {
    /// `start(when, …)`, context seconds.
    pub when: f64,
    /// `start(_, offset)`, buffer seconds. Chromium clamps it to the buffer duration.
    pub offset: f64,
    /// `start(_, _, duration)`: the grain duration in buffer seconds. None means no duration was
    /// given (the node plays to the buffer end or the stop time).
    pub duration: Option<f64>,
    /// `stop(t)`, context seconds. None means `stop()` was never called.
    pub stop: Option<f64>,
    /// playbackRate as Chromium stores it (f32), constant for the whole voice.
    pub playback_rate: f32,
    /// Chromium rounds the offset to the nearest buffer frame. True when the playbackRate param's
    /// intrinsic value is exactly 1.0 at `start()`: the app's Tone path (rate set by automation)
    /// at every pitch, or `playbackRate.value = 1`.
    pub snap_offset: bool,
}

impl StartParams {
    /// What `SamplerKit.trigger` makes Chromium do. Tone sets the rate via `setValueAtTime(rate, 0)`
    /// (offset snapped), calls `start(when, offset)` without a duration, and stops the native node
    /// at `stop`. Pass `stop: None` when the voice envelope does the cutting.
    pub fn sampler(when: f64, offset: f64, playback_rate: f64, stop: Option<f64>) -> Self {
        Self {
            when,
            offset,
            duration: None,
            stop,
            playback_rate: playback_rate as f32,
            snap_offset: true,
        }
    }

    /// A bare node whose rate was assigned with `playbackRate.value = rate` before `start()`.
    pub fn with_value(
        when: f64,
        offset: f64,
        duration: Option<f64>,
        stop: Option<f64>,
        playback_rate: f64,
    ) -> Self {
        let playback_rate = playback_rate as f32;
        Self {
            when,
            offset,
            duration,
            stop,
            playback_rate,
            snap_offset: playback_rate == 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Scheduled,
    Playing,
    Finished,
}

/// Blink `audio_utilities::TimeToSampleFrame` (1024× oversampled rounding).
fn time_to_sample_frame(time: f64, sample_rate: f64, round_up: bool) -> u64 {
    let frame = (time * sample_rate * 1024.0).round() / 1024.0;
    let frame = if round_up {
        frame.ceil()
    } else {
        frame.round()
    };
    if frame >= u64::MAX as f64 {
        u64::MAX
    } else {
        frame as u64
    }
}

/// `ClampTo<float>(double)`.
fn clamp_to_f32(x: f64) -> f32 {
    if x.is_nan() {
        0.0
    } else {
        x.clamp(f32::MIN as f64, f32::MAX as f64) as f32
    }
}

/// One `AudioBufferSourceNode` voice. Create it on any thread, then call
/// [`process`](Self::process) on the audio thread once per render quantum; `process` does not
/// allocate.
#[derive(Debug, Clone)]
pub struct BufferPlayer {
    buffer: Arc<AudioBuffer>,
    /// Context rate, as Chromium sees it (float → double).
    ctx_rate: f64,
    /// Buffer rate (float → double).
    buffer_rate: f64,
    start_time: f64,
    end_time: Option<f64>,
    grain_duration: f64,
    duration_given: bool,
    /// The constant computed playback rate (buffer frames per output frame).
    rate: f64,
    virtual_read_index: f64,
    buffer_played_frames: f64,
    state: State,
}

impl BufferPlayer {
    /// A node with `buffer` set and `start()` (and optionally `stop()`) already called, in a
    /// context running at `ctx_rate`. Negative times, offsets and durations are clamped to 0, since
    /// Chromium would throw. The rate is clamped to `[0, 1024]` buffer frames per output frame.
    pub fn new(buffer: Arc<AudioBuffer>, ctx_rate: u32, p: StartParams) -> Self {
        let ctx_rate = ctx_rate as f32 as f64;
        let buffer_rate = buffer.sample_rate as f32 as f64;
        let frames = buffer.frames();
        let duration = frames as f64 / buffer_rate;
        let offset = if p.offset.is_nan() {
            0.0
        } else {
            p.offset.clamp(0.0, duration)
        };
        let virtual_read_index = if p.snap_offset {
            time_to_sample_frame(offset, buffer_rate, false) as f64
        } else {
            offset * buffer_rate
        };
        let (grain_duration, duration_given) = match p.duration {
            Some(d) => (d.max(0.0), true),
            None => (f64::INFINITY, false),
        };
        let rate = ((buffer_rate / ctx_rate) * p.playback_rate as f64).clamp(0.0, MAX_RATE);
        Self {
            buffer,
            ctx_rate,
            buffer_rate,
            start_time: p.when.max(0.0),
            end_time: p.stop.map(|t| t.max(0.0)),
            grain_duration,
            duration_given,
            rate,
            virtual_read_index,
            buffer_played_frames: 0.0,
            state: State::Scheduled,
        }
    }

    /// `stop(when)` after start (the engine's choke / note-off). As in Chromium, a later call
    /// replaces the earlier one.
    pub fn stop(&mut self, when: f64) {
        self.end_time = Some(when.max(0.0));
    }

    /// The node has finished: the buffer or grain end was reached, or the stop time has passed.
    /// Every later quantum is silent.
    pub fn is_finished(&self) -> bool {
        self.state == State::Finished
    }

    /// The buffer frames per output frame this voice reads.
    pub fn computed_rate(&self) -> f64 {
        self.rate
    }

    /// The buffer this voice plays.
    pub fn buffer(&self) -> &Arc<AudioBuffer> {
        &self.buffer
    }

    /// Render one quantum that starts at context frame `frame_pos`; Chromium's
    /// `AudioBufferSourceHandler::Process` with `UpdateSchedulingInfo`.
    ///
    /// The quantum length is the length of the `out` slices. All slices must have the same length.
    /// For exact parity, use 128 frames aligned to multiples of 128. `out` gets one slice per
    /// buffer channel: the node's output has as many channels as the buffer. Extra slices are
    /// zeroed, and missing ones are skipped.
    ///
    /// Overwrites `out` and returns true when the quantum may hold sound (the node was
    /// playing). Does not allocate.
    pub fn process(&mut self, out: &mut [&mut [f32]], frame_pos: u64) -> bool {
        for ch in out.iter_mut() {
            ch.fill(0.0);
        }
        let Some(size) = out.first().map(|c| c.len()) else {
            return false;
        };
        if size == 0 || self.buffer.frames() == 0 {
            return false;
        }
        let Some((offset, frames, start_offset)) = self.update_scheduling_info(frame_pos, size)
        else {
            return false;
        };
        self.render_from_buffer(out, offset, frames, start_offset);
        true
    }

    /// `AudioScheduledSourceHandler::UpdateSchedulingInfo`. Returns
    /// `(quantum_frame_offset, frames_to_process, start_frame_offset)`, or None for a silent
    /// quantum.
    fn update_scheduling_info(&mut self, q_start: u64, size: usize) -> Option<(usize, usize, f64)> {
        let sr = self.ctx_rate;
        let q_end = q_start + size as u64;
        let start_frame = time_to_sample_frame(self.start_time, sr, true);
        let end_frame = self
            .end_time
            .map(|t| time_to_sample_frame(t, sr, true))
            .unwrap_or(0);
        if self.end_time.is_some() && end_frame <= q_start {
            self.state = State::Finished;
        }
        if self.state == State::Finished || start_frame >= q_end {
            return None;
        }
        let start_frame_offset = if self.state == State::Scheduled {
            self.state = State::Playing;
            self.start_time * sr - start_frame as f64
        } else {
            0.0
        };
        let q_offset = (start_frame.saturating_sub(q_start) as usize).min(size);
        let mut frames = size - q_offset;
        if frames == 0 {
            return None;
        }
        if self.end_time.is_some() && end_frame >= q_start && end_frame <= q_end {
            if end_frame < q_end {
                let to_zero = size - (end_frame - q_start) as usize;
                frames = frames.saturating_sub(to_zero);
            }
            self.state = State::Finished;
        }
        if frames == 0 {
            return None;
        }
        Some((q_offset, frames, start_frame_offset))
    }

    /// `AudioBufferSourceHandler::RenderFromBuffer` (forward, non-looping). `out` is already
    /// zeroed.
    fn render_from_buffer(
        &mut self,
        out: &mut [&mut [f32]],
        dest_offset: usize,
        number_of_frames: usize,
        start_time_offset: f64,
    ) {
        let buffer_length = self.buffer.frames();
        let len_f = buffer_length as f64;
        // effective_loop_end_ (= duration) × buffer rate: not always exactly `len`.
        let virtual_end_frame = (len_f / self.buffer_rate) * self.buffer_rate;
        let virtual_delta_frames = virtual_end_frame;
        let rate = self.rate;
        if rate.abs() > virtual_delta_frames {
            return;
        }
        let mut vri = self.virtual_read_index;
        let mut frames_to_process = number_of_frames as i64;

        if start_time_offset < 0.0 && rate != 0.0 {
            let skipped = (start_time_offset * rate).abs();
            vri += skipped;
            self.buffer_played_frames += skipped;
        }

        let mut stopping = false;
        if self.duration_given {
            let max_source_frames = self.grain_duration * self.buffer_rate;
            let left = max_source_frames - self.buffer_played_frames;
            if left <= 0.0 {
                frames_to_process = 0;
                stopping = true;
            } else if rate != 0.0 {
                let until = (left / rate.abs()).ceil();
                if until < frames_to_process as f64 {
                    frames_to_process = until as i64;
                    stopping = true;
                }
            }
        }
        if vri >= len_f {
            vri = vri.clamp(0.0, len_f);
            frames_to_process = 0;
            stopping = true;
        }

        let write_index = if rate == 1.0
            && vri == vri.floor()
            && virtual_delta_frames == virtual_delta_frames.floor()
            && virtual_end_frame == virtual_end_frame.floor()
        {
            self.fast_path(
                out,
                virtual_end_frame,
                frames_to_process,
                dest_offset,
                &mut vri,
            )
        } else {
            self.interpolated_path(
                out,
                virtual_end_frame,
                frames_to_process,
                dest_offset,
                &mut vri,
            )
        };

        if rate != 0.0 {
            let processed = (write_index - dest_offset) as u32;
            self.buffer_played_frames += processed as f64 * rate.abs();
        }
        if stopping {
            // The rest of the quantum stays zero.
            self.state = State::Finished;
        }
        self.virtual_read_index = vri;
    }

    /// `ProcessFastPath`: integral read position at rate 1, plain copy.
    fn fast_path(
        &mut self,
        out: &mut [&mut [f32]],
        virtual_end_frame: f64,
        frames_to_process: i64,
        mut write_index: usize,
        vri: &mut f64,
    ) -> usize {
        let buffer_length = self.buffer.frames();
        let mut read_index = *vri as usize;
        let end_frame = virtual_end_frame as usize;
        let n = frames_to_process.max(0) as usize;
        let this_time = end_frame.saturating_sub(read_index).min(n);
        for (o, src) in out.iter_mut().zip(&self.buffer.channels) {
            o[write_index..write_index + this_time]
                .copy_from_slice(&src[read_index..read_index + this_time]);
        }
        write_index += this_time;
        read_index += this_time;
        if read_index >= end_frame {
            // Not looping: silence for the rest (already zero), finish.
            self.state = State::Finished;
            read_index = read_index.min(buffer_length);
        }
        *vri = read_index as f64;
        write_index
    }

    /// `ProcessInterpolatedPath`: linear interpolation in f64, extrapolation at the last frame.
    fn interpolated_path(
        &mut self,
        out: &mut [&mut [f32]],
        virtual_end_frame: f64,
        frames_to_process: i64,
        mut write_index: usize,
        vri: &mut f64,
    ) -> usize {
        let buffer = &*self.buffer;
        let buffer_length = buffer.frames();
        let max_index = buffer_length - 1;
        let rate = self.rate;
        let mut pos = *vri;
        for _ in 0..frames_to_process.max(0) {
            let mut read_index = pos as u32 as usize;
            let factor = pos - read_index as f64;
            let mut read_index2 = read_index + 1;
            if read_index2 >= buffer_length {
                read_index2 = read_index;
            }
            read_index = read_index.min(max_index);
            read_index2 = read_index2.min(max_index);
            for (o, src) in out.iter_mut().zip(&buffer.channels) {
                let sample = if read_index == read_index2 && read_index >= 1 {
                    let s1 = src[read_index - 1] as f64;
                    let s2 = src[read_index] as f64;
                    s2 + (s2 - s1) * factor
                } else {
                    let s1 = src[read_index] as f64;
                    let s2 = src[read_index2] as f64;
                    (1.0 - factor) * s1 + factor * s2
                };
                o[write_index] = clamp_to_f32(sample);
            }
            write_index += 1;
            pos += rate;
            if pos >= virtual_end_frame {
                self.state = State::Finished;
                pos = pos.min(buffer_length as f64);
                break;
            }
        }
        *vri = pos;
        write_index
    }
}

/// Render a whole voice offline: `frames` output frames at `ctx_rate`, in 128-frame quanta from
/// context frame 0 (what an `OfflineAudioContext` does). Returns one channel per buffer channel.
pub fn render_offline(
    buffer: Arc<AudioBuffer>,
    ctx_rate: u32,
    params: StartParams,
    frames: usize,
) -> Vec<Vec<f32>> {
    const QUANTUM: usize = 128;
    let channels = buffer.channel_count();
    let mut player = BufferPlayer::new(buffer, ctx_rate, params);
    let mut out = vec![vec![0.0f32; frames]; channels];
    let mut pos = 0;
    while pos < frames {
        let n = QUANTUM.min(frames - pos);
        let mut slices: Vec<&mut [f32]> = out.iter_mut().map(|c| &mut c[pos..pos + n]).collect();
        player.process(&mut slices, pos as u64);
        pos += n;
    }
    out
}

#[cfg(test)]
mod parity_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(n: usize, sr: u32) -> Arc<AudioBuffer> {
        Arc::new(AudioBuffer::mono(
            sr,
            (0..n).map(|i| i as f32 / n as f32).collect(),
        ))
    }

    #[test]
    fn rate_one_aligned_is_a_copy() {
        let b = ramp(1000, 48000);
        let out = render_offline(
            b.clone(),
            48000,
            StartParams::with_value(0.0, 0.0, None, None, 1.0),
            1200,
        );
        assert_eq!(&out[0][..1000], &b.channels[0][..]);
        assert!(out[0][1000..].iter().all(|&x| x == 0.0));
    }

    #[test]
    fn start_is_rounded_up_and_sub_sample_offset_applied() {
        let b = ramp(1000, 48000);
        // when = 10.25 frames: first frame 11, read position 0.75 at rate 1.
        let when = 10.25 / 48000.0;
        let out = render_offline(
            b.clone(),
            48000,
            StartParams::with_value(when, 0.0, None, None, 1.0),
            256,
        );
        assert!(out[0][..11].iter().all(|&x| x == 0.0));
        let expect = (0.25 * b.channels[0][0] as f64 + 0.75 * b.channels[0][1] as f64) as f32;
        assert_eq!(out[0][11], expect);
    }

    #[test]
    fn stop_silences_from_the_rounded_up_frame() {
        let b = ramp(1000, 44100);
        let out = render_offline(
            b,
            48000,
            StartParams::sampler(0.0, 0.0, 1.0, Some(300.5 / 48000.0)),
            512,
        );
        assert!(out[0][300] != 0.0);
        assert!(out[0][301..].iter().all(|&x| x == 0.0));
    }

    #[test]
    fn last_frame_extrapolates() {
        let b = Arc::new(AudioBuffer::mono(44100, vec![0.0, 0.5, 1.0]));
        let out = render_offline(
            b,
            44100,
            StartParams::with_value(0.0, 0.0, None, None, 0.5),
            16,
        );
        // positions 0, .5, 1, 1.5, 2, 2.5 → the last two extrapolate past 1.0.
        assert_eq!(&out[0][..6], &[0.0, 0.25, 0.5, 0.75, 1.0, 1.25]);
        assert!(out[0][6..].iter().all(|&x| x == 0.0));
    }

    #[test]
    fn process_never_allocates_and_handles_odd_slices() {
        let b = Arc::new(AudioBuffer::new(32000, 2, 100));
        let mut p = BufferPlayer::new(b, 48000, StartParams::sampler(0.0, 0.0, 1.5, None));
        let mut l = [1.0f32; 128];
        let mut r = [1.0f32; 128];
        let mut extra = [1.0f32; 128];
        assert!(p.process(&mut [&mut l, &mut r, &mut extra], 0));
        assert!(extra.iter().all(|&x| x == 0.0));
        assert!(p.is_finished());
        assert!(!p.process(&mut [&mut l, &mut r], 128));
        assert!(!p.process(&mut [], 256));
    }
}
