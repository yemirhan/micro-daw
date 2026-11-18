/**
 * Loaded INSIDE the page (Vite dev server, real Tone.js, real Chromium Web Audio) by
 * capture-sampler.steps.mjs. Runs a scripted battery through the app's real SamplerKit
 * (trigger / release / stopAll / setKit / cancelIfPending) and renders it offline.
 *
 * Unlike `Tone.Offline` (which runs Tone's whole clock before rendering, so graph changes made in
 * a callback would apply from frame 0), this drives a native OfflineAudioContext wrapped in a
 * `Tone.OfflineContext` and applies every scripted step with `suspend()` / `resume()` at its
 * quantum: param automation, node creation (a pad filter inserted by setKit) and reconnections
 * take effect exactly at that frame, as in the realtime app. The Tone clock (`now()`) is set to
 * the suspend time before each step; Tone's lookAhead is 0 offline.
 *
 * Buffers: classic voices through the app's own `synthesizeClassicVoice` (bit-exact in Rust),
 * synthetic buffers from `synthChannels` (mirrored in Rust). Pads load them through the
 * SamplerKit `loadBuffer` option (sample source `{ kind: 'file', path: '/parity/<name>' }`).
 */
import * as Tone from 'tone';
import { SamplerKit, collectVoices, type VoiceHandle } from '@/audio/sampler/SamplerKit';
import { synthesizeClassicVoice } from '@/audio/sampler/classic/synth';
import { synthChannels } from './capture-playback-page';
import type { ClassicMachineId, KitState, PadSample } from '@/types/sampler';

export type BufferSpec =
  | { kind: 'classic'; machine: ClassicMachineId; voice: string }
  | { kind: 'synth'; sampleRate: number; frames: number; channels: number; seed: number };

export type Op =
  | { op: 'trigger'; pad: number; vel: number | null; time: number }
  | { op: 'release'; pad: number; time: number }
  | { op: 'stopAll'; time?: number }
  | { op: 'setKit'; kit: KitState }
  | { op: 'cancel'; handle: number };

export interface Step {
  frame: number;
  ops: Op[];
}

export interface Script {
  id: string;
  sr: number;
  frames: number;
  buffers: Record<string, BufferSpec>;
  kit: KitState;
  steps: Step[];
}

function b64(data: Float32Array): string {
  const bytes = new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  let s = '';
  const CHUNK = 0x8000;
  for (let i = 0; i < bytes.length; i += CHUNK) s += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  return btoa(s);
}

function makeBuffer(spec: BufferSpec): AudioBuffer {
  const data = spec.kind === 'classic' ? [synthesizeClassicVoice(spec.machine, spec.voice)] : synthChannels(spec.frames, spec.channels, spec.seed);
  const sr = spec.kind === 'classic' ? 44100 : spec.sampleRate;
  const buf = new AudioBuffer({ length: data[0].length, numberOfChannels: data.length, sampleRate: sr });
  data.forEach((d, c) => buf.copyToChannel(d, c));
  return buf;
}

export async function renderScript(s: Script): Promise<{ channels: string[]; log: unknown[] }> {
  const buffers = new Map(Object.entries(s.buffers).map(([name, spec]) => [`/parity/${name}`, makeBuffer(spec)]));
  const raw = new OfflineAudioContext({ numberOfChannels: 2, length: s.frames, sampleRate: s.sr });
  const ctx = new Tone.OfflineContext(raw);
  const clock = ctx as unknown as { _currentTime: number };
  const prev = Tone.getContext();
  Tone.setContext(ctx);
  const log: unknown[] = [];
  const errors: string[] = [];
  try {
    const loadBuffer = async (_pad: number, sample: PadSample) => {
      const path = sample.source.kind === 'file' ? sample.source.path : '';
      return buffers.get(path) ?? null;
    };
    const kit = new SamplerKit(s.kit, { loadBuffer });
    await kit.ready;
    kit.connect(raw.destination as unknown as Tone.InputNode);
    const handles: VoiceHandle[] = [];
    const run = async (step: Step) => {
      clock._currentTime = raw.currentTime;
      for (const o of step.ops) {
        switch (o.op) {
          case 'trigger': {
            const hs = collectVoices(() => kit.trigger(o.pad, o.vel === null ? (undefined as unknown as number) : o.vel, o.time));
            handles.push(...hs);
            log.push({ frame: step.frame, op: 'trigger', voices: hs.length });
            break;
          }
          case 'release':
            kit.release(o.pad, o.time);
            break;
          case 'stopAll':
            kit.stopAll(o.time);
            break;
          case 'setKit':
            kit.setKit(o.kit);
            break;
          case 'cancel':
            log.push({ frame: step.frame, op: 'cancel', cancelled: handles[o.handle]?.cancelIfPending() ?? null });
            break;
        }
      }
      await kit.ready;
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
    const rendered = await raw.startRendering();
    if (errors.length) throw new Error(errors.join('\n'));
    return { channels: [b64(rendered.getChannelData(0)), b64(rendered.getChannelData(1))], log };
  } finally {
    Tone.setContext(prev);
  }
}
