//! An audio track's region players (≙ one `Tone.Player` per audio region, `AE:1361-1388`).
//!
//! Each region is started by the transport at its start beat (the track schedule has one event per
//! region, `pad` = region index): `player.start(time, offsetSeconds, regionDurSec)` through
//! `Player.volume = gainDb`. As in Tone (`ToneBufferSource`): the native source starts at `time`
//! at `offset` (rate 1, offset snapped to a buffer frame), and its gain is 1 on the frames
//! `[time, time + duration)` (`_startGain` / `_stopGain` with no fades); a start of a region still
//! sounding cuts the old playback at the new start time (`Player.restart`).
//!
//! Quirk kept (`AE:1382-1384`): starting playback INSIDE a region does not start it (its event is
//! in the past), until a loop pass reaches its start again.

use mdaw_dsp::abs_node::{BufferPlayer, StartParams};
use mdaw_dsp::buffer::SharedBuffer;

use crate::contract::{Instrument, RENDER_QUANTUM};
use crate::core::AudioInstrument;

const Q: usize = RENDER_QUANTUM;
/// Garbage buffers held until the core collects them, beyond one per voice.
const GARBAGE_MARGIN: usize = 16;

/// What one region plays.
#[derive(Debug, Clone)]
pub struct RegionPlay {
    pub buffer: Option<SharedBuffer>,
    /// `region.audio.offsetSeconds`.
    pub offset: f64,
    /// `regionDurSec` = lengthBeats·60/bpm.
    pub duration: f64,
    /// `region.audio.gainDb` as a linear gain (Tone `Volume`, f32).
    pub gain: f32,
}

struct Voice {
    region: usize,
    player: Option<BufferPlayer>,
    /// Gain is 1 on frames [on, off).
    on: u64,
    off: u64,
}

/// See the module docs. Built on the control side (allocates), played on the audio thread.
pub struct AudioRegions {
    sr: u32,
    regions: Vec<RegionPlay>,
    voices: Vec<Voice>,
    garbage: Vec<SharedBuffer>,
    leaked: u64,
    tmp: Box<[[f32; Q]; 2]>,
}

fn frame_at(t: f64, sr: f64) -> u64 {
    (t * sr - 1e-9).ceil().max(0.0) as u64
}

/// `dbToGain` as the Tone `Volume` applies it.
pub fn db_gain(db: f64) -> f32 {
    10f64.powf(db / 20.0) as f32
}

impl AudioRegions {
    pub fn new(sample_rate: u32, regions: Vec<RegionPlay>) -> Self {
        let n = (2 * regions.len()).max(4);
        Self {
            sr: sample_rate,
            regions,
            voices: (0..n)
                .map(|_| Voice {
                    region: 0,
                    player: None,
                    on: 0,
                    off: 0,
                })
                .collect(),
            garbage: Vec::with_capacity(n + GARBAGE_MARGIN),
            leaked: 0,
            tmp: Box::new([[0.0; Q]; 2]),
        }
    }

    /// Retire a buffer: the core drains the list after every command / tick that touched this
    /// instrument and after each quantum (at most one buffer swap plus one per voice between two
    /// drains); if it is ever full the buffer is leaked (counted), never freed here.
    fn trash(&mut self, b: SharedBuffer) {
        if self.garbage.len() < self.garbage.capacity() {
            self.garbage.push(b);
        } else {
            std::mem::forget(b);
            self.leaked += 1;
        }
    }

    fn free(&mut self, i: usize) {
        if let Some(p) = self.voices[i].player.take() {
            let b = p.buffer().clone();
            drop(p);
            let region = self.voices[i].region;
            let current = self
                .regions
                .get(region)
                .and_then(|r| r.buffer.as_ref())
                .is_some_and(|cur| std::sync::Arc::ptr_eq(cur, &b));
            if current {
                drop(b); // the region still holds it: a decrement, never a free
            } else {
                self.trash(b);
            }
        }
    }
}

impl AudioInstrument for AudioRegions {
    fn start_region(&mut self, region: usize, time: f64) {
        let Some(r) = self.regions.get(region) else {
            return;
        };
        let Some(buffer) = r.buffer.clone() else {
            return;
        };
        let sr = self.sr as f64;
        let on = frame_at(time, sr);
        let off = frame_at(time + r.duration.max(0.0), sr);
        let offset = r.offset.max(0.0);
        // Player.restart: the playback still sounding stops at the new start.
        for v in self.voices.iter_mut() {
            if v.player.is_some() && v.region == region && v.off > on {
                v.off = on;
            }
        }
        let slot = match self.voices.iter().position(|v| v.player.is_none()) {
            Some(i) => i,
            None => {
                // Every slot busy (restarts faster than the voices end): reuse the one ending first.
                let i = (0..self.voices.len())
                    .min_by_key(|i| self.voices[*i].off)
                    .unwrap_or(0);
                self.free(i);
                i
            }
        };
        let player = BufferPlayer::new(
            buffer,
            self.sr,
            StartParams::sampler(time, offset, 1.0, None),
        );
        self.voices[slot] = Voice {
            region,
            player: Some(player),
            on,
            off,
        };
    }

