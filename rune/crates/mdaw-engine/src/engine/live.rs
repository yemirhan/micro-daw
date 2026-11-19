//! The live synth bus (`AudioEngine`'s effects, volume and mute; `fx/live.rs`), its MIDI CC
//! fallbacks (`handleCC`, `fx/cc.rs`) and the master fader (`setMasterVolumeDb`, masterBus.ts).
//!
//! The bus lives in its own core slot (after drumEngine's fallback kit), built when the engine
//! is. The live instrument is [`LiveSynthApi`] (`engine.live()`), the rest of `AudioEngine`:
//! - `start()` builds the instrument (preset 0 until `set_preset`) and starts decoding the piano
//!   samples in the background; notes before `start` are ignored, like the TS.
//! - `note_on` / `note_off` / `release_all` play at the next render quantum + 4 ms
//!   ([`LIVE_OFFSET`]: the TS `Tone.immediate() + LIVE_OFFSET`); a note-off is honoured while
//!   muted. The MIDI thread can send the same commands straight to the core
//!   (`LiveInput::synth_note` on [`ArrangementEngine::live_synth_slot`]).
//! - `set_preset(i)` (integers 0..41 only) resets the overrides and builds a new instrument; the
//!   old one gets `releaseAll` at Tone's `now()` (+100 ms look-ahead, [`RETIRE_RELEASE_DELAY`])
//!   and is disposed [`RETIRE_MS`] later, ringing out on the same bus until then.
//! - `set_synth_param('a.b' | key, value)` merges into the overrides and `set`s the running
//!   instrument (no rebuild; `Command::SynthSet` with a patch prepared here: the audio thread
//!   does not allocate; queued edits coalesce to the latest).
//! - getters: overrides, current preset, preset index, active voices / held notes (published by
//!   the instrument after every quantum: they lag the commands by up to one device buffer),
//!   `is_piano_ready`; `mute` / `unmute` / `set_volume` (30 ms ramp, default −12 dB) are the bus
//!   setters ([`LiveOp`]).
//!
//! Not ported here: the master volume's localStorage persistence (`micro-daw:masterVolume`) is
//! the settings store's job (the app reads it and calls [`ArrangementEngine::set_master_volume_db`]).

use std::sync::Arc;

use mdaw_model::knobs::KnobPickup;
use mdaw_model::types::EffectParams;
use serde_json::{Map, Value};

use super::ArrangementEngine;
use crate::core::{CoreHandle, SlotFx, SynthInstrument, SynthNoteOp, TrackInstrument};
use crate::fx::cc::{CcAction, cc_action, effect_to_normalized};
use crate::fx::live::{LiveFx, LiveFxState, LiveOp};
use crate::piano::PianoSlot;
use crate::player::{SynthPatch, SynthPlayer, SynthStatus};
use crate::synth::DEFAULT_NOISE_SEED;
use crate::synth::bank::synth_presets;
use crate::synth::params::{merge_overrides, merge_preset_params, path_to_object};
use crate::synth::presets::{SynthPreset, resolve_preset};

/// Live notes play at the next render quantum + this (`LIVE_OFFSET`, `AudioEngine.ts:19`).
pub const LIVE_OFFSET: f64 = 0.004;
/// A replaced live instrument rings out this long before it is disposed (`RETIRE_MS`).
pub const RETIRE_MS: f64 = 1200.0;
/// The replaced instrument's `releaseAll()` time: Tone's `now()` = immediate + the 0.1 s
/// look-ahead (the TS calls it without a time).
pub const RETIRE_RELEASE_DELAY: f64 = 0.1;

/// `MASTER_REFERENCE_DB` (= `DEFAULT_VOLUME`): the fader position of a 0 dB master gain.
pub const MASTER_REFERENCE_DB: f64 = -12.0;
/// The master fader's default (`DEFAULT_VOLUME`).
pub const MASTER_DEFAULT_DB: f64 = -12.0;
/// Fader range (`MIN_VOLUME` / `MAX_VOLUME`).
pub const MASTER_MIN_DB: f64 = -40.0;
pub const MASTER_MAX_DB: f64 = 0.0;

/// `clampVolume(db)` (NaN → the default).
pub fn clamp_master_volume(db: f64) -> f64 {
    if db.is_nan() {
        MASTER_DEFAULT_DB
    } else {
        db.clamp(MASTER_MIN_DB, MASTER_MAX_DB)
    }
}

/// `masterGainDb(volumeDb)`: the master `Volume`'s dB for a fader position.
pub fn master_gain_db(volume_db: f64) -> f64 {
    clamp_master_volume(volume_db) - MASTER_REFERENCE_DB
}

/// The live bus's slot, the params `getEffectParams` returns, and the live instrument's state.
pub(super) struct LiveSynth {
    pub slot: usize,
    pub state: LiveFxState,
    /// `ccPickup` of `useMidiNotes` (soft takeover of the effect CCs).
    pub pickup: KnobPickup,
    started: bool,
    preset_index: usize,
    overrides: Map<String, Value>,
    /// The running instrument's preset (`set` patches merged in, like `PolyInstrument.set`).
    instrument_preset: SynthPreset,
    status: Arc<SynthStatus>,
    /// Instance id of the current instrument in `status`.
    instance: u64,
}

