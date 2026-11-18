//! 909-style voice recipes (`voices.ts`).

use super::VoiceRecipe;
use super::common::{
    CLAP0, Clap, body, bpf, chain, clap, hpf, lpf, metal_hit, noise_hit, recipe, scaled, sum,
};
use crate::classic::dsp::{
    EnvOpts, LofiOpts, PARTIALS_ATTACK, drive, env, hp, lofi, metal, mix_into, mul, noise, partials,
};

const METAL_909: [f64; 6] = [263.0, 400.0, 421.0, 474.0, 587.0, 845.0];

#[derive(Clone, Copy)]
struct Kick909 {
    freq: f64,
    sweep: f64,
    sweep_tau: f64,
    sweep2: f64,
    sweep2_tau: f64,
    t60: f64,
    hold: f64,
    click: f64,
    drive: f64,
}

fn kick909(max_sec: f64, o: Kick909) -> VoiceRecipe {
    recipe(max_sec, move |n| {
        // `{ ...o, attack: 0.0003, shape: 'sine' }`
        let mut s = body(
            n,
            b!(freq: o.freq, sweep: o.sweep, sweep_tau: o.sweep_tau, sweep2: o.sweep2, sweep2_tau: o.sweep2_tau, t60: o.t60, hold: o.hold, attack: 0.0003),
        );
        mix_into(
            &mut s,
            &noise_hit(
                n,
                909,
                &[hpf(800.0), lpf(6000.0)],
                e!(attack: 0.0002, t60: 0.012),
            ),
            o.click,
            0,
        );
        mix_into(
            &mut s,
            &body(n, b!(freq: 1200, t60: 0.004, attack: 0.0001)),
            o.click * 0.5,
            0,
        );
        drive(s, o.drive)
    })
}

#[derive(Clone, Copy)]
struct Snare909 {
    f1: f64,
    f2: f64,
    t1: f64,
    t2: f64,
    noise_hp: f64,
    noise_lp: f64,
    noise_t60: f64,
    noise_gain: f64,
    tone: f64,
}

fn snare909(max_sec: f64, o: Snare909) -> VoiceRecipe {
    recipe(max_sec, move |n| {
        drive(
            sum(
                n,
                &[
                    (
                        body(n, b!(freq: o.f1, sweep: 0.35, sweep_tau: 0.012, t60: o.t1)),
                        0.5 * o.tone,
                    ),
                    (
                        body(
                            n,
                            b!(freq: o.f2, sweep: 0.3, sweep_tau: 0.012, t60: o.t2, phase: 0.15),
                        ),
                        0.3 * o.tone,
                    ),
                    (
                        noise_hit(
                            n,
                            9091,
                            &[hpf(o.noise_hp), lpf(o.noise_lp)],
                            e!(attack: 0.0005, t60: o.noise_t60),
                        ),
                        o.noise_gain,
                    ),
                ],
            ),
            1.3,
        )
    })
}

/// 909 hats: a metallic "ROM sample" quantized to 6 bit, then shaped by the VCA.
fn hat909(max_sec: f64, sample_t60: f64, vca: EnvOpts) -> VoiceRecipe {
    recipe(max_sec, move |n| {
        let s = metal_hit(
            n,
            &METAL_909,
            21,
            &[bpf(9500.0, 0.9), hpf(6500.0)],
            e!(attack: 0.0002, t60: sample_t60),
            0.35,
        );
        let s = lofi(
            s,
            LofiOpts {
                bits: 6,
                ..LofiOpts::D
            },
        );
        let s = hp(s, 5000.0, None);
        mul(s, &env(n, vca))
    })
}

fn tom909(freq: f64, t60: f64, seed: u32) -> VoiceRecipe {
    recipe(0.8, move |n| {
        sum(
            n,
            &[
                (
                    body(n, b!(freq: freq, sweep: 0.45, sweep_tau: 0.04, t60: t60)),
                    1.0,
                ),
                (
                    noise_hit(n, seed, &[lpf(2500.0)], e!(attack: 0.0003, t60: 0.06)),
                    0.25,
                ),
            ],
        )
    })
}

pub(super) const TR909_IDS: &[&str] = &[
    "kick",
    "kick-long",
    "kick-hard",
    "snare",
    "snare-tight",
    "snare-low",
    "rim",
    "clap",
    "chh",
    "phh",
    "ohh",
    "crash",
    "ride",
    "tom-lo",
    "tom-mid",
    "tom-hi",
];

