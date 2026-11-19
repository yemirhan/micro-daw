/**
 * Loaded INSIDE the page (Vite dev server, real Tone.js, real Chromium Web Audio) by
 * capture-sample-processor.steps.mjs: runs the app's real `processAudio` / `generateSignal`
 * (src/services/SampleProcessor.ts) on generated inputs, with `Math.random` replaced by a seeded
 * mulberry32 per case (the reverb's noise impulse response and the white-noise generator draw
 * from it; Rust replays the same sequence).
 */
import * as Tone from 'tone';
import { processAudio, generateSignal } from '@/services/SampleProcessor';
import type { SampleProcessingParams, SignalGeneratorParams } from '@/types/samples';

export interface Case {
  id: string;
  kind: 'process' | 'generate';
  seed: number;
  sr?: number;
  channels?: number;
  frames?: number;
  /** base64 f32 per channel. */
  input?: string[];
  params: SampleProcessingParams | SignalGeneratorParams;
}

const mulberry32 = (seed: number) => {
  let a = seed | 0;
  return () => {
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
};

function b64(data: Float32Array): string {
  const bytes = new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  let s = '';
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(s);
}

function unb64(s: string): Float32Array {
  const bin = atob(s);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return new Float32Array(bytes.buffer);
}

export function contextRate(): number {
  return Tone.getContext().sampleRate;
}

/**
 * Tone builds its white-noise buffer once per page (`_noiseBuffers.white`) from `Math.random`;
 * the app's own code has usually built it before the capture runs. Its AudioBuffer is reached
 * through a started Noise's source and overwritten with mulberry32(seed)·2 − 1 (channel 0, then
 * 1), so the Rust side can rebuild it. Returns the buffer's rate and length.
 */
export function seedNoise(seed: number): [number, number] {
  const n = new Tone.Noise() as any;
  n.start();
  const ab = n._source.buffer.get() as AudioBuffer;
  const rnd = mulberry32(seed);
  for (let c = 0; c < ab.numberOfChannels; c++) {
    const d = ab.getChannelData(c);
    for (let i = 0; i < d.length; i++) d[i] = rnd() * 2 - 1;
  }
  n.stop();
  n.dispose();
  return [ab.sampleRate, ab.length];
}

export async function runCase(
  c: Case,
): Promise<{ sr: number; channels: string[]; sourceUnchanged?: boolean }> {
  // Tone caches phase-shifted PeriodicWaves (PitchShift's LFOs) across contexts and reuses a wave
  // built at another rate with that rate's scale; the app renders every sample at the context
  // rate, so each case starts with an empty cache.
  (Tone.Oscillator as any)._periodicWaveCache.length = 0;
  const realRandom = Math.random;
  Math.random = mulberry32(c.seed);
  try {
    let out: AudioBuffer;
    let sourceUnchanged: boolean | undefined;
    if (c.kind === 'process') {
      const ctx = Tone.getContext().rawContext as AudioContext;
      const buf = ctx.createBuffer(c.channels!, c.frames!, c.sr!);
      c.input!.forEach((ch, i) => buf.copyToChannel(unb64(ch), i));
      out = await processAudio(buf, c.params as SampleProcessingParams);
      // The source sample must come back untouched (normalize scaled it in place before).
      sourceUnchanged = c.input!.every((ch, i) => b64(buf.getChannelData(i)) === ch);
    } else {
      out = await generateSignal(c.params as SignalGeneratorParams);
    }
    const channels: string[] = [];
    for (let i = 0; i < out.numberOfChannels; i++) channels.push(b64(out.getChannelData(i)));
    return { sr: out.sampleRate, channels, sourceUnchanged };
  } finally {
    Math.random = realRandom;
  }
}
