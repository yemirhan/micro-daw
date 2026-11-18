//! Shared building blocks of the classic voice recipes (the helper functions at the top of
//! `voices.ts`): body / noise / clap / cowbell / metal / jingle / membrane generators, the
//! filter-chain shorthand and the `b!` / `e!` option macros.

use crate::classic::dsp::{
    ClapEnvOpts, EnvOpts, FilterType, OscOpts, PARTIALS_ATTACK, Shape, bp, clap_env, env, env_sum,
    hp, lp, metal, mix_into, mul, noise, osc, partials, samples, square,
};

use super::VoiceRecipe;

/// Box a render closure into a [`VoiceRecipe`].
pub(super) fn recipe(
    max_sec: f64,
    render: impl Fn(usize) -> Vec<f32> + Send + Sync + 'static,
) -> VoiceRecipe {
    VoiceRecipe {
        max_sec,
        render: Box::new(render),
    }
}

/// `OscOpts & EnvOpts` (what `body` takes).
#[derive(Debug, Clone, Copy)]
pub(super) struct B {
    pub(super) freq: f64,
    pub(super) sweep: f64,
    pub(super) sweep_tau: f64,
    pub(super) sweep2: f64,
    pub(super) sweep2_tau: f64,
    pub(super) drift: f64,
    pub(super) phase: f64,
    pub(super) attack: f64,
    pub(super) hold: f64,
    pub(super) t60: f64,
    pub(super) delay: f64,
}

pub(super) const B0: B = B {
    freq: 0.0,
    sweep: OscOpts::D.sweep,
    sweep_tau: OscOpts::D.sweep_tau,
    sweep2: OscOpts::D.sweep2,
    sweep2_tau: OscOpts::D.sweep2_tau,
    drift: OscOpts::D.drift,
    phase: OscOpts::D.phase,
    attack: EnvOpts::D.attack,
    hold: EnvOpts::D.hold,
    t60: 0.0,
    delay: EnvOpts::D.delay,
};

/// `{ freq: …, t60: …, … }` body options (unset fields take the TS defaults).
macro_rules! b {
    ($($k:ident : $v:expr),* $(,)?) => {
        $crate::classic::voices::common::B {
            $($k: ($v) as f64,)*
            ..$crate::classic::voices::common::B0
        }
    };
}

/// `{ t60: …, … }` envelope options.
macro_rules! e {
    ($($k:ident : $v:expr),* $(,)?) => {
        $crate::classic::dsp::EnvOpts {
            $($k: ($v) as f64,)*
            ..$crate::classic::dsp::EnvOpts::D
        }
    };
}

pub(super) type Filter = (FilterType, f64, Option<f64>);
pub(super) const fn lpf(f: f64) -> Filter {
    (FilterType::Lp, f, None)
}
pub(super) const fn hpf(f: f64) -> Filter {
    (FilterType::Hp, f, None)
}
pub(super) const fn bpf(f: f64, q: f64) -> Filter {
    (FilterType::Bp, f, Some(q))
}

pub(super) fn chain(mut buf: Vec<f32>, filters: &[Filter]) -> Vec<f32> {
    for &(ty, f, q) in filters {
        buf = match ty {
            FilterType::Lp => lp(buf, f, q),
            FilterType::Hp => hp(buf, f, q),
            _ => bp(buf, f, q),
        };
    }
    buf
}

/// Pitched drum body: oscillator with pitch envelope under an AD envelope.
pub(super) fn body(n: usize, o: B) -> Vec<f32> {
    let oo = OscOpts {
        freq: o.freq,
        sweep: o.sweep,
        sweep_tau: o.sweep_tau,
        sweep2: o.sweep2,
        sweep2_tau: o.sweep2_tau,
        drift: o.drift,
        shape: Shape::Sine,
        phase: o.phase,
    };
    let eo = EnvOpts {
        attack: o.attack,
        hold: o.hold,
        t60: o.t60,
        delay: o.delay,
    };
    mul(osc(n, oo), &env(n, eo))
}

/// Filtered noise under an envelope.
pub(super) fn noise_hit(n: usize, seed: u32, filters: &[Filter], e: EnvOpts) -> Vec<f32> {
    mul(chain(noise(n, seed), filters), &env(n, e))
}

/// Sum of `(signal, gain)` pairs.
pub(super) fn sum(n: usize, parts: &[(Vec<f32>, f64)]) -> Vec<f32> {
    let mut out = vec![0.0; n];
    for (s, g) in parts {
        mix_into(&mut out, s, *g, 0);
    }
    out
}

pub(super) struct Clap<'a> {
    pub(super) center: f64,
    pub(super) q: f64,
    pub(super) bursts: &'a [f64],
    pub(super) burst_t60: f64,
    pub(super) tail_t60: f64,
    pub(super) tail_gain: f64,
    pub(super) hp_hz: f64,
}

