//! LinnDrum-style voice recipes (`voices.ts`).

use super::VoiceRecipe;
use super::common::{
    CLAP0, COWBELL0, Clap, Cowbell, Membrane, body, bpf, chain, clap, cowbell, hpf, jingles, lpf,
    membrane, noise_hit, recipe, scaled, sum,
};
use crate::classic::dsp::{
    EnvOpts, LofiOpts, PARTIALS_ATTACK, drive, env, lofi, metal, mix_into, mul, noise, partials,
    peak,
};

const LOFI_LINN: LofiOpts = LofiOpts {
    rate: 28000.0,
    bits: 8,
    mu: true,
};
const METAL_LINN: [f64; 6] = [327.0, 461.0, 552.0, 714.0, 943.0, 1203.0];

fn hat_linn(max_sec: f64, e: EnvOpts) -> VoiceRecipe {
    recipe(max_sec, move |n| {
        let s = sum(
            n,
            &[(metal(n, &METAL_LINN, 51), 0.5), (noise(n, 5151), 0.8)],
        );
        let s = chain(s, &[bpf(9000.0, 0.7), hpf(6000.0)]);
        let s = mul(s, &env(n, e));
        lofi(s, LOFI_LINN)
    })
}

fn tom_linn(freq: f64, t60: f64, seed: u32) -> VoiceRecipe {
    recipe(1.0, move |n| {
        lofi(
            membrane(
                n,
                seed,
                Membrane {
                    freq,
                    sweep: 0.3,
                    sweep_tau: 0.06,
                    t60,
                    overtone: 0.25,
                    noise_gain: 0.25,
                    attack: 0.0005,
                },
            ),
            LOFI_LINN,
        )
    })
}

fn conga_linn(freq: f64, t60: f64, seed: u32) -> VoiceRecipe {
    recipe(0.5, move |n| {
        let mut s = membrane(
            n,
            seed,
            Membrane {
                freq,
                sweep: 0.15,
                sweep_tau: 0.01,
                t60,
                overtone: 0.2,
                noise_gain: 0.0,
                attack: 0.0005,
            },
        );
        mix_into(
            &mut s,
            &noise_hit(
                n,
                seed + 1,
                &[bpf(2500.0, 1.0)],
                e!(attack: 0.0002, t60: 0.01),
            ),
            0.3,
            0,
        );
        lofi(s, LOFI_LINN)
    })
}

pub(super) const LINN_IDS: &[&str] = &[
    "kick",
    "snare",
    "sidestick",
    "clap",
    "chh",
    "ohh",
    "ride",
    "crash",
    "tom-lo",
    "tom-mid",
    "tom-hi",
    "conga-lo",
    "conga-hi",
    "cabasa",
    "tambourine",
    "cowbell",
];

