/**
 * Voice recipes of the built-in classic kits. Original syntheses that follow
 * how the classic machines generate each sound:
 *
 * - 808-style: bridged-T resonators ring as decaying sines with a slight pitch
 *   sweep (kick, toms, congas, rim, clave); snare = two tuned resonators +
 *   high-passed noise; six detuned square oscillators through band-passes
 *   for hats and cymbal; cowbell = two squares through a band-pass; clap =
 *   multi-burst noise + reverb tail.
 * - 909-style: kick with a steep pitch envelope + beater click and drive,
 *   noise-heavy snares, toms with noise; hats/crash/ride are metallic noise
 *   stored at 6 bit (quantized at full scale, then shaped by a VCA).
 * - 707-style: sample-like, punchy envelopes (hold + fast decay), 8-bit at a
 *   25 kHz-ish rate.
 * - 606-style: thinner, higher, shorter voices; very tight hats.
 * - LinnDrum-style: acoustic-modelled drums (membrane modes, beaters, room),
 *   μ-law companded 8 bit at 28 kHz.
 * - CR-78-style: soft attacks, rounded resonators, no clicks, gentle top end.
 *
 * Each recipe returns the raw render; `synthesizeClassicVoice` finishes it
 * (DC block, -1 dBFS peak, trimmed tail, capped at `maxSec`).
 */
import type { ClassicMachineId } from '@/types/sampler';
import {
  bp,
  clapEnv,
  drive,
  env,
  envSum,
  hp,
  lofi,
  lp,
  metal,
  mixInto,
  mul,
  noise,
  osc,
  partials,
  peak,
  samples,
  square,
  type EnvOpts,
  type OscOpts,
} from './dsp';

export interface VoiceRecipe {
  /** Render length cap, seconds (also the maximum sample length). */
  maxSec: number;
  render: () => Float32Array;
}

type Filter = ['lp' | 'hp' | 'bp', number, number?];

const recipe = (maxSec: number, render: (n: number) => Float32Array): VoiceRecipe => ({
  maxSec,
  render: () => render(samples(maxSec)),
});

function chain(buf: Float32Array, filters: readonly Filter[]): Float32Array {
  for (const [type, f, q] of filters) {
    if (type === 'lp') lp(buf, f, q);
    else if (type === 'hp') hp(buf, f, q);
    else bp(buf, f, q);
  }
  return buf;
}

/** Pitched drum body: oscillator with pitch envelope under an AD envelope. */
const body = (n: number, o: OscOpts & EnvOpts): Float32Array => mul(osc(n, o), env(n, o));

/** Filtered noise under an envelope. */
const noiseHit = (n: number, seed: number, filters: readonly Filter[], e: EnvOpts): Float32Array =>
  mul(chain(noise(n, seed), filters), env(n, e));

/** Sum of [signal, gain] pairs. */
function sum(n: number, parts: [Float32Array, number][]): Float32Array {
  const out = new Float32Array(n);
  for (const [s, g] of parts) mixInto(out, s, g);
  return out;
}

/** Hand clap: band-passed noise through a multi-burst envelope. */
function clap(
  n: number,
  seed: number,
  { center, q = 1.4, bursts, burstT60 = 0.022, tailT60, tailGain = 0.55, hpHz = 600 }:
  { center: number; q?: number; bursts: number[]; burstT60?: number; tailT60: number; tailGain?: number; hpHz?: number },
): Float32Array {
  const s = chain(noise(n, seed), [['bp', center, q], ['hp', hpHz]]);
  return mul(s, clapEnv(n, { bursts, burstT60, tailDelay: bursts[bursts.length - 1], tailT60, tailGain }));
}

/** Two-square cowbell through a band-pass, fast drop into a ringing tail. */
function cowbell(n: number, f1: number, f2: number, { center, q = 1, fast = 0.07, slow = 0.4, lpHz = 9000 }: { center: number; q?: number; fast?: number; slow?: number; lpHz?: number }): Float32Array {
  const s = sum(n, [[square(n, f1), 0.5], [square(n, f2, 0.3), 0.5]]);
  chain(s, [['bp', center, q], ['lp', lpHz]]);
  return mul(s, envSum(n, [[0.72, { attack: 0.0005, t60: fast }], [0.28, { attack: 0.0005, t60: slow }]]));
}

/** Hi-hat / cymbal from a metallic source (+ optional noise) through filters. */
function metalHit(n: number, freqs: readonly number[], seed: number, filters: readonly Filter[], e: EnvOpts, noiseMix = 0): Float32Array {
  const s = metal(n, freqs, seed);
  if (noiseMix) mixInto(s, noise(n, seed + 1), noiseMix);
  return mul(chain(s, filters), env(n, e));
}