pub(super) const CLAP0: Clap<'static> = Clap {
    center: 0.0,
    q: 1.4,
    bursts: &[],
    burst_t60: 0.022,
    tail_t60: 0.0,
    tail_gain: 0.55,
    hp_hz: 600.0,
};

/// Hand clap: band-passed noise through a multi-burst envelope.
pub(super) fn clap(n: usize, seed: u32, o: Clap) -> Vec<f32> {
    let s = chain(noise(n, seed), &[bpf(o.center, o.q), hpf(o.hp_hz)]);
    let ce = ClapEnvOpts {
        bursts: o.bursts,
        burst_t60: o.burst_t60,
        tail_delay: o.bursts[o.bursts.len() - 1],
        tail_t60: o.tail_t60,
        tail_gain: o.tail_gain,
    };
    mul(s, &clap_env(n, &ce))
}

pub(super) struct Cowbell {
    pub(super) center: f64,
    pub(super) q: f64,
    pub(super) fast: f64,
    pub(super) slow: f64,
    pub(super) lp_hz: f64,
}

pub(super) const COWBELL0: Cowbell = Cowbell {
    center: 0.0,
    q: 1.0,
    fast: 0.07,
    slow: 0.4,
    lp_hz: 9000.0,
};

/// Two-square cowbell through a band-pass, fast drop into a ringing tail.
pub(super) fn cowbell(n: usize, f1: f64, f2: f64, o: Cowbell) -> Vec<f32> {
    let s = sum(n, &[(square(n, f1, 0.0), 0.5), (square(n, f2, 0.3), 0.5)]);
    let s = chain(s, &[bpf(o.center, o.q), lpf(o.lp_hz)]);
    mul(
        s,
        &env_sum(
            n,
            &[
                (0.72, e!(attack: 0.0005, t60: o.fast)),
                (0.28, e!(attack: 0.0005, t60: o.slow)),
            ],
        ),
    )
}

/// Hi-hat / cymbal from a metallic source (+ optional noise) through filters.
pub(super) fn metal_hit(
    n: usize,
    freqs: &[f64],
    seed: u32,
    filters: &[Filter],
    e: EnvOpts,
    noise_mix: f64,
) -> Vec<f32> {
    let mut s = metal(n, freqs, seed);
    if noise_mix != 0.0 {
        mix_into(&mut s, &noise(n, seed + 1), noise_mix, 0);
    }
    mul(chain(s, filters), &env(n, e))
}

/// Tambourine-style jingles: high inharmonic partials + noise, struck `hits` times.
pub(super) fn jingles(
    n: usize,
    seed: u32,
    hits: &[(f64, f64)],
    t60: f64,
    freqs: &[f64],
) -> Vec<f32> {
    let list: Vec<(f64, f64, f64)> = freqs
        .iter()
        .enumerate()
        .map(|(i, &f)| {
            let i = i as f64;
            (f, 1.0 / (1.0 + i * 0.35), t60 * (1.0 - i * 0.08))
        })
        .collect();
    let mut one = partials(n, &list, PARTIALS_ATTACK);
    let nz = mul(
        chain(noise(n, seed), &[hpf(6000.0), bpf(9000.0, 0.7)]),
        &env(n, e!(attack: 0.001, t60: t60 * 0.6)),
    );
    mix_into(&mut one, &nz, 1.2, 0);
    let mut out = vec![0.0; n];
    for &(t, g) in hits {
        mix_into(&mut out, &one, g, samples(t));
    }
    out
}

pub(super) struct Membrane {
    pub(super) freq: f64,
    pub(super) sweep: f64,
    pub(super) sweep_tau: f64,
    pub(super) t60: f64,
    pub(super) overtone: f64,
    pub(super) noise_gain: f64,
    pub(super) attack: f64,
}

/// Inharmonic membrane: fundamental + (1.59×) overtone + optional head noise.
pub(super) fn membrane(n: usize, seed: u32, o: Membrane) -> Vec<f32> {
    let Membrane {
        freq,
        sweep,
        sweep_tau,
        t60,
        overtone,
        noise_gain,
        attack,
    } = o;
    sum(
        n,
        &[
            (
                body(
                    n,
                    b!(freq: freq, sweep: sweep, sweep_tau: sweep_tau, t60: t60, attack: attack),
                ),
                1.0,
            ),
            (
                body(
                    n,
                    b!(freq: freq * 1.59, sweep: sweep, sweep_tau: sweep_tau, t60: t60 * 0.4, attack: attack, phase: 0.1),
                ),
                overtone,
            ),
            (
                noise_hit(
                    n,
                    seed,
                    &[lpf(3000.0), hpf(200.0)],
                    e!(attack: 0.0003, t60: 0.02),
                ),
                noise_gain,
            ),
        ],
    )
}

pub(super) fn scaled(freqs: &[f64], k: f64) -> Vec<f64> {
    freqs.iter().map(|f| f * k).collect()
}
