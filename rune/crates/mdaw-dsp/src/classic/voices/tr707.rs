//! 707-style voice recipes (`voices.ts`).

use super::VoiceRecipe;
use super::common::{
    CLAP0, COWBELL0, Clap, Cowbell, body, bpf, chain, clap, cowbell, hpf, jingles, lpf, noise_hit,
    recipe, scaled, sum,
};
use crate::classic::dsp::{
    LofiOpts, PARTIALS_ATTACK, drive, env, hp, lofi, metal, mix_into, mul, noise, partials,
};

const LOFI_707: LofiOpts = LofiOpts {
    rate: 25000.0,
    bits: 8,
    mu: false,
};
const LOFI_32K: LofiOpts = LofiOpts {
    rate: 32000.0,
    bits: 8,
    mu: false,
};
const LOFI_30K: LofiOpts = LofiOpts {
    rate: 30000.0,
    bits: 8,
    mu: false,
};
const METAL_707: [f64; 6] = [312.0, 418.0, 497.0, 563.0, 685.0, 921.0];

fn tom707(freq: f64, t60: f64, seed: u32) -> VoiceRecipe {
    recipe(0.7, move |n| {
        lofi(
            drive(
                sum(
                    n,
                    &[
                        (
                            body(
                                n,
                                b!(freq: freq, sweep: 0.5, sweep_tau: 0.05, t60: t60, hold: 0.01),
                            ),
                            1.0,
                        ),
                        (
                            noise_hit(n, seed, &[lpf(4000.0)], e!(attack: 0.0003, t60: 0.02)),
                            0.3,
                        ),
                    ],
                ),
                1.4,
            ),
            LOFI_707,
        )
    })
}

fn hat707(max_sec: f64, t60: f64) -> VoiceRecipe {
    recipe(max_sec, move |n| {
        let s = sum(n, &[(noise(n, 7071), 0.8), (metal(n, &METAL_707, 31), 0.5)]);
        let s = chain(s, &[hpf(7000.0), bpf(9000.0, 0.8)]);
        let s = mul(s, &env(n, e!(attack: 0.0003, t60: t60, hold: 0.004)));
        lofi(s, LOFI_32K)
    })
}

pub(super) const TR707_IDS: &[&str] = &[
    "kick",
    "kick-2",
    "snare",
    "snare-2",
    "rim",
    "clap",
    "cowbell",
    "tambourine",
    "chh",
    "ohh",
    "crash",
    "ride",
    "ride-bell",
    "tom-lo",
    "tom-mid",
    "tom-hi",
];