/** Tambourine-style jingles: high inharmonic partials + noise, struck `hits` times. */
function jingles(n: number, seed: number, { hits, t60, freqs }: { hits: [time: number, gain: number][]; t60: number; freqs: readonly number[] }): Float32Array {
  const one = partials(n, freqs.map((f, i) => [f, 1 / (1 + i * 0.35), t60 * (1 - i * 0.08)] as [number, number, number]));
  mixInto(one, mul(chain(noise(n, seed), [['hp', 6000], ['bp', 9000, 0.7]]), env(n, { attack: 0.001, t60: t60 * 0.6 })), 1.2);
  const out = new Float32Array(n);
  for (const [t, g] of hits) mixInto(out, one, g, samples(t));
  return out;
}

/** Inharmonic membrane: fundamental + (1.59×) overtone + optional head noise. */
function membrane(n: number, seed: number, { freq, sweep, sweepTau, t60, overtone = 0.25, noiseGain = 0.2, attack = 0.0005 }: { freq: number; sweep: number; sweepTau: number; t60: number; overtone?: number; noiseGain?: number; attack?: number }): Float32Array {
  return sum(n, [
    [body(n, { freq, sweep, sweepTau, t60, attack }), 1],
    [body(n, { freq: freq * 1.59, sweep, sweepTau, t60: t60 * 0.4, attack, phase: 0.1 }), overtone],
    [noiseHit(n, seed, [['lp', 3000], ['hp', 200]], { attack: 0.0003, t60: 0.02 }), noiseGain],
  ]);
}

// ---------------------------------------------------------------------------
// 808-style
// ---------------------------------------------------------------------------

/** The six square oscillators of the 808 metal circuit (Hz). */
const METAL_808 = [205.3, 304.4, 369.6, 522.7, 540, 800];

function tom808(freq: number, t60: number, seed: number): VoiceRecipe {
  return recipe(1.2, (n) =>
    sum(n, [
      [body(n, { freq, sweep: 0.22, sweepTau: 0.03, t60, attack: 0.0006 }), 1],
      [noiseHit(n, seed, [['lp', 2500]], { attack: 0.0005, t60: 0.06 }), 0.12],
    ]),
  );
}

function conga808(freq: number, t60: number): VoiceRecipe {
  return recipe(0.5, (n) => body(n, { freq, sweep: 0.12, sweepTau: 0.008, t60, attack: 0.0006 }));
}

const TR808: Record<string, VoiceRecipe> = {
  kick: recipe(1.6, (n) => {
    const s = body(n, { freq: 50, sweep: 0.55, sweepTau: 0.006, sweep2: 0.12, sweep2Tau: 0.06, t60: 1.15, attack: 0.0004 });
    mixInto(s, body(n, { freq: 1400, t60: 0.006, attack: 0.0002 }), 0.12); // trigger-pulse click
    return lp(drive(s, 1.2), 5000);
  }),
  'kick-long': recipe(2.8, (n) => {
    const s = body(n, { freq: 46, sweep: 0.5, sweepTau: 0.008, sweep2: 0.1, sweep2Tau: 0.08, drift: 0.94, hold: 0.05, t60: 2.6, attack: 0.0004 });
    return lp(drive(s, 2.2), 1800);
  }),
  snare: recipe(0.6, (n) =>
    sum(n, [
      [body(n, { freq: 238, sweep: 0.25, sweepTau: 0.004, t60: 0.16 }), 0.55],
      [body(n, { freq: 476, sweep: 0.15, sweepTau: 0.004, t60: 0.09, phase: 0.2 }), 0.3],
      [noiseHit(n, 808, [['hp', 1800], ['lp', 11000]], { t60: 0.24 }), 0.6],
    ]),
  ),
  rim: recipe(0.15, (n) => {
    const s = sum(n, [
      [body(n, { freq: 1667, t60: 0.03, attack: 0.0002 }), 0.7],
      [body(n, { freq: 455, t60: 0.04, attack: 0.0002 }), 0.6],
    ]);
    return hp(drive(s, 2), 250);
  }),
  clap: recipe(0.8, (n) => clap(n, 8081, { center: 1100, q: 1.6, bursts: [0, 0.011, 0.022, 0.034], tailT60: 0.34 })),
  chh: recipe(0.3, (n) => metalHit(n, METAL_808, 11, [['bp', 10000, 1], ['hp', 7000]], { attack: 0.0003, t60: 0.09 })),
  ohh: recipe(1.2, (n) => metalHit(n, METAL_808, 11, [['bp', 10000, 1], ['hp', 7000]], { attack: 0.0003, hold: 0.02, t60: 0.75 })),
  cymbal: recipe(2.4, (n) => {
    const m = metal(n, METAL_808, 13);
    const low = mul(bp(m.slice(), 3440, 2), env(n, { attack: 0.0005, t60: 0.55 }));
    const high = mul(hp(bp(m, 7100, 1.2), 5500), env(n, { attack: 0.0005, t60: 2.0 }));
    return sum(n, [[low, 0.35], [high, 0.9]]);
  }),
  cowbell: recipe(0.7, (n) => cowbell(n, 540, 800, { center: 700, q: 0.9, fast: 0.07, slow: 0.5 })),
  clave: recipe(0.2, (n) => body(n, { freq: 2500, t60: 0.06, attack: 0.0002 })),
  maracas: recipe(0.2, (n) => noiseHit(n, 8082, [['hp', 5000], ['lp', 13000]], { attack: 0.004, t60: 0.07 })),
  'tom-lo': tom808(85, 0.75, 8083),
  'tom-mid': tom808(125, 0.6, 8084),
  'tom-hi': tom808(170, 0.5, 8085),
  'conga-lo': conga808(170, 0.22),
  'conga-mid': conga808(250, 0.2),
  'conga-hi': conga808(370, 0.17),
};

