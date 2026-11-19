//! Shaped voices: the per-pad sound shaping of [`PadShape`] (amp AHDS, filter type / resonance /
//! envelope, pitch envelope, drive) applied inside a [`SamplerKit`](super::SamplerKit) voice.
//!
//! A pad whose shape is neutral never reaches this module: its voices render through the
//! Chromium-parity path untouched (bit-identical to the pre-shaping engine). A voice latches its
//! shape at the hit ([`VoiceShape::latch`]); only the continuous controls (cutoff, resonance,
//! drive) follow the pad live, through per-pad one-pole smoothers ([`PadSmooth`]) advanced once
//! per quantum and interpolated linearly inside it, so turning a knob never steps.
//!
//! Per voice and sample: `read (pitch-bent) → drive → SVF (cutoff × 2^(filter env)) → × amp
//! (velocity / fade / release AutoParam) × AHDS`. Voices with their own filter feed the pad's
//! `direct` bus (amp → panner, skipping the pad's biquad); the others feed the classic bus.
//! Nothing here allocates.

#![allow(clippy::needless_range_loop)] // per-frame DSP over parallel buffers

use mdaw_dsp::buffer::AudioBuffer;
use mdaw_dsp::pad_shape::{Ahds, Drive, ModEnv, Svf, SvfCoefs, SvfMode, resonance_q};
use mdaw_model::types::{PadFilterType, PadShape};

use super::FILTER_BYPASS_HZ;
use super::slice::clamp;

const Q: usize = crate::contract::RENDER_QUANTUM;

/// What a voice latched from its pad's shape at the hit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct VoiceShape {
    /// Any shaping at all (false = classic voice).
    pub active: bool,
    pub ahds: Ahds,
    pub amp: bool,
    /// The voice runs its own filter (and feeds the direct bus).
    pub svf: bool,
    pub mode: SvfMode,
    pub fenv: ModEnv,
    /// The voice reads its buffer itself (pitch envelope).
    pub pitch: bool,
    pub penv: ModEnv,
    /// Read position / base step (buffer frames per output frame) / end (buffer frames).
    pub pos: f64,
    pub step: f64,
    pub end_pos: f64,
    pub state: [Svf; 2],
}

impl VoiceShape {
    pub const OFF: VoiceShape = VoiceShape {
        active: false,
        ahds: Ahds::FLAT,
        amp: false,
        svf: false,
        mode: SvfMode::LowPass,
        fenv: ModEnv::OFF,
        pitch: false,
        penv: ModEnv::OFF,
        pos: 0.0,
        step: 0.0,
        end_pos: 0.0,
        state: [Svf::ZERO; 2],
    };

    /// Latch a pad's shape. `drive_live` = the pad's smoothed drive is not exactly 0 (a voice
    /// hit while drive is fading out still needs the shaped path).
    pub fn latch(s: &PadShape, drive_live: bool) -> Self {
        if s.is_neutral() && !drive_live {
            return Self::OFF;
        }
        let amp = !s.amp_is_neutral();
        Self {
            active: true,
            ahds: if amp {
                Ahds {
                    attack: s.amp_attack,
                    hold: s.amp_hold,
                    decay: s.amp_decay,
                    sustain: s.amp_sustain,
                }
            } else {
                Ahds::FLAT
            },
            amp,
            svf: !s.filter_is_neutral(),
            mode: match s.filter_type {
                PadFilterType::Lp => SvfMode::LowPass,
                PadFilterType::Hp => SvfMode::HighPass,
                PadFilterType::Bp => SvfMode::BandPass,
            },
            fenv: ModEnv {
                amount: s.filter_env,
                decay: s.filter_env_decay,
            },
            pitch: s.pitch_env != 0.0,
            penv: ModEnv {
                amount: s.pitch_env,
                decay: s.pitch_env_decay,
            },
            ..Self::OFF
        }
    }
}

/// How long a hit of `slice` sounds on a pad shaped by `s` (before any release): the pitch
/// envelope bends the read speed, a zero sustain ends the voice after its amp stages. A neutral
/// shape gives exactly `slice.real_duration`.
pub fn voice_seconds(s: &PadShape, slice: &super::Slice) -> f64 {
    let mut d = if s.pitch_env != 0.0 {
        mdaw_dsp::pad_shape::pitched_duration(
            slice.length,
            slice.rate,
            ModEnv {
                amount: s.pitch_env,
                decay: s.pitch_env_decay,
            },
        )
    } else {
        slice.real_duration
    };
    if !s.amp_is_neutral() && s.amp_sustain <= 0.0 {
        d = d.min((s.amp_attack + s.amp_hold + s.amp_decay).max(0.001));
    }
    d
}

/// A pad's smoothed live controls.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PadSmooth {
    /// log2(cutoff Hz).
    pub cutoff: mdaw_dsp::pad_shape::Smoother,
    pub q: mdaw_dsp::pad_shape::Smoother,
    pub drive: mdaw_dsp::pad_shape::Smoother,
}

