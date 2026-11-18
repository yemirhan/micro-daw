/**
 * Loaded INSIDE the page (Vite dev server, real Tone.js, real Chromium Web Audio) by
 * capture-factory.steps.mjs. Renders factory voices through the app's own path
 * (`loadPadBuffer({ kind: 'factory' })` → bufferCache.renderFactoryVoice → Tone.Offline + trimTail).
 *
 * Seeded mode: the init script installed by the steps file replaces `Math.random` with
 * mulberry32(seed) when `window.__paritySeed(seed)` is called. Tone.Noise draws its (cached,
 * stereo, 5 s @ 44100 frames) white-noise buffer and every start offset from Math.random, so
 * seeding makes the noise voices reproducible sample-for-sample in Rust.
 */
import * as Tone from 'tone';
import { clearPadBufferCache, loadPadBuffer } from '@/audio/sampler/bufferCache';
import { DrumKit } from '@/audio/drumKit';
import type { FactoryVoiceId } from '@/types/sampler';

declare global {
  interface Window {
    __paritySeed?: (seed: number) => void;
    __parityRandomCalls?: () => number;
  }
}

function b64(data: Float32Array): string {
  const bytes = new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  let s = '';
  const CHUNK = 0x8000;
  for (let i = 0; i < bytes.length; i += CHUNK) {
    s += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  }
  return btoa(s);
}

export function contextSampleRate(): number {
  return Tone.getContext().sampleRate;
}

/** Generate Tone.Noise's cached white-noise buffer from mulberry32(seed) (2 × 220500 draws + 1 offset draw). */
export async function prepareSeededNoise(seed: number): Promise<number> {
  window.__paritySeed!(seed);
  await Tone.Offline(() => {
    new Tone.Noise('white').start(0);
  }, 0.01, 1);
  return window.__parityRandomCalls!();
}

/** Render one factory voice through the app path. Returns base64 Float32 mono data. */
export async function renderVoice(voice: FactoryVoiceId, seed?: number): Promise<{ sr: number; data: string; randomCalls: number }> {
  clearPadBufferCache();
  if (seed !== undefined) window.__paritySeed!(seed);
  const buf = await loadPadBuffer({ kind: 'factory', voice });
  const calls = seed !== undefined ? window.__parityRandomCalls!() : -1;
  return { sr: buf.sampleRate, data: b64(buf.getChannelData(0)), randomCalls: calls };
}

/** The kick / snare WaveShaper curves exactly as Tone built them (private fields read on purpose). */
export function shaperCurves(): { kick: string; snare: string } {
  const kit = new DrumKit() as unknown as Record<string, { curve: Float32Array | null }>;
  const out = { kick: b64(kit.kickShaper.curve!), snare: b64(kit.snareShaper.curve!) };
  (kit as unknown as DrumKit).dispose();
  return out;
}