pub(super) fn tr707(voice: &str) -> Option<VoiceRecipe> {
    Some(match voice {
        "kick" => recipe(0.6, |n| {
            let mut s = body(
                n,
                b!(freq: 56, sweep: 1.6, sweep_tau: 0.012, sweep2: 0.3, sweep2_tau: 0.04, hold: 0.015, t60: 0.38),
            );
            mix_into(
                &mut s,
                &noise_hit(n, 7072, &[bpf(3000.0, 0.8)], e!(attack: 0.0002, t60: 0.008)),
                0.5,
                0,
            );
            lofi(drive(s, 2.2), LOFI_707)
        }),
        "kick-2" => recipe(0.8, |n| {
            let mut s = body(
                n,
                b!(freq: 50, sweep: 1.0, sweep_tau: 0.015, hold: 0.03, t60: 0.55),
            );
            mix_into(
                &mut s,
                &noise_hit(n, 7073, &[lpf(1500.0)], e!(attack: 0.0002, t60: 0.01)),
                0.25,
                0,
            );
            lofi(drive(s, 1.5), LOFI_707)
        }),
        "snare" => recipe(0.45, |n| {
            lofi(
                drive(
                    sum(
                        n,
                        &[
                            (
                                body(n, b!(freq: 195, sweep: 0.3, sweep_tau: 0.01, t60: 0.1)),
                                0.55,
                            ),
                            (body(n, b!(freq: 340, t60: 0.07, phase: 0.2)), 0.25),
                            (
                                noise_hit(n, 7074, &[bpf(4500.0, 0.45)], e!(t60: 0.22, hold: 0.01)),
                                0.8,
                            ),
                            (
                                noise_hit(n, 7075, &[hpf(2500.0)], e!(attack: 0.0002, t60: 0.012)),
                                0.5,
                            ),
                        ],
                    ),
                    1.5,
                ),
                LOFI_707,
            )
        }),
        "snare-2" => recipe(0.4, |n| {
            lofi(
                drive(
                    sum(
                        n,
                        &[
                            (
                                body(n, b!(freq: 250, sweep: 0.3, sweep_tau: 0.01, t60: 0.08)),
                                0.5,
                            ),
                            (body(n, b!(freq: 430, t60: 0.05, phase: 0.2)), 0.25),
                            (
                                noise_hit(
                                    n,
                                    7076,
                                    &[hpf(2000.0), bpf(6000.0, 0.5)],
                                    e!(t60: 0.17, hold: 0.008),
                                ),
                                0.9,
                            ),
                        ],
                    ),
                    1.5,
                ),
                LOFI_707,
            )
        }),
        "rim" => recipe(0.12, |n| {
            lofi(
                sum(
                    n,
                    &[
                        (body(n, b!(freq: 1100, t60: 0.03, attack: 0.0002)), 0.6),
                        (body(n, b!(freq: 2900, t60: 0.02, attack: 0.0002)), 0.4),
                        (
                            noise_hit(n, 7077, &[hpf(3000.0)], e!(attack: 0.0001, t60: 0.006)),
                            0.4,
                        ),
                    ],
                ),
                LOFI_707,
            )
        }),
        "clap" => recipe(0.6, |n| {
            lofi(
                clap(
                    n,
                    7078,
                    Clap {
                        center: 1300.0,
                        q: 1.2,
                        bursts: &[0.0, 0.008, 0.018],
                        burst_t60: 0.02,
                        tail_t60: 0.3,
                        tail_gain: 0.7,
                        ..CLAP0
                    },
                ),
                LOFI_707,
            )
        }),
        "cowbell" => recipe(0.5, |n| {
            lofi(
                cowbell(
                    n,
                    562.0,
                    845.0,
                    Cowbell {
                        center: 845.0,
                        q: 1.1,
                        fast: 0.08,
                        slow: 0.35,
                        ..COWBELL0
                    },
                ),
                LOFI_707,
            )
        }),
        "tambourine" => recipe(0.5, |n| {
            lofi(
                jingles(
                    n,
                    7079,
                    &[(0.0, 1.0), (0.028, 0.6)],
                    0.25,
                    &[5200.0, 6100.0, 7300.0, 8800.0, 10500.0],
                ),
                LOFI_32K,
            )
        }),
        "chh" => hat707(0.25, 0.08),
        "ohh" => hat707(1.0, 0.6),
        "crash" => recipe(2.2, |n| {
            let s = sum(
                n,
                &[
                    (noise(n, 7080), 0.8),
                    (metal(n, &scaled(&METAL_707, 1.3), 33), 0.4),
                ],
            );
            let s = chain(s, &[hpf(4000.0), lpf(14000.0)]);
            let s = mul(s, &env(n, e!(attack: 0.002, t60: 1.8)));
            lofi(s, LOFI_30K)
        }),
        "ride" => recipe(1.8, |n| {
            let s = sum(
                n,
                &[
                    (
                        partials(
                            n,
                            &[
                                (2950.0, 1.0, 1.3),
                                (4300.0, 0.7, 1.0),
                                (5700.0, 0.5, 0.8),
                                (7600.0, 0.3, 0.6),
                            ],
                            PARTIALS_ATTACK,
                        ),
                        0.15,
                    ),
                    (metal(n, &scaled(&METAL_707, 1.7), 35), 0.4),
                    (noise(n, 7081), 0.25),
                ],
            );
            let s = chain(s, &[hpf(3000.0), lpf(13000.0)]);
            let s = mul(s, &env(n, e!(attack: 0.0004, t60: 1.5)));
            lofi(s, LOFI_30K)
        }),
        "ride-bell" => recipe(1.5, |n| {
            let s = sum(
                n,
                &[
                    (
                        partials(
                            n,
                            &[
                                (2400.0, 1.0, 1.3),
                                (3330.0, 0.8, 1.1),
                                (4900.0, 0.5, 0.8),
                                (6600.0, 0.35, 0.6),
                            ],
                            PARTIALS_ATTACK,
                        ),
                        1.0,
                    ),
                    (
                        noise_hit(n, 7082, &[hpf(5000.0)], e!(attack: 0.0003, t60: 0.4)),
                        0.12,
                    ),
                ],
            );
            lofi(hp(s, 1500.0, None), LOFI_30K)
        }),
        "tom-lo" => tom707(92.0, 0.42, 7083),
        "tom-mid" => tom707(128.0, 0.38, 7084),
        "tom-hi" => tom707(172.0, 0.33, 7085),
        _ => return None,
    })
}
