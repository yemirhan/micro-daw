//! The routing core: port of `src/hooks/useMidiNotes.ts` (note routing + live note state).
//!
//! [`Router`] is the same state machine as the hook, one method per hook callback, and reports
//! every effect as an [`Action`] to a [`RouterOutput`], in the order the TS makes its calls.
//! In the app ([`crate::engine_link`]) the engine actions go straight to the core through the
//! MIDI thread's `LiveInput` (no UI frame in between) and the rest is queued for the UI thread.
//!
//! **TS call → action** (the core does the rest, verified in phase 3):
//! - `pressPad(pad, v)` (`drumEngine.hit` unless muted, recorder capture, flash, `drum` event,
//!   `noteRepeat.press`) → [`Action::PadDown`]: the core plays it on the live sink (unless pad
//!   mute is on), captures it (the recorders and the flash read captures) and starts the repeat.
//! - `drumHit(pad, v)` (no repeat) → [`Action::PadHit`].
//! - `releasePad(pad)` (`noteRepeat.release`, settle pending repeats, `drumEngine.release`,
//!   capture note-off) → [`Action::PadUp`]. Pending repeats need no settling: the core fires a
//!   repeat at its frame, so "already heard" and "captured" are the same thing.
//! - `audioEngine.noteOn / noteOff / releaseAll` → [`Action::SynthOn`] / [`Action::SynthOff`] /
//!   [`Action::SynthReleaseAll`].
//! - `noteRepeat.releaseAll()` → [`Action::RepeatReleaseAll`].
//! - `muted` → [`Action::PadMute`] (pads; the synth keeps the hook's `attacked` flag).
//! - `arrangementEngine.captureNoteOn(note, v, false)` / `captureNoteOff(note)` for synth notes →
//!   [`Action::CaptureNoteOn`] / [`Action::CaptureNoteOff`]. The TS only calls them while
//!   recording; the engine's `capture_note_on/off` check that themselves, so the router emits
//!   them whenever the TS would if recording (one fewer cross-thread read).
//! - `noteEventBus.emit(noteon / noteoff / allnotesoff)` → [`Action::Bus`].
//! - `controllerMapping.handleCC` knob dispatches → [`Action::Knob`] (applied by the KnobRouter
//!   on the UI thread).
//! - The legacy volume / effect CC fallbacks → [`Action::LegacyCc`] (the engine's
//!   `handle_legacy_cc`, which owns the effect pickup, applies them).
//! - `setSustainOn` / `setActiveNotes` → [`Action::Sustain`] / [`Action::ActiveNotes`].
//!
//! **Routing modes** (`RoutingMode`, decided at note-on; a held note keeps its route):
//! - `Auto`: notes the controller profile maps to pads trigger them; the rest play the synth.
//! - `Keys`: everything plays the synth.
//! - `Split`: a keyboard split at [`Router::split_point`] (default [`DEFAULT_SPLIT_POINT`],
//!   C4 = MIDI 60). Notes below it trigger pads chromatically, ignoring the profile's pad map:
//!   the 16 notes just below the split are pads 0–15 (`note = split − 16 + pad`), and lower
//!   notes repeat that layout every 16 semitones (`pad = (note − split + 16) mod 16`), so every
//!   key left of the split is a drum. Notes at or above it play the synth.
//!
//!   **Electron bug fixed in both apps (user decision 2026-11-18):** `split` used to behave
//!   exactly like `auto` while its tooltip promised "left half drums, right half synth"
//!   (inventory §5 gap 1).
//!
//! Learn consumes a note before any routing. Channel 10 is not special.

use mdaw_model::controller::{ControllerMapping, KnobEvent, ProfileStore};
use mdaw_model::{PAD_COUNT, PadIndex};

