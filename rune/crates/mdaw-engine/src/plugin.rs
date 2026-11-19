//! Hosted plugins as the core runs them.
//!
//! The engine knows nothing about plugin formats: the app hands it [`PluginProcessor`]s (an
//! Audio Unit's render half, `mdaw-plugin-host`) through a [`PluginFactory`], and the engine
//! puts them where the document says:
//!
//! - a track's **insert effects** ([`InsertChain`], [`crate::core::Command::SetInserts`]): run
//!   in order on the slot's signal (instrument + monitored input) before its track strip;
//! - a synth track's **instrument** ([`PluginSynth`]): a [`SynthInstrument`] that turns the
//!   core's sample-timed notes into MIDI for the plugin.
//!
//! A processor renders on the audio thread and must not allocate or lock on the host side; what
//! a third-party plugin does inside its render is its own business (well-behaved ones do not
//! either). Retired chains and instruments leave through the core's garbage ring and are
//! dropped on the control side.

use std::collections::VecDeque;

use mdaw_model::PluginSlot;

use crate::contract::{Instrument, RENDER_QUANTUM};
use crate::core::SynthInstrument;

/// The host transport a plugin sees (tempo-synced delays, arpeggiators).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PluginTransport {
    pub bpm: f64,
    /// Position in quarter notes at the block's first frame.
    pub beat: f64,
    pub playing: bool,
}

/// One plugin's render half.
pub trait PluginProcessor: Send {
    /// An effect processes `left` / `right` in place; an instrument ADDS its output to them.
    fn process(&mut self, left: &mut [f32], right: &mut [f32]);
    /// A MIDI message for the next `process`, `offset` frames into it.
    fn midi(&mut self, _status: u8, _data1: u8, _data2: u8, _offset: u32) {}
    /// The transport during the next `process`.
    fn set_transport(&mut self, _transport: &PluginTransport) {}
    /// Clear tails and voices.
    fn reset(&mut self) {}
}

/// Where the engine gets its processors (the app's plugin manager).
pub trait PluginFactory {
    /// Every track is about to be (re)built for the project `session`
    /// (`ArrangementDoc::get_session_id`). Another session than the last one is another
    /// project: plugins loaded for the old one must not carry over (two projects can use the
    /// same slot ids: the same file opened again). The same session is an undo / redo: the
    /// loaded plugins stay as they are.
    fn begin_session(&mut self, _session: u64) {}
    /// The processor of the plugin instance `plugin` (loading it, and applying its stored state,
    /// when needed) at `sample_rate`. None: the plugin is not installed or failed to load; the
    /// track then plays without it.
    fn processor(
        &mut self,
        plugin: &PluginSlot,
        sample_rate: u32,
    ) -> Option<Box<dyn PluginProcessor>>;
    /// A fresh, independent processor for an offline render (an export) of `plugin`.
    fn offline_processor(
        &mut self,
        plugin: &PluginSlot,
        sample_rate: u32,
    ) -> Option<Box<dyn PluginProcessor>>;
}

/// No plugins (tests, headless runs).
#[derive(Debug, Clone, Copy, Default)]
pub struct NoPlugins;

impl PluginFactory for NoPlugins {
    fn processor(&mut self, _: &PluginSlot, _: u32) -> Option<Box<dyn PluginProcessor>> {
        None
    }
    fn offline_processor(&mut self, _: &PluginSlot, _: u32) -> Option<Box<dyn PluginProcessor>> {
        None
    }
}

/// One insert of a chain.
pub struct Insert {
    pub processor: Box<dyn PluginProcessor>,
    /// Skipped (the signal passes unchanged).
    pub bypassed: bool,
}

/// A track's insert effects, in order (see the module docs). Built on the control side.
#[derive(Default)]
pub struct InsertChain {
    inserts: Vec<Insert>,
}

impl InsertChain {
    pub fn new(inserts: Vec<Insert>) -> Self {
        Self { inserts }
    }

    pub fn len(&self) -> usize {
        self.inserts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inserts.is_empty()
    }

    /// Run the chain over one quantum, in place.
    pub fn process(&mut self, transport: &PluginTransport, left: &mut [f32], right: &mut [f32]) {
        for i in self.inserts.iter_mut().filter(|i| !i.bypassed) {
            i.processor.set_transport(transport);
            i.processor.process(left, right);
        }
    }

