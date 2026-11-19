//! Designed sounds: the editable recipe behind a sound made in the Sound Designer (the Samples
//! view). A recipe is a small, plain parameter set (no strings, no heap) so the audio thread can
//! copy it; `mdaw_dsp::sound_design` renders it. A [`DesignedSound`] (id + name + recipe) is
//! stored in the project (`Arrangement::sounds`) and in the user's sound library
//! ([`crate::settings::read_sound_library`]); pads point at it with
//! `PadSampleSource::Sound { sound_id }`.
//!
//! Every field has a default, so a recipe written by an older or newer build still loads; values
//! are clamped by [`SoundRecipe::sanitized`] before they reach the DSP.

use serde::{Deserialize, Serialize};

/// Oscillator layers per sound.
pub const LAYERS: usize = 3;

/// A layer's waveform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Wave {
    #[default]
    Sine,
    Triangle,
    Saw,
    /// Square / pulse (see [`OscLayer::width`]).
    Pulse,
    /// White noise.
    Noise,
    /// Pink noise (−3 dB / octave).
    Pink,
}

impl Wave {
    pub const ALL: [Wave; 6] = [
        Wave::Sine,
        Wave::Triangle,
        Wave::Saw,
        Wave::Pulse,
        Wave::Noise,
        Wave::Pink,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Wave::Sine => "Sine",
            Wave::Triangle => "Tri",
            Wave::Saw => "Saw",
            Wave::Pulse => "Pulse",
            Wave::Noise => "White",
            Wave::Pink => "Pink",
        }
    }

    pub fn is_noise(self) -> bool {
        matches!(self, Wave::Noise | Wave::Pink)
    }
}

/// One oscillator layer.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OscLayer {
    pub on: bool,
    pub wave: Wave,
    /// 0..1
    pub level: f64,
    /// Semitones, −48..48.
    pub coarse: f64,
    /// Cents, −100..100.
    pub fine: f64,
    /// Pulse width, 0.05..0.95 (pulse only).
    pub width: f64,
    /// Pitch envelope start offset in semitones (−48..48), decaying to 0.
    pub pitch_amount: f64,
    /// Seconds for the pitch envelope to fall to ~1 % of its offset (0.001..2).
    pub pitch_decay: f64,
}

impl Default for OscLayer {
    fn default() -> Self {
        Self {
            on: false,
            wave: Wave::Sine,
            level: 0.8,
            coarse: 0.0,
            fine: 0.0,
            width: 0.5,
            pitch_amount: 0.0,
            pitch_decay: 0.05,
        }
    }
}

/// Phase modulation of one layer by another (`from` → `to`, 0-based layer indexes).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FmRoute {
    pub from: u8,
    pub to: u8,
    /// 0..1 (0 = off): the modulation index, up to [`FM_MAX_INDEX`] radians.
    pub amount: f64,
}

/// The phase deviation at FM amount 1.
pub const FM_MAX_INDEX: f64 = 8.0;

impl Default for FmRoute {
    fn default() -> Self {
        Self {
            from: 1,
            to: 0,
            amount: 0.0,
        }
    }
}

/// The amplitude envelope: attack, hold, decay to the sustain level, release at the end of the
/// sound (a one-shot: the note is released `release` seconds before [`SoundRecipe::length`]).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AmpEnv {
    /// Seconds (0..2).
    pub attack: f64,
    /// Seconds (0..2).
    pub hold: f64,
    /// Seconds (0.001..4).
    pub decay: f64,
    /// 0..1
    pub sustain: f64,
    /// Seconds (0.001..4).
    pub release: f64,
    /// Segment shape, −1..1: 0 linear, > 0 fast-then-slow (punchy), < 0 slow-then-fast.
    pub curve: f64,
    /// Extra attacks before the main one (0..4), 11 ms apart: hand claps.
    pub retrig: u8,
}

/// Spacing of [`AmpEnv::retrig`] attacks.
pub const RETRIG_SPACING: f64 = 0.011;

