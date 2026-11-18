/**
 * Loaded INSIDE the page (Vite dev server, real Tone.js, real Chromium Web Audio) by
 * capture-playback.steps.mjs. Renders AudioBufferSourceNode playback in OfflineAudioContexts:
 *
 *  - `renderRaw`: a bare AudioBufferSourceNode with explicit start/stop/playbackRate parameters
 *    (each case says how the rate is set: `param` = playbackRate.setValueAtTime(rate, 0), which is
 *    what Tone does, or `value` = playbackRate.value = rate).
 *  - `renderSampler`: the app's real `SamplerKit.trigger` inside `Tone.Offline`. Every native
 *    AudioBufferSourceNode start/stop and every playbackRate automation call is recorded, and the
 *    native source is ALSO connected straight to the offline destination while the kit output is
 *    left unconnected, so the render is exactly the native node's output (no envelope, no panner):
 *    it proves which parameters SamplerKit/Tone hand to Chromium.
 *
 * Input buffers are regenerated identically in Rust (`mdaw_dsp::abs_node` parity tests): classic
 * voices through the app's own `synthesizeClassicVoice` (bit-exact in Rust), synthetic buffers
 * from `synthBuffer` below (integer LCG + f32 arithmetic only, so JS and Rust agree bit for bit).
 */
import * as Tone from 'tone';
import { SamplerKit } from '@/audio/sampler/SamplerKit';
import { getReversedBuffer } from '@/audio/sampler/bufferCache';
import { defaultPadSettings } from '@/audio/sampler/factoryKit';
import { synthesizeClassicVoice } from '@/audio/sampler/classic/synth';
import { PAD_COUNT, type ClassicMachineId, type KitState, type PadSettings } from '@/types/sampler';

export type BufferSpec =
  | { kind: 'classic'; machine: ClassicMachineId; voice: string; reverse?: boolean }
  | { kind: 'synth'; sampleRate: number; frames: number; channels: number; seed: number; reverse?: boolean };

export interface RawCase {
  ctxRate: number;
  frames: number;
  buffer: BufferSpec;
  rateMode: 'param' | 'value';
  playbackRate: number;
  when: number;
  offset: number;
  /** start(when, offset, duration) when set. */
  duration?: number;
  /** stop(stop) when set. */
  stop?: number;
}

export interface SamplerCase {
  ctxRate: number;
  frames: number;
  buffer: BufferSpec;
  pad: Pick<PadSettings, 'pitch' | 'startSec' | 'endSec' | 'reverse'>;
  when: number;
}

function b64(data: Float32Array): string {
  const bytes = new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  let s = '';
  const CHUNK = 0x8000;
  for (let i = 0; i < bytes.length; i += CHUNK) s += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  return btoa(s);
}

/** Deterministic test signal (mirrored by `synth_buffer` in Rust): LCG noise + triangle, f32 decay. */
export function synthChannels(frames: number, channels: number, seed: number): Float32Array[] {
  const out: Float32Array[] = [];
  for (let c = 0; c < channels; c++) {
    const x = new Float32Array(frames);
    let s = (seed + Math.imul(c, 0x9e3779b9)) >>> 0;
    let g = 1;
    for (let n = 0; n < frames; n++) {
      s = (Math.imul(s, 1664525) + 1013904223) >>> 0;
      const noise = (s >>> 8) / 16777216 - 0.5;
      const k = n % 200;
      const tri = (k < 100 ? k : 200 - k) / 100 - 0.5;
      x[n] = Math.fround((noise * 0.5 + tri) * g);
      g = Math.fround(g * 0.99995);
    }
    out.push(x);
  }
  return out;
}

function makeBuffer(spec: BufferSpec): AudioBuffer {
  let data: Float32Array[];
  let sr: number;
  if (spec.kind === 'classic') {
    data = [synthesizeClassicVoice(spec.machine, spec.voice)];
    sr = 44100;
  } else {
    data = synthChannels(spec.frames, spec.channels, spec.seed);
    sr = spec.sampleRate;
  }
  const buf = new AudioBuffer({ length: data[0].length, numberOfChannels: data.length, sampleRate: sr });
  data.forEach((d, c) => buf.copyToChannel(d, c));
  return buf;
}

/** The buffer as the app would play it (SamplerKit plays `getReversedBuffer(buffer)` for reverse pads). */
function playedBuffer(spec: BufferSpec): AudioBuffer {
  const b = makeBuffer(spec);
  return spec.reverse ? getReversedBuffer(b) : b;
}

export function bufferInfo(spec: BufferSpec): { frames: number; sampleRate: number; channels: number } {
  const b = makeBuffer(spec);
  return { frames: b.length, sampleRate: b.sampleRate, channels: b.numberOfChannels };
}

function outChannels(r: AudioBuffer): string[] {
  const out: string[] = [];
  for (let c = 0; c < r.numberOfChannels; c++) out.push(b64(r.getChannelData(c)));
  return out;
}

