//! Test instruments and an offline rig (real core, no mock).

use std::sync::{Arc, Mutex};

use mdaw_model::PadIndex;

use super::*;
use crate::contract::Instrument;

pub const SR: u32 = 48_000;

/// What a test instrument was asked to do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Call {
    Trigger { pad: PadIndex, vel: f64, time: f64 },
    Release { pad: PadIndex, time: f64 },
    StopAll { time: f64 },
    Cancel { time: f64 },
    NoteOn { note: f64, time: f64 },
    NoteOff { note: f64, time: f64 },
    ReleaseAll { time: f64 },
}

pub type Log = Arc<Mutex<Vec<(usize, Call)>>>;

/// One impulse per hit at frame `ceil(time·sr)`: left = pad + 1, right = velocity.
pub struct ImpulseKit {
    id: usize,
    sr: f64,
    pending: [Option<(u64, PadIndex, f64)>; 256],
    gate: [bool; 16],
    log: Option<Log>,
}

impl ImpulseKit {
    pub fn new(id: usize, log: Option<Log>) -> Self {
        Self {
            id,
            sr: SR as f64,
            pending: [None; 256],
            gate: [false; 16],
            log,
        }
    }
    pub fn with_gates(mut self, pads: &[PadIndex]) -> Self {
        for p in pads {
            self.gate[*p] = true;
        }
        self
    }
    fn note(&self, c: Call) {
        if let Some(l) = &self.log {
            l.lock().unwrap().push((self.id, c));
        }
    }
    fn frame(&self, t: f64) -> u64 {
        (t * self.sr - 1e-6).ceil().max(0.0) as u64
    }
}

impl Instrument for ImpulseKit {
    fn process(&mut self, frame: u64, left: &mut [f32], right: &mut [f32]) {
        let end = frame + left.len() as u64;
        for p in self.pending.iter_mut() {
            if let Some((f, pad, vel)) = *p
                && f < end
            {
                let i = f.saturating_sub(frame) as usize;
                left[i] += (pad + 1) as f32;
                right[i] += vel as f32;
                *p = None;
            }
        }
    }
}

impl DrumInstrument for ImpulseKit {
    fn trigger(&mut self, pad: PadIndex, velocity: f64, time: f64) {
        self.note(Call::Trigger {
            pad,
            vel: velocity,
            time,
        });
        let f = self.frame(time);
        if let Some(slot) = self.pending.iter_mut().find(|p| p.is_none()) {
            *slot = Some((f, pad, velocity));
        }
    }
    fn release(&mut self, pad: PadIndex, time: f64) {
        self.note(Call::Release { pad, time });
    }
    fn stop_all(&mut self, time: f64) {
        self.note(Call::StopAll { time });
        let f = self.frame(time);
        for p in self.pending.iter_mut() {
            if p.is_some_and(|(pf, _, _)| pf > f) {
                *p = None;
            }
        }
    }
    fn cancel_pending_after(&mut self, pad: Option<PadIndex>, time: f64) {
        self.note(Call::Cancel { time });
        let f = self.frame(time);
        for p in self.pending.iter_mut() {
            if p.is_some_and(|(pf, pp, _)| pf > f && pad.is_none_or(|x| x == pp)) {
                *p = None;
            }
        }
    }
    fn pad_is_gate(&self, pad: PadIndex) -> bool {
        self.gate.get(pad).copied().unwrap_or(false)
    }
}

/// A synth stub: logs note on/off.
pub struct StubSynth {
    pub log: Log,
}

impl Instrument for StubSynth {
    fn process(&mut self, _: u64, _: &mut [f32], _: &mut [f32]) {}
}

impl SynthInstrument for StubSynth {
    fn note_on(&mut self, note: f64, _: f64, time: f64) {
        self.log
            .lock()
            .unwrap()
            .push((99, Call::NoteOn { note, time }));
    }
    fn note_off(&mut self, note: f64, time: f64) {
        self.log
            .lock()
            .unwrap()
            .push((99, Call::NoteOff { note, time }));
    }
    fn release_all(&mut self, time: f64) {
        self.log
            .lock()
            .unwrap()
            .push((99, Call::ReleaseAll { time }));
    }
}

