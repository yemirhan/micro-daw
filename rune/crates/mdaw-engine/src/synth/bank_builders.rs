//! The `presets.ts` builders (`sub`, `fm`, `pluck`, `sampler`, `osc`, `drawbars`): each fills the
//! engine defaults, then the preset's edit overrides fields one by one (the TS object spreads).

use super::presets::*;

pub(super) type Meta = (&'static str, &'static str, PresetCategory, PresetVibe);

pub(super) fn meta(m: Meta) -> PresetMeta {
    PresetMeta {
        id: m.0.to_string(),
        name: m.1.to_string(),
        category: m.2,
        vibe: m.3,
        description: None,
        gain: 0.0,
        polyphony: 0.0,
        character: CharacterParams::default(),
    }
}

/// `osc(wave, level)` (semi 0, fine 0, unison 1, spread 0).
pub(super) fn osc(wave: OscWave, level: f64) -> OscParams {
    OscParams {
        wave,
        level,
        semi: 0.0,
        fine: 0.0,
        unison: 1.0,
        spread: 0.0,
        width: None,
        partials: None,
    }
}

impl OscParams {
    pub(super) fn unison(mut self, unison: f64, spread: f64) -> Self {
        self.unison = unison;
        self.spread = spread;
        self
    }
    pub(super) fn semi(mut self, semi: f64) -> Self {
        self.semi = semi;
        self
    }
    pub(super) fn fine(mut self, fine: f64) -> Self {
        self.fine = fine;
        self
    }
    pub(super) fn width(mut self, width: f64) -> Self {
        self.width = Some(width);
        self
    }
    pub(super) fn partials(mut self, partials: Vec<f64>) -> Self {
        self.partials = Some(partials);
        self
    }
}

pub(super) fn adsr(attack: f64, decay: f64, sustain: f64, release: f64) -> Adsr {
    Adsr {
        attack,
        decay,
        sustain,
        release,
    }
}

pub(super) fn amp(attack: f64, decay: f64, sustain: f64, release: f64, velocity: f64) -> AmpParams {
    AmpParams {
        attack,
        decay,
        sustain,
        release,
        velocity,
    }
}

/// Filter override: (cutoff, resonance, keyTrack, envAmount, velocity), rolloff / type default.
pub(super) fn filt(
    f: &mut VoiceFilterParams,
    cutoff: f64,
    resonance: f64,
    key_track: f64,
    env_amount: f64,
    velocity: f64,
) {
    f.cutoff = cutoff;
    f.resonance = resonance;
    f.key_track = key_track;
    f.env_amount = env_amount;
    f.velocity = velocity;
}

pub(super) fn lfo(rate: f64, vibrato: f64, filter: f64, delay: f64) -> LfoParams {
    LfoParams {
        rate,
        vibrato,
        filter,
        delay,
    }
}

pub(super) fn sub(m: Meta, edit: impl FnOnce(&mut SubtractivePreset)) -> SynthPreset {
    let mut meta = meta(m);
    meta.polyphony = 12.0;
    let mut p = SubtractivePreset {
        meta,
        osc_a: osc(OscWave::Sawtooth, 1.0),
        osc_b: osc(OscWave::Sawtooth, 0.0),
        sub: SubOscParams {
            level: 0.0,
            wave: SubWave::Sine,
            octave: -1.0,
        },
        noise: NoiseParams {
            level: 0.0,
            kind: NoiseType::White,
            decay: 0.0,
        },
        filter: VoiceFilterParams {
            kind: FilterType::Lowpass,
            cutoff: 2000.0,
            resonance: 1.0,
            rolloff: -24.0,
            key_track: 0.5,
            env_amount: 2.0,
            velocity: 0.5,
        },
        filter_env: adsr(0.005, 0.4, 0.3, 0.4),
        amp: amp(0.004, 0.3, 0.8, 0.3, 0.7),
        pitch_env: PitchEnvParams {
            amount: 0.0,
            decay: 0.0,
        },
        lfo: lfo(5.0, 0.0, 0.0, 0.0),
        mono: false,
        legato: true,
        glide: 0.0,
    };
    edit(&mut p);
    SynthPreset::Subtractive(p)
}

pub(super) fn fm(m: Meta, edit: impl FnOnce(&mut FmPreset)) -> FmPreset {
    let mut meta = meta(m);
    meta.polyphony = 16.0;
    let mut p = FmPreset {
        meta,
        carrier: Carrier {
            wave: CarrierWave::Sine,
        },
        ratio: 1.0,
        index: 2.0,
        velocity_to_index: 0.7,
        key_scale: 0.5,
        detune: 0.0,
        mod_env: adsr(0.001, 0.8, 0.2, 0.4),
        amp: amp(0.002, 2.0, 0.0, 0.4, 0.7),
        ping: PingParams {
            ratio: 4.0,
            level: 0.0,
            decay: 0.05,
        },
        lfo: lfo(5.0, 0.0, 0.0, 0.0),
    };
    edit(&mut p);
    p
}

/// FM preset with its common fields: ratio, index, velocityToIndex, keyScale.
#[allow(clippy::too_many_arguments)]
pub(super) fn fm_core(
    p: &mut FmPreset,
    gain: f64,
    ratio: f64,
    index: f64,
    v2i: f64,
    key_scale: f64,
    mod_env: Adsr,
    amp_: AmpParams,
) {
    p.meta.gain = gain;
    p.ratio = ratio;
    p.index = index;
    p.velocity_to_index = v2i;
    p.key_scale = key_scale;
    p.mod_env = mod_env;
    p.amp = amp_;
}

pub(super) fn ping(ratio: f64, level: f64, decay: f64) -> PingParams {
    PingParams {
        ratio,
        level,
        decay,
    }
}

pub(super) fn pluck(m: Meta, edit: impl FnOnce(&mut PluckPreset)) -> SynthPreset {
    let mut meta = meta(m);
    meta.polyphony = 8.0;
    let mut p = PluckPreset {
        meta,
        attack_noise: 1.0,
        dampening: 4000.0,
        resonance: 0.97,
        release: 1.0,
        velocity: 0.7,
    };
    edit(&mut p);
    SynthPreset::Pluck(p)
}

pub(super) fn sampler(m: Meta, edit: impl FnOnce(&mut SamplerPreset)) -> SynthPreset {
    let mut meta = meta(m);
    meta.polyphony = 48.0;
    let mut p = SamplerPreset {
        meta,
        sample: SampleKind::Piano,
        release: 0.45,
        velocity_curve: 1.6,
    };
    edit(&mut p);
    SynthPreset::Sampler(p)
}

/// Organ drawbars as harmonics of the 8' fundamental: 8', 4', 2 2/3', 2', (1 3/5'), 1 1/3', -, 1'.
pub(super) fn drawbars(
    b8: f64,
    b4: f64,
    b223: f64,
    b2: f64,
    b135: f64,
    b113: f64,
    b1: f64,
) -> Vec<f64> {
    vec![b8, b4, b223, b2, b135, b113, 0.0, b1]
}

pub(super) fn ch(c: &mut CharacterParams, drive: f64, chorus: f64, tremolo: f64) {
    c.drive = drive;
    c.chorus = chorus;
    c.tremolo = tremolo;
}

pub(super) fn desc(m: &mut PresetMeta, d: &str) {
    m.description = Some(d.to_string());
}
