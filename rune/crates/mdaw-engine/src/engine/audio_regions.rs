//! An audio track's region players (≙ one `Tone.Player` per audio region, `AE:1361-1388`).
//!
//! Each region is started by the transport at its start beat (the track schedule has one event per
//! region, `pad` = region index): `player.start(time, offsetSeconds, regionDurSec)` through
//! `Player.volume = gainDb`. As in Tone (`ToneBufferSource`): the native source starts at `time`
//! at `offset` (rate 1, offset snapped to a buffer frame), and its gain is 1 on the frames
//! `[time, time + duration)` (`_startGain` / `_stopGain` with no fades); a start of a region still
//! sounding cuts the old playback at the new start time (`Player.restart`).
//!
//! Rust-only (the TS quirk `AE:1382-1384`, a region the playback starts INSIDE stayed silent, is
//! gone): when the transport lands inside a region (a start, a seek while playing, a loop wrap)
//! the core resumes it at the matching offset (`Core::chase`).
//!
//! **Edits during playback** (Rust-only: the TS rebuilt nothing). The control side rebuilds the
//! instrument with the new region table; the core hands the sounding playbacks of the old one to
//! the new one ([`AudioInstrument::adopt`]), so a clip that is still there keeps playing,
//! sample-continuous. A playback whose region was edited or removed fades out over
//! [`EDIT_FADE`] ([`AudioInstrument::fade_out`]); a region that covers the playhead after the edit
//! and is not playing resumes at the current offset with a fade-in of the same length
//! ([`AudioInstrument::resume_region`]). Each region carries a `key` (its identity: region id,
//! sample and every setting that changes what it plays), which is how a playback is matched.

use std::any::Any;

use mdaw_dsp::abs_node::{BufferPlayer, StartParams};
use mdaw_dsp::buffer::SharedBuffer;

use mdaw_model::Track;

use crate::contract::{Instrument, PPQ, RENDER_QUANTUM};
use crate::core::{AudioInstrument, ScheduledNote, event_id};

const Q: usize = RENDER_QUANTUM;
/// Garbage buffers held until the core collects them, beyond one per voice.
const GARBAGE_MARGIN: usize = 16;
/// Fewest playback voices (an edit hands the old instrument's sounding ones over).
const MIN_VOICES: usize = 16;
/// Fade of a playback whose region an edit removed, and fade-in of one resumed by an edit.
pub const EDIT_FADE: f64 = 0.005;

/// What one region plays.
#[derive(Debug, Clone)]
pub struct RegionPlay {
    /// The region's identity (see the module docs): equal keys play the same thing.
    pub key: u64,
    pub buffer: Option<SharedBuffer>,
    /// `region.audio.offsetSeconds`.
    pub offset: f64,
    /// `regionDurSec` = lengthBeats·60/bpm.
    pub duration: f64,
    /// `region.audio.gainDb` as a linear gain (Tone `Volume`, f32).
    pub gain: f32,
}

struct Voice {
    key: u64,
    /// When the region's playback started (context seconds; a resumed one: when it would have).
    start: f64,
    gain: f32,
    player: Option<BufferPlayer>,
    /// Gain is 1 on frames [on, off).
    on: u64,
    off: u64,
    /// Fade-in over [on, fade_in_end) (== on: none).
    fade_in_end: u64,
    /// Fade-out over [fade_from, off) (u64::MAX: none).
    fade_from: u64,
}

impl Voice {
    const IDLE: Voice = Voice {
        key: 0,
        start: 0.0,
        gain: 1.0,
        player: None,
        on: 0,
        off: 0,
        fade_in_end: 0,
        fade_from: u64::MAX,
    };