/// MIDI CC numbers with special meaning (`useMidiNotes.ts:29-32`).
pub const CC_SUSTAIN: u8 = 64;
pub const CC_ALL_SOUND_OFF: u8 = 120;
pub const CC_RESET_ALL_CONTROLLERS: u8 = 121;
pub const CC_ALL_NOTES_OFF: u8 = 123;
/// `VOLUME_CCS` (`constants.ts:38`).
pub const VOLUME_CCS: [u8; 2] = [5, 74];
/// The CCs of `CC_MAPPINGS` (`constants.ts:26-35`).
pub const EFFECT_CCS: [u8; 8] = [1, 70, 2, 71, 3, 72, 4, 73];

/// Default split point of [`RoutingMode::Split`]: C4 (MIDI 60). Stored in the settings
/// ([`crate::settings::MIDI_SPLIT_POINT_KEY`]).
pub const DEFAULT_SPLIT_POINT: i64 = 60;

/// `RoutingMode` (`constants.ts:66`), default `Auto`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RoutingMode {
    #[default]
    Auto,
    Keys,
    Split,
}

impl RoutingMode {
    pub fn as_str(self) -> &'static str {
        match self {
            RoutingMode::Auto => "auto",
            RoutingMode::Keys => "keys",
            RoutingMode::Split => "split",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "auto" => RoutingMode::Auto,
            "keys" => RoutingMode::Keys,
            "split" => RoutingMode::Split,
            _ => return None,
        })
    }
}

/// `NoteEvent`s of `NoteEventBus` the router emits (the `drum` event comes from core captures).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BusEvent {
    NoteOn {
        note: i64,
        velocity: f64,
        channel: i64,
    },
    NoteOff {
        note: i64,
        channel: i64,
    },
    AllNotesOff,
}

/// One effect of the router (see the module docs). `time`: the context time the input event
/// should sound at (None: as soon as possible).
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    PadDown {
        pad: PadIndex,
        velocity: f64,
        time: Option<f64>,
    },
    PadHit {
        pad: PadIndex,
        velocity: f64,
        time: Option<f64>,
    },
    PadUp {
        pad: PadIndex,
        time: Option<f64>,
    },
    SynthOn {
        note: i64,
        velocity: f64,
        time: Option<f64>,
    },
    SynthOff {
        note: i64,
        time: Option<f64>,
    },
    SynthReleaseAll {
        time: Option<f64>,
    },
    RepeatReleaseAll {
        time: Option<f64>,
    },
    PadMute {
        muted: bool,
        time: Option<f64>,
    },
    CaptureNoteOn {
        note: i64,
        velocity: f64,
        time: Option<f64>,
    },
    CaptureNoteOff {
        note: i64,
        time: Option<f64>,
    },
    Bus(BusEvent),
    Knob(KnobEvent),
    /// An unmapped volume (5 / 74) or effect (1-4, 70-73) CC.
    /// `master`: volume CCs go to the master volume (`onVolumeChange` set) rather than the
    /// live synth's level.
    LegacyCc {
        cc: u8,
        value: u8,
        master: bool,
    },
    Sustain(bool),
    /// The held notes changed (`setActiveNotes`); read them with [`Router::active_notes`].
    ActiveNotes,
}

impl Action {
    /// Runs on the audio side (the core) rather than the UI thread.
    pub fn is_engine(&self) -> bool {
        matches!(
            self,
            Action::PadDown { .. }
                | Action::PadHit { .. }
                | Action::PadUp { .. }
                | Action::SynthOn { .. }
                | Action::SynthOff { .. }
                | Action::SynthReleaseAll { .. }
                | Action::RepeatReleaseAll { .. }
                | Action::PadMute { .. }
        )
    }
}

/// Where the router's actions go.
pub trait RouterOutput {
    fn emit(&mut self, action: Action);
}

impl RouterOutput for Vec<Action> {
    fn emit(&mut self, action: Action) {
        self.push(action);
    }
}

/// `LiveVoice`: one per pitch that is (or, muted, would be) sounding on the live synth.
#[derive(Debug, Clone, Copy, PartialEq)]
struct LiveVoice {
    velocity: f64,
    /// `audioEngine.noteOn` was called (false while muted).
    attacked: bool,
    /// The key is physically down.
    held: bool,
}

