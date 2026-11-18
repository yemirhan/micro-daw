//! 808-style voice recipes (`voices.ts`).

use super::VoiceRecipe;
use super::common::{
    CLAP0, COWBELL0, Clap, Cowbell, body, bpf, clap, cowbell, hpf, lpf, metal_hit, noise_hit,
    recipe, sum,
};
use crate::classic::dsp::{bp, drive, env, hp, lp, metal, mix_into, mul};

/// The six square oscillators of the 808 metal circuit (Hz).
const METAL_808: [f64; 6] = [205.3, 304.4, 369.6, 522.7, 540.0, 800.0];

fn tom808(freq: f64, t60: f64, seed: u32) -> VoiceRecipe {
    recipe(1.2, move |n| {
        sum(
            n,
            &[
                (
                    body(
                        n,
                        b!(freq: freq, sweep: 0.22, sweep_tau: 0.03, t60: t60, attack: 0.0006),
                    ),
                    1.0,
                ),
                (
                    noise_hit(n, seed, &[lpf(2500.0)], e!(attack: 0.0005, t60: 0.06)),
                    0.12,
                ),
            ],
        )
    })
}

fn conga808(freq: f64, t60: f64) -> VoiceRecipe {
    recipe(0.5, move |n| {
        body(
            n,
            b!(freq: freq, sweep: 0.12, sweep_tau: 0.008, t60: t60, attack: 0.0006),
        )
    })
}

pub(super) const TR808_IDS: &[&str] = &[
    "kick",
    "kick-long",
    "snare",
    "rim",
    "clap",
    "chh",
    "ohh",
    "cymbal",
    "cowbell",
    "clave",
    "maracas",
    "tom-lo",
    "tom-mid",
    "tom-hi",
    "conga-lo",
    "conga-mid",
    "conga-hi",
];

pub(super) fn tr808(voice: &str) -> Option<VoiceRecipe> {
    Some(match voice {
        "kick" => recipe(1.6, |n| {
            let mut s = body(
                n,
                b!(freq: 50, sweep: 0.55, sweep_tau: 0.006, sweep2: 0.12, sweep2_tau: 0.06, t60: 1.15, attack: 0.0004),
            );
            mix_into(
                &mut s,
                &body(n, b!(freq: 1400, t60: 0.006, attack: 0.0002)),
                0.12,
                0,
            ); // trigger-pulse click
            lp(drive(s, 1.2), 5000.0, None)
        }),
        "kick-long" => recipe(2.8, |n| {
            let s = body(
                n,
                b!(freq: 46, sweep: 0.5, sweep_tau: 0.008, sweep2: 0.1, sweep2_tau: 0.08, drift: 0.94, hold: 0.05, t60: 2.6, attack: 0.0004),
            );
            lp(drive(s, 2.2), 1800.0, None)
        }),
        "snare" => recipe(0.6, |n| {
            sum(
                n,
                &[
                    (
                        body(n, b!(freq: 238, sweep: 0.25, sweep_tau: 0.004, t60: 0.16)),
                        0.55,
                    ),
                    (
                        body(
                            n,
                            b!(freq: 476, sweep: 0.15, sweep_tau: 0.004, t60: 0.09, phase: 0.2),
                        ),
                        0.3,
                    ),
                    (
                        noise_hit(n, 808, &[hpf(1800.0), lpf(11000.0)], e!(t60: 0.24)),
                        0.6,
                    ),
                ],
            )
        }),
        "rim" => recipe(0.15, |n| {
            let s = sum(
                n,
                &[
                    (body(n, b!(freq: 1667, t60: 0.03, attack: 0.0002)), 0.7),
                    (body(n, b!(freq: 455, t60: 0.04, attack: 0.0002)), 0.6),
                ],
            );
            hp(drive(s, 2.0), 250.0, None)
        }),
        "clap" => recipe(0.8, |n| {
            clap(
                n,
                8081,
                Clap {
                    center: 1100.0,
                    q: 1.6,
                    bursts: &[0.0, 0.011, 0.022, 0.034],
                    tail_t60: 0.34,
                    ..CLAP0
                },
            )
        }),
        "chh" => recipe(0.3, |n| {
            metal_hit(
                n,
                &METAL_808,
                11,
                &[bpf(10000.0, 1.0), hpf(7000.0)],
                e!(attack: 0.0003, t60: 0.09),
                0.0,
            )
        }),
        "ohh" => recipe(1.2, |n| {
            metal_hit(
                n,
                &METAL_808,
                11,
                &[bpf(10000.0, 1.0), hpf(7000.0)],
                e!(attack: 0.0003, hold: 0.02, t60: 0.75),
                0.0,
            )
        }),
        "cymbal" => recipe(2.4, |n| {
            let m = metal(n, &METAL_808, 13);
            let low = mul(
                bp(m.clone(), 3440.0, Some(2.0)),
                &env(n, e!(attack: 0.0005, t60: 0.55)),
            );
            let high = mul(
                hp(bp(m, 7100.0, Some(1.2)), 5500.0, None),
                &env(n, e!(attack: 0.0005, t60: 2.0)),
            );
            sum(n, &[(low, 0.35), (high, 0.9)])
        }),
        "cowbell" => recipe(0.7, |n| {
            cowbell(
                n,
                540.0,
                800.0,
                Cowbell {
                    center: 700.0,
                    q: 0.9,
                    fast: 0.07,
                    slow: 0.5,
                    ..COWBELL0
                },
            )
        }),
        "clave" => recipe(0.2, |n| body(n, b!(freq: 2500, t60: 0.06, attack: 0.0002))),
        "maracas" => recipe(0.2, |n| {
            noise_hit(
                n,
                8082,
                &[hpf(5000.0), lpf(13000.0)],
                e!(attack: 0.004, t60: 0.07),
            )
        }),
        "tom-lo" => tom808(85.0, 0.75, 8083),
        "tom-mid" => tom808(125.0, 0.6, 8084),
        "tom-hi" => tom808(170.0, 0.5, 8085),
        "conga-lo" => conga808(170.0, 0.22),
        "conga-mid" => conga808(250.0, 0.2),
        "conga-hi" => conga808(370.0, 0.17),
        _ => return None,
    })
}