// ---------------------------------------------------------------------------
// 909-style
// ---------------------------------------------------------------------------

const METAL_909 = [263, 400, 421, 474, 587, 845];

function kick909(maxSec: number, o: { freq: number; sweep: number; sweepTau: number; sweep2: number; sweep2Tau: number; t60: number; hold: number; click: number; drive: number }): VoiceRecipe {
  return recipe(maxSec, (n) => {
    const s = body(n, { ...o, attack: 0.0003, shape: 'sine' });
    mixInto(s, noiseHit(n, 909, [['hp', 800], ['lp', 6000]], { attack: 0.0002, t60: 0.012 }), o.click);
    mixInto(s, body(n, { freq: 1200, t60: 0.004, attack: 0.0001 }), o.click * 0.5);
    return drive(s, o.drive);
  });
}

function snare909(maxSec: number, o: { f1: number; f2: number; t1: number; t2: number; noiseHp: number; noiseLp: number; noiseT60: number; noiseGain: number; tone: number }): VoiceRecipe {
  return recipe(maxSec, (n) =>
    drive(
      sum(n, [
        [body(n, { freq: o.f1, sweep: 0.35, sweepTau: 0.012, t60: o.t1 }), 0.5 * o.tone],
        [body(n, { freq: o.f2, sweep: 0.3, sweepTau: 0.012, t60: o.t2, phase: 0.15 }), 0.3 * o.tone],
        [noiseHit(n, 9091, [['hp', o.noiseHp], ['lp', o.noiseLp]], { attack: 0.0005, t60: o.noiseT60 }), o.noiseGain],
      ]),
      1.3,
    ),
  );
}

/** 909 hats: a metallic "ROM sample" quantized to 6 bit, then shaped by the VCA. */
function hat909(maxSec: number, sampleT60: number, vca: EnvOpts): VoiceRecipe {
  return recipe(maxSec, (n) => {
    const s = metalHit(n, METAL_909, 21, [['bp', 9500, 0.9], ['hp', 6500]], { attack: 0.0002, t60: sampleT60 }, 0.35);
    lofi(s, { bits: 6 });
    hp(s, 5000);
    return mul(s, env(n, vca));
  });
}

function tom909(freq: number, t60: number, seed: number): VoiceRecipe {
  return recipe(0.8, (n) =>
    sum(n, [
      [body(n, { freq, sweep: 0.45, sweepTau: 0.04, t60 }), 1],
      [noiseHit(n, seed, [['lp', 2500]], { attack: 0.0003, t60: 0.06 }), 0.25],
    ]),
  );
}

