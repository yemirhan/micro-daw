//! Instrument output processing (port of `src/audio/CharacterChain.ts`), built only from the
//! stages a preset uses:
//!
//! ```text
//! input → [drive] → [presence shelf] → [tone lowpass] → [tremolo] → [chorus] → Volume(gain dB)
//! ```
//!
//! - **drive** (`drive > 0.001`): `WaveShaper(driveCurve(drive), 2048)`, `oversample = '2x'`
//!   (Chromium's up/down-samplers, [`oversample`]); a drive change re-maps the curve at once.
//! - **presence** (`|presence| > 0.05`): `Tone.Filter` high-shelf at 3.2 kHz, gain = presence dB
//!   (Web Audio highshelf, [`filter`]); later changes ramp exponentially over 50 ms (Tone's
//!   `rampTo` of a decibel Signal).
//! - **tone** (`tone < 19000`): `Tone.Filter` lowpass −12 dB/oct, Q 0.3, `clamp(tone, 200, 20k)`;
//!   frequency changes ramp exponentially over 50 ms.
//! - **tremolo** (`tremolo > 0.001`): `Tone.Tremolo({ frequency: rate, depth, spread: 140 })`;
//!   depth ramps linearly to `clamp(t, 0, 1)`, rate exponentially to `clamp(rate, 0.1, 20)`.
//! - **chorus** (`chorus > 0.001`): `Tone.Chorus({ 0.6 Hz, 3.2 ms, depth .25+.45a, spread 180,
//!   wet .2+.35a })`; depth is set at once, wet ramps linearly ([`stereo`]).
//! - **Volume**: `INSTRUMENT_HEADROOM_DB + gain` dB; `update` ramps it exponentially (in gain)
//!   over 50 ms.
//!
//! A stage that is switched off is removed; switching it on again creates a fresh node (state
//! cleared, LFOs restarted at that time). Every stage is allocated up front, so [`CharacterChain::
//! update`] and [`CharacterChain::process`] never allocate (the audio thread may call them).
//! Mono instruments: the chain runs both channels with identical input, which equals Web Audio's
//! mono path followed by the destination's up-mix.

pub mod filter;
pub mod lfo;
pub mod oversample;
pub mod param;
pub mod stereo;

#[cfg(test)]
pub(crate) mod parity_tests;
#[cfg(test)]
mod tests;

use filter::{FilterKind, ToneFilter};
use oversample::Oversampled;
use param::{Automated, Curve};
use stereo::{Chorus, RAMP, Tremolo};

/// `INSTRUMENT_HEADROOM_DB`: common output trim (dB).
pub const INSTRUMENT_HEADROOM_DB: f64 = 0.0;
/// Drive curve length.
pub const DRIVE_CURVE_LEN: usize = 2048;
/// Presence shelf frequency (Hz).
pub const PRESENCE_HZ: f64 = 3200.0;
/// Tone lowpass Q.
pub const TONE_Q: f64 = 0.3;

/// `CharacterParams` (`src/types/audio.ts`).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CharacterParams {
    /// 0..1 soft saturation.
    pub drive: f64,
    /// 0..1 stereo chorus amount.
    pub chorus: f64,
    /// Tremolo depth 0..1.
    pub tremolo: f64,
    /// Tremolo rate (Hz).
    pub tremolo_rate: f64,
    /// Output lowpass (Hz, 20000 = open).
    pub tone: f64,
    /// High-shelf (3.2 kHz) gain in dB.
    pub presence: f64,
}

impl Default for CharacterParams {
    /// `DEFAULT_CHARACTER` (`withCharacterDefaults` of a missing / partial object).
    fn default() -> Self {
        Self {
            drive: 0.0,
            chorus: 0.0,
            tremolo: 0.0,
            tremolo_rate: 4.5,
            tone: 20000.0,
            presence: 0.0,
        }
    }
}

fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    hi.min(lo.max(v))
}

