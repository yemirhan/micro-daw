import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { it } from 'vitest';
import { CLASSIC_MACHINES } from '@/audio/sampler/classic/machines';
import { synthesizeClassicVoice } from '@/audio/sampler/classic/synth';
import { analyzeVoice } from '@/audio/sampler/classic/analysis';
import {
  SR,
  TARGET_PEAK,
  biquad,
  clapEnv,
  dcBlock,
  drive,
  env,
  envSum,
  finalize,
  lofi,
  makeRng,
  metal,
  noise,
  osc,
  partials,
  square,
  type FilterType,
} from '@/audio/sampler/classic/dsp';
import { writeFixture } from './fixture';

const arr = (a: Float32Array | number[]): number[] => Array.from(a);

/** A deterministic, non-trivial test signal (decaying two-tone + noise). */
function testSignal(n: number): Float32Array {
  const a = osc(n, { freq: 440, sweep: 0.5, sweepTau: 0.01 });
  const b = osc(n, { freq: 3100, phase: 0.3 });
  const z = noise(n, 99);
  const e = env(n, { t60: (n / SR) * 0.8 });
  const out = new Float32Array(n);
  for (let i = 0; i < n; i++) out[i] = (a[i] * 0.6 + b[i] * 0.3 + z[i] * 0.2) * e[i];
  return out;
}

it('dumps classic DSP primitives and every classic voice', () => {
  const rng: Record<string, number[]> = {};
  for (const seed of [0, 1, 7, 11, 808, 9091, 123456789, 4294967295]) {
    const r = makeRng(seed);
    rng[seed] = Array.from({ length: 64 }, () => r());
  }

  const envs = [
    { t60: 0.01 },
    { attack: 0.002, hold: 0.003, t60: 0.02, delay: 0.001 },
    { attack: 0.0001, t60: 0.0005 },
    { attack: 0.0003, t60: 0.004, delay: 0.0065 },
  ].map((o) => ({ opts: o, out: arr(env(600, o)) }));

  const envSumOut = arr(envSum(600, [[0.72, { attack: 0.0005, t60: 0.007 }], [0.28, { attack: 0.0005, t60: 0.04 }]]));
  const clapOut = arr(clapEnv(1200, { bursts: [0, 0.0065, 0.017], burstT60: 0.018, tailDelay: 0.017, tailT60: 0.03, tailGain: 0.5 }));

  const oscs = [
    { freq: 440 },
    { freq: 50, sweep: 0.55, sweepTau: 0.006, sweep2: 0.12, sweep2Tau: 0.06 },
    { freq: 46, sweep: 0.5, sweepTau: 0.008, drift: 0.94, phase: 0.1 },
    { freq: 1000, shape: 'tri' as const, phase: 0.3 },
    { freq: 180, sweep: 8, sweepTau: 0.012 },
  ].map((o) => ({ opts: o, out: arr(osc(512, o)) }));

  const squares = [
    [1000, 0],
    [205.3, 0.3],
    [9000, 0.77],
  ].map(([f, p]) => ({ freq: f, phase: p, out: arr(square(512, f, p)) }));

  const metalOut = arr(metal(256, [205.3, 304.4, 369.6, 522.7, 540, 800], 11));
  const partialsOut = arr(partials(256, [[3170, 1, 1.4], [4650, 0.6, 1.0], [5980, 0.4, 0.8]]));

  const step = (n: number) => new Float32Array(n).fill(1);
  const biquads: { type: FilterType; freq: number; q?: number; gainDb?: number; input: string; out: number[] }[] = [];
  const cases: [FilterType, number, number | undefined, number | undefined][] = [
    ['lp', 5000, undefined, undefined],
    ['lp', 30000, undefined, undefined],
    ['hp', 250, undefined, undefined],
    ['hp', 7000, 1.3, undefined],
    ['bp', 10000, 1, undefined],
    ['bp', 3440, 2, undefined],
    ['peak', 2500, 1, 4],
    ['peak', 800, 0.7, -6],
  ];
  for (const [type, freq, q, gainDb] of cases) {
    biquads.push({ type, freq, q, gainDb, input: 'step', out: arr(biquad(step(256), type, freq, q, gainDb)) });
    biquads.push({ type, freq, q, gainDb, input: 'signal', out: arr(biquad(testSignal(256), type, freq, q, gainDb)) });
  }

  const dc = arr(dcBlock(testSignal(256).map((x) => x + 0.3)));

  const lofis = [
    { bits: 6 },
    { rate: 25000, bits: 8 },
    { rate: 28000, bits: 8, mu: true },
    { rate: 32000, bits: 8 },
    { bits: 3 },
  ].map((o) => ({ opts: o, out: arr(lofi(testSignal(1000), o)) }));

  const drives = [1.2, 2, 3.5].map((amount) => ({ amount, out: arr(drive(testSignal(256), amount)) }));
  const driveSilent = arr(drive(new Float32Array(8), 2));

  const finalizeOut = arr(finalize(testSignal(4000), { maxSec: 0.08 }));

  const voices: Record<string, unknown>[] = [];
  const dumpDir = process.env.CLASSIC_DUMP_DIR;
  if (dumpDir) mkdirSync(dumpDir, { recursive: true });
  for (const m of CLASSIC_MACHINES) {
    for (const v of m.voices) {
      const data = synthesizeClassicVoice(m.id, v.id);
      if (dumpDir) writeFileSync(join(dumpDir, `${m.id}_${v.id}.f32`), Buffer.from(data.buffer, data.byteOffset, data.byteLength));
      const len = data.length;
      let peak = 0, sq = 0, sum = 0;
      for (let i = 0; i < len; i++) {
        peak = Math.max(peak, Math.abs(data[i]));
        sq += data[i] * data[i];
        sum += data[i];
      }
      const idx = Array.from({ length: 64 }, (_, k) => Math.floor((k * (len - 1)) / 63));
      const a = analyzeVoice(data, SR);
      voices.push({
        machine: m.id,
        voice: v.id,
        len,
        peak,
        rms: Math.sqrt(sq / len),
        sum,
        idx,
        at: idx.map((i) => data[i]),
        first: arr(data.subarray(0, 32)),
        last: arr(data.subarray(Math.max(0, len - 32))),
        analysis: { centroidHz: a.centroidHz, lowFrac: a.lowFrac, highFrac: a.highFrac, peakHz: a.peakHz },
      });
    }
  }

  writeFixture('classic', {
    sr: SR,
    targetPeak: TARGET_PEAK,
    rng,
    noise: arr(noise(64, 5)),
    envs,
    envSum: envSumOut,
    clapEnv: clapOut,
    oscs,
    squares,
    metal: metalOut,
    partials: partialsOut,
    biquads,
    dcBlock: dc,
    lofis,
    drives,
    driveSilent,
    finalize: finalizeOut,
    voices,
  });
});
