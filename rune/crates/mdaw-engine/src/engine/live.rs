//! The live synth bus (`AudioEngine`'s effects, volume and mute; `fx/live.rs`), its MIDI CC
//! fallbacks (`handleCC`, `fx/cc.rs`) and the master fader (`setMasterVolumeDb`, masterBus.ts).
//!
//! The bus lives in its own core slot (after drumEngine's fallback kit), built when the engine
//! is (≙ `audioEngine.start()`). The live instrument (presets, voices) is plugged into that slot
//! with [`ArrangementEngine::set_live_instrument`].
//!
//! Not ported here: the master volume's localStorage persistence (`micro-daw:masterVolume`) is
//! the settings store's job (the app reads it and calls [`ArrangementEngine::set_master_volume_db`]).

use mdaw_model::knobs::KnobPickup;
use mdaw_model::types::EffectParams;

use super::ArrangementEngine;
use crate::core::{CoreHandle, SlotFx, SynthInstrument, TrackInstrument};
use crate::fx::cc::{CcAction, cc_action, effect_to_normalized};
use crate::fx::live::{LiveFx, LiveFxState, LiveOp};

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

/// The live bus's slot and the params `getEffectParams` returns.
pub(super) struct LiveSynth {
    pub slot: usize,
    pub state: LiveFxState,
    /// `ccPickup` of `useMidiNotes` (soft takeover of the effect CCs).
    pub pickup: KnobPickup,
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
        }
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

    /// Plug the live instrument (≙ `createInstrument`) into the live bus; None removes it.
    pub fn set_live_instrument(&mut self, instrument: Option<Box<dyn SynthInstrument>>) {
        self.h
            .set_instrument(self.live_synth.slot, instrument.map(TrackInstrument::Synth));
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