impl Default for AmpEnv {
    fn default() -> Self {
        Self {
            attack: 0.001,
            hold: 0.0,
            decay: 0.3,
            sustain: 0.0,
            release: 0.05,
            curve: 0.5,
            retrig: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FilterMode {
    #[default]
    Lowpass,
    Highpass,
    Bandpass,
}

impl FilterMode {
    pub const ALL: [FilterMode; 3] = [
        FilterMode::Lowpass,
        FilterMode::Highpass,
        FilterMode::Bandpass,
    ];
    pub fn label(self) -> &'static str {
        match self {
            FilterMode::Lowpass => "LP",
            FilterMode::Highpass => "HP",
            FilterMode::Bandpass => "BP",
        }
    }
}

/// A state-variable filter with its own decaying envelope.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FilterParams {
    pub mode: FilterMode,
    /// Hz, 20..20000.
    pub cutoff: f64,
    /// 0..1
    pub resonance: f64,
    /// Octaves added to the cutoff at the start, decaying to 0 (−8..8).
    pub env_amount: f64,
    /// Seconds for the filter envelope to fall to ~1 % (0.001..4).
    pub env_decay: f64,
}

impl Default for FilterParams {
    fn default() -> Self {
        Self {
            mode: FilterMode::Lowpass,
            cutoff: 20000.0,
            resonance: 0.0,
            env_amount: 0.0,
            env_decay: 0.2,
        }
    }
}

/// The whole sound.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SoundRecipe {
    /// The pitch layers are tuned from (MIDI note, 12..108). C4 (60) on a keyboard plays it as
    /// designed; other keys transpose.
    pub note: f64,
    /// Rendered length in seconds (0.02..4).
    pub length: f64,
    /// Cut the silent tail of the render (below −60 dBFS), with a short fade.
    pub trim_tail: bool,
    /// Output gain, dB (−24..12).
    pub gain_db: f64,
    pub layers: [OscLayer; LAYERS],
    pub fm: FmRoute,
    pub amp: AmpEnv,
    pub filter: FilterParams,
    /// Soft-clip drive, 0..1.
    pub drive: f64,
    /// Bit depth of the bit crusher (0 = off, else 2..16).
    pub crush: f64,
    /// Level of a short noise click at the start (0..1).
    pub click: f64,
}

impl Default for SoundRecipe {
    fn default() -> Self {
        let mut layers = [OscLayer::default(); LAYERS];
        layers[0].on = true;
        Self {
            note: 60.0,
            length: 0.6,
            trim_tail: true,
            gain_db: 0.0,
            layers,
            fm: FmRoute::default(),
            amp: AmpEnv::default(),
            filter: FilterParams::default(),
            drive: 0.0,
            crush: 0.0,
            click: 0.0,
        }
    }
}

/// `v` clamped into `[lo, hi]`; NaN / ±∞ → `d`.
fn c(v: f64, lo: f64, hi: f64, d: f64) -> f64 {
    if v.is_finite() { v.clamp(lo, hi) } else { d }
}

impl SoundRecipe {
    /// Every value clamped to its range (non-finite values take the default).
    pub fn sanitized(&self) -> SoundRecipe {
        let d = SoundRecipe::default();
        let dl = OscLayer::default();
        let mut r = *self;
        r.note = c(r.note, 12.0, 108.0, d.note);
        r.length = c(r.length, 0.02, 4.0, d.length);
        r.gain_db = c(r.gain_db, -24.0, 12.0, 0.0);
        for l in &mut r.layers {
            l.level = c(l.level, 0.0, 1.0, dl.level);
            l.coarse = c(l.coarse, -48.0, 48.0, 0.0);
            l.fine = c(l.fine, -100.0, 100.0, 0.0);
            l.width = c(l.width, 0.05, 0.95, 0.5);
            l.pitch_amount = c(l.pitch_amount, -48.0, 48.0, 0.0);
            l.pitch_decay = c(l.pitch_decay, 0.001, 2.0, dl.pitch_decay);
        }
        r.fm.from = r.fm.from.min(LAYERS as u8 - 1);
        r.fm.to = r.fm.to.min(LAYERS as u8 - 1);
        r.fm.amount = c(r.fm.amount, 0.0, 1.0, 0.0);
        let a = &mut r.amp;
        a.attack = c(a.attack, 0.0, 2.0, 0.001);
        a.hold = c(a.hold, 0.0, 2.0, 0.0);
        a.decay = c(a.decay, 0.001, 4.0, d.amp.decay);
        a.sustain = c(a.sustain, 0.0, 1.0, 0.0);
        a.release = c(a.release, 0.001, 4.0, d.amp.release);
        a.curve = c(a.curve, -1.0, 1.0, 0.0);
        a.retrig = a.retrig.min(4);
        let f = &mut r.filter;
        f.cutoff = c(f.cutoff, 20.0, 20000.0, 20000.0);
        f.resonance = c(f.resonance, 0.0, 1.0, 0.0);
        f.env_amount = c(f.env_amount, -8.0, 8.0, 0.0);
        f.env_decay = c(f.env_decay, 0.001, 4.0, d.filter.env_decay);
        r.drive = c(r.drive, 0.0, 1.0, 0.0);
        r.crush = if r.crush.is_finite() && r.crush >= 2.0 {
            r.crush.min(16.0).round()
        } else {
            0.0
        };
        r.click = c(r.click, 0.0, 1.0, 0.0);
        r
    }
}

