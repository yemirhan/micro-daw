//! The legacy MIDI CC map of the live synth (`CC_MAPPINGS` / `VOLUME_CCS` in src/utils/constants.ts
//! and `handleCC` in src/hooks/useMidiNotes.ts), for controllers the controller profile does not
//! map: CC 1 / 70 cutoff, 2 / 71 resonance, 3 / 72 reverb, 4 / 73 chorus; CC 5 / 74 volume.
//!
//! The pickup (soft takeover) uses [`effect_to_normalized`] with the shared `KnobPickup`; the
//! caller checks it before applying an effect CC (volume CCs have no pickup).

use mdaw_model::types::EffectParams;

use super::live::LiveOp;

/// The live effects a CC can drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CcEffect {
    FilterCutoff,
    FilterResonance,
    ReverbWet,
    ChorusDepth,
}

/// `CC_MAPPINGS`.
pub const CC_MAPPINGS: [(u8, CcEffect, &str); 8] = [
    (1, CcEffect::FilterCutoff, "Filter"),
    (2, CcEffect::FilterResonance, "Resonance"),
    (3, CcEffect::ReverbWet, "Reverb"),
    (4, CcEffect::ChorusDepth, "Chorus"),
    (70, CcEffect::FilterCutoff, "Filter"),
    (71, CcEffect::FilterResonance, "Resonance"),
    (72, CcEffect::ReverbWet, "Reverb"),
    (73, CcEffect::ChorusDepth, "Chorus"),
];

/// `VOLUME_CCS`.
pub const VOLUME_CCS: [u8; 2] = [5, 74];

/// What a CC does (before the pickup check).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CcAction {
    /// A volume CC: `Math.round(−40 + value/127 · 40)` dB (the app routes it to the master
    /// volume; without a handler it is the live synth's level).
    Volume(f64),
    /// An effect CC: the effect (for the pickup) and the setter it applies.
    Effect(CcEffect, LiveOp),
}

/// The effect CC mapped to `cc`.
pub fn cc_effect(cc: u8) -> Option<CcEffect> {
    CC_MAPPINGS.iter().find(|m| m.0 == cc).map(|m| m.1)
}

/// `handleCC`'s legacy fallbacks for `cc` at `value` (0..127).
pub fn cc_action(cc: u8, value: u8) -> Option<CcAction> {
    let v = value as f64;
    if VOLUME_CCS.contains(&cc) {
        return Some(CcAction::Volume(crate::core::js_round(
            -40.0 + (v / 127.0) * 40.0,
        )));
    }
    let effect = cc_effect(cc)?;
    let n = v / 127.0;
    let op = match effect {
        CcEffect::ReverbWet => LiveOp::ReverbWet(n),
        CcEffect::ChorusDepth => LiveOp::ChorusDepth(n),
        // log scale, 60 Hz to 18000 Hz
        CcEffect::FilterCutoff => LiveOp::FilterCutoff(60.0 * (18000.0f64 / 60.0).powf(n)),
        CcEffect::FilterResonance => LiveOp::FilterResonance(n * 20.0),
    };
    Some(CcAction::Effect(effect, op))
}

/// `effectToNormalized` (the pickup's software position of an effect).
pub fn effect_to_normalized(effect: CcEffect, p: &EffectParams) -> f64 {
    match effect {
        CcEffect::ReverbWet => p.reverb_wet,
        CcEffect::ChorusDepth => p.chorus_depth,
        CcEffect::FilterCutoff => (p.filter_cutoff / 60.0).ln() / (18000.0f64 / 60.0).ln(),
        CcEffect::FilterResonance => p.filter_resonance / 20.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_like_handle_cc() {
        assert_eq!(cc_action(5, 127), Some(CcAction::Volume(0.0)));
        assert_eq!(cc_action(74, 0), Some(CcAction::Volume(-40.0)));
        assert_eq!(
            cc_action(5, 64),
            Some(CcAction::Volume(-20.0)),
            "−19.84 rounds"
        );
        assert_eq!(
            cc_action(72, 127),
            Some(CcAction::Effect(
                CcEffect::ReverbWet,
                LiveOp::ReverbWet(1.0)
            ))
        );
        let Some(CcAction::Effect(CcEffect::FilterCutoff, LiveOp::FilterCutoff(hz))) =
            cc_action(1, 127)
        else {
            panic!()
        };
        assert!((hz - 18000.0).abs() < 1e-9);
        let Some(CcAction::Effect(_, LiveOp::FilterCutoff(lo))) = cc_action(70, 0) else {
            panic!()
        };
        assert_eq!(lo, 60.0);
        assert_eq!(
            cc_action(2, 127),
            Some(CcAction::Effect(
                CcEffect::FilterResonance,
                LiveOp::FilterResonance(20.0)
            ))
        );
        assert_eq!(cc_action(10, 64), None);
    }

    #[test]
    fn normalized_positions_invert_the_mapping() {
        let mut p = EffectParams::default();
        assert!((effect_to_normalized(CcEffect::FilterCutoff, &p) - 1.0).abs() < 1e-12);
        p.filter_cutoff = 60.0 * (300f64).powf(0.25);
        assert!((effect_to_normalized(CcEffect::FilterCutoff, &p) - 0.25).abs() < 1e-12);
        p.filter_resonance = 5.0;
        assert_eq!(effect_to_normalized(CcEffect::FilterResonance, &p), 0.25);
    }
}