pub(super) fn linn(voice: &str) -> Option<VoiceRecipe> {
    Some(match voice {
        "kick" => recipe(0.5, |n| {
            let s = sum(
                n,
                &[
                    (
                        body(
                            n,
                            b!(freq: 60, sweep: 0.9, sweep_tau: 0.015, sweep2: 0.15, sweep2_tau: 0.05, hold: 0.005, t60: 0.3),
                        ),
                        1.0,
                    ),
                    (body(n, b!(freq: 140, t60: 0.06, phase: 0.1)), 0.2),
                    (
                        noise_hit(
                            n,
                            5152,
                            &[lpf(3500.0), hpf(300.0)],
                            e!(attack: 0.0003, t60: 0.012),
                        ),
                        0.6,
                    ),
                ],
            );
            lofi(drive(s, 1.6), LOFI_LINN)
        }),
        "snare" => recipe(0.6, |n| {
            let s = sum(
                n,
                &[
                    (
                        body(n, b!(freq: 185, sweep: 0.15, sweep_tau: 0.01, t60: 0.13)),
                        0.55,
                    ),
                    (body(n, b!(freq: 330, t60: 0.09, phase: 0.2)), 0.3),
                    (body(n, b!(freq: 510, t60: 0.05, phase: 0.4)), 0.12),
                    (
                        noise_hit(
                            n,
                            5153,
                            &[bpf(5000.0, 0.35), lpf(10000.0)],
                            e!(t60: 0.32, hold: 0.005),
                        ),
                        0.7,
                    ),
                    (
                        noise_hit(
                            n,
                            5154,
                            &[lpf(6000.0)],
                            e!(attack: 0.01, delay: 0.01, t60: 0.4),
                        ),
                        0.15,
                    ),
                ],
            );
            lofi(drive(s, 1.3), LOFI_LINN)
        }),
        "sidestick" => recipe(0.15, |n| {
            lofi(
                sum(
                    n,
                    &[
                        (
                            body(
                                n,
                                b!(freq: 420, sweep: 0.1, sweep_tau: 0.005, t60: 0.05, attack: 0.0002),
                            ),
                            0.5,
                        ),
                        (body(n, b!(freq: 1600, t60: 0.03, attack: 0.0002)), 0.45),
                        (body(n, b!(freq: 2650, t60: 0.02, attack: 0.0002)), 0.2),
                        (
                            noise_hit(n, 5155, &[hpf(2000.0)], e!(attack: 0.0001, t60: 0.005)),
                            0.4,
                        ),
                    ],
                ),
                LOFI_LINN,
            )
        }),
        "clap" => recipe(0.6, |n| {
            let s = clap(
                n,
                5156,
                Clap {
                    center: 1200.0,
                    q: 1.0,
                    bursts: &[0.0, 0.0065, 0.017, 0.024, 0.036],
                    burst_t60: 0.018,
                    tail_t60: 0.38,
                    tail_gain: 0.5,
                    ..CLAP0
                },
            );
            lofi(peak(s, 2500.0, 1.0, 4.0), LOFI_LINN)
        }),
        "chh" => hat_linn(0.25, e!(attack: 0.0008, t60: 0.09)),
        "ohh" => hat_linn(1.1, e!(attack: 0.0008, hold: 0.02, t60: 0.75)),
        "ride" => recipe(2.5, |n| {
            let wash = sum(
                n,
                &[
                    (noise(n, 5157), 0.5),
                    (metal(n, &scaled(&METAL_LINN, 1.4), 53), 0.5),
                ],
            );
            let wash = chain(wash, &[hpf(4500.0), bpf(7000.0, 0.5)]);
            let wash = mul(wash, &env(n, e!(attack: 0.001, t60: 2.2)));
            let s = sum(
                n,
                &[
                    (wash, 0.6),
                    (
                        partials(
                            n,
                            &[
                                (2400.0, 1.0, 1.5),
                                (3550.0, 0.8, 1.2),
                                (5200.0, 0.6, 1.0),
                                (6300.0, 0.4, 0.8),
                            ],
                            PARTIALS_ATTACK,
                        ),
                        0.12,
                    ),
                    (
                        noise_hit(n, 5158, &[hpf(3000.0)], e!(attack: 0.0002, t60: 0.01)),
                        0.2,
                    ),
                ],
            );
            lofi(s, LOFI_LINN)
        }),
        "crash" => recipe(2.6, |n| {
            let s = sum(
                n,
                &[
                    (noise(n, 5159), 0.7),
                    (metal(n, &scaled(&METAL_LINN, 1.2), 55), 0.5),
                ],
            );
            let s = chain(s, &[hpf(3000.0), lpf(12000.0)]);
            let s = mul(s, &env(n, e!(attack: 0.003, t60: 2.4)));
            lofi(s, LOFI_LINN)
        }),
        "tom-lo" => tom_linn(82.0, 0.7, 5160),
        "tom-mid" => tom_linn(110.0, 0.6, 5161),
        "tom-hi" => tom_linn(147.0, 0.5, 5162),
        "conga-lo" => conga_linn(200.0, 0.3, 5163),
        "conga-hi" => conga_linn(320.0, 0.25, 5165),
        "cabasa" => recipe(0.2, |n| {
            lofi(
                noise_hit(
                    n,
                    5167,
                    &[hpf(4000.0), bpf(8500.0, 0.8)],
                    e!(attack: 0.012, hold: 0.006, t60: 0.08),
                ),
                LOFI_LINN,
            )
        }),
        "tambourine" => recipe(0.6, |n| {
            lofi(
                jingles(
                    n,
                    5168,
                    &[(0.0, 1.0), (0.022, 0.5), (0.05, 0.3)],
                    0.3,
                    &[4800.0, 5900.0, 7100.0, 8400.0, 9800.0],
                ),
                LOFI_LINN,
            )
        }),
        "cowbell" => recipe(0.6, |n| {
            lofi(
                cowbell(
                    n,
                    587.0,
                    845.0,
                    Cowbell {
                        center: 850.0,
                        q: 0.9,
                        fast: 0.06,
                        slow: 0.4,
                        ..COWBELL0
                    },
                ),
                LOFI_LINN,
            )
        }),
        _ => return None,
    })
}
