import * as Tone from 'tone';
import type { FactoryVoiceId, KitState, PadSampleSource } from '@/types/sampler';
import { DrumKit, FACTORY_VOICE_RENDER, playFactoryVoice } from '@/audio/drumKit';
import { sampleManager } from '@/services/SampleManager';
import { renderClassicVoice } from './classic/render';
import { currentPacksDir } from './factoryKit';
import { packDirPrefix, packRefFromPath, resolvePackFilePath } from '@/shared/packSource';

/**
 * Pad sample buffers, shared by every SamplerKit (live pads, drum tracks,
 * offline export) and the browser preview.
 *
 * - Factory voices are rendered once with Tone.Offline from the synthesized
 *   DrumKit, their silent tail trimmed.
 * - Classic voices (built-in 808-/909-/…-style kits) are synthesized once by
 *   ./classic (pure DSP, mono 44.1 kHz, -1 dBFS, trimmed).
 * - Files are decoded through SampleManager (so the samples view and the
 *   waveform helpers see them too).
 * - Pack sounds (`{ kind: 'pack' }`) resolve against the packs folder and are
 *   decoded directly. Their cache keys carry a per-pack generation, bumped by
 *   `invalidatePackBuffers` when a pack is installed, updated or removed, so
 *   SamplerKit / usePadBuffer see a new key and reload (subscribe with
 *   `onPadBuffersInvalidated`). A missing pack fails with
 *   MissingPackSampleError.
 * - Sample-browser previews are NOT pad buffers: they live in a small LRU of
 *   their own (`PREVIEW_CACHE_SIZE`) and are never registered with
 *   SampleManager, so auditioning a folder does not keep every file decoded.
 *
 * Plain AudioBuffers are not bound to an AudioContext, so the same buffer
 * plays in the real-time context and inside Tone.Offline.
 */

const cache = new Map<string, AudioBuffer>();
const pending = new Map<string, Promise<AudioBuffer>>();
const reversed = new WeakMap<AudioBuffer, AudioBuffer>();

/** Bumped per pack id by invalidatePackBuffers (part of the cache key of that pack's sounds). */
const packGenerations = new Map<string, number>();
const invalidationListeners = new Set<(packId: string) => void>();

/** A pack sound whose pack is not installed (or whose file is gone). */
export class MissingPackSampleError extends Error {
  readonly packId: string;
  readonly file: string;
  constructor(packId: string, file: string, cause?: unknown) {
    super(`Pack not installed: ${packId} (${file})`, { cause });
    this.name = 'MissingPackSampleError';
    this.packId = packId;
    this.file = file;
  }
}

/** Pack id a source belongs to: pack sources, and file sources pointing into the packs folder. */
export function packIdOfSource(source: PadSampleSource): string | null {
  if (source.kind === 'pack') return source.packId;
  if (source.kind === 'file') return packRefFromPath(currentPacksDir(), source.path)?.packId ?? null;
  return null;
}

/**
 * Stable cache key of a sample source (file sources are keyed by path, not
 * sampleId). Pack sounds include the pack's generation, so the key changes
 * when the pack is reinstalled / updated / removed.
 */
export function padSourceKey(source: PadSampleSource): string {
  switch (source.kind) {
    case 'factory':
      return `factory:${source.voice}`;
    case 'classic':
      return `classic:${source.machine}:${source.voice}`;
    case 'pack':
      return `pack:${source.packId}@${packGenerations.get(source.packId) ?? 0}:${source.file}`;
    case 'file': {
      const packId = packIdOfSource(source);
      return packId ? `file@${packGenerations.get(packId) ?? 0}:${source.path}` : `file:${source.path}`;
    }
  }
}

/** Absolute path of a pack sound; throws MissingPackSampleError when it cannot be resolved. */
export function resolvePackSource(source: Extract<PadSampleSource, { kind: 'pack' }>): string {
  const dir = currentPacksDir();
  const path = dir ? resolvePackFilePath(dir, source.packId, source.file) : null;
  if (!path) throw new MissingPackSampleError(source.packId, source.file);
  return path;
}

/**
 * Drops every cached pad + preview buffer of pack `packId` and bumps its
 * generation, reloads SampleManager samples under the pack folder in place
 * (same sampleId; see sampleManager.onSamplesReloaded), then notifies
 * `onPadBuffersInvalidated` listeners so live samplers reload (call after
 * install / update / uninstall).
 */
export function invalidatePackBuffers(packId: string): void {
  packGenerations.set(packId, (packGenerations.get(packId) ?? 0) + 1);
  const belongs = (key: string) => key.startsWith(`pack:${packId}@`) || (key.startsWith('file@') && packIdOfKeyPath(key) === packId);
  for (const key of [...cache.keys()]) if (belongs(key)) cache.delete(key);
  for (const key of [...pending.keys()]) if (belongs(key)) pending.delete(key);
  for (const key of previews.keys()) if (belongs(key)) previews.delete(key);
  for (const key of [...previewPending.keys()]) if (belongs(key)) previewPending.delete(key);
  const dir = currentPacksDir();
  const packDir = dir ? packDirPrefix(dir, packId) : null;
  // B3: SampleManager samples under the pack folder (Song audio regions,
  // legacy file pads) are reloaded in place, keeping their sampleId; never
  // dropped (that silenced audio regions in playback and export).
  if (packDir) void sampleManager.reloadByPathPrefix?.(packDir);
  for (const cb of [...invalidationListeners]) {
    try {
      cb(packId);
    } catch (err) {
      console.warn('[sampler] invalidation listener failed', err);
    }
  }
}

