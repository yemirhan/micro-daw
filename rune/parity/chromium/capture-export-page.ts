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
 *
 * Synth tracks: Tone's white / pink / brown noise buffers are drawn from the synth capture's
 * seeds (`prepareNoise`, capture-synth-page.ts) and `Math.random` is re-seeded with the Rust
 * instruments' noise seed (`DEFAULT_NOISE_SEED`) right before each export, so a project with
 * ONE noise-drawing instrument (a pluck track) draws the same start offsets as Rust. The piano
 * samples are decoded by `exportWav` itself (`loadPianoSamples`).
 *
 * Tone bug neutralized (like the doubled Part events): `Tone.Offline` runs its whole clock
 * before rendering, and Tone creates AudioWorklet nodes (the pluck voices' FeedbackCombFilter)
 * in a promise callback that only runs when the clock loop yields (every second of audio, and
 * only once the module has loaded, which takes real time). Pluck notes handed out before that
 * keep their comb on the placeholder param (delay 0.1 s, the wrong feedback): the app's export
 * of the first seconds of a pluck track is wrong AND varies from run to run.
 * `loadWorkletsFirst` loads the module before the clock loop and creates the nodes right after
 * the attack that built the voice, i.e. when the realtime app's microtask would (its module is
 * loaded long before a note): the swapped-in params carry the attack's values from Tone's clock
 * time on.
 */
import * as Tone from 'tone';
import { loadPadBuffer } from '@/audio/sampler/bufferCache';
import { createFactoryKit } from '@/audio/sampler/factoryKit';
import { SamplerKit } from '@/audio/sampler/SamplerKit';
import { PolyInstrument } from '@/audio/PolyInstrument';
import { SampledPiano } from '@/audio/SampledPiano';
import { exportWav } from '@/services/ExportService';
import type { Arrangement } from '@/types/arrangement';
import type { FactoryVoiceId, KitState } from '@/types/sampler';
import { prepareSeededNoise } from './capture-page';
export { prepareNoise } from './capture-synth-page';

function b64(bytes: Uint8Array): string {
  let s = '';
  const CHUNK = 0x8000;
  for (let i = 0; i < bytes.length; i += CHUNK) s += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  return btoa(s);
}

let workletsPatched = false;

/** See the file comment: worklet modules loaded before `Tone.Offline`'s clock loop. */
function loadWorkletsFirst(): void {
  if (workletsPatched) return;
  workletsPatched = true;
  type Ctx = { __workletsLoaded?: boolean; workletsAreReady(): Promise<void> };
  const proto = Tone.OfflineContext.prototype as unknown as Record<string, unknown>;
  const render = proto.render as (this: Ctx, ...a: unknown[]) => Promise<unknown>;
  proto.render = async function (this: Ctx, ...a: unknown[]) {
    const probe = new Tone.FeedbackCombFilter({ context: this as unknown as Tone.BaseContext });
    await this.workletsAreReady();
    probe.dispose();
    this.__workletsLoaded = true;
    return render.apply(this, a);
  };
  const add = proto.addAudioWorkletModule as (this: Ctx, url: string) => Promise<void>;
  const pending: (() => void)[] = [];
  proto.addAudioWorkletModule = function (this: Ctx, url: string) {
    if (this.__workletsLoaded) {
      // already loaded: create the node before the voice's first attack (flushed below), not
      // on a microtask after the clock loop
      return {
        then(cb: () => void) {
          pending.push(cb);
          return Promise.resolve();
        },
      };
    }
    return add.call(this, url);
  };
  const pluck = Tone.PluckSynth.prototype as unknown as Record<string, unknown>;
  const attack = pluck.triggerAttack as (...a: unknown[]) => unknown;
  pluck.triggerAttack = function (this: unknown, ...a: unknown[]) {
    const r = attack.apply(this, a);
    // the realtime app's microtask: right after the attack that built the voice
    for (const cb of pending.splice(0)) cb();
    return r;
  };
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

type Trigger = [pad: number | string, velocity: number, time: number];
type Hook = { proto: Record<string, unknown>; method: string; args: (a: unknown[]) => Trigger };

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
  seed: number | null = null,
): Promise<{ sr: number; wav: string; triggers: Trigger[][]; duplicates: Trigger[]; randomCalls: number }> {
  // SamplerKit.trigger(pad, velocity, time); the synths' triggerAttackRelease(note, dur, time, velocity)
  // (SampledPiano before PolyInstrument: a piano still on its fallback logs both, one per instance).
  const hooks: Hook[] = [
    { proto: SamplerKit.prototype as unknown as Record<string, unknown>, method: 'trigger', args: (a) => [a[0] as number, a[1] as number, a[2] as number] },
    { proto: SampledPiano.prototype as unknown as Record<string, unknown>, method: 'triggerAttackRelease', args: (a) => [String(a[0]), a[3] as number, a[2] as number] },
    { proto: PolyInstrument.prototype as unknown as Record<string, unknown>, method: 'triggerAttackRelease', args: (a) => [String(a[0]), a[3] as number, a[2] as number] },
  ];
  const logs = new Map<object, Trigger[]>();
  const duplicates: Trigger[] = [];
  const natives = hooks.map((h) => h.proto[h.method] as (...a: unknown[]) => void);
  hooks.forEach((h, i) => {
    const native = natives[i];
    h.proto[h.method] = function (this: object, ...a: unknown[]) {
      const t = h.args(a);
      let log = logs.get(this);
      if (!log) logs.set(this, (log = []));
      const dup = log.some((x) => x[0] === t[0] && Math.abs(x[2] - t[2]) < 1e-6);
      if (dup) duplicates.push(t);
      log.push(t);
      if (dup && dedupe) return;
      native.apply(this, a);
    };
  });
  loadWorkletsFirst();
  try {
    if (seed !== null) window.__paritySeed!(seed);
    const blob = await exportWav(arrangement);
    const bytes = new Uint8Array(await blob.arrayBuffer());
    const randomCalls = seed !== null ? window.__parityRandomCalls!() : 0;
    return { sr: Tone.getContext().sampleRate, wav: b64(bytes), triggers: [...logs.values()], duplicates, randomCalls };
  } finally {
    hooks.forEach((h, i) => {
      h.proto[h.method] = natives[i];
    });
  }
}
