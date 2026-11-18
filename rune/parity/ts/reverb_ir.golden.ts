/**
 * RoomReverb impulse-response golden (src/audio/RoomReverb.ts createImpulseResponse is pure apart
 * from ctx.createBuffer, so a fake context with Float32Array channels is enough).
 * Per option set and channel: length, FNV-1a 64 of the f32 bit patterns (exactness check),
 * sum / sum of squares, the first 256 samples after the pre-delay, the samples around every
 * early-reflection tap, and every 499th sample.
 */
import { it } from 'vitest';
import { createImpulseResponse, type ReverbOptions } from '@/audio/RoomReverb';
import { writeFixture } from './fixture';

function fakeCtx(sampleRate: number) {
  return {
    sampleRate,
    createBuffer(channels: number, length: number, sr: number) {
      const data = Array.from({ length: channels }, () => new Float32Array(length));
      return { sampleRate: sr, length, numberOfChannels: channels, getChannelData: (c: number) => data[c] };
    },
  } as unknown as BaseAudioContext;
}

function fnv1a64(data: Float32Array): string {
  const bytes = new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  let h = 0xcbf29ce484222325n;
  const p = 0x100000001b3n;
  const mask = (1n << 64n) - 1n;
  for (const b of bytes) {
    h ^= BigInt(b);
    h = (h * p) & mask;
  }
  return h.toString(16).padStart(16, '0');
}

const CASES: { sr: number; opts: Partial<ReverbOptions> }[] = [
  { sr: 44100, opts: {} },
  { sr: 48000, opts: {} },
  { sr: 44100, opts: { decay: 0.5, preDelay: 0, damping: 0 } },
  { sr: 48000, opts: { decay: 3.3, preDelay: 0.05, damping: 1 } },
  { sr: 48000, opts: { decay: 0.05, preDelay: 0.5, damping: 1.7 } },
  { sr: 22050, opts: { decay: 12, preDelay: -1, damping: -0.5 } },
];

it('dumps RoomReverb impulse responses', () => {
  const out = CASES.map(({ sr, opts }) => {
    const buf = createImpulseResponse(fakeCtx(sr), opts) as unknown as {
      length: number;
      getChannelData(c: number): Float32Array;
    };
    const pre = Math.floor(Math.min(0.2, Math.max(0, opts.preDelay ?? 0.022)) * sr);
    const channels = [0, 1].map((ch) => {
      const d = buf.getChannelData(ch);
      let sum = 0;
      let sumSq = 0;
      for (const x of d) {
        sum += x;
        sumSq += x * x;
      }
      const taps = ch === 0 ? [0.007, 0.013, 0.021, 0.034, 0.047] : [0.009, 0.017, 0.026, 0.039, 0.053];
      const points: [number, number][] = [];
      for (let i = pre; i < Math.min(d.length, pre + 256); i++) points.push([i, d[i]]);
      for (const tap of taps) {
        const idx = pre + Math.floor(tap * sr);
        for (let i = idx - 1; i <= idx + 1; i++) if (i >= 0 && i < d.length) points.push([i, d[i]]);
      }
      for (let i = 0; i < d.length; i += 499) points.push([i, d[i]]);
      return { hash: fnv1a64(d), sum, sumSq, points };
    });
    return { sr, opts, length: buf.length, channels };
  });
  writeFixture('reverb_ir', out);
});
