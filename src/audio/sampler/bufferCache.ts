import * as Tone from 'tone';
import type { FactoryVoiceId, KitState, PadSampleSource } from '@/types/sampler';
import { DrumKit, FACTORY_VOICE_RENDER, playFactoryVoice } from '@/audio/drumKit';
import { sampleManager } from '@/services/SampleManager';

/**
 * Pad sample buffers, shared by every SamplerKit (live pads, drum tracks,
 * offline export) and the browser preview.
 *
 * - Factory voices are rendered once with Tone.Offline from the synthesized
 *   DrumKit, their silent tail trimmed.
 * - Files are decoded through SampleManager (so the samples view and the
 *   waveform helpers see them too).
 *
 * Plain AudioBuffers are not bound to an AudioContext, so the same buffer
 * plays in the real-time context and inside Tone.Offline.
 */

const cache = new Map<string, AudioBuffer>();
const pending = new Map<string, Promise<AudioBuffer>>();
const reversed = new WeakMap<AudioBuffer, AudioBuffer>();

/** Stable cache key of a sample source (file sources are keyed by path, not sampleId). */
export function padSourceKey(source: PadSampleSource): string {
  return source.kind === 'factory' ? `factory:${source.voice}` : `file:${source.path}`;
}

export function getCachedPadBuffer(source: PadSampleSource): AudioBuffer | null {
  return cache.get(padSourceKey(source)) ?? null;
}

export function loadPadBuffer(source: PadSampleSource): Promise<AudioBuffer> {
  const key = padSourceKey(source);
  const hit = cache.get(key);
  if (hit) return Promise.resolve(hit);
  const inflight = pending.get(key);
  if (inflight) {
    if (source.kind === 'file') void inflight.then(() => attachSampleId(source), () => undefined);
    return inflight;
  }
  const promise = (source.kind === 'factory' ? renderFactoryVoice(source.voice) : loadFile(source))
    .then((buffer) => {
      cache.set(key, buffer);
      return buffer;
    })
    .finally(() => pending.delete(key));
  pending.set(key, promise);
  return promise;
}

/** Load every pad sample of a kit; failures are logged, never thrown. */
export async function preloadKitBuffers(kit: KitState): Promise<void> {
  await Promise.all(
    kit.pads.map((p) =>
      p.sample
        ? loadPadBuffer(p.sample.source).catch((err) => {
            console.warn('[sampler] failed to load pad sample', p.sample?.name, err);
          })
        : undefined,
    ),
  );
}

/** Drop a file from the cache (e.g. after it changed on disk). */
export function invalidatePadBuffer(source: PadSampleSource): void {
  cache.delete(padSourceKey(source));
}

/** Time-reversed copy of a buffer (cached per buffer). */
export function getReversedBuffer(buffer: AudioBuffer): AudioBuffer {
  const hit = reversed.get(buffer);
  if (hit) return hit;
  const out = createAudioBuffer(buffer.numberOfChannels, buffer.length, buffer.sampleRate);
  for (let c = 0; c < buffer.numberOfChannels; c++) {
    const src = buffer.getChannelData(c);
    const dst = out.getChannelData(c);
    for (let i = 0, n = src.length; i < n; i++) dst[i] = src[n - 1 - i];
  }
  reversed.set(buffer, out);
  return out;
}

// ---------------------------------------------------------------------------
// Files
// ---------------------------------------------------------------------------

/** Record the runtime SampleManager id on the source (skipped for frozen state objects). */
function attachSampleId(source: Extract<PadSampleSource, { kind: 'file' }>): void {
  const ref = sampleManager.getAllSamples().find((s) => s.path === source.path);
  if (!ref || source.sampleId === ref.id || !Object.isExtensible(source) || Object.isFrozen(source)) return;
  try {
    source.sampleId = ref.id;
  } catch {
    /* read-only state object: the id is a runtime hint only */
  }
}

