/**
 * Loaded INSIDE the page (Vite dev server, real Tone.js, real Chromium Web Audio) by
 * capture-export.steps.mjs. Exports a project through the app's real
 * `ExportService.exportWav` (Tone.Offline at the context rate, the real track chain, master bus,
 * SamplerKit, Part scheduling with swing) and returns the WAV bytes.
 *
 * Factory voices: rendered first with the same seeded `Math.random` as the factory capture
 * (noise buffer from NOISE_SEED, each voice's noise offset from OFFSET_SEED + its index), so the
 * kit plays the exact buffers Rust renders (`cached_factory_voice`). Classic voices are
 * deterministic.
 */
import * as Tone from 'tone';
import { loadPadBuffer } from '@/audio/sampler/bufferCache';
import { createFactoryKit } from '@/audio/sampler/factoryKit';
import { SamplerKit } from '@/audio/sampler/SamplerKit';
import { exportWav } from '@/services/ExportService';
import type { Arrangement } from '@/types/arrangement';
import type { FactoryVoiceId, KitState } from '@/types/sampler';
import { prepareSeededNoise } from './capture-page';

function b64(bytes: Uint8Array): string {
  let s = '';
  const CHUNK = 0x8000;
  for (let i = 0; i < bytes.length; i += CHUNK) s += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  return btoa(s);
}

export function contextSampleRate(): number {
  return Tone.getContext().sampleRate;
}

export function factoryKit(): KitState {
  return createFactoryKit();
}

/** Seeded factory voice buffers into the shared pad-buffer cache. */
export async function preloadFactory(voices: FactoryVoiceId[], noiseSeed: number, offsetSeed: number): Promise<number> {
  const calls = await prepareSeededNoise(noiseSeed);
  for (let i = 0; i < voices.length; i++) {
    window.__paritySeed!(offsetSeed + i);
    await loadPadBuffer({ kind: 'factory', voice: voices[i] });
  }
  return calls;
}

type Trigger = [pad: number, velocity: number, time: number];

/**
 * Export through the app. Every `SamplerKit.trigger` is logged (per kit instance, in call order).
 * `dedupe`: a trigger repeating an earlier (kit, pad, time ± 1 µs) call is skipped — Tone's
 * offline clock can hand the same Part event out twice when its tick time lands on a 128-frame
 * block boundary (accumulated float error in `OfflineContext._currentTime` + rounded ticks in
 * `TickSource.forEachTickBetween`); the raw run keeps them, as the app does.
 */
export async function exportProject(
  arrangement: Arrangement,
  dedupe: boolean,
): Promise<{ sr: number; wav: string; triggers: Trigger[][]; duplicates: Trigger[] }> {
  const proto = SamplerKit.prototype as unknown as { trigger: (pad: number, velocity: number, time: number) => void };
  const native = proto.trigger;
  const kits = new Map<object, Trigger[]>();
  const duplicates: Trigger[] = [];
  proto.trigger = function (this: object, pad: number, velocity: number, time: number) {
    let log = kits.get(this);
    if (!log) kits.set(this, (log = []));
    const dup = log.some((t) => t[0] === pad && Math.abs(t[2] - time) < 1e-6);
    if (dup) duplicates.push([pad, velocity, time]);
    log.push([pad, velocity, time]);
    if (dup && dedupe) return;
    native.call(this, pad, velocity, time);
  };
  try {
    const blob = await exportWav(arrangement);
    const bytes = new Uint8Array(await blob.arrayBuffer());
    return { sr: Tone.getContext().sampleRate, wav: b64(bytes), triggers: [...kits.values()], duplicates };
  } finally {
    proto.trigger = native;
  }
}