/// Non-destructive edits of an imported audio file (a sample-based sound).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SampleEdit {
    /// The source file (absolute path).
    pub path: String,
    /// Trim start, seconds.
    pub start: f64,
    /// Trim end, seconds (None = the end of the file).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<f64>,
    /// Fade in / out, seconds.
    pub fade_in: f64,
    pub fade_out: f64,
    /// dB, −24..24 (after normalize).
    pub gain_db: f64,
    /// Scale the peak to 0 dBFS (before the gain).
    pub normalize: bool,
    pub reverse: bool,
}

impl Default for SampleEdit {
    fn default() -> Self {
        Self {
            path: String::new(),
            start: 0.0,
            end: None,
            fade_in: 0.0,
            fade_out: 0.0,
            gain_db: 0.0,
            normalize: false,
            reverse: false,
        }
    }
}

impl SampleEdit {
    pub fn new(path: &str) -> Self {
        Self {
            path: path.to_owned(),
            ..Self::default()
        }
    }

    /// Values clamped (`duration` = the file's length in seconds).
    pub fn sanitized(&self, duration: f64) -> SampleEdit {
        let mut e = self.clone();
        let dur = if duration.is_finite() {
            duration.max(0.0)
        } else {
            0.0
        };
        e.start = c(e.start, 0.0, dur, 0.0);
        e.end = e
            .end
            .filter(|x| x.is_finite())
            .map(|x| x.clamp(e.start, dur));
        let len = e.end.unwrap_or(dur) - e.start;
        e.fade_in = c(e.fade_in, 0.0, len, 0.0);
        e.fade_out = c(e.fade_out, 0.0, len, 0.0);
        e.gain_db = c(e.gain_db, -24.0, 24.0, 0.0);
        e
    }

    /// Nothing changes the file's audio.
    pub fn is_identity(&self) -> bool {
        self.start == 0.0
            && self.end.is_none()
            && self.fade_in == 0.0
            && self.fade_out == 0.0
            && self.gain_db == 0.0
            && !self.normalize
            && !self.reverse
    }
}

/// A sound made in the Sound Designer: synthesized from `recipe`, or, with `sample`, an
/// imported file with non-destructive edits (the recipe is then unused).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesignedSound {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub recipe: SoundRecipe,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample: Option<SampleEdit>,
}

impl DesignedSound {
    pub fn is_sample(&self) -> bool {
        self.sample.is_some()
    }
}

/// The starting points of the Sound Designer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Template {
    Kick808,
    PunchyKick,
    Snare,
    Clap,
    ClosedHat,
    OpenHat,
    Tom,
    Zap,
    BassPluck,
    Sub,
}