/// An onset in the rendered output.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Onset {
    pub frame: u64,
    pub pad: PadIndex,
    pub vel: f64,
}

/// Offline rig: core + handles + everything rendered so far + every event.
pub struct Rig {
    pub core: Core,
    pub h: CoreHandle,
    pub midi: LiveInput,
    pub left: Vec<f32>,
    pub right: Vec<f32>,
    pub events: Vec<CoreEvent>,
    pub log: Log,
}

impl Rig {
    /// A core at 48 kHz with an impulse kit in slot 0 (the live sink).
    pub fn new() -> Self {
        let (core, mut h, midi) = Core::new(CoreConfig::new(SR));
        let log: Log = Arc::new(Mutex::new(Vec::new()));
        h.set_instrument(
            0,
            Some(TrackInstrument::Drum(Box::new(ImpulseKit::new(
                0,
                Some(log.clone()),
            )))),
        );
        h.set_live_sink(Some(0));
        Self {
            core,
            h,
            midi,
            left: Vec::new(),
            right: Vec::new(),
            events: Vec::new(),
            log,
        }
    }

    pub fn sr(&self) -> f64 {
        SR as f64
    }

    /// Core time = frames rendered (renders are whole quanta, see `run`).
    pub fn now(&self) -> f64 {
        self.h.now()
    }

    /// Render whole quanta until at least `seconds` more have passed.
    pub fn run(&mut self, seconds: f64) {
        let frames = ((seconds * self.sr()) / 128.0).ceil() as usize * 128;
        let mut l = vec![0f32; frames];
        let mut r = vec![0f32; frames];
        self.core.process(&mut l, &mut r);
        self.left.extend_from_slice(&l);
        self.right.extend_from_slice(&r);
        while let Some(e) = self.h.poll_event() {
            self.events.push(e);
        }
        self.h.collect_garbage();
    }

    /// Render until core time ≥ `t`.
    pub fn run_until(&mut self, t: f64) {
        if t > self.now() {
            self.run(t - self.now());
        }
    }

    /// Onsets (impulses) in the output, from frame `from` on.
    pub fn onsets_from(&self, from: u64) -> Vec<Onset> {
        (from as usize..self.left.len())
            .filter(|i| self.left[*i] >= 1.0)
            .map(|i| Onset {
                frame: i as u64,
                pad: self.left[i].round() as usize - 1,
                vel: self.right[i] as f64,
            })
            .collect()
    }

    pub fn onsets(&self) -> Vec<Onset> {
        self.onsets_from(0)
    }

    pub fn captures(&self) -> Vec<PadCapture> {
        self.events
            .iter()
            .filter_map(|e| match e {
                CoreEvent::Capture(c) => Some(*c),
                _ => None,
            })
            .collect()
    }

    pub fn calls(&self) -> Vec<(usize, Call)> {
        self.log.lock().unwrap().clone()
    }

    pub fn triggers(&self) -> Vec<(PadIndex, f64)> {
        self.calls()
            .into_iter()
            .filter_map(|(_, c)| match c {
                Call::Trigger { pad, time, .. } => Some((pad, time)),
                _ => None,
            })
            .collect()
    }

    /// Replace slot 0's kit (e.g. with gate pads).
    pub fn set_kit(&mut self, kit: ImpulseKit) {
        self.h
            .set_instrument(0, Some(TrackInstrument::Drum(Box::new(kit))));
    }

    /// A drum schedule on slot 0 with `(pad, beat)` notes (0.25 beat long, velocity 0.8).
    pub fn pattern(&mut self, notes: &[(PadIndex, f64)]) {
        let events = notes
            .iter()
            .map(|(pad, beat)| ScheduledNote {
                tick: beat * PPQ as f64,
                beat: *beat,
                note: 36.0 + *pad as f64,
                velocity: 0.8,
                duration_beats: 0.25,
                pad: Some(*pad),
            })
            .collect();
        self.h
            .set_schedule(0, Some(Arc::new(TrackSchedule::new(true, events))));
    }

    pub fn frame_of(&self, t: f64) -> u64 {
        (t * self.sr() - 1e-6).ceil() as u64
    }
}