/// A JS `Map` (insertion order; `set` on an existing key keeps its position).
#[derive(Debug, Clone)]
struct OrderedMap<K, V>(Vec<(K, V)>);

impl<K: PartialEq + Copy, V> OrderedMap<K, V> {
    fn new() -> Self {
        Self(Vec::new())
    }
    fn get(&self, k: K) -> Option<&V> {
        self.0.iter().find(|(key, _)| *key == k).map(|(_, v)| v)
    }
    fn get_mut(&mut self, k: K) -> Option<&mut V> {
        self.0.iter_mut().find(|(key, _)| *key == k).map(|(_, v)| v)
    }
    fn set(&mut self, k: K, v: V) {
        match self.get_mut(k) {
            Some(slot) => *slot = v,
            None => self.0.push((k, v)),
        }
    }
    fn delete(&mut self, k: K) -> Option<V> {
        let i = self.0.iter().position(|(key, _)| *key == k)?;
        Some(self.0.remove(i).1)
    }
    fn clear(&mut self) {
        self.0.clear();
    }
    fn keys(&self) -> Vec<K> {
        self.0.iter().map(|(k, _)| *k).collect()
    }
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// `noteKey(note, channel)`.
type NoteKey = (i64, i64);

/// A held note for the UI (`ActiveNote`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActiveNote {
    pub note: i64,
    pub velocity: f64,
}

/// The routing core (see the module docs). Owns the controller mapping (learn state, profile):
/// every note-on / CC goes through it first, like the TS singleton.
pub struct Router<St: ProfileStore> {
    pub mapping: ControllerMapping<St>,
    routing_mode: RoutingMode,
    split_point: i64,
    muted: bool,
    voices: OrderedMap<i64, LiveVoice>,
    pad_notes_down: OrderedMap<NoteKey, PadIndex>,
    learned_notes_down: OrderedMap<NoteKey, ()>,
    sustain: bool,
    /// `onVolumeChange` is set (the app routes volume CCs to the master volume).
    master_volume_handler: bool,
    /// Time of the input being handled.
    time: Option<f64>,
}

impl<St: ProfileStore> Router<St> {
    pub fn new(mapping: ControllerMapping<St>) -> Self {
        Self {
            mapping,
            routing_mode: RoutingMode::Auto,
            split_point: DEFAULT_SPLIT_POINT,
            muted: false,
            voices: OrderedMap::new(),
            pad_notes_down: OrderedMap::new(),
            learned_notes_down: OrderedMap::new(),
            sustain: false,
            master_volume_handler: true,
            time: None,
        }
    }

    // --- Options / state ------------------------------------------------------------------

    pub fn routing_mode(&self) -> RoutingMode {
        self.routing_mode
    }
    /// `setRoutingMode` (held notes keep the route chosen at their note-on).
    pub fn set_routing_mode(&mut self, mode: RoutingMode) {
        self.routing_mode = mode;
    }
    /// The split point of [`RoutingMode::Split`] (MIDI note).
    pub fn split_point(&self) -> i64 {
        self.split_point
    }
    /// `setSplitPoint` (clamped to 0..=127; held notes keep their route). Persist it with
    /// [`crate::settings::write_split_point`].
    pub fn set_split_point(&mut self, note: i64) {
        self.split_point = clamp_split_point(note);
    }
    pub fn muted(&self) -> bool {
        self.muted
    }
    /// The `muted` option (`audioInput.muted`): pads are captured but silent, new synth notes are
    /// tracked without sounding. Held synth voices keep sounding (like the TS).
    pub fn set_muted(&mut self, muted: bool, out: &mut impl RouterOutput) {
        if self.muted != muted {
            self.muted = muted;
            out.emit(Action::PadMute {
                muted,
                time: self.time,
            });
        }
    }
    /// Whether `onVolumeChange` is provided (default true, as in the app): volume CCs then go to
    /// the master volume, else to the live synth's level.
    pub fn set_master_volume_handler(&mut self, on: bool) {
        self.master_volume_handler = on;
    }
    pub fn master_volume_handler(&self) -> bool {
        self.master_volume_handler
    }
    pub fn sustain_on(&self) -> bool {
        self.sustain
    }
    /// `activeNotes`: held keys in insertion order.
    pub fn active_notes(&self) -> Vec<ActiveNote> {
        self.voices
            .0
            .iter()
            .filter(|(_, v)| v.held)
            .map(|(note, v)| ActiveNote {
                note: *note,
                velocity: v.velocity,
            })
            .collect()
    }
    /// `getSoundingNotes()`: attacked live-synth notes, sorted.
    pub fn sounding_notes(&self) -> Vec<i64> {
        let mut v: Vec<i64> = self
            .voices
            .0
            .iter()
            .filter(|(_, v)| v.attacked)
            .map(|(n, _)| *n)
            .collect();
        v.sort_unstable();
        v
    }