/// One quantum's start / end values of a pad's live controls.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct PadBlock {
    pub lc0: f32,
    pub lc1: f32,
    pub q0: f32,
    pub q1: f32,
    pub d0: Drive,
    pub d1: Drive,
}

/// The pad's cutoff as the shaped filter sees it (log2 Hz).
pub(crate) fn cutoff_log2(cutoff: f64) -> f64 {
    clamp(cutoff, 20.0, FILTER_BYPASS_HZ).log2()
}

impl PadSmooth {
    pub fn new(s: &PadShape, cutoff: f64) -> Self {
        use mdaw_dsp::pad_shape::Smoother;
        Self {
            cutoff: Smoother::new(cutoff_log2(cutoff)),
            q: Smoother::new(resonance_q(s.filter_resonance)),
            drive: Smoother::new(s.drive),
        }
    }

    pub fn block(&mut self, s: &PadShape, cutoff: f64, n: usize, sr: f64) -> PadBlock {
        let (lc0, lc1) = self.cutoff.block(cutoff_log2(cutoff), n, sr);
        let (q0, q1) = self.q.block(resonance_q(s.filter_resonance), n, sr);
        let (d0, d1) = self.drive.block(s.drive, n, sr);
        PadBlock {
            lc0: lc0 as f32,
            lc1: lc1 as f32,
            q0: q0 as f32,
            q1: q1 as f32,
            d0: Drive::new(d0),
            d1: Drive::new(d1),
        }
    }

    /// Drive is (or is still gliding) above 0.
    pub fn drive_live(&self) -> bool {
        self.drive.value > 0.0
    }
}

/// Fill `out[c][..n]` with the voice's pitch-bent read (linear interpolation; 0 outside
/// `[src_on, end)`), advancing its position. `t0` is the voice's start in seconds.
#[allow(clippy::too_many_arguments)]
pub(crate) fn read_bent(
    sh: &mut VoiceShape,
    buf: &AudioBuffer,
    out: &mut [[f32; Q]; 2],
    nch: usize,
    frame: u64,
    n: usize,
    src_on: u64,
    t0: f64,
    sr: f64,
) {
    for o in out.iter_mut() {
        o[..n].fill(0.0);
    }
    let frames = buf.frames();
    if frames == 0 {
        return;
    }
    let end = sh.end_pos.min(frames as f64 - 1.0);
    for i in 0..n {
        let k = frame + i as u64;
        if k < src_on {
            continue;
        }
        if sh.pos >= end || sh.pos < 0.0 {
            break;
        }
        let i0 = sh.pos as usize;
        let fr = (sh.pos - i0 as f64) as f32;
        let i1 = (i0 + 1).min(frames - 1);
        for c in 0..nch {
            if let Some(d) = buf.channel(c) {
                out[c][i] = d[i0] + (d[i1] - d[i0]) * fr;
            }
        }
        let t = (k as f64 / sr - t0).max(0.0);
        let semis = sh.penv.value(t);
        let bend = if semis == 0.0 {
            1.0
        } else {
            (semis / 12.0).exp2()
        };
        sh.pos += sh.step * bend;
    }
}

/// Shape one quantum of a voice in place: `vbuf` holds the read samples, `env` the voice's
/// AutoParam gain. Frames outside `[src_on, src_off)` are left to the caller's gating.
#[allow(clippy::too_many_arguments)]
pub(crate) fn shape_block(
    sh: &mut VoiceShape,
    blk: &PadBlock,
    vbuf: &mut [[f32; Q]; 2],
    env: &[f32; Q],
    nch: usize,
    frame: u64,
    n: usize,
    t0: f64,
    sr: f64,
) {
    let srf = sr as f32;
    let inv_n = 1.0 / n as f32;
    let drive_on = !(blk.d0.is_bypass() && blk.d1.is_bypass());
    for i in 0..n {
        let f = i as f32 * inv_n;
        let t = ((frame + i as u64) as f64 / sr - t0).max(0.0);
        let drive = if drive_on {
            Some(Drive::lerp(&blk.d0, &blk.d1, f))
        } else {
            None
        };
        let coefs = if sh.svf {
            let lc = blk.lc0 + (blk.lc1 - blk.lc0) * f + sh.fenv.value(t) as f32;
            let q = blk.q0 + (blk.q1 - blk.q0) * f;
            Some(SvfCoefs::new(lc.exp2(), q, srf))
        } else {
            None
        };
        let g = if sh.amp {
            env[i] * sh.ahds.level(t) as f32
        } else {
            env[i]
        };
        for c in 0..nch {
            let mut x = vbuf[c][i];
            if let Some(d) = &drive {
                x = d.tick(x);
            }
            if let Some(k) = &coefs {
                x = sh.state[c].tick(x, k, sh.mode);
            }
            vbuf[c][i] = x * g;
        }
    }
}
