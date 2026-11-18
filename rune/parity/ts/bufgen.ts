/**
 * Deterministic test buffers shared by the beat/notes golden dumps. Only exactly-rounded
 * operations (integer PRNG, + - * /, Math.round, Float32 stores) so the Rust tests can rebuild
 * bit-identical data from the spec (see rune/crates/mdaw-model/src/beat_helpers_golden_tests.rs).
 */
export interface ClickSpec { t: number; amp: number; decay: number }
export interface ChannelSpec { seed: number; noise: number; clicks: ClickSpec[] }
export interface BufferSpec { sr: number; len: number; channels: ChannelSpec[] }

export function mulberry32(seed: number): () => number {
  let a = seed;
  return () => {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

export function genChannel(spec: BufferSpec, ch: ChannelSpec): Float32Array {
  const data = new Float32Array(spec.len);
  const rnd = mulberry32(ch.seed);
  if (ch.noise > 0) for (let i = 0; i < data.length; i++) data[i] = (rnd() * 2 - 1) * ch.noise;
  for (const c of ch.clicks) {
    const i0 = Math.round(c.t * spec.sr);
    for (let i = 0; i < c.decay && i0 + i < data.length; i++) data[i0 + i] = c.amp * (1 - i / c.decay);
  }
  return data;
}

export function genBuffer(spec: BufferSpec) {
  const chans = spec.channels.map((c) => genChannel(spec, c));
  return {
    numberOfChannels: chans.length,
    length: spec.len,
    sampleRate: spec.sr,
    duration: spec.len / spec.sr,
    getChannelData: (c: number) => chans[c],
  };
}

/** JSON-safe number: NaN / ±Infinity / -0 as strings. */
export const N = (x: number): number | string =>
  Number.isNaN(x) ? 'NaN' : x === Infinity ? 'Infinity' : x === -Infinity ? '-Infinity' : Object.is(x, -0) ? '-0' : x;

export const BUFFER_SPECS: BufferSpec[] = [
  { sr: 8000, len: 16000, channels: [{ seed: 1, noise: 0, clicks: [0.1, 0.6, 1.1, 1.6].map((t) => ({ t, amp: 0.9, decay: 400 })) }] },
  { sr: 8000, len: 8000, channels: [{ seed: 2, noise: 0.001, clicks: [{ t: 0, amp: 0.8, decay: 300 }, { t: 0.25, amp: 0.3, decay: 200 }, { t: 0.5, amp: 0.6, decay: 300 }, { t: 0.52, amp: 0.9, decay: 100 }, { t: 0.75, amp: 0.05, decay: 200 }] }] },
  { sr: 22050, len: 22050, channels: [
    { seed: 3, noise: 0.002, clicks: [0.05, 0.3, 0.55, 0.8].map((t, i) => ({ t, amp: 0.2 + i * 0.2, decay: 900 })) },
    { seed: 4, noise: 0.002, clicks: [0.15, 0.42, 0.68, 0.93].map((t, i) => ({ t, amp: 0.9 - i * 0.2, decay: 600 })) },
  ] },
  { sr: 8000, len: 4000, channels: [{ seed: 5, noise: 0.3, clicks: [] }] },
  { sr: 8000, len: 4000, channels: [{ seed: 6, noise: 0, clicks: [] }] },
  { sr: 16000, len: 32000, channels: Array.from({ length: 3 }, (_, c) => ({ seed: 10 + c, noise: 0.01, clicks: Array.from({ length: 20 }, (_, i) => ({ t: 0.05 + i * 0.097 + c * 0.003, amp: 0.3 + ((i * 7) % 10) / 15, decay: 300 + 40 * c })) })) },
  { sr: 1000, len: 37, channels: [{ seed: 7, noise: 0.5, clicks: [{ t: 0.01, amp: 1, decay: 5 }] }] },
  { sr: 44100, len: 0, channels: [{ seed: 8, noise: 0, clicks: [] }] },
];