    fn stop_all(&mut self, time: f64) {
        let f = frame_at(time, self.sr as f64);
        for v in self.voices.iter_mut() {
            if v.player.is_some() && v.off > f {
                v.off = f;
            }
        }
    }

    fn set_region_buffer(&mut self, region: usize, buffer: Option<SharedBuffer>) {
        let Some(r) = self.regions.get_mut(region) else {
            if let Some(b) = buffer {
                self.trash(b);
            }
            return;
        };
        if let Some(old) = std::mem::replace(&mut r.buffer, buffer) {
            self.trash(old);
        }
    }
}

impl Instrument for AudioRegions {
    fn process(&mut self, frame: u64, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len()).min(Q);
        let end = frame + n as u64;
        for i in 0..self.voices.len() {
            let (on, off, region) = {
                let v = &self.voices[i];
                if v.player.is_none() {
                    continue;
                }
                (v.on, v.off, v.region)
            };
            if frame >= off {
                self.free(i);
                continue;
            }
            let gain = self.regions.get(region).map_or(1.0, |r| r.gain);
            let Some(player) = self.voices[i].player.as_mut() else {
                continue;
            };
            let nch = player.buffer().channel_count().min(2);
            {
                let [a, b] = &mut *self.tmp;
                let mut outs: [&mut [f32]; 2] = [&mut a[..n], &mut b[..n]];
                player.process(&mut outs[..nch.max(1)], frame);
            }
            if end <= on {
                continue;
            }
            for k in 0..n {
                let f = frame + k as u64;
                if f < on || f >= off {
                    continue;
                }
                let l = self.tmp[0][k] * gain;
                let r = if nch == 2 { self.tmp[1][k] * gain } else { l };
                left[k] += l;
                right[k] += r;
            }
        }
    }

    fn take_garbage(&mut self, sink: &mut dyn FnMut(SharedBuffer)) {
        for b in self.garbage.drain(..) {
            sink(b);
        }
    }

    fn take_leaks(&mut self) -> u64 {
        std::mem::take(&mut self.leaked)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdaw_dsp::buffer::AudioBuffer;
    use std::sync::Arc;

    fn ramp(frames: usize) -> SharedBuffer {
        Arc::new(AudioBuffer::mono(
            48_000,
            (0..frames).map(|i| i as f32 / 1000.0).collect(),
        ))
    }

    fn render(a: &mut AudioRegions, frames: usize) -> Vec<f32> {
        let mut out = Vec::new();
        let mut f = 0u64;
        while out.len() < frames {
            let (mut l, mut r) = ([0f32; Q], [0f32; Q]);
            a.process(f, &mut l, &mut r);
            assert_eq!(l, r, "mono buffers play on both channels");
            out.extend_from_slice(&l);
            f += Q as u64;
        }
        out
    }

    #[test]
    fn plays_from_the_offset_for_the_region_length_with_its_gain() {
        let mut a = AudioRegions::new(
            48_000,
            vec![RegionPlay {
                buffer: Some(ramp(48_000)),
                offset: 0.01,
                duration: 0.005,
                gain: db_gain(-6.0),
            }],
        );
        a.start_region(0, 0.002);
        let out = render(&mut a, 1024);
        assert!(out[..96].iter().all(|x| *x == 0.0));
        let g = db_gain(-6.0);
        assert_eq!(out[96], 0.48 * g, "offset 0.01 s = frame 480 of the ramp");
        assert!(out[96 + 240..].iter().all(|x| *x == 0.0), "5 ms long");
        // a restart cuts the old playback; stop_all silences
        a.start_region(0, 0.03);
        a.stop_all(0.031);
        let mut b = vec![0f32; 0];
        let mut f = 1024u64;
        while b.len() < 2048 {
            let (mut l, mut r) = ([0f32; Q], [0f32; Q]);
            a.process(f, &mut l, &mut r);
            b.extend_from_slice(&l);
            f += Q as u64;
        }
        let on = 1440 - 1024;
        assert!(b[on] != 0.0 && b[on + 48..].iter().all(|x| *x == 0.0));
    }
}
