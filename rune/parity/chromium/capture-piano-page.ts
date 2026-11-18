/**
 * Loaded INSIDE the page (Vite dev server, real Tone.js, real Chromium Web Audio) by
 * capture-piano.steps.mjs and capture-character.steps.mjs.
 *
 * - `renderPiano`: the app's real SampledPiano (src/audio/SampledPiano.ts) over the real
 *   Salamander set, decoded by the app's own `loadPianoSamples` (decodeAudioData at the context
 *   rate + leading-silence trim), playing a scripted battery offline. Steps are applied with
 *   `suspend()` / `resume()` at their quantum, as in capture-sampler-page.ts.
 * - `pianoBuffers`: length / channel count / FNV-1a digest of every decoded + trimmed buffer.
 * - `renderCharacter`: the app's real CharacterChain fed by a deterministic stereo test signal
 *   (`testSignal`, mirrored in Rust), with live `update()` steps.
 */
import * as Tone from 'tone';
import { SampledPiano } from '@/audio/SampledPiano';
import { CharacterChain } from '@/audio/CharacterChain';
import { loadPianoSamples } from '@/audio/pianoSamples';
import { FALLBACK_PIANO, SYNTH_PRESETS } from '@/audio/presets';
import { mergePresetParams } from '@/audio/presetParams';
import type { CharacterParams, SamplerPreset } from '@/types/audio';

export type PianoOp =
  | { op: 'on'; midi: number; vel: number; time: number }
  | { op: 'off'; midi: number; time: number }
  | { op: 'ar'; midi: number; dur: number; time: number; vel: number }
  | { op: 'releaseAll'; time: number }
  | { op: 'set'; params: Record<string, unknown> }
  | { op: 'probe'; time: number };

export interface Step<O> {
  frame: number;
  ops: O[];
}

export interface PianoScript {
  id: string;
  sr: number;
  frames: number;
  /** SYNTH_PRESETS index + overrides (mergePresetParams). */
  preset: number;
  overrides: Record<string, unknown>;
  steps: Step<PianoOp>[];
}

export type CharOp = { op: 'update'; character: Partial<CharacterParams>; gain: number };

export interface CharScript {
  id: string;
  sr: number;
  frames: number;
  character: Partial<CharacterParams>;
  gain: number;
  mono: boolean;
  steps: Step<CharOp>[];
}

function b64(data: Float32Array): string {
  const bytes = new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  let s = '';
  const CHUNK = 0x8000;
  for (let i = 0; i < bytes.length; i += CHUNK) s += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  return btoa(s);
}

/** FNV-1a 64 over the f32 bit patterns (little endian), as a hex string. */
function fnv(data: Float32Array): string {
  let h = 0xcbf29ce484222325n;
  const p = 0x100000001b3n;
  const bytes = new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  for (const b of bytes) {
    h ^= BigInt(b);
    h = (h * p) & 0xffffffffffffffffn;
  }
  return h.toString(16).padStart(16, '0');
}

async function withOffline<T>(
  sr: number,
  frames: number,
  build: (raw: OfflineAudioContext, clock: { _currentTime: number }) => Promise<T>,
): Promise<{ raw: OfflineAudioContext; built: T; restore: () => void }> {
  const raw = new OfflineAudioContext({ numberOfChannels: 2, length: frames, sampleRate: sr });
  const ctx = new Tone.OfflineContext(raw);
  const clock = ctx as unknown as { _currentTime: number };
  const prev = Tone.getContext();
  Tone.setContext(ctx);
  const built = await build(raw, clock);
  return { raw, built, restore: () => Tone.setContext(prev) };
}

async function runSteps<O>(
  raw: OfflineAudioContext,
  clock: { _currentTime: number },
  sr: number,
  steps: Step<O>[],
  run: (o: O, frame: number) => void,
): Promise<AudioBuffer> {
  const errors: string[] = [];
  for (const step of steps) {
    if (step.frame === 0) {
      clock._currentTime = 0;
      for (const o of step.ops) run(o, 0);
      continue;
    }
    void raw.suspend(step.frame / sr).then(async () => {
      try {
        if (raw.currentTime !== step.frame / sr) throw new Error(`suspended at ${raw.currentTime * sr}, wanted ${step.frame}`);
        clock._currentTime = raw.currentTime;
        for (const o of step.ops) run(o, step.frame);
      } catch (err) {
        errors.push(`${step.frame}: ${err instanceof Error ? err.stack : String(err)}`);
      }
      await raw.resume();
    });
  }
  const rendered = await raw.startRendering();
  if (errors.length) throw new Error(errors.join('\n'));
  return rendered;
}

let samplesAt: number | null = null;

/** Decode the piano set at `sr` (memoised by the app: the first context's rate wins). */
async function ensureSamples(sr: number) {
  if (samplesAt !== null && samplesAt !== sr) throw new Error(`samples already decoded at ${samplesAt}`);
  const raw = new OfflineAudioContext({ numberOfChannels: 2, length: 128, sampleRate: sr });
  const ctx = new Tone.OfflineContext(raw);
  const prev = Tone.getContext();
  Tone.setContext(ctx);
  try {
    const b = await loadPianoSamples();
    samplesAt = sr;
    return b;
  } finally {
    Tone.setContext(prev);
  }
}

