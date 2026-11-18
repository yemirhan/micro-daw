//! Applying one command at its frame (see `Command` for the semantics of each).

use mdaw_model::PAD_COUNT;

use super::api::{
    CaptureKind, Command, CoreEvent, Garbage, Phase, SlotFx, SynthNoteOp, TrackInstrument,
};
use super::click::{ACCENT_HZ, BEAT_HZ};
use super::handle::Stamped;
use super::render::trash_to;
use super::slots::SlotSet;
use super::{Core, CountInPlan, RETIRE_SLOTS, Retiree, SILENT_QUANTA};
use crate::fx::tone::{ToneParam, Units};
use crate::fx::track::StripOp;
use crate::sampler::SamplerCommand;
use crate::sampler::param::Clock;

impl Core {
    pub(super) fn apply(&mut self, s: Stamped, t: f64) {
        let (producer, seq) = (s.producer as usize, s.seq);
        self.apply_cmd(s, t);
        // after any garbage it produced is in the ring (the control side's backpressure)
        self.shared.set_applied(producer, seq);
    }

    /// Start (or re-anchor) the transport at `beat` at `at` (≥ `t`).
    fn start_at(&mut self, at: f64, beat: f64, t: f64) {
        let was_running = self.tr.phase != Phase::Stopped;
        if was_running {
            // Hits of the old run handed out for later than `t` (fractional ticks).
            self.for_each_drum(|d| d.cancel_pending_after(None, t));
            self.drain_all_slots();
        }
        let beat = if beat.is_finite() { beat.max(0.0) } else { 0.0 };
        self.tr.start(at.max(t), beat);
        self.suppress.clear();
        if !was_running {
            self.repeat.restart_all(self.clock(t));
        }
    }

    /// [`Command::ReplaceSynth`]: the new instrument in, the old one releasing into the retire
    /// list (see [`RETIRE_SLOTS`]: when it is full, silent retirees are reclaimed first, then
    /// the oldest one fades out).
    fn replace_synth(
        &mut self,
        slot: usize,
        instrument: Option<TrackInstrument>,
        t: f64,
        release_delay: f64,
        retire_after: f64,
    ) {
        let Some(s) = self.tracks.get_mut(slot) else {
            if let Some(i) = instrument {
                self.trash(Garbage::Instrument(i));
            }
            return;
        };
        for targets in &mut self.pad_targets {
            targets.remove(slot);
        }
        let Some(mut old) = std::mem::replace(&mut s.instrument, instrument) else {
            return;
        };
        let release_at = t + release_delay.max(0.0);
        if let TrackInstrument::Synth(o) = &mut old {
            o.release_all(release_at);
            o.retire();
        }
        let until = t + retire_after.max(0.0);
        let ringing = |r: &Option<Retiree>| r.as_ref().is_some_and(|r| r.fade.is_none());
        if s.retiring.iter().filter(|r| ringing(r)).count() >= RETIRE_SLOTS {
            // 1. reclaim the retirees that are already silent
            for r in &mut s.retiring {
                if r.as_ref()
                    .is_some_and(|r| r.fade.is_none() && r.quiet >= SILENT_QUANTA)
                    && let Some(g) = r.take()
                {
                    trash_to(
                        &mut self.garbage,
                        &mut self.overflow,
                        &self.shared,
                        Garbage::Instrument(g.inst),
                    );
                }
            }
            // 2. still full: fade the oldest one out
            if s.retiring.iter().filter(|r| ringing(r)).count() >= RETIRE_SLOTS
                && let Some(r) = s
                    .retiring
                    .iter_mut()
                    .filter_map(|r| r.as_mut())
                    .filter(|r| r.fade.is_none())
                    .min_by(|a, b| a.until.total_cmp(&b.until))
            {
                r.fade = Some(0);
            }
        }
        let entry = Retiree {
            inst: old,
            until,
            release_at,
            quiet: 0,
            fade: None,
        };
        let idx = match s.retiring.iter().position(|r| r.is_none()) {
            Some(i) => i,
            // every entry taken (two evictions within the last fade): cut the fade that is
            // furthest along
            None => (0..s.retiring.len())
                .max_by_key(|&i| s.retiring[i].as_ref().and_then(|r| r.fade).unwrap_or(0))
                .unwrap_or(0),
        };
        if let Some(evicted) = s.retiring[idx].replace(entry) {
            trash_to(
                &mut self.garbage,
                &mut self.overflow,
                &self.shared,
                Garbage::Instrument(evicted.inst),
            );
        }
    }