async function loadFile(source: Extract<PadSampleSource, { kind: 'file' }>): Promise<AudioBuffer> {
  if (source.sampleId) {
    const known = sampleManager.getSample(source.sampleId);
    const buf = known?.path === source.path ? sampleManager.getBuffer(source.sampleId) : undefined;
    if (buf) return buf;
  }
  const ref = await sampleManager.loadFromPath(source.path);
  const buffer = sampleManager.getBuffer(ref.id);
  if (!buffer) throw new Error(`Sample not decoded: ${source.path}`);
  attachSampleId(source);
  return buffer;
}

// ---------------------------------------------------------------------------
// Factory voices
// ---------------------------------------------------------------------------

/** Silence threshold for tail trimming (-60 dBFS). */
const SILENCE = 0.001;
/** Peaks above this are scaled down so a pad at 0 dB never clips. */
const MAX_PEAK = 0.98;

// Tone.Offline swaps the global context while it renders; run one at a time.
let renderQueue: Promise<unknown> = Promise.resolve();

function renderFactoryVoice(voice: FactoryVoiceId): Promise<AudioBuffer> {
  const run = async () => {
    const spec = FACTORY_VOICE_RENDER[voice];
    const rendered = await Tone.Offline(
      (ctx) => {
        const kit = new DrumKit();
        kit.connect(ctx.destination);
        playFactoryVoice(kit, voice, 1, 0);
      },
      spec.seconds,
      1,
    );
    const raw = rendered.get();
    if (!raw) throw new Error(`Factory voice render failed: ${voice}`);
    const channels = Array.from({ length: raw.numberOfChannels }, (_, c) => raw.getChannelData(c));
    const trimmed = trimTail(channels, raw.sampleRate);
    const out = createAudioBuffer(trimmed.length, trimmed[0].length, raw.sampleRate);
    trimmed.forEach((data, c) => out.getChannelData(c).set(data));
    return out;
  };
  const next = renderQueue.then(run, run);
  renderQueue = next.catch(() => undefined);
  return next;
}

/**
 * Pure: cut the trailing silence of rendered audio (keeping `padSec` after the
 * last sample above -60 dB), fade the last few ms to zero and scale the
 * whole thing down if it would clip. Always returns at least `minSec`.
 */
export function trimTail(
  channels: Float32Array[],
  sampleRate: number,
  { padSec = 0.01, fadeSec = 0.005, minSec = 0.01 }: { padSec?: number; fadeSec?: number; minSec?: number } = {},
): Float32Array[] {
  const length = channels[0]?.length ?? 0;
  let last = -1;
  let peak = 0;
  for (const ch of channels) {
    for (let i = 0; i < ch.length; i++) {
      const a = Math.abs(ch[i]);
      if (a > peak) peak = a;
      if (a > SILENCE && i > last) last = i;
    }
  }
  const minLen = Math.min(length, Math.max(1, Math.round(minSec * sampleRate)));
  const end = Math.min(length, Math.max(minLen, last + 1 + Math.round(padSec * sampleRate)));
  const fadeLen = Math.min(end, Math.round(fadeSec * sampleRate));
  const scale = peak > MAX_PEAK ? MAX_PEAK / peak : 1;
  return channels.map((ch) => {
    const out = ch.slice(0, end);
    if (scale !== 1) for (let i = 0; i < out.length; i++) out[i] *= scale;
    for (let i = 0; i < fadeLen; i++) out[end - 1 - i] *= i / fadeLen;
    return out;
  });
}

function createAudioBuffer(channels: number, length: number, sampleRate: number): AudioBuffer {
  const n = Math.max(1, length);
  if (typeof AudioBuffer !== 'undefined') {
    try {
      return new AudioBuffer({ numberOfChannels: channels, length: n, sampleRate });
    } catch {
      /* fall through to the context factory */
    }
  }
  return (Tone.getContext().rawContext as BaseAudioContext).createBuffer(channels, n, sampleRate);
}

/** Test hook: forget everything cached. */
export function clearPadBufferCache(): void {
  cache.clear();
  pending.clear();
}