impl Template {
    pub const ALL: [Template; 10] = [
        Template::Kick808,
        Template::PunchyKick,
        Template::Snare,
        Template::Clap,
        Template::ClosedHat,
        Template::OpenHat,
        Template::Tom,
        Template::Zap,
        Template::BassPluck,
        Template::Sub,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Template::Kick808 => "Kick 808",
            Template::PunchyKick => "Punchy Kick",
            Template::Snare => "Snare",
            Template::Clap => "Clap",
            Template::ClosedHat => "Closed Hat",
            Template::OpenHat => "Open Hat",
            Template::Tom => "Tom",
            Template::Zap => "Zap",
            Template::BassPluck => "Bass Pluck",
            Template::Sub => "Sub",
        }
    }

    /// The template's recipe.
    pub fn recipe(self) -> SoundRecipe {
        let base = SoundRecipe::default();
        let osc = |wave: Wave, level: f64| OscLayer {
            on: true,
            wave,
            level,
            ..OscLayer::default()
        };
        let off = OscLayer::default();
        let amp = |attack: f64, decay: f64, release: f64, curve: f64| AmpEnv {
            attack,
            hold: 0.0,
            decay,
            sustain: 0.0,
            release,
            curve,
            retrig: 0,
        };
        let filter = |mode: FilterMode, cutoff: f64, resonance: f64| FilterParams {
            mode,
            cutoff,
            resonance,
            env_amount: 0.0,
            env_decay: 0.2,
        };
        match self {
            Template::Kick808 => SoundRecipe {
                note: 33.0, // A1, 55 Hz
                length: 1.4,
                layers: [
                    OscLayer {
                        pitch_amount: 24.0,
                        pitch_decay: 0.06,
                        ..osc(Wave::Sine, 1.0)
                    },
                    off,
                    off,
                ],
                amp: AmpEnv {
                    hold: 0.02,
                    ..amp(0.001, 1.1, 0.15, 0.6)
                },
                drive: 0.15,
                click: 0.15,
                ..base
            },
            Template::PunchyKick => SoundRecipe {
                note: 36.0,
                length: 0.5,
                layers: [
                    OscLayer {
                        pitch_amount: 36.0,
                        pitch_decay: 0.035,
                        ..osc(Wave::Sine, 1.0)
                    },
                    OscLayer {
                        coarse: 12.0,
                        pitch_amount: 24.0,
                        pitch_decay: 0.02,
                        ..osc(Wave::Triangle, 0.35)
                    },
                    off,
                ],
                amp: amp(0.0005, 0.35, 0.06, 0.8),
                filter: filter(FilterMode::Lowpass, 4000.0, 0.1),
                drive: 0.45,
                click: 0.35,
                ..base
            },
            Template::Snare => SoundRecipe {
                note: 55.0, // G3 body
                length: 0.4,
                layers: [
                    OscLayer {
                        pitch_amount: 7.0,
                        pitch_decay: 0.04,
                        ..osc(Wave::Triangle, 0.6)
                    },
                    osc(Wave::Noise, 0.75),
                    off,
                ],
                amp: amp(0.0005, 0.2, 0.08, 0.7),
                filter: FilterParams {
                    env_amount: 1.0,
                    env_decay: 0.1,
                    ..filter(FilterMode::Highpass, 180.0, 0.15)
                },
                drive: 0.2,
                click: 0.2,
                ..base
            },
            Template::Clap => SoundRecipe {
                length: 0.45,
                layers: [osc(Wave::Noise, 1.0), off, off],
                amp: AmpEnv {
                    retrig: 3,
                    ..amp(0.0005, 0.18, 0.08, 0.8)
                },
                filter: filter(FilterMode::Bandpass, 1300.0, 0.35),
                drive: 0.1,
                gain_db: 4.0,
                ..base
            },
            Template::ClosedHat => SoundRecipe {
                length: 0.12,
                layers: [
                    osc(Wave::Noise, 0.9),
                    OscLayer {
                        coarse: 31.0,
                        width: 0.3,
                        ..osc(Wave::Pulse, 0.25)
                    },
                    off,
                ],
                amp: amp(0.0005, 0.05, 0.03, 0.8),
                filter: filter(FilterMode::Highpass, 7500.0, 0.2),
                ..base
            },
            Template::OpenHat => SoundRecipe {
                length: 0.7,
                layers: [
                    osc(Wave::Noise, 0.9),
                    OscLayer {
                        coarse: 31.0,
                        width: 0.3,
                        ..osc(Wave::Pulse, 0.25)
                    },
                    off,
                ],
                amp: amp(0.001, 0.45, 0.15, 0.4),
                filter: filter(FilterMode::Highpass, 6500.0, 0.25),
                ..base
            },
            Template::Tom => SoundRecipe {
                note: 45.0, // A2
                length: 0.7,
                layers: [
                    OscLayer {
                        pitch_amount: 12.0,
                        pitch_decay: 0.15,
                        ..osc(Wave::Sine, 1.0)
                    },
                    OscLayer {
                        coarse: 7.0,
                        pitch_amount: 12.0,
                        pitch_decay: 0.15,
                        ..osc(Wave::Triangle, 0.25)
                    },
                    off,
                ],
                amp: amp(0.001, 0.5, 0.1, 0.6),
                click: 0.2,
                drive: 0.1,
                ..base
            },
            Template::Zap => SoundRecipe {
                note: 48.0,
                length: 0.35,
                layers: [
                    OscLayer {
                        pitch_amount: 48.0,
                        pitch_decay: 0.18,
                        ..osc(Wave::Saw, 0.8)
                    },
                    OscLayer {
                        coarse: 7.0,
                        pitch_amount: 48.0,
                        pitch_decay: 0.18,
                        ..osc(Wave::Sine, 0.5)
                    },
                    off,
                ],
                fm: FmRoute {
                    from: 1,
                    to: 0,
                    amount: 0.25,
                },
                amp: amp(0.001, 0.25, 0.08, 0.5),
                filter: FilterParams {
                    env_amount: 3.0,
                    env_decay: 0.2,
                    ..filter(FilterMode::Lowpass, 1500.0, 0.5)
                },
                ..base
            },
            Template::BassPluck => SoundRecipe {
                note: 36.0,
                length: 0.8,
                layers: [
                    osc(Wave::Saw, 0.8),
                    OscLayer {
                        width: 0.35,
                        fine: 8.0,
                        ..osc(Wave::Pulse, 0.5)
                    },
                    OscLayer {
                        coarse: -12.0,
                        ..osc(Wave::Sine, 0.6)
                    },
                ],
                amp: AmpEnv {
                    sustain: 0.25,
                    ..amp(0.002, 0.3, 0.12, 0.5)
                },
                filter: FilterParams {
                    env_amount: 4.0,
                    env_decay: 0.25,
                    ..filter(FilterMode::Lowpass, 300.0, 0.45)
                },
                drive: 0.25,
                ..base
            },
            Template::Sub => SoundRecipe {
                note: 36.0,
                length: 1.2,
                layers: [
                    osc(Wave::Sine, 1.0),
                    OscLayer {
                        coarse: 12.0,
                        ..osc(Wave::Triangle, 0.12)
                    },
                    off,
                ],
                amp: AmpEnv {
                    sustain: 0.8,
                    ..amp(0.005, 0.3, 0.25, 0.0)
                },
                drive: 0.1,
                ..base
            },
        }
    }
}

