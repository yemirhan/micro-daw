//! The Web Audio / Tone.js building blocks the synth voices are made of, reproduced the way
//! Chromium renders them (128-frame quanta, f32 signals, f64 where Chromium computes in double):
//!
//! - [`param`]: `Tone.Param` over a native AudioParam ([`tone_tl`] = Tone's JS mirror,
//!   [`native_tl`] = Chromium's timeline, incl. `setTargetAtTime`);
//! - [`envelope`]: `Tone.Envelope` / `AmplitudeEnvelope`;
//! - [`wave`] + [`osc`]: `PeriodicWave` tables and `OscillatorNode`;
//! - [`source`]: `Tone.Source` start/stop state and the `OneShotSource` gain gate;
//! - [`noise`]: `Tone.Noise` buffers and their looping buffer source;
//! - [`biquad`]: `Tone.Filter` (cascaded a-rate `BiquadFilterNode`s).

pub(crate) mod biquad;
pub(crate) mod envelope;
pub(crate) mod native_tl;
pub(crate) mod noise;
pub(crate) mod osc;
pub(crate) mod param;
pub(crate) mod source;
pub(crate) mod tone_tl;
pub(crate) mod wave;

/// Tone `dbToGain(gainToDb(g))` as the app sets volumes (`volume.value = gainToDb(g)` with the
/// app's `20·log10`, converted back by Tone's `10^(db/20)`): the gain value Tone schedules.
pub(crate) fn volume_gain_f64(g: f64) -> f64 {
    if g <= 0.0 {
        return 0.0;
    }
    let db = 20.0 * g.log10();
    10f64.powf(db / 20.0)
}

/// Tone `dbToGain(db)`.
pub(crate) fn db_to_gain(db: f64) -> f32 {
    10f64.powf(db / 20.0) as f32
}