const TR909: Record<string, VoiceRecipe> = {
  kick: kick909(1.0, { freq: 52, sweep: 3.5, sweepTau: 0.008, sweep2: 0.6, sweep2Tau: 0.045, t60: 0.55, hold: 0.02, click: 0.4, drive: 1.8 }),
  'kick-long': kick909(1.4, { freq: 48, sweep: 3.2, sweepTau: 0.009, sweep2: 0.5, sweep2Tau: 0.05, t60: 1.1, hold: 0.04, click: 0.3, drive: 1.6 }),
  'kick-hard': kick909(0.7, { freq: 55, sweep: 5, sweepTau: 0.006, sweep2: 0.8, sweep2Tau: 0.03, t60: 0.4, hold: 0.015, click: 0.6, drive: 3.5 }),
  snare: snare909(0.6, { f1: 180, f2: 330, t1: 0.12, t2: 0.08, noiseHp: 900, noiseLp: 10000, noiseT60: 0.28, noiseGain: 0.85, tone: 1 }),
  'snare-tight': snare909(0.4, { f1: 220, f2: 400, t1: 0.09, t2: 0.06, noiseHp: 1800, noiseLp: 12000, noiseT60: 0.17, noiseGain: 0.9, tone: 0.9 }),
  'snare-low': snare909(0.6, { f1: 160, f2: 290, t1: 0.15, t2: 0.1, noiseHp: 700, noiseLp: 7000, noiseT60: 0.32, noiseGain: 0.7, tone: 1.1 }),
  rim: recipe(0.15, (n) =>
    hp(
      drive(
        sum(n, [
          [body(n, { freq: 500, t60: 0.03, attack: 0.0002 }), 0.5],
          [body(n, { freq: 1700, t60: 0.025, attack: 0.0002 }), 0.6],
          [body(n, { freq: 2400, t60: 0.02, attack: 0.0002 }), 0.3],
        ]),
        2,
      ),
      400,
    ),
  ),
  clap: recipe(0.7, (n) => clap(n, 9092, { center: 1350, q: 1.3, bursts: [0, 0.009, 0.019, 0.03], tailT60: 0.42, tailGain: 0.6, hpHz: 700 })),
  chh: hat909(0.3, 0.4, { attack: 0.0002, t60: 0.1 }),
  phh: hat909(0.4, 0.4, { attack: 0.002, t60: 0.22 }),
  ohh: hat909(1.2, 1.2, { attack: 0.0002, hold: 0.02, t60: 0.8 }),
  crash: recipe(2.6, (n) => {
    const s = sum(n, [[noise(n, 9093), 0.7], [metal(n, METAL_909.map((f) => f * 1.1), 23), 0.5]]);
    chain(s, [['hp', 3500], ['bp', 7000, 0.5]]);
    mul(s, env(n, { attack: 0.002, t60: 2.2 }));
    lofi(s, { bits: 6 });
    return hp(s, 3000);
  }),
  ride: recipe(2.4, (n) => {
    const s = sum(n, [
      [metal(n, [336, 497, 620, 740, 912, 1173], 25), 0.6],
      [partials(n, [[3170, 1, 1.4], [4650, 0.6, 1.0], [5980, 0.4, 0.8]]), 0.12],
      [noise(n, 9094), 0.25],
    ]);
    chain(s, [['bp', 6000, 0.7], ['hp', 3500]]);
    mul(s, env(n, { attack: 0.0005, t60: 1.8 }));
    return lofi(s, { bits: 6 });
  }),
  'tom-lo': tom909(95, 0.5, 9095),
  'tom-mid': tom909(135, 0.45, 9096),
  'tom-hi': tom909(190, 0.4, 9097),
};

// ---------------------------------------------------------------------------
// 707-style
// ---------------------------------------------------------------------------

const LOFI_707 = { rate: 25000, bits: 8 } as const;
const METAL_707 = [312, 418, 497, 563, 685, 921];

function tom707(freq: number, t60: number, seed: number): VoiceRecipe {
  return recipe(0.7, (n) =>
    lofi(
      drive(
        sum(n, [
          [body(n, { freq, sweep: 0.5, sweepTau: 0.05, t60, hold: 0.01 }), 1],
          [noiseHit(n, seed, [['lp', 4000]], { attack: 0.0003, t60: 0.02 }), 0.3],
        ]),
        1.4,
      ),
      LOFI_707,
    ),
  );
}

function hat707(maxSec: number, t60: number): VoiceRecipe {
  return recipe(maxSec, (n) => {
    const s = sum(n, [[noise(n, 7071), 0.8], [metal(n, METAL_707, 31), 0.5]]);
    chain(s, [['hp', 7000], ['bp', 9000, 0.8]]);
    mul(s, env(n, { attack: 0.0003, t60, hold: 0.004 }));
    return lofi(s, { rate: 32000, bits: 8 });
  });
}