/// Note name of a MIDI note (`C4` = 60).
pub fn note_name(note: f64) -> String {
    crate::notes::midi_to_note_name(note.round())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipe_round_trips_through_json() {
        for t in Template::ALL {
            let s = DesignedSound {
                id: "x".into(),
                name: t.name().into(),
                recipe: t.recipe(),
                sample: None,
            };
            let json = serde_json::to_string(&s).unwrap();
            let back: DesignedSound = serde_json::from_str(&json).unwrap();
            assert_eq!(back, s, "{}", t.name());
        }
    }

    #[test]
    fn partial_recipe_takes_defaults() {
        let s: DesignedSound =
            serde_json::from_str(r#"{"id":"a","recipe":{"length":0.3,"amp":{"decay":0.1}}}"#)
                .unwrap();
        assert_eq!(s.recipe.length, 0.3);
        assert_eq!(s.recipe.amp.decay, 0.1);
        assert_eq!(s.recipe.amp.release, AmpEnv::default().release);
        assert_eq!(s.recipe.layers, SoundRecipe::default().layers);
        assert_eq!(s.name, "");
        assert_eq!(s.sample, None);
    }

    #[test]
    fn sample_edits_round_trip_and_clamp() {
        let s = DesignedSound {
            id: "f".into(),
            name: "Kick".into(),
            recipe: SoundRecipe::default(),
            sample: Some(SampleEdit {
                end: Some(0.5),
                fade_out: 0.1,
                reverse: true,
                ..SampleEdit::new("/x/kick.wav")
            }),
        };
        let back: DesignedSound =
            serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        let e = SampleEdit {
            start: 2.0,
            end: Some(9.0),
            fade_in: 5.0,
            gain_db: f64::NAN,
            ..SampleEdit::new("p")
        }
        .sanitized(1.0);
        assert_eq!(
            (e.start, e.end, e.fade_in, e.gain_db),
            (1.0, Some(1.0), 0.0, 0.0)
        );
        assert!(SampleEdit::new("p").is_identity());
    }

    #[test]
    fn sanitized_clamps_and_replaces_non_finite() {
        let mut r = SoundRecipe {
            length: 99.0,
            note: f64::NAN,
            crush: 1.0,
            ..SoundRecipe::default()
        };
        r.layers[1].level = -3.0;
        r.fm.from = 9;
        r.amp.retrig = 200;
        let s = r.sanitized();
        assert_eq!(s.length, 4.0);
        assert_eq!(s.note, 60.0);
        assert_eq!(s.layers[1].level, 0.0);
        assert_eq!(s.fm.from, 2);
        assert_eq!(s.crush, 0.0);
        assert_eq!(s.amp.retrig, 4);
        for t in Template::ALL {
            assert_eq!(t.recipe().sanitized(), t.recipe(), "{}", t.name());
        }
    }
}