    fn with_time<R>(&mut self, time: Option<f64>, f: impl FnOnce(&mut Self) -> R) -> R {
        let prev = self.time;
        self.time = time;
        let r = f(self);
        self.time = prev;
        r
    }

    // --- Pads -----------------------------------------------------------------------------

    fn press_pad(&mut self, pad: PadIndex, velocity: f64, out: &mut impl RouterOutput) {
        out.emit(Action::PadDown {
            pad,
            velocity,
            time: self.time,
        });
    }

    fn release_pad(&mut self, pad: PadIndex, out: &mut impl RouterOutput) {
        out.emit(Action::PadUp {
            pad,
            time: self.time,
        });
    }

    /// `drumHit`: one-shot pad hit (previews), never the synth, never repeated.
    pub fn drum_hit(&mut self, pad: f64, velocity: f64, out: &mut impl RouterOutput) {
        if let Some(pad) = pad_index(pad) {
            out.emit(Action::PadHit {
                pad,
                velocity,
                time: self.time,
            });
        }
    }
    /// `drumPress`: on-screen / computer-key pad held (pair with [`Self::drum_release`]).
    pub fn drum_press(&mut self, pad: f64, velocity: f64, out: &mut impl RouterOutput) {
        if let Some(pad) = pad_index(pad) {
            self.press_pad(pad, velocity, out);
        }
    }
    pub fn drum_release(&mut self, pad: f64, out: &mut impl RouterOutput) {
        if let Some(pad) = pad_index(pad) {
            self.release_pad(pad, out);
        }
    }

    // --- Synth notes ----------------------------------------------------------------------

    fn release_voice(&mut self, note: i64, out: &mut impl RouterOutput) {
        let Some(voice) = self.voices.delete(note) else {
            return;
        };
        if voice.attacked {
            out.emit(Action::SynthOff {
                note,
                time: self.time,
            });
        }
    }

    fn key_down(&mut self, note: i64, velocity: f64, channel: i64, out: &mut impl RouterOutput) {
        if let Some(existing) = self.voices.get(note).copied() {
            if existing.attacked {
                out.emit(Action::SynthOff {
                    note,
                    time: self.time,
                });
            }
            if existing.held {
                out.emit(Action::CaptureNoteOff {
                    note,
                    time: self.time,
                });
            }
        }
        let attacked = !self.muted;
        if attacked {
            out.emit(Action::SynthOn {
                note,
                velocity,
                time: self.time,
            });
        }
        self.voices.set(
            note,
            LiveVoice {
                velocity,
                attacked,
                held: true,
            },
        );
        out.emit(Action::CaptureNoteOn {
            note,
            velocity,
            time: self.time,
        });
        out.emit(Action::ActiveNotes);
        out.emit(Action::Bus(BusEvent::NoteOn {
            note,
            velocity,
            channel,
        }));
    }