impl LiveSynth {
    pub fn new(h: &mut CoreHandle, slot: usize) -> Self {
        let state = LiveFxState::default();
        let fx = LiveFx::new(&state.params, h.shared().sample_rate, h.now());
        h.set_track_strip(slot, Some(SlotFx::Live(Box::new(fx))));
        h.set_track_gain(slot, 1.0);
        h.set_live_only(slot, true);
        Self {
            slot,
            state,
            pickup: KnobPickup::default(),
            started: false,
            preset_index: 0,
            overrides: Map::new(),
            instrument_preset: synth_presets()[0].clone(),
            status: Arc::new(SynthStatus::default()),
            instance: 0,
        }
    }
}

/// `AudioEngine`'s instrument API (see the module docs), borrowed from the engine with
/// [`ArrangementEngine::live`].
pub struct LiveSynthApi<'a> {
    s: &'a mut LiveSynth,
    h: &'a mut CoreHandle,
    piano: &'a mut Option<PianoSlot>,
    sr: u32,
}

impl LiveSynthApi<'_> {
    /// `start()`: idempotent; builds the instrument and starts decoding the piano samples.
    pub fn start(&mut self) {
        if self.s.started {
            return;
        }
        self.piano_slot();
        self.create_instrument();
        self.s.started = true;
    }

    pub fn is_started(&self) -> bool {
        self.s.started
    }

    fn piano_slot(&mut self) -> PianoSlot {
        let sr = self.sr;
        self.piano
            .get_or_insert_with(|| PianoSlot::load(sr))
            .clone()
    }

    /// `createInstrument()`: the old instrument releases and retires, the new one plays.
    fn create_instrument(&mut self) {
        let overrides = Value::Object(self.s.overrides.clone());
        let preset = resolve_preset(self.s.preset_index as f64, Some(&overrides));
        let piano = self.piano_slot();
        self.s.instance += 1;
        let player = SynthPlayer::new(&preset, self.sr, self.h.now(), &piano, DEFAULT_NOISE_SEED)
            .with_status(self.s.status.clone(), self.s.instance);
        self.s.instrument_preset = preset;
        self.h.replace_synth(
            self.s.slot,
            Some(TrackInstrument::Synth(Box::new(player))),
            RETIRE_RELEASE_DELAY,
            RETIRE_MS / 1000.0,
        );
    }

    fn note(&mut self, op: SynthNoteOp) {
        self.h.synth_note(self.s.slot, op, LIVE_OFFSET);
    }

    /// `noteOn(midi, velocity)` (velocity 0..1; ignored before `start`).
    pub fn note_on(&mut self, midi: f64, velocity: f64) {
        if self.s.started {
            self.note(SynthNoteOp::On {
                note: midi,
                velocity,
            });
        }
    }

    /// `noteOff(midi)`: always honoured (even while muted).
    pub fn note_off(&mut self, midi: f64) {
        if self.s.started {
            self.note(SynthNoteOp::Off { note: midi });
        }
    }

    /// `releaseAll()`: every live voice fades out (also ones whose note-off was lost).
    pub fn release_all(&mut self) {
        if self.s.started {
            self.note(SynthNoteOp::ReleaseAll);
        }
    }

    /// `setPreset(index)`: integer indices of the bank only; resets the overrides.
    pub fn set_preset(&mut self, index: f64) {
        if index.fract() != 0.0 || !(0.0..synth_presets().len() as f64).contains(&index) {
            return;
        }
        self.s.preset_index = index as usize;
        self.s.overrides = Map::new();
        if self.s.started {
            self.create_instrument();
        }
    }

    /// `setSynthParam(key, value)`: `key` is a top-level preset key (value = a partial object)
    /// or a dotted path (`'filter.cutoff'`).
    pub fn set_synth_param(&mut self, key: &str, value: Value) {
        let patch = if key.contains('.') {
            path_to_object(key, value)
        } else {
            let mut m = Map::new();
            m.insert(key.to_owned(), value);
            Value::Object(m)
        };
        let Value::Object(p) = &patch else { return };
        self.s.overrides = merge_overrides(Some(&self.s.overrides), p);
        if self.s.started {
            let next = merge_preset_params(&self.s.instrument_preset, Some(&patch));
            self.s.instrument_preset = next.clone();
            // Voices the instrument used so far (a lower bound: it only grows): fresh voices
            // are prepared for the others.
            let snap = self.s.status.snapshot();
            let used = if snap.instance == self.s.instance {
                snap.used
            } else {
                0
            };
            let patch = SynthPatch::prepare(next, self.sr, self.h.now(), used);
            self.h.synth_set(self.s.slot, patch);
        }
    }

    /// `getSynthOverrides()`.
    pub fn synth_overrides(&self) -> Map<String, Value> {
        self.s.overrides.clone()
    }

    /// `getCurrentPreset()`: the preset with the live edits applied.
    pub fn current_preset(&self) -> SynthPreset {
        resolve_preset(
            self.s.preset_index as f64,
            Some(&Value::Object(self.s.overrides.clone())),
        )
    }

    /// `getPresetIndex()`.
    pub fn preset_index(&self) -> usize {
        self.s.preset_index
    }

    /// `getActiveVoiceCount()`: held + releasing voices of the current instrument, as of the
    /// last rendered quantum.
    pub fn active_voice_count(&self) -> usize {
        match self.status() {
            Some(s) => s.active,
            None => 0,
        }
    }

    /// The current instrument's last published status (None before `start` or before the new
    /// instrument's first quantum: it has no voices yet).
    fn status(&self) -> Option<crate::player::StatusSnapshot> {
        let snap = self.s.status.snapshot();
        (self.s.started && snap.instance == self.s.instance).then_some(snap)
    }

    /// `getHeldNotes()`, as of the last rendered quantum.
    pub fn held_notes(&self) -> Vec<i32> {
        self.status().map(|s| s.held_notes()).unwrap_or_default()
    }

    /// `isPianoReady()`: the piano samples are decoded.
    pub fn is_piano_ready(&self) -> bool {
        self.piano.as_ref().is_some_and(|p| p.get().is_some())
    }

    fn bus(&mut self, op: LiveOp) {
        self.s.state.record(op);
        self.h.live_op(self.s.slot, op);
    }

    /// `mute()` (the live bus; note-offs still reach the instrument).
    pub fn mute(&mut self) {
        self.bus(LiveOp::Mute(true));
    }

    pub fn unmute(&mut self) {
        self.bus(LiveOp::Mute(false));
    }

    /// `setVolume(db)`: 30 ms ramp.
    pub fn set_volume(&mut self, db: f64) {
        self.bus(LiveOp::Volume(db));
    }
}