    fn apply_cmd(&mut self, s: Stamped, t: f64) {
        let producer = s.producer;
        match s.cmd {
            Command::Start {
                at,
                beat,
                generation,
            } => {
                if !self.accept_gen(generation) {
                    return;
                }
                self.start_at(at.filter(|a| a.is_finite()).unwrap_or(t), beat, t);
            }
            Command::Stop { generation } => {
                if !self.accept_gen(generation) {
                    return;
                }
                let was_running = self.tr.phase != Phase::Stopped;
                self.tr.stop();
                for slot in &mut self.tracks {
                    if slot.live_only {
                        continue;
                    }
                    match &mut slot.instrument {
                        Some(TrackInstrument::Drum(d)) => d.stop_all(t),
                        Some(TrackInstrument::Synth(s)) => s.release_all(t),
                        Some(TrackInstrument::Audio(a)) => a.stop_all(t),
                        None => {}
                    }
                }
                self.drain_all_slots();
                self.count_in.silence_from(t);
                self.ci = None;
                self.suppress.clear();
                self.punch_tick = None;
                self.emit(CoreEvent::Stopped {
                    time: t,
                    generation,
                });
                if was_running {
                    self.repeat.restart_all(self.clock(t));
                }
            }
            Command::Seek { beat, generation } => {
                if !self.accept_gen(generation) || !beat.is_finite() {
                    return;
                }
                self.tr.seek(t, beat.max(0.0));
                self.settle_wrap(t);
                self.emit(CoreEvent::Seeked {
                    time: t,
                    beat: self.tr.beat_at(t),
                    generation,
                });
            }
            Command::CountIn {
                t0,
                beats,
                sec_per_beat,
                start_beat,
                generation,
            } => {
                if !self.accept_gen(generation) || !(t0.is_finite() && sec_per_beat > 0.0) {
                    return;
                }
                self.count_in.silence_from(t);
                let dur = self.tr.sec_per_beat() / 8.0; // '32n'
                let beats = beats.min(32);
                for i in 0..beats {
                    let hz = if i % 4 == 0 { ACCENT_HZ } else { BEAT_HZ };
                    self.count_in
                        .schedule(t0 + i as f64 * sec_per_beat, dur, hz);
                }
                self.ci = Some(CountInPlan {
                    t0,
                    spb: sec_per_beat,
                    beats,
                    next: 0,
                });
                self.start_at(t0 + beats as f64 * sec_per_beat, start_beat, t);
            }
            Command::SetBpm { bpm } => {
                if bpm.is_finite() && bpm > 0.0 {
                    self.tr.set_bpm(t, bpm);
                }
            }
            Command::SetLoop { range } => self.tr.set_loop(range),
            Command::SetMetronome { enabled } => self.metronome_on = enabled,
            Command::SetNoteRepeat {
                enabled,
                rate_beats,
            } => self.repeat.configure(enabled, rate_beats, self.clock(t)),
            Command::SetInstrument { slot, instrument } => {
                let old = match self.tracks.get_mut(slot) {
                    Some(s) => {
                        // the new instrument has no live pads held yet
                        for targets in &mut self.pad_targets {
                            targets.remove(slot);
                        }
                        std::mem::replace(&mut s.instrument, instrument)
                    }
                    None => instrument,
                };
                if let Some(old) = old {
                    self.trash(Garbage::Instrument(old));
                }
            }
            Command::SetSchedule { slot, schedule } => {
                let old = match self.tracks.get_mut(slot) {
                    Some(s) => std::mem::replace(&mut s.schedule, schedule),
                    None => schedule,
                };
                if let Some(old) = old {
                    self.trash(Garbage::Schedule(old));
                }
            }
            Command::SetTrackGain { slot, gain } => {
                if let Some(s) = self.tracks.get_mut(slot) {
                    s.gain = if gain.is_finite() { gain } else { 0.0 };
                }
            }
            Command::SetTrackMute { slot, muted } => {
                if let Some(s) = self.tracks.get_mut(slot) {
                    s.muted = muted;
                }
            }
            Command::SetTrackPan { slot, pan } => {
                let pan = if pan.is_finite() { pan as f64 } else { 0.0 };
                if let Some(SlotFx::Track(s)) =
                    self.tracks.get_mut(slot).and_then(|s| s.strip.as_mut())
                {
                    s.apply(StripOp::Pan(pan), t);
                }
            }
            Command::StripOp { slot, op } => {
                if let Some(SlotFx::Track(s)) =
                    self.tracks.get_mut(slot).and_then(|s| s.strip.as_mut())
                {
                    s.apply(*op, t);
                }
                self.trash(Garbage::StripOp(op));
            }
            Command::LiveOp { slot, op } => {
                if let Some(SlotFx::Live(l)) =
                    self.tracks.get_mut(slot).and_then(|s| s.strip.as_mut())
                {
                    l.apply(op, t);
                }
            }
            Command::SetTrackStrip { slot, strip } => {
                let old = match self.tracks.get_mut(slot) {
                    Some(s) => std::mem::replace(&mut s.strip, strip),
                    None => strip,
                };
                if let Some(old) = old {
                    self.trash(Garbage::Strip(old));
                }
            }
            Command::SetLiveOnly { slot, live_only } => {
                if let Some(s) = self.tracks.get_mut(slot) {
                    s.live_only = live_only;
                }
            }
            Command::SetMasterBus { db } => {
                let c = Clock {
                    now: t,
                    sr: self.sr,
                };
                self.master = db
                    .filter(|d| d.is_finite())
                    .map(|d| ToneParam::gain(d, Units::Decibels, c));
            }
            Command::MasterVolume { db } => {
                if let (Some(m), true) = (self.master.as_mut(), db.is_finite()) {
                    m.ramp_to(
                        db,
                        0.03,
                        Clock {
                            now: t,
                            sr: self.sr,
                        },
                    );
                }
            }
            Command::KitCommand { slot, mut cmd } => {
                let c = std::mem::replace(
                    &mut *cmd,
                    SamplerCommand::SetPadBuffer {
                        pad: usize::MAX,
                        buffer: None,
                    },
                );
                match self
                    .tracks
                    .get_mut(slot)
                    .and_then(|s| s.instrument.as_mut())
                {
                    Some(TrackInstrument::Drum(d)) => d.apply_kit(c),
                    _ => *cmd = c,
                }
                self.trash(Garbage::KitCommand(cmd));
                self.drain_slot(slot);
            }
            Command::SetRegionBuffer {
                slot,
                region,
                buffer,
            } => {
                match self
                    .tracks
                    .get_mut(slot)
                    .and_then(|s| s.instrument.as_mut())
                {
                    Some(TrackInstrument::Audio(a)) => a.set_region_buffer(region, buffer),
                    _ => {
                        if let Some(b) = buffer {
                            self.trash(Garbage::Buffer(b));
                        }
                    }
                }
                self.drain_slot(slot);
            }
            Command::SetLiveSink { slot } => self.set_sink(slot, t),
            Command::PadDown { pad, velocity } => {
                if pad >= PAD_COUNT {
                    return;
                }
                self.settle_wrap(t);
                self.live_trigger(pad, velocity, t);
                self.capture(CaptureKind::Hit, Some(producer), pad, velocity, t, None);
                self.repeat.press(pad, velocity, self.clock(t));
            }
            Command::PadHit { pad, velocity } => {
                if pad >= PAD_COUNT {
                    return;
                }
                self.settle_wrap(t);
                self.live_trigger(pad, velocity, t);
                self.capture(CaptureKind::Hit, Some(producer), pad, velocity, t, None);
            }
            Command::PadUp { pad } => {
                if pad >= PAD_COUNT {
                    return;
                }
                self.repeat.release(pad);
                let mut targets = std::mem::take(&mut self.pad_targets[pad]);
                let switched = self.switch_released & (1 << pad) != 0;
                self.switch_released &= !(1 << pad);
                if targets.is_empty()
                    && !switched
                    && let Some(sink) = self.sink
                {
                    // drumEngine.release: no hit since the last release → the current sink.
                    targets = SlotSet::single(sink);
                }
                for i in 0..self.tracks.len() {
                    if targets.contains(i)
                        && let Some(TrackInstrument::Drum(d)) = &mut self.tracks[i].instrument
                    {
                        d.release(pad, t);
                        self.drain_slot(i);
                    }
                }
                self.settle_wrap(t);
                self.capture(CaptureKind::Release, Some(producer), pad, 0.0, t, None);
            }
            Command::SetLivePadMute { muted } => self.live_pad_muted = muted,
            Command::RepeatReleaseAll => {
                for pad in 0..PAD_COUNT {
                    self.repeat.release(pad);
                }
            }
            Command::StopVoices { slot } => {
                match self
                    .tracks
                    .get_mut(slot)
                    .and_then(|s| s.instrument.as_mut())
                {
                    Some(TrackInstrument::Drum(d)) => d.stop_all(t),
                    Some(TrackInstrument::Synth(s)) => s.release_all(t),
                    Some(TrackInstrument::Audio(a)) => a.stop_all(t),
                    None => {}
                }
                self.drain_slot(slot);
            }
            Command::SynthNote { slot, op, delay } => {
                let at = t + if delay.is_finite() {
                    delay.max(0.0)
                } else {
                    0.0
                };
                if let Some(TrackInstrument::Synth(s)) = self
                    .tracks
                    .get_mut(slot)
                    .and_then(|s| s.instrument.as_mut())
                {
                    match op {
                        SynthNoteOp::On { note, velocity } => s.note_on(note, velocity, at),
                        SynthNoteOp::Off { note } => s.note_off(note, at),
                        SynthNoteOp::ReleaseAll => s.release_all(at),
                    }
                }
            }
            Command::SynthSet { slot, mut patch } => {
                if let Some(TrackInstrument::Synth(s)) = self
                    .tracks
                    .get_mut(slot)
                    .and_then(|s| s.instrument.as_mut())
                {
                    s.apply_patch(&mut patch, t);
                }
                self.trash(Garbage::Patch(patch));
            }
            Command::ReplaceSynth {
                slot,
                instrument,
                release_delay,
                retire_after,
            } => self.replace_synth(slot, instrument, t, release_delay, retire_after),
            Command::SuppressPatternHit {
                note,
                beat,
                until,
                generation,
            } => {
                if generation == self.generation && self.tr.phase != Phase::Stopped {
                    self.suppress.insert(note, beat, until, generation);
                }
            }
            Command::SetPunchClose { tick } => self.punch_tick = tick,
            Command::CaptureSynthNote { note, velocity, on } => {
                self.settle_wrap(t);
                self.emit(CoreEvent::SynthCapture {
                    note,
                    velocity,
                    on,
                    time: t,
                    beat: self.tr.beat_at(t),
                    generation: self.generation,
                });
            }
        }
    }
}