    fn key_up(&mut self, note: i64, channel: i64, out: &mut impl RouterOutput) {
        let Some(voice) = self.voices.get_mut(note) else {
            return;
        };
        if !voice.held {
            return;
        }
        voice.held = false;
        out.emit(Action::CaptureNoteOff {
            note,
            time: self.time,
        });
        if !self.sustain {
            self.release_voice(note, out);
        }
        out.emit(Action::ActiveNotes);
        out.emit(Action::Bus(BusEvent::NoteOff { note, channel }));
    }

    fn pad_for_midi_note(&self, note: i64, channel: i64) -> Option<PadIndex> {
        match self.routing_mode {
            RoutingMode::Keys => None,
            RoutingMode::Auto => self.mapping.pad_for_note(note, channel),
            RoutingMode::Split => split_pad(note, self.split_point),
        }
    }

    /// MIDI note-on (routing applies). `velocity` 0..1; ≤ 0 is a note-off.
    pub fn midi_note_on(
        &mut self,
        note: i64,
        velocity: f64,
        channel: i64,
        time: Option<f64>,
        out: &mut impl RouterOutput,
    ) {
        self.with_time(time, |r| r.midi_note_on_now(note, velocity, channel, out));
    }

    fn midi_note_on_now(
        &mut self,
        note: i64,
        velocity: f64,
        channel: i64,
        out: &mut impl RouterOutput,
    ) {
        // Running-status note-off sent as note-on velocity 0 (NaN is not ≤ 0, in JS or here).
        if velocity <= 0.0 {
            self.midi_note_off_now(note, channel, out);
            return;
        }
        let key = (note, channel);
        if self.mapping.handle_note_on(note, velocity, channel) {
            self.learned_notes_down.set(key, ());
            return;
        }
        if let Some(pad) = self.pad_for_midi_note(note, channel) {
            if let Some(previous) = self.pad_notes_down.get(key).copied() {
                self.release_pad(previous, out); // re-hit without a note-off
            }
            self.pad_notes_down.set(key, pad);
            self.press_pad(pad, velocity, out);
            return;
        }
        self.key_down(note, velocity, channel, out);
    }

    /// MIDI note-off: uses the route chosen at note-on.
    pub fn midi_note_off(
        &mut self,
        note: i64,
        channel: i64,
        time: Option<f64>,
        out: &mut impl RouterOutput,
    ) {
        self.with_time(time, |r| r.midi_note_off_now(note, channel, out));
    }

    fn midi_note_off_now(&mut self, note: i64, channel: i64, out: &mut impl RouterOutput) {
        let key = (note, channel);
        if self.learned_notes_down.delete(key).is_some() {
            return;
        }
        if let Some(pad) = self.pad_notes_down.delete(key) {
            self.release_pad(pad, out);
            return;
        }
        self.key_up(note, channel, out);
    }

    /// On-screen keyboard note-on: always the synth, channel 0.
    pub fn note_on(&mut self, note: i64, velocity: f64, out: &mut impl RouterOutput) {
        self.key_down(note, velocity, 0, out);
    }
    pub fn note_off(&mut self, note: i64, out: &mut impl RouterOutput) {
        self.key_up(note, 0, out);
    }

    // --- Sustain / panic ------------------------------------------------------------------

    /// `setSustain(on)`: pedal up releases every voice that is no longer held.
    pub fn set_sustain(&mut self, on: bool, out: &mut impl RouterOutput) {
        if self.sustain == on {
            return;
        }
        self.sustain = on;
        out.emit(Action::Sustain(on));
        if !on {
            for note in self.voices.keys() {
                if self.voices.get(note).is_some_and(|v| !v.held) {
                    self.release_voice(note, out);
                }
            }
        }
    }