    /// The gain envelope at frame `f` (inside [on, off)).
    fn ramp(&self, f: u64) -> f32 {
        let mut g = 1.0f32;
        if f < self.fade_in_end {
            g = (f - self.on + 1) as f32 / (self.fade_in_end - self.on + 1) as f32;
        }
        if f >= self.fade_from {
            let len = (self.off - self.fade_from) as f32;
            g = g.min((self.off - f) as f32 / (len + 1.0));
        }
        g
    }
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

/// An audio track's region players and their schedule events (one per region with a loaded
/// sample, `pad` = its index in `plays`), as playback and export build them.
pub struct AudioPlan {
    pub plays: Vec<RegionPlay>,
    pub events: Vec<ScheduledNote>,
    /// (region id, sample id) per play.
    pub ids: Vec<(String, String)>,
}

/// Build [`AudioPlan`] for `track` at `spb` seconds per beat; `buffer` looks a sample up.
pub fn plan_audio_track(
    track: &Track,
    spb: f64,
    buffer: impl Fn(&str) -> Option<SharedBuffer>,
) -> AudioPlan {
    let mut plan = AudioPlan {
        plays: Vec::new(),
        events: Vec::new(),
        ids: Vec::new(),
    };
    for region in &track.regions {
        let Some(audio) = &region.audio else { continue };
        let Some(buf) = buffer(&audio.sample_id) else {
            continue;
        };
        let duration = region.length_beats * spb;
        // identity: the region and everything that changes what it plays
        let key = event_id(
            &format!("{}\u{0}{}", region.id, audio.sample_id),
            &[
                region.start_beat,
                duration,
                audio.offset_seconds,
                audio.gain_db,
            ],
        );
        plan.events.push(ScheduledNote {
            tick: region.start_beat * PPQ as f64,
            beat: region.start_beat,
            note: 0.0,
            velocity: 1.0,
            duration_beats: region.length_beats,
            pad: Some(plan.plays.len()),
            id: key,
        });
        plan.plays.push(RegionPlay {
            key,
            buffer: Some(buf),
            offset: audio.offset_seconds,
            duration,
            gain: if audio.gain_db != 0.0 {
                db_gain(audio.gain_db)
            } else {
                1.0
            },
        });
        plan.ids.push((region.id.clone(), audio.sample_id.clone()));
    }
    plan
}

/// `dbToGain` as the Tone `Volume` applies it.
pub fn db_gain(db: f64) -> f32 {
    10f64.powf(db / 20.0) as f32
}

impl AudioRegions {
    pub fn new(sample_rate: u32, regions: Vec<RegionPlay>) -> Self {
        let n = (2 * regions.len()).max(MIN_VOICES);
        Self {
            sr: sample_rate,
            regions,
            voices: (0..n).map(|_| Voice::IDLE).collect(),
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

    /// End a voice: its buffer reference goes to the garbage (a region, or another instrument,
    /// may still hold the buffer, but nothing here can tell for sure, so it is never dropped on
    /// the audio thread).
    fn free(&mut self, i: usize) {
        if let Some(p) = self.voices[i].player.take() {
            let b = p.buffer().clone();
            drop(p);
            self.trash(b);
        }
    }

    /// A free voice, else the one ending first (freed).
    fn take_voice(&mut self) -> usize {
        match self.voices.iter().position(|v| v.player.is_none()) {
            Some(i) => i,
            None => {
                // Every slot busy (restarts faster than the voices end): reuse the one ending first.
                let i = (0..self.voices.len())
                    .min_by_key(|i| self.voices[*i].off)
                    .unwrap_or(0);
                self.free(i);
                i
            }
        }
    }

    /// Start `region`'s playback at `time`, `elapsed` seconds into it, fading in over `fade`.
    fn play(&mut self, region: usize, time: f64, elapsed: f64, fade: f64) {
        let Some(r) = self.regions.get(region) else {
            return;
        };
        let Some(buffer) = r.buffer.clone() else {
            return;
        };
        let (key, gain) = (r.key, r.gain);
        let sr = self.sr as f64;
        let on = frame_at(time, sr);
        let off = frame_at(time + (r.duration - elapsed).max(0.0), sr);
        let offset = r.offset.max(0.0) + elapsed;
        // Player.restart: the playback still sounding stops at the new start.
        for v in self.voices.iter_mut() {
            if v.player.is_some() && v.key == key && v.off > on {
                v.off = on;
            }
        }
        let slot = self.take_voice();
        let player = BufferPlayer::new(
            buffer,
            self.sr,
            StartParams::sampler(time, offset, 1.0, None),
        );
        self.voices[slot] = Voice {
            key,
            start: time - elapsed,
            gain,
            player: Some(player),
            on,
            off,
            fade_in_end: frame_at(time + fade, sr).min(off),
            fade_from: u64::MAX,
        };
    }

    /// Voices currently holding a playback (tests / diagnostics).
    pub fn playing(&self) -> usize {
        self.voices.iter().filter(|v| v.player.is_some()).count()
    }
}

impl AudioInstrument for AudioRegions {
    fn start_region(&mut self, region: usize, time: f64) {
        self.play(region, time, 0.0, 0.0);
    }

    fn resume_region(&mut self, region: usize, time: f64, elapsed: f64) {
        let Some(r) = self.regions.get(region) else {
            return;
        };
        if !(elapsed >= 0.0 && elapsed < r.duration) {
            return;
        }
        self.play(region, time, elapsed, EDIT_FADE);
    }

    fn fade_out(&mut self, key: u64, started: f64, time: f64) {
        let sr = self.sr as f64;
        let f = frame_at(time, sr);
        let end = frame_at(time + EDIT_FADE, sr);
        for v in self.voices.iter_mut() {
            if v.player.is_some() && v.key == key && (v.start - started).abs() <= 1e-6 && v.off > f
            {
                if v.on >= f {
                    v.off = v.on; // not started yet: never sounds
                    continue;
                }
                v.fade_from = v.fade_from.min(f);
                v.off = v.off.min(end);
            }
        }
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

    fn adopt(&mut self, old: &mut dyn AudioInstrument) {
        let Some(old) = old
            .as_any_mut()
            .and_then(|a| a.downcast_mut::<AudioRegions>())
        else {
            return;
        };
        for i in 0..old.voices.len() {
            if old.voices[i].player.is_none() {
                continue;
            }
            // Only into a free voice (never evict a playback for another); a playback that
            // finds none stays with the old instrument and ends with it.
            let Some(slot) = self.voices.iter().position(|v| v.player.is_none()) else {
                break;
            };
            self.voices[slot] = std::mem::replace(&mut old.voices[i], Voice::IDLE);
        }
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn Any> {
        Some(self)
    }
}

impl Instrument for AudioRegions {
    fn process(&mut self, frame: u64, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len()).min(Q);
        let end = frame + n as u64;
        for i in 0..self.voices.len() {
            let (on, off) = {
                let v = &self.voices[i];
                if v.player.is_none() {
                    continue;
                }
                (v.on, v.off)
            };
            if frame >= off {
                self.free(i);
                continue;
            }
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
            let v = &self.voices[i];
            let plain = v.fade_in_end <= frame && v.fade_from >= end;
            for k in 0..n {
                let f = frame + k as u64;
                if f < on || f >= off {
                    continue;
                }
                let gain = if plain { v.gain } else { v.gain * v.ramp(f) };
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
                key: 1,
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