/// `dbToGain`.
pub fn db_to_gain(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

/// `driveCurve(drive)`: `tanh(kx)·0.35/tanh(0.35k)` clamped to ±1, `k = 1 + 9·clamp(drive, 0, 1)`.
pub fn drive_curve(drive: f64) -> impl Fn(f64) -> f64 {
    let k = 1.0 + clamp(drive, 0.0, 1.0) * 9.0;
    let reference = 0.35;
    let norm = reference / (k * reference).tanh();
    move |x| clamp((k * x).tanh() * norm, -1.0, 1.0)
}

/// Fill `out` like `WaveShaper.setMap(driveCurve(drive), out.len())`, without allocating.
pub fn fill_drive_curve(drive: f64, out: &mut [f32]) {
    let f = drive_curve(drive);
    let last = (out.len().max(2) - 1) as f64;
    for (i, v) in out.iter_mut().enumerate() {
        *v = f((i as f64 / last) * 2.0 - 1.0) as f32;
    }
}

struct Drive {
    on: bool,
    amount: f64,
    curve: Box<[f32; DRIVE_CURVE_LEN]>,
    shaper: [Oversampled; 2],
}

/// The chain (see the module docs).
pub struct CharacterChain {
    sr: u32,
    c: CharacterParams,
    volume: Automated,
    drive: Drive,
    presence_on: bool,
    presence_f: ToneFilter,
    tone_on: bool,
    tone_f: ToneFilter,
    tremolo_on: bool,
    tremolo: Tremolo,
    chorus_on: bool,
    chorus: Chorus,
}

impl CharacterChain {
    /// `new CharacterChain(character, gainDb)` created at context time `now` (LFOs start then).
    pub fn new(sr: u32, character: CharacterParams, gain_db: f64, now: f64) -> Self {
        let start = start_frame(now, sr);
        let mut chain = Self {
            sr,
            c: character,
            volume: Automated::new(db_to_gain(INSTRUMENT_HEADROOM_DB + gain_db)),
            drive: Drive {
                on: false,
                amount: 0.0,
                curve: Box::new([0.0; DRIVE_CURVE_LEN]),
                shaper: [Oversampled::new(), Oversampled::new()],
            },
            presence_on: false,
            presence_f: ToneFilter::new(FilterKind::HighShelf, PRESENCE_HZ, 1.0, 0.0, sr),
            tone_on: false,
            tone_f: ToneFilter::new(FilterKind::Lowpass, 20000.0, TONE_Q, 0.0, sr),
            tremolo_on: false,
            tremolo: Tremolo::new(sr, character.tremolo_rate, character.tremolo, start),
            chorus_on: false,
            chorus: Chorus::new(sr, 0.25, 0.2, start),
        };
        chain.sync(now);
        chain
    }

    /// The current parameters.
    pub fn params(&self) -> CharacterParams {
        self.c
    }

    /// `update(character, gainDb)` at context time `now`.
    pub fn update(&mut self, character: CharacterParams, gain_db: f64, now: f64) {
        self.c = character;
        self.volume.ramp_to(
            db_to_gain(INSTRUMENT_HEADROOM_DB + gain_db),
            RAMP,
            now,
            Curve::Exponential,
        );
        self.sync(now);
    }

    /// Which stages are in the signal path: `[drive, presence, tone, tremolo, chorus]`.
    pub fn stages(&self) -> [bool; 5] {
        [
            self.drive.on,
            self.presence_on,
            self.tone_on,
            self.tremolo_on,
            self.chorus_on,
        ]
    }

    fn sync(&mut self, now: f64) {
        let c = self.c;
        let start = start_frame(now, self.sr);
        if c.drive > 0.001 {
            if !self.drive.on {
                self.drive.on = true;
                for s in &mut self.drive.shaper {
                    s.reset();
                }
                fill_drive_curve(c.drive, &mut self.drive.curve[..]);
            } else if (c.drive - self.drive.amount).abs() > 1e-4 {
                fill_drive_curve(c.drive, &mut self.drive.curve[..]);
            }
            self.drive.amount = c.drive;
        } else {
            self.drive.on = false;
        }

        if c.presence.abs() > 0.05 {
            if !self.presence_on {
                self.presence_on = true;
                self.presence_f.reset(PRESENCE_HZ, c.presence);
            } else {
                self.presence_f
                    .gain
                    .ramp_to(c.presence, RAMP, now, Curve::Exponential);
            }
        } else {
            self.presence_on = false;
        }

        if c.tone < 19000.0 {
            let f = clamp(c.tone, 200.0, 20000.0);
            if !self.tone_on {
                self.tone_on = true;
                self.tone_f.reset(f, 0.0);
            } else {
                self.tone_f
                    .frequency
                    .ramp_to(f, RAMP, now, Curve::Exponential);
            }
        } else {
            self.tone_on = false;
        }

        if c.tremolo > 0.001 {
            if !self.tremolo_on {
                self.tremolo_on = true;
                self.tremolo.reset(c.tremolo_rate, c.tremolo, start);
            } else {
                self.tremolo.ramp(
                    clamp(c.tremolo, 0.0, 1.0),
                    clamp(c.tremolo_rate, 0.1, 20.0),
                    now,
                );
            }
        } else {
            self.tremolo_on = false;
        }

        if c.chorus > 0.001 {
            let amt = clamp(c.chorus, 0.0, 1.0);
            if !self.chorus_on {
                self.chorus_on = true;
                self.chorus
                    .reset(0.25 + 0.45 * amt, 0.2 + 0.35 * amt, start);
            } else {
                self.chorus.set_depth(0.25 + 0.45 * amt);
                self.chorus
                    .wet
                    .ramp_to(0.2 + 0.35 * amt, RAMP, now, Curve::Linear);
            }
        } else {
            self.chorus_on = false;
        }
    }

    /// Process `l` / `r` in place (≤ 128 frames, starting at context frame `frame`). Does not
    /// allocate.
    pub fn process(&mut self, frame: u64, l: &mut [f32], r: &mut [f32]) {
        let n = l.len().min(r.len());
        let (l, r) = (&mut l[..n], &mut r[..n]);
        let sr = self.sr as f64;
        if self.drive.on {
            let curve = &self.drive.curve[..];
            self.drive.shaper[0].process(curve, l);
            self.drive.shaper[1].process(curve, r);
        }
        if self.presence_on {
            self.presence_f.process(frame, sr, l, r);
        }
        if self.tone_on {
            self.tone_f.process(frame, sr, l, r);
        }
        if self.tremolo_on {
            self.tremolo.process(frame, sr, l, r);
        }
        if self.chorus_on {
            self.chorus.process(frame, sr, l, r);
        }
        for i in 0..n {
            let g = self.volume.at((frame + i as u64) as f64 / sr);
            l[i] *= g;
            r[i] *= g;
        }
    }
}

/// The frame a source started at context time `t` begins on (Chromium rounds up).
fn start_frame(t: f64, sr: u32) -> u64 {
    let f = (t * sr as f64 * 1024.0).round() / 1024.0;
    f.ceil().max(0.0) as u64
}
