//! Port of `src/audio/sampler/classic/voices.ts`: the voice recipes of the built-in classic kits
//! (808-, 909-, 707-, 606-, LinnDrum-, CR-78-style). Each recipe returns the raw render;
//! [`super::synthesize_classic_voice`] finishes it (DC block, -1 dBFS peak, trimmed tail, capped at
//! `max_sec`). Kept in the same order and with the same literals as the TS file.

use super::dsp::{
    ClapEnvOpts, EnvOpts, FilterType, LofiOpts, OscOpts, PARTIALS_ATTACK, Shape, bp, clap_env,
    drive, env, env_sum, hp, lofi, lp, metal, mix_into, mul, noise, osc, partials, peak, samples,
    square,
};
use mdaw_model::ClassicMachineId;

type Render = Box<dyn Fn(usize) -> Vec<f32> + Send + Sync>;

/// `VoiceRecipe`.
pub struct VoiceRecipe {
    /// Render length cap, seconds (also the maximum sample length).
    pub max_sec: f64,
    render: Render,
}

impl VoiceRecipe {
    /// The raw (unfinished) render.
    pub fn render(&self) -> Vec<f32> {
        (self.render)(samples(self.max_sec))
    }
}

fn recipe(max_sec: f64, render: impl Fn(usize) -> Vec<f32> + Send + Sync + 'static) -> VoiceRecipe {
    VoiceRecipe {
        max_sec,
        render: Box::new(render),
    }
}

/// `OscOpts & EnvOpts` (what `body` takes).
#[derive(Debug, Clone, Copy)]
struct B {
    freq: f64,
    sweep: f64,
    sweep_tau: f64,
    sweep2: f64,
    sweep2_tau: f64,
    drift: f64,
    phase: f64,
    attack: f64,
    hold: f64,
    t60: f64,
    delay: f64,
}

const B0: B = B {
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
    ($($k:ident : $v:expr),* $(,)?) => { B { $($k: ($v) as f64,)* ..B0 } };
}

/// `{ t60: …, … }` envelope options.
macro_rules! e {
    ($($k:ident : $v:expr),* $(,)?) => { EnvOpts { $($k: ($v) as f64,)* ..EnvOpts::D } };
}

type Filter = (FilterType, f64, Option<f64>);
const fn lpf(f: f64) -> Filter {
    (FilterType::Lp, f, None)
}
const fn hpf(f: f64) -> Filter {
    (FilterType::Hp, f, None)
}
const fn bpf(f: f64, q: f64) -> Filter {
    (FilterType::Bp, f, Some(q))
}

fn chain(mut buf: Vec<f32>, filters: &[Filter]) -> Vec<f32> {
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
fn body(n: usize, o: B) -> Vec<f32> {
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
fn noise_hit(n: usize, seed: u32, filters: &[Filter], e: EnvOpts) -> Vec<f32> {
    mul(chain(noise(n, seed), filters), &env(n, e))
}

/// Sum of `(signal, gain)` pairs.
fn sum(n: usize, parts: &[(Vec<f32>, f64)]) -> Vec<f32> {
    let mut out = vec![0.0; n];
    for (s, g) in parts {
        mix_into(&mut out, s, *g, 0);
    }
    out
}

struct Clap<'a> {
    center: f64,
    q: f64,
    bursts: &'a [f64],
    burst_t60: f64,
    tail_t60: f64,
    tail_gain: f64,
    hp_hz: f64,
}

const CLAP0: Clap<'static> = Clap {
    center: 0.0,
    q: 1.4,
    bursts: &[],
    burst_t60: 0.022,
    tail_t60: 0.0,
    tail_gain: 0.55,
    hp_hz: 600.0,
};

/// Hand clap: band-passed noise through a multi-burst envelope.
fn clap(n: usize, seed: u32, o: Clap) -> Vec<f32> {
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

struct Cowbell {
    center: f64,
    q: f64,
    fast: f64,
    slow: f64,
    lp_hz: f64,
}

const COWBELL0: Cowbell = Cowbell {
    center: 0.0,
    q: 1.0,
    fast: 0.07,
    slow: 0.4,
    lp_hz: 9000.0,
};

/// Two-square cowbell through a band-pass, fast drop into a ringing tail.
fn cowbell(n: usize, f1: f64, f2: f64, o: Cowbell) -> Vec<f32> {
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
fn metal_hit(
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
fn jingles(n: usize, seed: u32, hits: &[(f64, f64)], t60: f64, freqs: &[f64]) -> Vec<f32> {
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

struct Membrane {
    freq: f64,
    sweep: f64,
    sweep_tau: f64,
    t60: f64,
    overtone: f64,
    noise_gain: f64,
    attack: f64,
}

/// Inharmonic membrane: fundamental + (1.59×) overtone + optional head noise.
fn membrane(n: usize, seed: u32, o: Membrane) -> Vec<f32> {
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

fn scaled(freqs: &[f64], k: f64) -> Vec<f64> {
    freqs.iter().map(|f| f * k).collect()
}

// ---------------------------------------------------------------------------
// 808-style
// ---------------------------------------------------------------------------

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

const TR808_IDS: &[&str] = &[
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

fn tr808(voice: &str) -> Option<VoiceRecipe> {
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

// ---------------------------------------------------------------------------
// 909-style
// ---------------------------------------------------------------------------

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

const TR909_IDS: &[&str] = &[
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

fn tr909(voice: &str) -> Option<VoiceRecipe> {
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

// ---------------------------------------------------------------------------
// 707-style
// ---------------------------------------------------------------------------

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

const TR707_IDS: &[&str] = &[
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

fn tr707(voice: &str) -> Option<VoiceRecipe> {
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

// ---------------------------------------------------------------------------
// 606-style
// ---------------------------------------------------------------------------

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

const TR606_IDS: &[&str] = &[
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

fn tr606(voice: &str) -> Option<VoiceRecipe> {
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

// ---------------------------------------------------------------------------
// LinnDrum-style
// ---------------------------------------------------------------------------

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

const LINN_IDS: &[&str] = &[
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

fn linn(voice: &str) -> Option<VoiceRecipe> {
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

// ---------------------------------------------------------------------------
// CR-78-style
// ---------------------------------------------------------------------------

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

const CR78_IDS: &[&str] = &[
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

fn cr78(voice: &str) -> Option<VoiceRecipe> {
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

/// `CLASSIC_RECIPES[machine][voice]`.
pub fn classic_recipe(machine: ClassicMachineId, voice: &str) -> Option<VoiceRecipe> {
    match machine {
        ClassicMachineId::Tr808 => tr808(voice),
        ClassicMachineId::Tr909 => tr909(voice),
        ClassicMachineId::Tr707 => tr707(voice),
        ClassicMachineId::Tr606 => tr606(voice),
        ClassicMachineId::Linn => linn(voice),
        ClassicMachineId::Cr78 => cr78(voice),
    }
}

/// `Object.keys(CLASSIC_RECIPES[machine])`, in definition order.
pub fn classic_recipe_ids(machine: ClassicMachineId) -> &'static [&'static str] {
    match machine {
        ClassicMachineId::Tr808 => TR808_IDS,
        ClassicMachineId::Tr909 => TR909_IDS,
        ClassicMachineId::Tr707 => TR707_IDS,
        ClassicMachineId::Tr606 => TR606_IDS,
        ClassicMachineId::Linn => LINN_IDS,
        ClassicMachineId::Cr78 => CR78_IDS,
    }
}