    /// A non-finite output was found (and silenced) after the chain.
    pub fn reset(&mut self) {
        for i in &mut self.inserts {
            i.processor.reset();
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Message {
    On { note: u8, velocity: u8 },
    Off { note: u8 },
    AllOff,
}

#[derive(Debug, Clone, Copy)]
struct Event {
    frame: u64,
    message: Message,
}

/// Notes a plugin instrument holds queued (sample-timed, across quanta).
const EVENT_CAPACITY: usize = 4096;

/// A plugin instrument in a track slot (see the module docs). Notes arrive stamped in context
/// seconds, possibly in the future (a pattern note's release); each is handed to the plugin in
/// the quantum that contains its frame, at its offset.
pub struct PluginSynth {
    processor: Box<dyn PluginProcessor>,
    sr: f64,
    /// Sorted by frame (stable: equal frames keep their arrival order).
    events: VecDeque<Event>,
    /// Notes the plugin was told are down.
    held: [bool; 128],
}

fn key(note: f64) -> u8 {
    if note.is_finite() {
        note.round().clamp(0.0, 127.0) as u8
    } else {
        60
    }
}

impl PluginSynth {
    pub fn new(processor: Box<dyn PluginProcessor>, sample_rate: u32) -> Self {
        Self {
            processor,
            sr: sample_rate.max(1) as f64,
            events: VecDeque::with_capacity(EVENT_CAPACITY),
            held: [false; 128],
        }
    }

    fn frame_at(&self, time: f64) -> u64 {
        if time.is_finite() {
            (time * self.sr - 1e-6).ceil().max(0.0) as u64
        } else {
            0
        }
    }

    fn push(&mut self, time: f64, message: Message) {
        let frame = self.frame_at(time);
        if self.events.len() >= EVENT_CAPACITY {
            // Full (thousands of notes pending): never drop a release.
            if !matches!(message, Message::On { .. }) {
                self.send(message, 0);
            }
            return;
        }
        let at = self.events.partition_point(|e| e.frame <= frame);
        self.events.insert(at, Event { frame, message });
    }

    fn send(&mut self, message: Message, offset: u32) {
        match message {
            Message::On { note, velocity } => {
                self.held[note as usize] = true;
                self.processor.midi(0x90, note, velocity, offset);
            }
            Message::Off { note } => {
                self.held[note as usize] = false;
                self.processor.midi(0x80, note, 0, offset);
            }
            Message::AllOff => {
                for note in 0..128u8 {
                    if std::mem::take(&mut self.held[note as usize]) {
                        self.processor.midi(0x80, note, 0, offset);
                    }
                }
                // Sustain off, then "all notes off" for whatever the plugin still holds.
                self.processor.midi(0xB0, 64, 0, offset);
                self.processor.midi(0xB0, 123, 0, offset);
            }
        }
    }
}

impl Instrument for PluginSynth {
    fn process(&mut self, frame: u64, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len()).min(RENDER_QUANTUM);
        let end = frame + n as u64;
        while let Some(e) = self.events.front().copied() {
            if e.frame >= end {
                break;
            }
            self.events.pop_front();
            let offset = e
                .frame
                .saturating_sub(frame)
                .min(n.saturating_sub(1) as u64);
            self.send(e.message, offset as u32);
        }
        self.processor.process(&mut left[..n], &mut right[..n]);
    }

    fn set_transport(&mut self, transport: &PluginTransport) {
        self.processor.set_transport(transport);
    }

    fn clear_non_finite(&mut self) {
        self.processor.reset();
        self.held = [false; 128];
    }
}

impl SynthInstrument for PluginSynth {
    fn note_on(&mut self, note: f64, velocity: f64, time: f64) {
        let velocity = if velocity.is_finite() {
            (velocity * 127.0).round().clamp(1.0, 127.0) as u8
        } else {
            100
        };
        self.push(
            time,
            Message::On {
                note: key(note),
                velocity,
            },
        );
    }

    fn note_off(&mut self, note: f64, time: f64) {
        self.push(time, Message::Off { note: key(note) });
    }

    fn release_all(&mut self, time: f64) {
        // Notes queued to start later belong to the run that just ended.
        let frame = self.frame_at(time);
        self.events
            .retain(|e| e.frame <= frame || !matches!(e.message, Message::On { .. }));
        self.push(time, Message::AllOff);
    }

    fn cut_scheduled(&mut self, note: f64, _started: f64, time: f64) {
        self.push(time, Message::Off { note: key(note) });
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::{Arc, Mutex};

    use super::*;

    /// What a [`Probe`] was asked to do.
    #[derive(Debug, Clone, PartialEq)]
    pub enum Call {
        Midi(u8, u8, u8, u32),
        Process(usize),
        Reset,
    }

    /// A processor that logs its calls; as an effect it multiplies by `gain`, as an instrument
    /// it adds `gain` while a note is down.
    pub struct Probe {
        pub log: Arc<Mutex<Vec<Call>>>,
        pub gain: f32,
        pub instrument: bool,
        pub down: u32,
        pub transport: Arc<Mutex<Option<PluginTransport>>>,
    }

    impl Probe {
        pub fn effect(gain: f32) -> (Self, Arc<Mutex<Vec<Call>>>) {
            let log = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    log: log.clone(),
                    gain,
                    instrument: false,
                    down: 0,
                    transport: Arc::default(),
                },
                log,
            )
        }

        pub fn instrument(gain: f32) -> (Self, Arc<Mutex<Vec<Call>>>) {
            let (mut p, log) = Self::effect(gain);
            p.instrument = true;
            (p, log)
        }
    }

    impl PluginProcessor for Probe {
        fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
            self.log.lock().unwrap().push(Call::Process(left.len()));
            for v in left.iter_mut().chain(right.iter_mut()) {
                if self.instrument {
                    if self.down > 0 {
                        *v += self.gain;
                    }
                } else {
                    *v *= self.gain;
                }
            }
        }
        fn midi(&mut self, status: u8, d1: u8, d2: u8, offset: u32) {
            match status {
                0x90 => self.down += 1,
                0x80 => self.down = self.down.saturating_sub(1),
                _ => {}
            }
            self.log
                .lock()
                .unwrap()
                .push(Call::Midi(status, d1, d2, offset));
        }
        fn set_transport(&mut self, t: &PluginTransport) {
            *self.transport.lock().unwrap() = Some(*t);
        }
        fn reset(&mut self) {
            self.log.lock().unwrap().push(Call::Reset);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{Call, Probe};
    use super::*;

    const Q: usize = RENDER_QUANTUM;

    #[test]
    fn notes_reach_the_plugin_in_the_quantum_of_their_frame_at_their_offset() {
        let (probe, log) = Probe::instrument(0.5);
        let mut s = PluginSynth::new(Box::new(probe), 48_000);
        // frame 200 (second quantum, offset 72); its release 1000 frames later
        s.note_on(60.0, 0.8, 200.0 / 48_000.0);
        s.note_off(60.0, 1200.0 / 48_000.0);
        let (mut l, mut r) = ([0f32; Q], [0f32; Q]);
        s.process(0, &mut l, &mut r);
        assert_eq!(*log.lock().unwrap(), vec![Call::Process(Q)]);
        s.process(128, &mut l, &mut r);
        assert_eq!(
            log.lock().unwrap()[1..],
            [Call::Midi(0x90, 60, 102, 72), Call::Process(Q)]
        );
        assert_eq!(l[0], 0.5, "the instrument adds its output");
        for q in 2..9 {
            s.process(q * 128, &mut l, &mut r);
        }
        // 1200 = 9·128 + 48
        log.lock().unwrap().clear();
        s.process(9 * 128, &mut l, &mut r);
        assert_eq!(log.lock().unwrap()[0], Call::Midi(0x80, 60, 0, 48));
    }

    #[test]
    fn release_all_ends_every_held_note_and_drops_notes_not_started() {
        let (probe, log) = Probe::instrument(0.5);
        let mut s = PluginSynth::new(Box::new(probe), 48_000);
        s.note_on(60.0, 1.0, 0.0);
        s.note_on(64.0, 1.0, 0.0);
        let (mut l, mut r) = ([0f32; Q], [0f32; Q]);
        s.process(0, &mut l, &mut r);
        s.note_on(67.0, 1.0, 1.0); // far in the future
        s.release_all(128.0 / 48_000.0);
        log.lock().unwrap().clear();
        s.process(128, &mut l, &mut r);
        assert_eq!(
            *log.lock().unwrap(),
            vec![
                Call::Midi(0x80, 60, 0, 0),
                Call::Midi(0x80, 64, 0, 0),
                Call::Midi(0xB0, 64, 0, 0),
                Call::Midi(0xB0, 123, 0, 0),
                Call::Process(Q),
            ]
        );
        assert!(s.events.is_empty(), "the pending note-on was dropped");
    }

    #[test]
    fn a_chain_runs_its_inserts_in_order_and_skips_bypassed_ones() {
        let (a, _) = Probe::effect(0.5);
        let (b, log_b) = Probe::effect(4.0);
        let (c, _) = Probe::effect(0.25);
        let seen = a.transport.clone();
        let mut chain = InsertChain::new(vec![
            Insert {
                processor: Box::new(a),
                bypassed: false,
            },
            Insert {
                processor: Box::new(b),
                bypassed: true,
            },
            Insert {
                processor: Box::new(c),
                bypassed: false,
            },
        ]);
        let (mut l, mut r) = ([1f32; Q], [2f32; Q]);
        let t = PluginTransport {
            bpm: 97.0,
            beat: 3.5,
            playing: true,
        };
        chain.process(&t, &mut l, &mut r);
        assert_eq!((l[0], r[0]), (0.125, 0.25));
        assert!(log_b.lock().unwrap().is_empty());
        assert_eq!(*seen.lock().unwrap(), Some(t));
        chain.reset();
        assert_eq!(*log_b.lock().unwrap(), vec![Call::Reset]);
    }
}