function packIdOfKeyPath(key: string): string | null {
  const path = key.slice(key.indexOf(':') + 1);
  return packRefFromPath(currentPacksDir(), path)?.packId ?? null;
}

/**
 * Called with the pack id after `invalidatePackBuffers`: re-apply the current
 * kit (SamplerKit.setKit) so pads of that pack reload. Returns unsubscribe.
 */
export function onPadBuffersInvalidated(cb: (packId: string) => void): () => void {
  invalidationListeners.add(cb);
  return () => {
    invalidationListeners.delete(cb);
  };
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
  const promise = (
    source.kind === 'factory'
      ? renderFactoryVoice(source.voice)
      : source.kind === 'classic'
        ? renderClassicVoice(source.machine, source.voice)
        : source.kind === 'pack'
          ? loadPackSound(source)
          : loadFile(source)
  )
    .then((buffer) => {
      // Not cached when the pack was invalidated meanwhile (the key moved on).
      if (padSourceKey(source) === key) {
        cache.set(key, buffer);
        previews.delete(key); // now a pad buffer: one copy is enough
      }
      return buffer;
    })
    .finally(() => {
      if (pending.get(key) === promise) pending.delete(key);
    });
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
  previews.delete(padSourceKey(source));
}

// ---------------------------------------------------------------------------
// Previews (sample browser audition)
// ---------------------------------------------------------------------------

/** Decoded previews kept for quick re-audition (least recently used is dropped). */
export const PREVIEW_CACHE_SIZE = 8;

/** Pure LRU map: `get` refreshes an entry, `set` evicts the oldest beyond `capacity`. */
export class LruCache<K, V> {
  private readonly map = new Map<K, V>();
  constructor(readonly capacity: number) {}
  get(key: K): V | undefined {
    const v = this.map.get(key);
    if (v === undefined) return undefined;
    this.map.delete(key);
    this.map.set(key, v);
    return v;
  }
  set(key: K, value: V): void {
    this.map.delete(key);
    this.map.set(key, value);
    while (this.map.size > this.capacity) {
      const oldest = this.map.keys().next().value as K;
      this.map.delete(oldest);
    }
  }
  delete(key: K): void {
    this.map.delete(key);
  }
  has(key: K): boolean {
    return this.map.has(key);
  }
  keys(): K[] {
    return [...this.map.keys()];
  }
  get size(): number {
    return this.map.size;
  }
  clear(): void {
    this.map.clear();
  }
}

const previews = new LruCache<string, AudioBuffer>(PREVIEW_CACHE_SIZE);
const previewPending = new Map<string, Promise<AudioBuffer>>();

/**
 * Buffer for auditioning a sample. Reuses a pad buffer when one is loaded
 * (or loading); otherwise decodes the file into the preview LRU only.
 * Factory and classic voices go through the (bounded) pad cache.
 */
export function loadPreviewBuffer(source: PadSampleSource): Promise<AudioBuffer> {
  if (source.kind !== 'file' && source.kind !== 'pack') return loadPadBuffer(source);
  const key = padSourceKey(source);
  const pad = cache.get(key);
  if (pad) return Promise.resolve(pad);
  const padLoading = pending.get(key);
  if (padLoading) return padLoading;
  const hit = previews.get(key);
  if (hit) return Promise.resolve(hit);
  const inflight = previewPending.get(key);
  if (inflight) return inflight;
  const promise = (source.kind === 'pack' ? loadPackSound(source) : decodeFileDirect(source.path))
    .then((buffer) => {
      if (padSourceKey(source) === key) previews.set(key, buffer);
      return buffer;
    })
    .finally(() => {
      if (previewPending.get(key) === promise) previewPending.delete(key);
    });
  previewPending.set(key, promise);
  return promise;
}

/** Paths currently held by the preview cache, most recent last (tests / diagnostics). */
export function previewCacheKeys(): string[] {
  return previews.keys();
}

/** Decode a file without registering it anywhere. */
async function decodeFileDirect(path: string): Promise<AudioBuffer> {
  const api = typeof window !== 'undefined' ? window.electronAPI : undefined;
  if (!api) throw new Error('Electron API not available');
  const data = await api.sampleReadFile(path);
  const ctx = Tone.getContext().rawContext as BaseAudioContext;
  return ctx.decodeAudioData(data);
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

/** Decodes a pack sound from the packs folder (not registered with SampleManager). */
async function loadPackSound(source: Extract<PadSampleSource, { kind: 'pack' }>): Promise<AudioBuffer> {
  const path = resolvePackSource(source);
  let data: ArrayBuffer;
  try {
    const api = typeof window !== 'undefined' ? window.electronAPI : undefined;
    if (!api) throw new Error('Electron API not available');
    data = await api.sampleReadFile(path);
  } catch (err) {
    throw new MissingPackSampleError(source.packId, source.file, err);
  }
  const ctx = Tone.getContext().rawContext as BaseAudioContext;
  return ctx.decodeAudioData(data);
}

async function loadFile(source: Extract<PadSampleSource, { kind: 'file' }>): Promise<AudioBuffer> {
  // Optional call: some tests mock SampleManager without the B3 API.
  const reload = sampleManager.whenReloaded?.(source.path);
  if (reload) await reload;
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
  previews.clear();
  previewPending.clear();
  packGenerations.clear();
}
