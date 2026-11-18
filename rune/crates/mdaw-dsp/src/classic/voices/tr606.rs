//! 606-style voice recipes (`voices.ts`).

use super::VoiceRecipe;
use super::common::{Clap, body, bpf, clap, hpf, lpf, metal_hit, noise_hit, recipe, sum};
use crate::classic::dsp::{EnvOpts, bp, env, hp, lp, metal, mix_into, mul};

const METAL_606: [f64; 6] = [245.0, 306.0, 365.0, 415.0, 437.0, 619.0];

fn kick606(max_sec: f64, freq: f64, sweep: f64, t60: f64) -> VoiceRecipe {
    recipe(max_sec, move |n| {
        let mut s = body(
            n,
            b!(freq: freq, sweep: sweep, sweep_tau: 0.008, t60: t60, attack: 0.0004),
        );
        mix_into(
            &mut s,
            &body(n, b!(freq: 1800, t60: 0.004, attack: 0.0001)),
            0.12,
            0,
        );
        lp(hp(s, 35.0, None), 4000.0, None)
    })
}

fn hat606(max_sec: f64, e: EnvOpts) -> VoiceRecipe {
    recipe(max_sec, move |n| {
        metal_hit(n, &METAL_606, 41, &[bpf(11000.0, 1.2), hpf(8000.0)], e, 0.0)
    })
}

fn tom606(freq: f64, t60: f64) -> VoiceRecipe {
    recipe(0.45, move |n| {
        body(
            n,
            b!(freq: freq, sweep: 0.3, sweep_tau: 0.02, t60: t60, attack: 0.0005),
        )
    })
}

pub(super) const TR606_IDS: &[&str] = &[
    "kick",
    "kick-long",
    "kick-short",
    "snare",
    "snare-snappy",
    "rim",
    "clap",
    "chh",
    "phh",
    "ohh",
    "cymbal",
    "shaker",
    "zap",
    "tom-lo",
    "tom-mid",
    "tom-hi",
];

pub(super) fn tr606(voice: &str) -> Option<VoiceRecipe> {
    Some(match voice {
        "kick" => kick606(0.5, 64.0, 0.9, 0.32),
        "kick-long" => kick606(0.9, 58.0, 0.8, 0.7),
        "kick-short" => kick606(0.3, 70.0, 1.2, 0.18),
        "snare" => recipe(0.35, |n| {
            sum(
                n,
                &[
                    (
                        body(n, b!(freq: 220, sweep: 0.2, sweep_tau: 0.005, t60: 0.07)),
                        0.35,
                    ),
                    (body(n, b!(freq: 380, t60: 0.05, phase: 0.2)), 0.15),
                    (
                        noise_hit(n, 6061, &[hpf(2500.0), lpf(12000.0)], e!(t60: 0.16)),
                        0.8,
                    ),
                ],
            )
        }),
        "snare-snappy" => recipe(0.45, |n| {
            sum(
                n,
                &[
                    (
                        body(n, b!(freq: 230, sweep: 0.2, sweep_tau: 0.005, t60: 0.06)),
                        0.3,
                    ),
                    (
                        noise_hit(n, 6062, &[hpf(1800.0), lpf(13000.0)], e!(t60: 0.24)),
                        1.0,
                    ),
                ],
            )
        }),
        "rim" => recipe(0.1, |n| {
            hp(
                sum(
                    n,
                    &[
                        (body(n, b!(freq: 1800, t60: 0.02, attack: 0.0002)), 0.7),
                        (body(n, b!(freq: 600, t60: 0.025, attack: 0.0002)), 0.4),
                    ],
                ),
                500.0,
                None,
            )
        }),
        "clap" => recipe(0.45, |n| {
            clap(
                n,
                6063,
                Clap {
                    center: 1600.0,
                    q: 1.5,
                    bursts: &[0.0, 0.007, 0.015, 0.024],
                    burst_t60: 0.018,
                    tail_t60: 0.22,
                    tail_gain: 0.45,
                    hp_hz: 900.0,
                },
            )
        }),
        "chh" => hat606(0.15, e!(attack: 0.0002, t60: 0.045)),
        "phh" => hat606(0.25, e!(attack: 0.0015, t60: 0.13)),
        "ohh" => hat606(0.7, e!(attack: 0.0002, hold: 0.01, t60: 0.42)),
        "cymbal" => recipe(1.6, |n| {
            let m = metal(n, &METAL_606, 43);
            let high = mul(
                hp(bp(m.clone(), 8000.0, Some(0.8)), 5500.0, None),
                &env(n, e!(attack: 0.001, t60: 1.3)),
            );
            let low = mul(
                bp(m, 4000.0, Some(1.5)),
                &env(n, e!(attack: 0.001, t60: 0.3)),
            );
            sum(n, &[(high, 1.0), (low, 0.25)])
        }),
        "shaker" => recipe(0.15, |n| {
            noise_hit(
                n,
                6064,
                &[hpf(6000.0), bpf(9000.0, 0.7)],
                e!(attack: 0.01, hold: 0.01, t60: 0.06),
            )
        }),
        "zap" => recipe(0.2, |n| {
            body(
                n,
                b!(freq: 180, sweep: 8, sweep_tau: 0.012, t60: 0.12, attack: 0.0003),
            )
        }),
        "tom-lo" => tom606(115.0, 0.32),
        "tom-mid" => tom606(160.0, 0.28),
        "tom-hi" => tom606(215.0, 0.24),
        _ => return None,
    })
}
