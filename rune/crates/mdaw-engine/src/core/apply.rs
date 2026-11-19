//! Applying one command at its frame (see `Command` for the semantics of each).

use mdaw_model::PAD_COUNT;

use super::api::{
    CaptureKind, Command, CoreEvent, Garbage, Phase, SlotFx, SynthNoteOp, TrackInstrument,
};
use super::click::{ACCENT_HZ, BEAT_HZ};
use super::handle::Stamped;
use super::render::trash_to;
use super::schedule::TrackSchedule;
use super::slots::SlotSet;
use super::sounding::{SOUNDING_CAP, Sounding};
use super::{Core, CountInPlan, RETIRE_SLOTS, Retiree, SILENT_QUANTA};
use crate::contract::PPQ;
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

    /// A slot's schedule is being replaced (an edit during playback): reconcile the events of
    /// the old one that are sounding at `t` ([`SoundingSet`], bounded by polyphony) with `new`.
    /// - Still in `new` (same identity, see [`super::ScheduledNote::id`]): keeps sounding; if
    ///   its note got shorter, it ends at its new end (or now).
    /// - Gone (its clip deleted, moved or trimmed, the note edited away): that one instance
    ///   ends now — a synth note is released (by note and onset; a voice it took over from an
    ///   earlier strike of the same note that is still scheduled ends with that one), a
    ///   gate-pad voice released by its handle, an audio playback faded out.
    /// - Audio tracks: a region of `new` covering the playhead that is not playing resumes at
    ///   the current offset (`AudioInstrument::resume_region`).
    ///
    /// Allocation-free; one binary search per sounding event, plus the events of `new` that
    /// overlap the playhead for audio tracks.
    fn reconcile_sounding(&mut self, slot: usize, new: Option<&TrackSchedule>, t: f64) {
        let running = self.tr.phase == Phase::Started;
        let ppq = PPQ as f64;
        let k = self.tr.beat_at(t) * ppq;
        let spb = self.tr.sec_per_beat();
        let fired = self.tr.fired_tick;
        let Some(s) = self.tracks.get_mut(slot) else {
            return;
        };
        s.sounding.prune(t);
        let Some(inst) = s.instrument.as_mut() else {
            s.sounding.clear();
            return;
        };
        let n = s.sounding.len();
        let mut kept = [false; SOUNDING_CAP];
        for (i, e) in s.sounding.items_mut().iter_mut().enumerate() {
            let Some(x) = new.and_then(|n| n.find(e.tick, e.id)) else {
                continue;
            };
            kept[i] = true;
            let new_end = e.start + x.duration_beats * e.spb;
            if matches!(inst, TrackInstrument::Audio(_)) || new_end >= e.end - 1e-9 {
                continue; // (a longer note keeps its booked release)
            }
            let at = new_end.max(t);
            match inst {
                TrackInstrument::Synth(syn) => syn.cut_scheduled(e.note, e.start, at),
                TrackInstrument::Drum(d) => {
                    if let Some(pad) = e.pad {
                        d.release_voice(pad, e.voice, at);
                    }
                }
                TrackInstrument::Audio(_) => {}
            }
            e.end = at;
        }
        let items = s.sounding.items();
        for i in 0..n {
            if kept[i] {
                continue;
            }
            let e = items[i];
            match inst {
                TrackInstrument::Synth(syn) => {
                    // A re-strike of a sounding note takes its voice over: if a kept strike of
                    // the same note at or before this onset is still due, the voice now ends
                    // with it instead of now.
                    let mut until = t;
                    for (j, o) in items.iter().enumerate().take(n) {
                        if kept[j] && o.note == e.note && o.start <= e.start + 1e-9 {
                            until = until.max(o.end);
                        }
                    }
                    if until < e.end - 1e-9 {
                        syn.cut_scheduled(e.note, e.start, until);
                    }
                }
                TrackInstrument::Drum(d) => {
                    if let Some(pad) = e.pad {
                        d.release_voice(pad, e.voice, t);
                    }
                }
                TrackInstrument::Audio(a) => a.fade_out(e.id, e.start, t),
            }
        }
        s.sounding.retain_indexed(&kept[..n]);
        if running && let (Some(new), TrackInstrument::Audio(a)) = (new, &mut *inst) {
            for x in new.around(k) {
                let Some(region) = x.pad else { continue };
                let len = x.duration_beats * ppq;
                let ft = (x.tick + 1e-9).floor() as i64;
                // picked up already (else the transport starts it itself)
                let passed = (ft as f64) < k - 1e-9 || fired.is_some_and(|f| ft <= f);
                if !passed || x.tick + len <= k + 1e-9 || s.sounding.contains_id(x.id) {
                    continue;
                }
                let elapsed = (k - x.tick).max(0.0) / ppq * spb;
                a.resume_region(region, t, elapsed);
                let start = t - elapsed;
                let end = start + x.duration_beats * spb;
                s.sounding.push(
                    Sounding {
                        id: x.id,
                        tick: x.tick,
                        note: x.note,
                        pad: x.pad,
                        voice: 0,
                        start,
                        end,
                        spb,
                    },
                    t,
                );
            }
        }
        self.drain_slot(slot);
    }

    /// Swap a slot's schedule (after reconciling what it sounds).
    fn set_schedule(
        &mut self,
        slot: usize,
        schedule: Option<std::sync::Arc<TrackSchedule>>,
        t: f64,
    ) {
        self.reconcile_sounding(slot, schedule.as_deref(), t);
        let old = match self.tracks.get_mut(slot) {
            Some(s) => std::mem::replace(&mut s.schedule, schedule),
            None => schedule,
        };
        if let Some(old) = old {
            self.trash(Garbage::Schedule(old));
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
        s.sounding.clear();
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
                    slot.sounding.clear();
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
                        s.sounding.clear();
                        std::mem::replace(&mut s.instrument, instrument)
                    }
                    None => instrument,
                };
                if let Some(old) = old {
                    self.trash(Garbage::Instrument(old));
                }
            }
            Command::SetSchedule { slot, schedule } => self.set_schedule(slot, schedule, t),
            Command::SetAudioTrack {
                slot,
                instrument,
                schedule,
            } => {
                let old = match self.tracks.get_mut(slot) {
                    Some(s) => {
                        let mut old = std::mem::replace(&mut s.instrument, instrument);
                        match (&mut s.instrument, &mut old) {
                            // its sounding playbacks continue in the new one
                            (
                                Some(TrackInstrument::Audio(new)),
                                Some(TrackInstrument::Audio(o)),
                            ) => new.adopt(&mut **o),
                            _ => s.sounding.clear(),
                        }
                        old
                    }
                    None => instrument,
                };
                if let Some(old) = old {
                    self.trash(Garbage::Instrument(old));
                }
                self.set_schedule(slot, schedule, t);
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
            Command::DrumNote {
                slot,
                pad,
                velocity,
                duration,
            } => {
                if let Some(TrackInstrument::Drum(d)) = self
                    .tracks
                    .get_mut(slot)
                    .and_then(|s| s.instrument.as_mut())
                {
                    d.trigger(pad, velocity, t);
                    if d.pad_is_gate(pad) {
                        d.release(pad, t + duration.max(0.01));
                    }
                }
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
            Command::SetInput { source } => {
                if let Some(old) = std::mem::replace(&mut self.input, source) {
                    self.trash(Garbage::Input(old));
                }
            }
            Command::SetSlotInput { slot, input } => {
                if let Some(s) = self.tracks.get_mut(slot) {
                    s.input = input;
                }
            }
            Command::SetSlotRecord { slot, tap } => {
                let old = match self.tracks.get_mut(slot) {
                    Some(s) => std::mem::replace(&mut s.record, tap),
                    None => tap,
                };
                if let Some(old) = old {
                    self.trash(Garbage::RecordTap(old));
                }
            }
            Command::SetInserts { slot, chain } => {
                let old = match self.tracks.get_mut(slot) {
                    Some(s) => std::mem::replace(&mut s.inserts, chain),
                    None => chain,
                };
                if let Some(old) = old {
                    self.trash(Garbage::Inserts(old));
                }
            }
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
