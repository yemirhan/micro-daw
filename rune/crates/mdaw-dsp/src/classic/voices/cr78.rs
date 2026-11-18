//! CR-78-style voice recipes (`voices.ts`).

use super::VoiceRecipe;
use super::common::{
    Cowbell, body, bpf, chain, cowbell, hpf, jingles, lpf, noise_hit, recipe, scaled, sum,
};
use crate::classic::dsp::{
    EnvOpts, PARTIALS_ATTACK, env, lp, metal, mix_into, mul, noise, partials,
};

const METAL_CR78: [f64; 6] = [295.0, 413.0, 532.0, 677.0, 811.0, 1092.0];

fn hat_cr78(max_sec: f64, e: EnvOpts) -> VoiceRecipe {
    recipe(max_sec, move |n| {
        let s = sum(
            n,
            &[(noise(n, 7801), 0.7), (metal(n, &METAL_CR78, 61), 0.4)],
        );
        let s = chain(s, &[bpf(8000.0, 1.4), hpf(5500.0), lpf(13000.0)]);
        mul(s, &env(n, e))
    })
}

fn hand_drum(freq: f64, t60: f64, max_sec: f64) -> VoiceRecipe {
    recipe(max_sec, move |n| {
        sum(
            n,
            &[
                (
                    body(
                        n,
                        b!(freq: freq, sweep: 0.1, sweep_tau: 0.006, t60: t60, attack: 0.0012),
                    ),
                    1.0,
                ),
                (
                    body(
                        n,
                        b!(freq: freq * 1.6, t60: t60 * 0.5, attack: 0.0012, phase: 0.1),
                    ),
                    0.15,
                ),
            ],
        )
    })
}

/// Guiro: a run of short, accelerating scrapes.
fn guiro(n: usize) -> Vec<f32> {
    let s = chain(noise(n, 7802), &[bpf(2500.0, 1.5), hpf(1200.0)]);
    let mut e = vec![0.0; n];
    let mut t = 0.0f64;
    for k in 0..10 {
        let k = k as f64;
        mix_into(
            &mut e,
            &env(n, e!(attack: 0.002, t60: 0.018, delay: t)),
            0.6 + 0.04 * k,
            0,
        );
        t += 0.024 - k * 0.0012;
    }
    mul(s, &e)
}

pub(super) const CR78_IDS: &[&str] = &[
    "kick",
    "snare",
    "rim",
    "chh",
    "ohh",
    "cymbal",
    "metal",
    "maracas",
    "guiro",
    "tambourine",
    "clave",
    "cowbell",
    "bongo-lo",
    "bongo-hi",
    "conga-lo",
    "conga-hi",
];

pub(super) fn cr78(voice: &str) -> Option<VoiceRecipe> {
    Some(match voice {
        "kick" => recipe(0.45, |n| {
            lp(
                body(
                    n,
                    b!(freq: 62, sweep: 0.25, sweep_tau: 0.01, attack: 0.003, t60: 0.28),
                ),
                1200.0,
                None,
            )
        }),
        "snare" => recipe(0.35, |n| {
            lp(
                sum(
                    n,
                    &[
                        (body(n, b!(freq: 205, t60: 0.09, attack: 0.001)), 0.6),
                        (
                            body(n, b!(freq: 340, t60: 0.06, attack: 0.001, phase: 0.2)),
                            0.25,
                        ),
                        (
                            noise_hit(
                                n,
                                7803,
                                &[bpf(3500.0, 0.6), lpf(8000.0)],
                                e!(attack: 0.002, t60: 0.14),
                            ),
                            0.7,
                        ),
                    ],
                ),
                9000.0,
                None,
            )
        }),
        "rim" => recipe(0.1, |n| {
            sum(
                n,
                &[
                    (body(n, b!(freq: 820, t60: 0.035, attack: 0.0008)), 0.7),
                    (body(n, b!(freq: 1650, t60: 0.025, attack: 0.0008)), 0.3),
                ],
            )
        }),
        "chh" => hat_cr78(0.2, e!(attack: 0.001, t60: 0.07)),
        "ohh" => hat_cr78(0.6, e!(attack: 0.001, hold: 0.01, t60: 0.38)),
        "cymbal" => recipe(1.4, |n| {
            let s = sum(
                n,
                &[
                    (noise(n, 7804), 0.7),
                    (metal(n, &scaled(&METAL_CR78, 1.2), 63), 0.4),
                ],
            );
            let s = chain(s, &[bpf(6000.0, 0.7), hpf(4500.0), lpf(12000.0)]);
            mul(s, &env(n, e!(attack: 0.002, t60: 1.1)))
        }),
        "metal" => recipe(0.3, |n| {
            partials(
                n,
                &[
                    (1250.0, 0.5, 0.18),
                    (1830.0, 0.4, 0.15),
                    (2650.0, 0.35, 0.12),
                    (3700.0, 0.2, 0.1),
                    (5100.0, 0.15, 0.08),
                ],
                PARTIALS_ATTACK,
            )
        }),
        "maracas" => recipe(0.15, |n| {
            noise_hit(
                n,
                7805,
                &[hpf(6000.0), lpf(13000.0)],
                e!(attack: 0.006, hold: 0.005, t60: 0.06),
            )
        }),
        "guiro" => recipe(0.35, guiro),
        "tambourine" => recipe(0.45, |n| {
            lp(
                jingles(
                    n,
                    7806,
                    &[(0.0, 1.0), (0.03, 0.5)],
                    0.2,
                    &[5000.0, 6200.0, 7400.0, 8700.0],
                ),
                12000.0,
                None,
            )
        }),
        "clave" => recipe(0.15, |n| {
            sum(
                n,
                &[
                    (body(n, b!(freq: 2350, t60: 0.07, attack: 0.0005)), 1.0),
                    (body(n, b!(freq: 4800, t60: 0.03, attack: 0.0005)), 0.1),
                ],
            )
        }),
        "cowbell" => recipe(0.4, |n| {
            cowbell(
                n,
                540.0,
                800.0,
                Cowbell {
                    center: 900.0,
                    q: 1.2,
                    fast: 0.08,
                    slow: 0.25,
                    lp_hz: 4000.0,
                },
            )
        }),
        "bongo-lo" => hand_drum(290.0, 0.14, 0.3),
        "bongo-hi" => hand_drum(420.0, 0.11, 0.3),
        "conga-lo" => hand_drum(195.0, 0.26, 0.45),
        "conga-hi" => hand_drum(275.0, 0.21, 0.45),
        _ => return None,
    })
}