const TR707: Record<string, VoiceRecipe> = {
  kick: recipe(0.6, (n) => {
    const s = body(n, { freq: 56, sweep: 1.6, sweepTau: 0.012, sweep2: 0.3, sweep2Tau: 0.04, hold: 0.015, t60: 0.38 });
    mixInto(s, noiseHit(n, 7072, [['bp', 3000, 0.8]], { attack: 0.0002, t60: 0.008 }), 0.5);
    return lofi(drive(s, 2.2), LOFI_707);
  }),
  'kick-2': recipe(0.8, (n) => {
    const s = body(n, { freq: 50, sweep: 1.0, sweepTau: 0.015, hold: 0.03, t60: 0.55 });
    mixInto(s, noiseHit(n, 7073, [['lp', 1500]], { attack: 0.0002, t60: 0.01 }), 0.25);
    return lofi(drive(s, 1.5), LOFI_707);
  }),
  snare: recipe(0.45, (n) =>
    lofi(
      drive(
        sum(n, [
          [body(n, { freq: 195, sweep: 0.3, sweepTau: 0.01, t60: 0.1 }), 0.55],
          [body(n, { freq: 340, t60: 0.07, phase: 0.2 }), 0.25],
          [noiseHit(n, 7074, [['bp', 4500, 0.45]], { t60: 0.22, hold: 0.01 }), 0.8],
          [noiseHit(n, 7075, [['hp', 2500]], { attack: 0.0002, t60: 0.012 }), 0.5],
        ]),
        1.5,
      ),
      LOFI_707,
    ),
  ),
  'snare-2': recipe(0.4, (n) =>
    lofi(
      drive(
        sum(n, [
          [body(n, { freq: 250, sweep: 0.3, sweepTau: 0.01, t60: 0.08 }), 0.5],
          [body(n, { freq: 430, t60: 0.05, phase: 0.2 }), 0.25],
          [noiseHit(n, 7076, [['hp', 2000], ['bp', 6000, 0.5]], { t60: 0.17, hold: 0.008 }), 0.9],
        ]),
        1.5,
      ),
      LOFI_707,
    ),
  ),
  rim: recipe(0.12, (n) =>
    lofi(
      sum(n, [
        [body(n, { freq: 1100, t60: 0.03, attack: 0.0002 }), 0.6],
        [body(n, { freq: 2900, t60: 0.02, attack: 0.0002 }), 0.4],
        [noiseHit(n, 7077, [['hp', 3000]], { attack: 0.0001, t60: 0.006 }), 0.4],
      ]),
      LOFI_707,
    ),
  ),
  clap: recipe(0.6, (n) => lofi(clap(n, 7078, { center: 1300, q: 1.2, bursts: [0, 0.008, 0.018], burstT60: 0.02, tailT60: 0.3, tailGain: 0.7 }), LOFI_707)),
  cowbell: recipe(0.5, (n) => lofi(cowbell(n, 562, 845, { center: 845, q: 1.1, fast: 0.08, slow: 0.35 }), LOFI_707)),
  tambourine: recipe(0.5, (n) =>
    lofi(jingles(n, 7079, { hits: [[0, 1], [0.028, 0.6]], t60: 0.25, freqs: [5200, 6100, 7300, 8800, 10500] }), { rate: 32000, bits: 8 }),
  ),
  chh: hat707(0.25, 0.08),
  ohh: hat707(1.0, 0.6),
  crash: recipe(2.2, (n) => {
    const s = sum(n, [[noise(n, 7080), 0.8], [metal(n, METAL_707.map((f) => f * 1.3), 33), 0.4]]);
    chain(s, [['hp', 4000], ['lp', 14000]]);
    mul(s, env(n, { attack: 0.002, t60: 1.8 }));
    return lofi(s, { rate: 30000, bits: 8 });
  }),
  ride: recipe(1.8, (n) => {
    const s = sum(n, [
      [partials(n, [[2950, 1, 1.3], [4300, 0.7, 1.0], [5700, 0.5, 0.8], [7600, 0.3, 0.6]]), 0.15],
      [metal(n, METAL_707.map((f) => f * 1.7), 35), 0.4],
      [noise(n, 7081), 0.25],
    ]);
    chain(s, [['hp', 3000], ['lp', 13000]]);
    mul(s, env(n, { attack: 0.0004, t60: 1.5 }));
    return lofi(s, { rate: 30000, bits: 8 });
  }),
  'ride-bell': recipe(1.5, (n) => {
    const s = sum(n, [
      [partials(n, [[2400, 1, 1.3], [3330, 0.8, 1.1], [4900, 0.5, 0.8], [6600, 0.35, 0.6]]), 1],
      [noiseHit(n, 7082, [['hp', 5000]], { attack: 0.0003, t60: 0.4 }), 0.12],
    ]);
    return lofi(hp(s, 1500), { rate: 30000, bits: 8 });
  }),
  'tom-lo': tom707(92, 0.42, 7083),
  'tom-mid': tom707(128, 0.38, 7084),
  'tom-hi': tom707(172, 0.33, 7085),
};

// ---------------------------------------------------------------------------
// 606-style
// ---------------------------------------------------------------------------

const METAL_606 = [245, 306, 365, 415, 437, 619];

function kick606(maxSec: number, freq: number, sweep: number, t60: number): VoiceRecipe {
  return recipe(maxSec, (n) => {
    const s = body(n, { freq, sweep, sweepTau: 0.008, t60, attack: 0.0004 });
    mixInto(s, body(n, { freq: 1800, t60: 0.004, attack: 0.0001 }), 0.12);
    return lp(hp(s, 35), 4000);
  });
}

