/**
 * Tiny, deterministic, sample-accurate DSP toolkit for the built-in classic
 * kits. Everything is pure (Float32Array in, Float32Array out), renders at
 * 44.1 kHz mono and uses a seeded PRNG, so a voice renders to exactly the same
 * samples every time (cacheable, testable without an AudioContext).
 */

export const SR = 44100;
/** Peak level every rendered voice is normalized to (-1 dBFS). */
export const TARGET_PEAK = 10 ** (-1 / 20);
/** -60 dBFS: trailing audio below this is trimmed. */
export const SILENCE = 0.001;

const TAU = Math.PI * 2;
const LN1000 = Math.log(1000);

export const samples = (sec: number): number => Math.max(1, Math.round(sec * SR));

/** mulberry32: small, fast, seeded PRNG in [0, 1). */
export function makeRng(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** White noise in [-1, 1). */
export function noise(n: number, seed = 1): Float32Array {
  const rnd = makeRng(seed);
  const out = new Float32Array(n);
  for (let i = 0; i < n; i++) out[i] = rnd() * 2 - 1;
  return out;
}

// ---------------------------------------------------------------------------
// Envelopes
// ---------------------------------------------------------------------------

export interface EnvOpts {
  /** Linear attack, seconds. */
  attack?: number;
  /** Flat hold after the attack, seconds. */
  hold?: number;
  /** Time to decay by 60 dB, seconds. */
  t60: number;
  /** Start of the envelope, seconds (silence before). */
  delay?: number;
}

/** Attack / hold / exponential decay amplitude envelope. */
export function env(n: number, { attack = 0.0005, hold = 0, t60, delay = 0 }: EnvOpts): Float32Array {
  const out = new Float32Array(n);
  const d = delay * SR;
  const a = Math.max(1, attack * SR);
  const h = hold * SR;
  const k = LN1000 / (Math.max(0.001, t60) * SR);
  for (let i = 0; i < n; i++) {
    const j = i - d;
    if (j < 0) out[i] = 0;
    else if (j < a) out[i] = j / a;
    else if (j < a + h) out[i] = 1;
    else out[i] = Math.exp(-k * (j - a - h));
  }
  return out;
}

/** Sum of envelopes (e.g. a fast initial drop into a slower tail). */
export function envSum(n: number, parts: [gain: number, opts: EnvOpts][]): Float32Array {
  const out = new Float32Array(n);
  for (const [g, o] of parts) mixInto(out, env(n, o), g);
  return out;
}

/**
 * Hand-clap envelope: a few fast sawtooth bursts (several hands, a few ms
 * apart) followed by a longer diffuse "room" tail.
 */
export function clapEnv(
  n: number,
  { bursts, burstT60, tailDelay, tailT60, tailGain }: { bursts: number[]; burstT60: number; tailDelay: number; tailT60: number; tailGain: number },
): Float32Array {
  const out = new Float32Array(n);
  for (const b of bursts) mixInto(out, env(n, { attack: 0.0003, t60: burstT60, delay: b }), 1);
  mixInto(out, env(n, { attack: 0.002, t60: tailT60, delay: tailDelay }), tailGain);
  return out;
}

// ---------------------------------------------------------------------------
// Oscillators
// ---------------------------------------------------------------------------

export interface OscOpts {
  /** Base (settled) frequency, Hz. */
  freq: number;
  /** Extra frequency at t=0 as a ratio of `freq`, decaying with `sweepTau` (pitch envelope). */
  sweep?: number;
  sweepTau?: number;
  /** Second, slower pitch-envelope component. */
  sweep2?: number;
  sweep2Tau?: number;
  /** Slow exponential pitch drift: frequency ratio reached after 1 s (e.g. 0.9 = sags 10 %/s). */
  drift?: number;
  shape?: 'sine' | 'tri';
  /** Start phase in cycles (0 = starts at zero, rising). */
  phase?: number;
}

/** Oscillator with an exponential pitch envelope (bridged-T / VCO style). */
export function osc(n: number, o: OscOpts): Float32Array {
  const { freq, sweep = 0, sweepTau = 0.01, sweep2 = 0, sweep2Tau = 0.05, drift = 1, shape = 'sine' } = o;
  const out = new Float32Array(n);
  let ph = o.phase ?? 0;
  const lnDrift = Math.log(drift);
  for (let i = 0; i < n; i++) {
    const t = i / SR;
    const f = freq * (1 + sweep * Math.exp(-t / sweepTau) + sweep2 * Math.exp(-t / sweep2Tau)) * (drift === 1 ? 1 : Math.exp(lnDrift * t));
    out[i] = shape === 'sine' ? Math.sin(TAU * ph) : 1 - 4 * Math.abs(((ph + 0.25) % 1) - 0.5);
    ph += f / SR;
    ph -= Math.floor(ph);
  }
  return out;
}

function polyBlep(t: number, dt: number): number {
  if (t < dt) {
    const x = t / dt;
    return x + x - x * x - 1;
  }
  if (t > 1 - dt) {
    const x = (t - 1) / dt;
    return x * x + x + x + 1;
  }
  return 0;
}

/** Band-limited (polyBLEP) square wave. */
export function square(n: number, freq: number, phase = 0): Float32Array {
  const out = new Float32Array(n);
  const dt = freq / SR;
  let ph = phase % 1;
  for (let i = 0; i < n; i++) {
    let v = ph < 0.5 ? 1 : -1;
    v += polyBlep(ph, dt);
    v -= polyBlep((ph + 0.5) % 1, dt);
    out[i] = v;
    ph += dt;
    if (ph >= 1) ph -= 1;
  }
  return out;
}

/**
 * "Metal" source: several detuned square oscillators summed (the 808/606
 * cymbal/hi-hat circuit uses six). Random but seeded start phases.
 */
export function metal(n: number, freqs: readonly number[], seed = 7): Float32Array {
  const rnd = makeRng(seed);
  const out = new Float32Array(n);
  for (const f of freqs) mixInto(out, square(n, f, rnd()), 1 / freqs.length);
  return out;
}

/** Sum of exponentially decaying sine partials: [freq, amplitude, t60][] (bells, plates, membranes). */
export function partials(n: number, list: readonly [freq: number, amp: number, t60: number][], attack = 0.0005): Float32Array {
  const out = new Float32Array(n);
  list.forEach(([f, a, t60], i) => {
    const s = osc(n, { freq: f, phase: (i * 0.137) % 1 });
    mul(s, env(n, { attack, t60 }));
    mixInto(out, s, a);
  });
  return out;
}

// ---------------------------------------------------------------------------
// Filters (RBJ biquads, run in place)
// ---------------------------------------------------------------------------

export type FilterType = 'lp' | 'hp' | 'bp' | 'peak';

export function biquad(buf: Float32Array, type: FilterType, freq: number, q = Math.SQRT1_2, gainDb = 0): Float32Array {
  const f = Math.min(freq, SR * 0.49);
  const w0 = (TAU * f) / SR;
  const cos = Math.cos(w0);
  const alpha = Math.sin(w0) / (2 * q);
  let b0: number, b1: number, b2: number, a0: number, a1: number, a2: number;
  switch (type) {
    case 'lp':
      b0 = (1 - cos) / 2; b1 = 1 - cos; b2 = b0;
      a0 = 1 + alpha; a1 = -2 * cos; a2 = 1 - alpha;
      break;
    case 'hp':
      b0 = (1 + cos) / 2; b1 = -(1 + cos); b2 = b0;
      a0 = 1 + alpha; a1 = -2 * cos; a2 = 1 - alpha;
      break;
    case 'bp':
      b0 = alpha; b1 = 0; b2 = -alpha;
      a0 = 1 + alpha; a1 = -2 * cos; a2 = 1 - alpha;
      break;
    case 'peak': {
      const A = 10 ** (gainDb / 40);
      b0 = 1 + alpha * A; b1 = -2 * cos; b2 = 1 - alpha * A;
      a0 = 1 + alpha / A; a1 = -2 * cos; a2 = 1 - alpha / A;
      break;
    }
  }
  b0 /= a0; b1 /= a0; b2 /= a0; a1 /= a0; a2 /= a0;
  let x1 = 0, x2 = 0, y1 = 0, y2 = 0;
  for (let i = 0; i < buf.length; i++) {
    const x = buf[i];
    const y = b0 * x + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
    x2 = x1; x1 = x; y2 = y1; y1 = y;
    buf[i] = y;
  }
  return buf;
}

export const lp = (b: Float32Array, f: number, q?: number) => biquad(b, 'lp', f, q);
export const hp = (b: Float32Array, f: number, q?: number) => biquad(b, 'hp', f, q);
export const bp = (b: Float32Array, f: number, q?: number) => biquad(b, 'bp', f, q);
export const peak = (b: Float32Array, f: number, q: number, gainDb: number) => biquad(b, 'peak', f, q, gainDb);

/** One-pole DC blocker (≈ 10 Hz). */
export function dcBlock(buf: Float32Array, hz = 10): Float32Array {
  const r = Math.exp((-TAU * hz) / SR);
  let x1 = 0, y1 = 0;
  for (let i = 0; i < buf.length; i++) {
    const y = buf[i] - x1 + r * y1;
    x1 = buf[i];
    y1 = y;
    buf[i] = y;
  }
  return buf;
}

// ---------------------------------------------------------------------------
// Arithmetic / shaping
// ---------------------------------------------------------------------------

/** dst += src * gain (src may be shorter; `offset` delays it, in samples). */
export function mixInto(dst: Float32Array, src: Float32Array, gain = 1, offset = 0): Float32Array {
  const end = Math.min(dst.length, src.length + offset);
  for (let i = Math.max(0, offset); i < end; i++) dst[i] += src[i - offset] * gain;
  return dst;
}

/** a *= b, element-wise (in place). */
export function mul(a: Float32Array, b: Float32Array): Float32Array {
  const n = Math.min(a.length, b.length);
  for (let i = 0; i < n; i++) a[i] *= b[i];
  for (let i = n; i < a.length; i++) a[i] = 0;
  return a;
}

export function scale(a: Float32Array, g: number): Float32Array {
  for (let i = 0; i < a.length; i++) a[i] *= g;
  return a;
}

export function peakOf(a: Float32Array): number {
  let p = 0;
  for (let i = 0; i < a.length; i++) {
    const v = Math.abs(a[i]);
    if (v > p) p = v;
  }
  return p;
}

/** Normalize to peak 1 (no-op for silence). */
export function normalize(a: Float32Array, target = 1): Float32Array {
  const p = peakOf(a);
  return p > 0 ? scale(a, target / p) : a;
}

/** Soft clip: tanh(drive·x), level-compensated so a full-scale input stays full scale. */
export function drive(a: Float32Array, amount: number): Float32Array {
  const p = peakOf(a) || 1;
  const k = Math.tanh(amount);
  for (let i = 0; i < a.length; i++) a[i] = (Math.tanh((amount * a[i]) / p) / k) * p;
  return a;
}

/**
 * Early-sampler character: band-limit to the sample rate, sample-and-hold at
 * `rate`, quantize to `bits` (optionally μ-law companded like 8-bit
 * LinnDrum-era ROMs) and filter the images. Quantization happens at full
 * scale (the level the sound was "recorded" at), so the grit rides on the
 * decay like it does on the hardware.
 */
export function lofi(buf: Float32Array, { rate = SR, bits = 16, mu = false }: { rate?: number; bits?: number; mu?: boolean }): Float32Array {
  const p = peakOf(buf);
  if (p === 0) return buf;
  if (rate < SR) {
    lp(buf, rate * 0.45);
    lp(buf, rate * 0.45);
  }
  const L = 2 ** (bits - 1) - 1;
  const MU = 255;
  const lnMu = Math.log(1 + MU);
  const step = SR / rate;
  let acc = step;
  let held = 0;
  for (let i = 0; i < buf.length; i++) {
    if (acc >= step) {
      acc -= step;
      const x = buf[i] / p;
      if (mu) {
        const y = (Math.sign(x) * Math.log(1 + MU * Math.abs(x))) / lnMu;
        const q = Math.round(y * L) / L;
        held = (Math.sign(q) * ((1 + MU) ** Math.abs(q) - 1)) / MU;
      } else {
        held = Math.round(x * L) / L;
      }
    }
    acc += 1;
    buf[i] = held * p;
  }
  if (rate < SR) lp(buf, Math.min(rate * 0.48, SR * 0.45));
  return buf;
}

// ---------------------------------------------------------------------------
// Finishing
// ---------------------------------------------------------------------------

/**
 * Turn a raw render into a pad sample: DC-block, normalize to -1 dBFS, cut
 * everything after the last sample above -60 dBFS (plus `padSec`), cap at
 * `maxSec` and fade the last few ms to zero.
 */
export function finalize(raw: Float32Array, { maxSec = 3, padSec = 0.01, fadeSec = 0.006 }: { maxSec?: number; padSec?: number; fadeSec?: number } = {}): Float32Array {
  const buf = dcBlock(raw.slice(0, Math.min(raw.length, samples(maxSec))));
  normalize(buf, TARGET_PEAK);
  let last = 0;
  for (let i = buf.length - 1; i >= 0; i--) {
    if (Math.abs(buf[i]) > SILENCE) {
      last = i;
      break;
    }
  }
  const end = Math.min(buf.length, last + 1 + samples(padSec));
  const out = buf.slice(0, Math.max(1, end));
  const fade = Math.min(out.length, samples(fadeSec));
  for (let i = 0; i < fade; i++) out[out.length - 1 - i] *= i / fade;
  return out;
}