impl ArrangementEngine {
    /// One `AudioEngine` effect / volume / mute setter.
    pub fn live_fx(&mut self, op: LiveOp) {
        self.live_synth.state.record(op);
        self.h.live_op(self.live_synth.slot, op);
    }

    /// `audioEngine.getEffectParams()`.
    pub fn live_effect_params(&self) -> EffectParams {
        self.live_synth.state.params
    }

    /// `audioEngine.getVolume()`.
    pub fn live_volume(&self) -> f64 {
        self.live_synth.state.volume()
    }

    pub fn live_muted(&self) -> bool {
        self.live_synth.state.muted
    }

    /// The live synth's core slot.
    pub fn live_synth_slot(&self) -> usize {
        self.live_synth.slot
    }

    /// Plug any instrument into the live bus (tests, custom instruments); None removes it.
    /// The app uses [`Self::live`].
    pub fn set_live_instrument(&mut self, instrument: Option<Box<dyn SynthInstrument>>) {
        self.h
            .set_instrument(self.live_synth.slot, instrument.map(TrackInstrument::Synth));
    }

    /// `audioEngine`'s instrument API.
    pub fn live(&mut self) -> LiveSynthApi<'_> {
        LiveSynthApi {
            s: &mut self.live_synth,
            h: &mut self.h,
            piano: &mut self.piano,
            sr: self.sr,
        }
    }

    /// `handleCC`'s legacy fallbacks: a volume CC (5 / 74) goes to the master volume when
    /// `master_volume` (the app's handler), else to the live synth's level; an effect CC passes
    /// the pickup, then drives its live effect. Returns what it did (None: not a legacy CC, or
    /// the pickup held it back).
    pub fn handle_legacy_cc(&mut self, cc: u8, value: u8, master_volume: bool) -> Option<CcAction> {
        let action = cc_action(cc, value)?;
        match action {
            CcAction::Volume(db) => {
                if master_volume {
                    self.set_master_volume_db(db);
                } else {
                    self.live_fx(LiveOp::Volume(db));
                }
            }
            CcAction::Effect(effect, op) => {
                let key = cc.to_string();
                let current = effect_to_normalized(effect, &self.live_synth.state.params);
                if !self
                    .live_synth
                    .pickup
                    .check(&key, value as f64 / 127.0, current)
                {
                    return None;
                }
                self.live_fx(op);
                let now = effect_to_normalized(effect, &self.live_synth.state.params);
                self.live_synth.pickup.applied(&key, now);
            }
        }
        Some(action)
    }

    /// `setMasterVolumeDb(db)`: clamped to −40..0, the master `Volume` ramps to `db + 12` in
    /// 30 ms.
    pub fn set_master_volume_db(&mut self, db: f64) {
        let v = clamp_master_volume(db);
        self.master_volume_db = v;
        self.h.set_master_volume(master_gain_db(v));
    }

    /// `getMasterVolumeDb()`.
    pub fn get_master_volume_db(&self) -> f64 {
        self.master_volume_db
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn master_fader_mapping() {
        assert_eq!(master_gain_db(-12.0), 0.0);
        assert_eq!(master_gain_db(0.0), 12.0);
        assert_eq!(master_gain_db(-60.0), -28.0);
        assert_eq!(clamp_master_volume(f64::NAN), -12.0);
    }
}