function hat606(maxSec: number, e: EnvOpts): VoiceRecipe {
  return recipe(maxSec, (n) => metalHit(n, METAL_606, 41, [['bp', 11000, 1.2], ['hp', 8000]], e));
}

const tom606 = (freq: number, t60: number): VoiceRecipe =>
  recipe(0.45, (n) => body(n, { freq, sweep: 0.3, sweepTau: 0.02, t60, attack: 0.0005 }));

const TR606: Record<string, VoiceRecipe> = {
  kick: kick606(0.5, 64, 0.9, 0.32),
  'kick-long': kick606(0.9, 58, 0.8, 0.7),
  'kick-short': kick606(0.3, 70, 1.2, 0.18),
  snare: recipe(0.35, (n) =>
    sum(n, [
      [body(n, { freq: 220, sweep: 0.2, sweepTau: 0.005, t60: 0.07 }), 0.35],
      [body(n, { freq: 380, t60: 0.05, phase: 0.2 }), 0.15],
      [noiseHit(n, 6061, [['hp', 2500], ['lp', 12000]], { t60: 0.16 }), 0.8],
    ]),
  ),
  'snare-snappy': recipe(0.45, (n) =>
    sum(n, [
      [body(n, { freq: 230, sweep: 0.2, sweepTau: 0.005, t60: 0.06 }), 0.3],
      [noiseHit(n, 6062, [['hp', 1800], ['lp', 13000]], { t60: 0.24 }), 1],
    ]),
  ),
  rim: recipe(0.1, (n) =>
    hp(sum(n, [[body(n, { freq: 1800, t60: 0.02, attack: 0.0002 }), 0.7], [body(n, { freq: 600, t60: 0.025, attack: 0.0002 }), 0.4]]), 500),
  ),
  clap: recipe(0.45, (n) => clap(n, 6063, { center: 1600, q: 1.5, bursts: [0, 0.007, 0.015, 0.024], burstT60: 0.018, tailT60: 0.22, tailGain: 0.45, hpHz: 900 })),
  chh: hat606(0.15, { attack: 0.0002, t60: 0.045 }),
  phh: hat606(0.25, { attack: 0.0015, t60: 0.13 }),
  ohh: hat606(0.7, { attack: 0.0002, hold: 0.01, t60: 0.42 }),
  cymbal: recipe(1.6, (n) => {
    const m = metal(n, METAL_606, 43);
    const high = mul(hp(bp(m.slice(), 8000, 0.8), 5500), env(n, { attack: 0.001, t60: 1.3 }));
    const low = mul(bp(m, 4000, 1.5), env(n, { attack: 0.001, t60: 0.3 }));
    return sum(n, [[high, 1], [low, 0.25]]);
  }),
  shaker: recipe(0.15, (n) => noiseHit(n, 6064, [['hp', 6000], ['bp', 9000, 0.7]], { attack: 0.01, hold: 0.01, t60: 0.06 })),
  zap: recipe(0.2, (n) => body(n, { freq: 180, sweep: 8, sweepTau: 0.012, t60: 0.12, attack: 0.0003 })),
  'tom-lo': tom606(115, 0.32),
  'tom-mid': tom606(160, 0.28),
  'tom-hi': tom606(215, 0.24),
};

// ---------------------------------------------------------------------------
// LinnDrum-style
// ---------------------------------------------------------------------------

const LOFI_LINN = { rate: 28000, bits: 8, mu: true } as const;
const METAL_LINN = [327, 461, 552, 714, 943, 1203];

function hatLinn(maxSec: number, e: EnvOpts): VoiceRecipe {
  return recipe(maxSec, (n) => {
    const s = sum(n, [[metal(n, METAL_LINN, 51), 0.5], [noise(n, 5151), 0.8]]);
    chain(s, [['bp', 9000, 0.7], ['hp', 6000]]);
    mul(s, env(n, e));
    return lofi(s, LOFI_LINN);
  });
}

const tomLinn = (freq: number, t60: number, seed: number): VoiceRecipe =>
  recipe(1.0, (n) => lofi(membrane(n, seed, { freq, sweep: 0.3, sweepTau: 0.06, t60, noiseGain: 0.25 }), LOFI_LINN));

const congaLinn = (freq: number, t60: number, seed: number): VoiceRecipe =>
  recipe(0.5, (n) => {
    const s = membrane(n, seed, { freq, sweep: 0.15, sweepTau: 0.01, t60, overtone: 0.2, noiseGain: 0 });
    mixInto(s, noiseHit(n, seed + 1, [['bp', 2500, 1]], { attack: 0.0002, t60: 0.01 }), 0.3);
    return lofi(s, LOFI_LINN);
  });

