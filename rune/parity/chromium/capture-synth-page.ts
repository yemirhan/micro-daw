/**
 * Loaded INSIDE the page (Vite dev server, real Tone.js, real Chromium Web Audio) by
 * capture-synth.steps.mjs. Plays a scripted battery through the app's real `PolyInstrument`
 * (built by `createSynthFromPreset`, i.e. the real Subtractive / FM / Pluck voices) and renders
 * it offline, tapping the instrument's voice bus BEFORE its CharacterChain.
 *
 * Like capture-sampler-page.ts this drives a native OfflineAudioContext wrapped in a
 * `Tone.OfflineContext` and applies every scripted step with `suspend()` / `resume()` at its
 * render quantum (the Tone clock is set to the suspend time first; lookAhead is 0 offline), so
 * lazily created voices, param automation and source starts happen exactly as in the app.
 *
 * Two Tone clock effects the offline capture must reproduce by hand:
 * - Tone's `context.setTimeout` only fires on clock ticks, which a suspended raw offline context
 *   never emits. The one timeout that changes the sound is `OnePoleFilter._createFilter`'s
 *   "disconnect the old IIR filter on the next block" (every pluck attack sets `dampening`). In
 *   the realtime app that tick comes long before the note's audio (Tone's lookahead), so here
 *   that callback runs immediately. Every other timeout (native source stops behind gain gates
 *   that are already 0) is inaudible and left alone.
 * - The FeedbackCombFilter AudioWorklet loads asynchronously; each step waits for pending
 *   worklet modules before the render resumes (the app schedules notes ahead of time).
 *
 * Noise: Tone draws its white / pink / brown noise buffers (2 × 220500 frames each, cached per
 * page) and every noise start offset from Math.random. `prepareNoise` draws the three buffers
 * from mulberry32 seeds (installed by the steps' init script) and `renderScript` re-seeds before
 * each render, so the Rust replay can draw the same offsets.
 */
import * as Tone from 'tone';
import { createSynthFromPreset } from '@/utils/synthFactory';
import { SYNTH_PRESETS, FALLBACK_PIANO } from '@/audio/presets';
import { mergePresetParams } from '@/audio/presetParams';
import type { SynthPreset } from '@/types/audio';

declare global {
  interface Window {
    __paritySeed?: (seed: number) => void;
    __parityRandomCalls?: () => number;
  }
}

export type Op =
  | { op: 'on'; note: number; vel: number; time: number }
  | { op: 'off'; note: number; time: number }
  | { op: 'ar'; note: number; dur: number; vel: number; time: number }
  | { op: 'releaseAll'; time: number }
  | { op: 'set'; params: Record<string, unknown>; time: number };

export interface Step {
  frame: number;
  ops: Op[];
}

export interface Script {
  id: string;
  /** Index into SYNTH_PRESETS, or -1 for FALLBACK_PIANO. */
  preset: number;
  sr: number;
  frames: number;
  /** mulberry32 seed of the noise start offsets. */
  seed: number;
  /** Optional preset overrides (debug probes). */
  overrides?: Record<string, unknown>;
  steps: Step[];
}

function b64(data: Float32Array): string {
  const bytes = new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  let s = '';
  const CHUNK = 0x8000;
  for (let i = 0; i < bytes.length; i += CHUNK) s += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  return btoa(s);
}

export function contextSampleRate(): number {
  return Tone.getContext().sampleRate;
}

/** Draw Tone's white, pink and brown noise buffers, each from its own mulberry32 seed. */
export async function prepareNoise(seeds: { white: number; pink: number; brown: number }, sr: number): Promise<number[]> {
  const calls: number[] = [];
  for (const type of ['white', 'pink', 'brown'] as const) {
    window.__paritySeed!(seeds[type]);
    await Tone.Offline(() => {
      new Tone.Noise(type).start(0);
    }, 0.01, 1, sr);
    calls.push(window.__parityRandomCalls!());
  }
  return calls;
}

function presetOf(i: number): SynthPreset {
  return i < 0 ? FALLBACK_PIANO : SYNTH_PRESETS[i];
}

export async function renderScript(s: Script): Promise<{ channels: string[]; randomCalls: number; pool: number }> {
  const raw = new OfflineAudioContext({ numberOfChannels: 2, length: s.frames, sampleRate: s.sr });
  const ctx = new Tone.OfflineContext(raw);
  const clock = ctx as unknown as { _currentTime: number };
  const origTimeout = ctx.setTimeout.bind(ctx);
  ctx.setTimeout = (fn: (...args: unknown[]) => void, timeout: number): number => {
    // OnePoleFilter: "dispose the old filter on the next block" (see the file comment).
    if (fn.toString().includes('oldFilter')) {
      fn();
      return -1;
    }
    return origTimeout(fn, timeout);
  };
  const prev = Tone.getContext();
  Tone.setContext(ctx);
  const errors: string[] = [];
  try {
    window.__paritySeed!(s.seed);
    const inst = createSynthFromPreset(mergePresetParams(presetOf(s.preset), s.overrides ?? null)) as unknown as {
      bus: Tone.Gain;
      noteOn(m: number, v: number, t: number): void;
      noteOff(m: number, t: number): void;
      triggerAttackRelease(n: number, d: number, t: number, v: number): void;
      releaseAll(t: number): void;
      set(params: Record<string, unknown>): void;
      getPoolSize(): number;
    };
    inst.bus.connect(raw.destination as unknown as Tone.InputNode);
    const settle = async () => {
      // Let pending AudioWorklet modules (FeedbackCombFilter) load and connect.
      for (let i = 0; i < 5; i++) await new Promise((r) => setTimeout(r, 10));
    };
    const run = async (step: Step) => {
      clock._currentTime = raw.currentTime;
      for (const o of step.ops) {
        switch (o.op) {
          case 'on':
            inst.noteOn(o.note, o.vel, o.time);
            break;
          case 'off':
            inst.noteOff(o.note, o.time);
            break;
          case 'ar':
            inst.triggerAttackRelease(o.note, o.dur, o.time, o.vel);
            break;
          case 'releaseAll':
            inst.releaseAll(o.time);
            break;
          case 'set':
            inst.set(o.params);
            break;
        }
      }
      await settle();
    };
    for (const step of s.steps) {
      if (step.frame === 0) {
        await run(step);
        continue;
      }
      void raw.suspend(step.frame / s.sr).then(async () => {
        try {
          if (raw.currentTime !== step.frame / s.sr) throw new Error(`suspended at ${raw.currentTime * s.sr}, wanted ${step.frame}`);
          await run(step);
        } catch (err) {
          errors.push(`${step.frame}: ${err instanceof Error ? err.stack : String(err)}`);
        }
        await raw.resume();
      });
    }
    await settle();
    const rendered = await raw.startRendering();
    if (errors.length) throw new Error(errors.join('\n'));
    return {
      channels: [b64(rendered.getChannelData(0)), b64(rendered.getChannelData(1))],
      randomCalls: window.__parityRandomCalls!(),
      pool: inst.getPoolSize(),
    };
  } finally {
    Tone.setContext(prev);
  }
}