export async function renderRaw(k: RawCase): Promise<{ sr: number; channels: string[] }> {
  const buf = playedBuffer(k.buffer);
  const ctx = new OfflineAudioContext({ numberOfChannels: buf.numberOfChannels, length: k.frames, sampleRate: k.ctxRate });
  const src = new AudioBufferSourceNode(ctx);
  src.buffer = buf;
  if (k.rateMode === 'param') src.playbackRate.setValueAtTime(k.playbackRate, 0);
  else src.playbackRate.value = k.playbackRate;
  src.connect(ctx.destination);
  if (k.duration !== undefined) src.start(k.when, k.offset, k.duration);
  else src.start(k.when, k.offset);
  if (k.stop !== undefined) src.stop(k.stop);
  const r = await ctx.startRendering();
  return { sr: r.sampleRate, channels: outChannels(r) };
}

interface NativeCall {
  start?: unknown[];
  stop?: unknown[];
  rateEvents: { kind: string; args: unknown[] }[];
  bufferRate?: number;
  bufferFrames?: number;
}

export async function renderSampler(k: SamplerCase): Promise<{ sr: number; channels: string[]; native: NativeCall[] }> {
  const buf = makeBuffer(k.buffer);
  const calls: NativeCall[] = [];
  const paramEvents = new WeakMap<AudioParam, { kind: string; args: unknown[] }[]>();
  const proto = AudioBufferSourceNode.prototype;
  const pproto = AudioParam.prototype;
  const nativeStart = proto.start;
  const nativeStop = proto.stop;
  const nativeSet = pproto.setValueAtTime;
  const valueDesc = Object.getOwnPropertyDescriptor(pproto, 'value')!;
  const record = (p: AudioParam, kind: string, args: unknown[]) => {
    const l = paramEvents.get(p) ?? [];
    l.push({ kind, args });
    paramEvents.set(p, l);
  };
  const byNode = new Map<AudioBufferSourceNode, NativeCall>();
  const callOf = (n: AudioBufferSourceNode) => {
    let c = byNode.get(n);
    if (!c) {
      c = { rateEvents: [] };
      byNode.set(n, c);
      calls.push(c);
    }
    return c;
  };
  pproto.setValueAtTime = function (this: AudioParam, ...args: [number, number]) {
    record(this, 'setValueAtTime', args);
    return nativeSet.apply(this, args);
  };
  Object.defineProperty(pproto, 'value', {
    ...valueDesc,
    set(this: AudioParam, v: number) {
      record(this, 'value', [v]);
      valueDesc.set!.call(this, v);
    },
  });
  // Tone starts other buffer sources of its own (constant signals): only the voice's source, which
  // plays `buf` or its reversed copy, is recorded and routed to the destination.
  const isVoice = (n: AudioBufferSourceNode) =>
    !!n.buffer && n.buffer.length === buf.length && n.buffer.sampleRate === buf.sampleRate && n.buffer.numberOfChannels === buf.numberOfChannels;
  proto.start = function (this: AudioBufferSourceNode, ...args: unknown[]) {
    if (!isVoice(this)) return (nativeStart as (...a: unknown[]) => void).apply(this, args);
    const c = callOf(this);
    c.start = args;
    c.rateEvents = [...(paramEvents.get(this.playbackRate) ?? [])];
    c.bufferRate = this.buffer?.sampleRate;
    c.bufferFrames = this.buffer?.length;
    // Route the native node straight to the destination (the kit output is not connected).
    this.connect(this.context.destination);
    return (nativeStart as (...a: unknown[]) => void).apply(this, args);
  } as typeof proto.start;
  proto.stop = function (this: AudioBufferSourceNode, ...args: unknown[]) {
    if (isVoice(this)) callOf(this).stop = args;
    return (nativeStop as (...a: unknown[]) => void).apply(this, args);
  } as typeof proto.stop;

  try {
    const pads: PadSettings[] = Array.from({ length: PAD_COUNT }, () => defaultPadSettings());
    pads[0] = {
      ...defaultPadSettings(),
      ...k.pad,
      sample: { source: { kind: 'file', path: '/parity/buffer.wav' }, name: 'parity' },
    };
    const kitState: KitState = { id: 'parity', name: 'parity', pads };
    const rendered = await Tone.Offline(
      async () => {
        const kit = new SamplerKit(kitState, { loadBuffer: async () => buf });
        await kit.ready;
        kit.trigger(0, 1, k.when);
      },
      k.frames / k.ctxRate,
      buf.numberOfChannels,
      k.ctxRate,
    );
    const native = rendered.get()!;
    if (native.length !== k.frames) throw new Error(`offline length ${native.length} != ${k.frames}`);
    return { sr: native.sampleRate, channels: outChannels(native), native: calls };
  } finally {
    proto.start = nativeStart;
    proto.stop = nativeStop;
    pproto.setValueAtTime = nativeSet;
    Object.defineProperty(pproto, 'value', valueDesc);
  }
}

export function contextSampleRate(): number {
  return Tone.getContext().sampleRate;
}