pub(super) fn tr909(voice: &str) -> Option<VoiceRecipe> {
    Some(match voice {
        "kick" => kick909(
            1.0,
            Kick909 {
                freq: 52.0,
                sweep: 3.5,
                sweep_tau: 0.008,
                sweep2: 0.6,
                sweep2_tau: 0.045,
                t60: 0.55,
                hold: 0.02,
                click: 0.4,
                drive: 1.8,
            },
        ),
        "kick-long" => kick909(
            1.4,
            Kick909 {
                freq: 48.0,
                sweep: 3.2,
                sweep_tau: 0.009,
                sweep2: 0.5,
                sweep2_tau: 0.05,
                t60: 1.1,
                hold: 0.04,
                click: 0.3,
                drive: 1.6,
            },
        ),
        "kick-hard" => kick909(
            0.7,
            Kick909 {
                freq: 55.0,
                sweep: 5.0,
                sweep_tau: 0.006,
                sweep2: 0.8,
                sweep2_tau: 0.03,
                t60: 0.4,
                hold: 0.015,
                click: 0.6,
                drive: 3.5,
            },
        ),
        "snare" => snare909(
            0.6,
            Snare909 {
                f1: 180.0,
                f2: 330.0,
                t1: 0.12,
                t2: 0.08,
                noise_hp: 900.0,
                noise_lp: 10000.0,
                noise_t60: 0.28,
                noise_gain: 0.85,
                tone: 1.0,
            },
        ),
        "snare-tight" => snare909(
            0.4,
            Snare909 {
                f1: 220.0,
                f2: 400.0,
                t1: 0.09,
                t2: 0.06,
                noise_hp: 1800.0,
                noise_lp: 12000.0,
                noise_t60: 0.17,
                noise_gain: 0.9,
                tone: 0.9,
            },
        ),
        "snare-low" => snare909(
            0.6,
            Snare909 {
                f1: 160.0,
                f2: 290.0,
                t1: 0.15,
                t2: 0.1,
                noise_hp: 700.0,
                noise_lp: 7000.0,
                noise_t60: 0.32,
                noise_gain: 0.7,
                tone: 1.1,
            },
        ),
        "rim" => recipe(0.15, |n| {
            hp(
                drive(
                    sum(
                        n,
                        &[
                            (body(n, b!(freq: 500, t60: 0.03, attack: 0.0002)), 0.5),
                            (body(n, b!(freq: 1700, t60: 0.025, attack: 0.0002)), 0.6),
                            (body(n, b!(freq: 2400, t60: 0.02, attack: 0.0002)), 0.3),
                        ],
                    ),
                    2.0,
                ),
                400.0,
                None,
            )
        }),
        "clap" => recipe(0.7, |n| {
            clap(
                n,
                9092,
                Clap {
                    center: 1350.0,
                    q: 1.3,
                    bursts: &[0.0, 0.009, 0.019, 0.03],
                    tail_t60: 0.42,
                    tail_gain: 0.6,
                    hp_hz: 700.0,
                    ..CLAP0
                },
            )
        }),
        "chh" => hat909(0.3, 0.4, e!(attack: 0.0002, t60: 0.1)),
        "phh" => hat909(0.4, 0.4, e!(attack: 0.002, t60: 0.22)),
        "ohh" => hat909(1.2, 1.2, e!(attack: 0.0002, hold: 0.02, t60: 0.8)),
        "crash" => recipe(2.6, |n| {
            let s = sum(
                n,
                &[
                    (noise(n, 9093), 0.7),
                    (metal(n, &scaled(&METAL_909, 1.1), 23), 0.5),
                ],
            );
            let s = chain(s, &[hpf(3500.0), bpf(7000.0, 0.5)]);
            let s = mul(s, &env(n, e!(attack: 0.002, t60: 2.2)));
            let s = lofi(
                s,
                LofiOpts {
                    bits: 6,
                    ..LofiOpts::D
                },
            );
            hp(s, 3000.0, None)
        }),
        "ride" => recipe(2.4, |n| {
            let s = sum(
                n,
                &[
                    (
                        metal(n, &[336.0, 497.0, 620.0, 740.0, 912.0, 1173.0], 25),
                        0.6,
                    ),
                    (
                        partials(
                            n,
                            &[(3170.0, 1.0, 1.4), (4650.0, 0.6, 1.0), (5980.0, 0.4, 0.8)],
                            PARTIALS_ATTACK,
                        ),
                        0.12,
                    ),
                    (noise(n, 9094), 0.25),
                ],
            );
            let s = chain(s, &[bpf(6000.0, 0.7), hpf(3500.0)]);
            let s = mul(s, &env(n, e!(attack: 0.0005, t60: 1.8)));
            lofi(
                s,
                LofiOpts {
                    bits: 6,
                    ..LofiOpts::D
                },
            )
        }),
        "tom-lo" => tom909(95.0, 0.5, 9095),
        "tom-mid" => tom909(135.0, 0.45, 9096),
        "tom-hi" => tom909(190.0, 0.4, 9097),
        _ => return None,
    })
}