const LINN: Record<string, VoiceRecipe> = {
  kick: recipe(0.5, (n) => {
    const s = sum(n, [
      [body(n, { freq: 60, sweep: 0.9, sweepTau: 0.015, sweep2: 0.15, sweep2Tau: 0.05, hold: 0.005, t60: 0.3 }), 1],
      [body(n, { freq: 140, t60: 0.06, phase: 0.1 }), 0.2],
      [noiseHit(n, 5152, [['lp', 3500], ['hp', 300]], { attack: 0.0003, t60: 0.012 }), 0.6],
    ]);
    return lofi(drive(s, 1.6), LOFI_LINN);
  }),
  snare: recipe(0.6, (n) => {
    const s = sum(n, [
      [body(n, { freq: 185, sweep: 0.15, sweepTau: 0.01, t60: 0.13 }), 0.55],
      [body(n, { freq: 330, t60: 0.09, phase: 0.2 }), 0.3],
      [body(n, { freq: 510, t60: 0.05, phase: 0.4 }), 0.12],
      [noiseHit(n, 5153, [['bp', 5000, 0.35], ['lp', 10000]], { t60: 0.32, hold: 0.005 }), 0.7],
      [noiseHit(n, 5154, [['lp', 6000]], { attack: 0.01, delay: 0.01, t60: 0.4 }), 0.15],
    ]);
    return lofi(drive(s, 1.3), LOFI_LINN);
  }),
  sidestick: recipe(0.15, (n) =>
    lofi(
      sum(n, [
        [body(n, { freq: 420, sweep: 0.1, sweepTau: 0.005, t60: 0.05, attack: 0.0002 }), 0.5],
        [body(n, { freq: 1600, t60: 0.03, attack: 0.0002 }), 0.45],
        [body(n, { freq: 2650, t60: 0.02, attack: 0.0002 }), 0.2],
        [noiseHit(n, 5155, [['hp', 2000]], { attack: 0.0001, t60: 0.005 }), 0.4],
      ]),
      LOFI_LINN,
    ),
  ),
  clap: recipe(0.6, (n) => {
    const s = clap(n, 5156, { center: 1200, q: 1.0, bursts: [0, 0.0065, 0.017, 0.024, 0.036], burstT60: 0.018, tailT60: 0.38, tailGain: 0.5 });
    return lofi(peak(s, 2500, 1, 4), LOFI_LINN);
  }),
  chh: hatLinn(0.25, { attack: 0.0008, t60: 0.09 }),
  ohh: hatLinn(1.1, { attack: 0.0008, hold: 0.02, t60: 0.75 }),
  ride: recipe(2.5, (n) => {
    const wash = sum(n, [[noise(n, 5157), 0.5], [metal(n, METAL_LINN.map((f) => f * 1.4), 53), 0.5]]);
    chain(wash, [['hp', 4500], ['bp', 7000, 0.5]]);
    mul(wash, env(n, { attack: 0.001, t60: 2.2 }));
    const s = sum(n, [
      [wash, 0.6],
      [partials(n, [[2400, 1, 1.5], [3550, 0.8, 1.2], [5200, 0.6, 1.0], [6300, 0.4, 0.8]]), 0.12],
      [noiseHit(n, 5158, [['hp', 3000]], { attack: 0.0002, t60: 0.01 }), 0.2],
    ]);
    return lofi(s, LOFI_LINN);
  }),
  crash: recipe(2.6, (n) => {
    const s = sum(n, [[noise(n, 5159), 0.7], [metal(n, METAL_LINN.map((f) => f * 1.2), 55), 0.5]]);
    chain(s, [['hp', 3000], ['lp', 12000]]);
    mul(s, env(n, { attack: 0.003, t60: 2.4 }));
    return lofi(s, LOFI_LINN);
  }),
  'tom-lo': tomLinn(82, 0.7, 5160),
  'tom-mid': tomLinn(110, 0.6, 5161),
  'tom-hi': tomLinn(147, 0.5, 5162),
  'conga-lo': congaLinn(200, 0.3, 5163),
  'conga-hi': congaLinn(320, 0.25, 5165),
  cabasa: recipe(0.2, (n) => lofi(noiseHit(n, 5167, [['hp', 4000], ['bp', 8500, 0.8]], { attack: 0.012, hold: 0.006, t60: 0.08 }), LOFI_LINN)),
  tambourine: recipe(0.6, (n) =>
    lofi(jingles(n, 5168, { hits: [[0, 1], [0.022, 0.5], [0.05, 0.3]], t60: 0.3, freqs: [4800, 5900, 7100, 8400, 9800] }), LOFI_LINN),
  ),
  cowbell: recipe(0.6, (n) => lofi(cowbell(n, 587, 845, { center: 850, q: 0.9, fast: 0.06, slow: 0.4 }), LOFI_LINN)),
};