export async function pianoBuffers(sr: number) {
  const map = await ensureSamples(sr);
  return [...map.entries()]
    .sort((a, b) => a[0] - b[0])
    .map(([midi, tb]) => {
      const buf = tb.get() as AudioBuffer;
      const ch = Array.from({ length: buf.numberOfChannels }, (_, c) => buf.getChannelData(c));
      return { midi, sampleRate: buf.sampleRate, frames: buf.length, digests: ch.map(fnv) };
    });
}

/** One decoded + trimmed channel (debugging digest mismatches), base64 f32. */
export async function pianoBufferChannel(sr: number, midi: number, ch: number): Promise<string> {
  const map = await ensureSamples(sr);
  const buf = map.get(midi)?.get() as AudioBuffer;
  return b64(buf.getChannelData(ch));
}

export async function renderPiano(s: PianoScript): Promise<{ channels: string[]; log: unknown[] }> {
  await ensureSamples(s.sr);
  const log: unknown[] = [];
  const { raw, built: piano, restore } = await withOffline(s.sr, s.frames, async (raw) => {
    const preset = mergePresetParams(SYNTH_PRESETS[s.preset], s.overrides) as SamplerPreset;
    log.push({ frame: 0, preset });
    const p = new SampledPiano(preset, FALLBACK_PIANO);
    if (!p.isSampled()) throw new Error('piano not sampled');
    p.connect(raw.destination as unknown as Tone.InputNode);
    return p;
  });
  try {
    const clock = Tone.getContext() as unknown as { _currentTime: number };
    const rendered = await runSteps(raw, clock, s.sr, s.steps, (o, frame) => {
      switch (o.op) {
        case 'on':
          piano.noteOn(o.midi, o.vel, o.time);
          break;
        case 'off':
          piano.noteOff(o.midi, o.time);
          break;
        case 'ar':
          piano.triggerAttackRelease(o.midi, o.dur, o.time, o.vel);
          break;
        case 'releaseAll':
          piano.releaseAll(o.time);
          break;
        case 'set':
          piano.set(o.params);
          log.push({ frame, preset: piano.getPreset() });
          break;
        case 'probe':
          log.push({ frame, held: piano.getHeldNotes(), active: piano.getActiveVoiceCount(o.time) });
          break;
      }
    });
    return { channels: [b64(rendered.getChannelData(0)), b64(rendered.getChannelData(1))], log };
  } finally {
    restore();
  }
}

/**
 * Deterministic stereo test signal (mirrored by `test_signal` in the Rust parity test): a linear
 * saw sweep 60 Hz → 6 kHz plus a three-note triangle chord, piecewise-linear amplitude. Only
 * +, −, ×, ÷ and floor in f64, so both sides compute the same f32 samples.
 */
export function testSignal(frames: number, sr: number): [Float32Array, Float32Array] {
  const l = new Float32Array(frames);
  const r = new Float32Array(frames);
  const dur = frames / sr;
  let ps = 0;
  const pc = [0, 0.25, 0.5];
  const fc = [220, 277.1826, 329.6276];
  for (let i = 0; i < frames; i++) {
    const t = i / sr;
    const f = 60 + (6000 - 60) * (t / dur);
    ps += f / sr;
    ps -= Math.floor(ps);
    const saw = 2 * ps - 1;
    let chord = 0;
    for (let k = 0; k < 3; k++) {
      pc[k] += fc[k] / sr;
      pc[k] -= Math.floor(pc[k]);
      const x = pc[k];
      chord += x < 0.5 ? 4 * x - 1 : 3 - 4 * x;
    }
    // Amplitude: fade in over 10 ms, a gap from 45 to 50 % of the render, otherwise 1.
    const u = t / dur;
    let a = Math.min(1, t / 0.01);
    if (u >= 0.45 && u < 0.5) a = 0;
    l[i] = a * (0.3 * saw + 0.15 * chord);
    r[i] = a * (0.15 * saw + 0.2 * chord - 0.05);
  }
  return [l, r];
}

export async function renderCharacter(s: CharScript): Promise<{ channels: string[] }> {
  const { raw, built: chain, restore } = await withOffline(s.sr, s.frames, async (raw) => {
    const c = new CharacterChain(s.character, s.gain);
    const [l, r] = testSignal(s.frames, s.sr);
    const buf = new AudioBuffer({ length: s.frames, numberOfChannels: s.mono ? 1 : 2, sampleRate: s.sr });
    buf.copyToChannel(l, 0);
    if (!s.mono) buf.copyToChannel(r, 1);
    const src = raw.createBufferSource();
    src.buffer = buf;
    src.connect(c.input.input as unknown as AudioNode);
    src.start(0);
    c.output.connect(raw.destination as unknown as Tone.InputNode);
    return c;
  });
  try {
    const clock = Tone.getContext() as unknown as { _currentTime: number };
    const rendered = await runSteps(raw, clock, s.sr, s.steps, (o) => {
      chain.update(o.character, o.gain);
    });
    return { channels: [b64(rendered.getChannelData(0)), b64(rendered.getChannelData(1))] };
  } finally {
    restore();
  }
}

/** Every preset's character settings and gain (the capture dedupes them). */
export function presetCharacters() {
  return SYNTH_PRESETS.map((p, index) => ({ index, id: p.id, character: p.character, gain: p.gain }));
}