    /// `allNotesOff()` (panic; also on every app-mode switch and on dispose).
    pub fn all_notes_off(&mut self, out: &mut impl RouterOutput) {
        for note in self.voices.keys() {
            if self.voices.get(note).is_some_and(|v| v.held) {
                out.emit(Action::CaptureNoteOff {
                    note,
                    time: self.time,
                });
            }
            self.release_voice(note, out);
        }
        let pads: Vec<PadIndex> = self.pad_notes_down.0.iter().map(|(_, p)| *p).collect();
        self.pad_notes_down.clear();
        self.learned_notes_down.clear();
        for pad in pads {
            self.release_pad(pad, out);
        }
        out.emit(Action::RepeatReleaseAll { time: self.time });
        self.voices.clear();
        self.sustain = false;
        out.emit(Action::Sustain(false));
        out.emit(Action::SynthReleaseAll { time: self.time });
        out.emit(Action::ActiveNotes);
        out.emit(Action::Bus(BusEvent::AllNotesOff));
    }

    // --- CCs ------------------------------------------------------------------------------

    /// `handleCC(cc, value, channel)`.
    pub fn handle_cc(
        &mut self,
        cc: u8,
        value: u8,
        channel: i64,
        time: Option<f64>,
        out: &mut impl RouterOutput,
    ) {
        self.with_time(time, |r| r.handle_cc_now(cc, value, channel, out));
    }

    fn handle_cc_now(&mut self, cc: u8, value: u8, channel: i64, out: &mut impl RouterOutput) {
        match cc {
            CC_SUSTAIN => return self.set_sustain(value >= 64, out),
            CC_ALL_SOUND_OFF | CC_ALL_NOTES_OFF => return self.all_notes_off(out),
            CC_RESET_ALL_CONTROLLERS => return self.set_sustain(false, out),
            _ => {}
        }
        let outcome = self
            .mapping
            .handle_cc(i64::from(cc), f64::from(value), channel);
        if outcome.consumed {
            for k in outcome.knobs {
                out.emit(Action::Knob(k));
            }
            return;
        }
        if VOLUME_CCS.contains(&cc) || EFFECT_CCS.contains(&cc) {
            out.emit(Action::LegacyCc {
                cc,
                value,
                master: self.master_volume_handler,
            });
        }
    }

    /// A parsed MIDI event (`MidiService`'s callbacks).
    pub fn handle_event(
        &mut self,
        ev: crate::message::MidiEvent,
        time: Option<f64>,
        out: &mut impl RouterOutput,
    ) {
        use crate::message::MidiEvent;
        match ev {
            MidiEvent::NoteOn {
                note,
                velocity,
                channel,
            } => self.midi_note_on(i64::from(note), velocity, i64::from(channel), time, out),
            MidiEvent::NoteOff { note, channel } => {
                self.midi_note_off(i64::from(note), i64::from(channel), time, out)
            }
            MidiEvent::ControlChange { cc, value, channel } => {
                self.handle_cc(cc, value, i64::from(channel), time, out)
            }
        }
    }

    /// True while any note / pad / learned note is down or a voice sounds (tests, debug).
    pub fn is_idle(&self) -> bool {
        self.voices.is_empty()
            && self.pad_notes_down.is_empty()
            && self.learned_notes_down.is_empty()
    }
}

/// `clampSplitPoint`: an integer MIDI note 0..=127 (non-integers truncate, like the TS's
/// `Math.trunc`).
pub fn clamp_split_point(note: i64) -> i64 {
    note.clamp(0, 127)
}

/// `splitPad(note, split)`: the pad a note below the split triggers (see the module docs).
pub fn split_pad(note: i64, split: i64) -> Option<PadIndex> {
    (note < split)
        .then(|| (note - split + PAD_COUNT as i64).rem_euclid(PAD_COUNT as i64) as PadIndex)
}

/// `isPadIndex(pad)`: an integer 0..15.
fn pad_index(pad: f64) -> Option<PadIndex> {
    (pad.fract() == 0.0 && (0.0..PAD_COUNT as f64).contains(&pad)).then_some(pad as PadIndex)
}

#[cfg(test)]
#[path = "router_tests.rs"]
mod tests;