// ---------------------------------------------------------------------------
// CR-78-style
// ---------------------------------------------------------------------------

const METAL_CR78 = [295, 413, 532, 677, 811, 1092];

function hatCr78(maxSec: number, e: EnvOpts): VoiceRecipe {
  return recipe(maxSec, (n) => {
    const s = sum(n, [[noise(n, 7801), 0.7], [metal(n, METAL_CR78, 61), 0.4]]);
    chain(s, [['bp', 8000, 1.4], ['hp', 5500], ['lp', 13000]]);
    return mul(s, env(n, e));
  });
}

const handDrum = (freq: number, t60: number, maxSec: number): VoiceRecipe =>
  recipe(maxSec, (n) =>
    sum(n, [
      [body(n, { freq, sweep: 0.1, sweepTau: 0.006, t60, attack: 0.0012 }), 1],
      [body(n, { freq: freq * 1.6, t60: t60 * 0.5, attack: 0.0012, phase: 0.1 }), 0.15],
    ]),
  );

/** Guiro: a run of short, accelerating scrapes. */
function guiro(n: number): Float32Array {
  const s = chain(noise(n, 7802), [['bp', 2500, 1.5], ['hp', 1200]]);
  const e = new Float32Array(n);
  let t = 0;
  for (let k = 0; k < 10; k++) {
    mixInto(e, env(n, { attack: 0.002, t60: 0.018, delay: t }), 0.6 + 0.04 * k);
    t += 0.024 - k * 0.0012;
  }
  return mul(s, e);
}

const CR78: Record<string, VoiceRecipe> = {
  kick: recipe(0.45, (n) => lp(body(n, { freq: 62, sweep: 0.25, sweepTau: 0.01, attack: 0.003, t60: 0.28 }), 1200)),
  snare: recipe(0.35, (n) =>
    lp(
      sum(n, [
        [body(n, { freq: 205, t60: 0.09, attack: 0.001 }), 0.6],
        [body(n, { freq: 340, t60: 0.06, attack: 0.001, phase: 0.2 }), 0.25],
        [noiseHit(n, 7803, [['bp', 3500, 0.6], ['lp', 8000]], { attack: 0.002, t60: 0.14 }), 0.7],
      ]),
      9000,
    ),
  ),
  rim: recipe(0.1, (n) =>
    sum(n, [[body(n, { freq: 820, t60: 0.035, attack: 0.0008 }), 0.7], [body(n, { freq: 1650, t60: 0.025, attack: 0.0008 }), 0.3]]),
  ),
  chh: hatCr78(0.2, { attack: 0.001, t60: 0.07 }),
  ohh: hatCr78(0.6, { attack: 0.001, hold: 0.01, t60: 0.38 }),
  cymbal: recipe(1.4, (n) => {
    const s = sum(n, [[noise(n, 7804), 0.7], [metal(n, METAL_CR78.map((f) => f * 1.2), 63), 0.4]]);
    chain(s, [['bp', 6000, 0.7], ['hp', 4500], ['lp', 12000]]);
    return mul(s, env(n, { attack: 0.002, t60: 1.1 }));
  }),
  metal: recipe(0.3, (n) => partials(n, [[1250, 0.5, 0.18], [1830, 0.4, 0.15], [2650, 0.35, 0.12], [3700, 0.2, 0.1], [5100, 0.15, 0.08]])),
  maracas: recipe(0.15, (n) => noiseHit(n, 7805, [['hp', 6000], ['lp', 13000]], { attack: 0.006, hold: 0.005, t60: 0.06 })),
  guiro: recipe(0.35, guiro),
  tambourine: recipe(0.45, (n) => lp(jingles(n, 7806, { hits: [[0, 1], [0.03, 0.5]], t60: 0.2, freqs: [5000, 6200, 7400, 8700] }), 12000)),
  clave: recipe(0.15, (n) =>
    sum(n, [[body(n, { freq: 2350, t60: 0.07, attack: 0.0005 }), 1], [body(n, { freq: 4800, t60: 0.03, attack: 0.0005 }), 0.1]]),
  ),
  cowbell: recipe(0.4, (n) => cowbell(n, 540, 800, { center: 900, q: 1.2, fast: 0.08, slow: 0.25, lpHz: 4000 })),
  'bongo-lo': handDrum(290, 0.14, 0.3),
  'bongo-hi': handDrum(420, 0.11, 0.3),
  'conga-lo': handDrum(195, 0.26, 0.45),
  'conga-hi': handDrum(275, 0.21, 0.45),
};

export const CLASSIC_RECIPES: Record<ClassicMachineId, Record<string, VoiceRecipe>> = {
  tr808: TR808,
  tr909: TR909,
  tr707: TR707,
  tr606: TR606,
  linn: LINN,
  cr78: CR78,
};
