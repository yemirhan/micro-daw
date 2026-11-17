/**
 * Pre-install kit previews: a catalog pack's kit as a KitState (names + pad
 * settings for the demo groove) plus the remote URL of each pad's file, and a
 * small decoded-buffer cache fed by the preview IPC (main only fetches URLs
 * listed in the loaded catalog).
 */
import * as Tone from 'tone';
import { defaultPadSettings, normalizePad } from '@/audio/sampler/factoryKit';
import { LruCache } from '@/audio/sampler/bufferCache';
import { ipcErrorMessage } from '@/services/PackService';
import type { PackManifest } from '@/types/packs';
import { PAD_COUNT, type KitState, type PadIndex, type PadSettings } from '@/types/sampler';
import { packFileLabel } from './packsModel';

export interface RemoteKitPlan {
  kit: KitState;
  /** Remote URL per pad (null = empty pad). */
  urls: (string | null)[];
}

/** Pure: the kit `kitId` of a (not installed) catalog pack, sounds by URL. Null when there is no such kit. */
export function remoteKitPlan(manifest: PackManifest, kitId: string): RemoteKitPlan | null {
  const kit = manifest.kits.find((k) => k.id === kitId);
  if (!kit) return null;
  const files = new Map(manifest.files.map((f) => [f.path, f]));
  const pads: PadSettings[] = Array.from({ length: PAD_COUNT }, defaultPadSettings);
  const urls: (string | null)[] = new Array(PAD_COUNT).fill(null);
  for (const p of kit.pads) {
    if (!Number.isInteger(p.pad) || p.pad < 0 || p.pad >= PAD_COUNT) continue;
    const file = files.get(p.file);
    if (!file) continue;
    urls[p.pad] = file.url;
    pads[p.pad] = normalizePad({
      ...defaultPadSettings(),
      ...(p.settings ?? {}),
      // Never loaded: the preview plays the remote buffer instead.
      sample: { source: { kind: 'file', path: `remote:${file.url}` }, name: p.name || packFileLabel(file) },
    });
  }
  return { kit: { id: `preview:${manifest.id}:${kit.id}`, name: kit.name, pads }, urls };
}

const decoded = new LruCache<string, AudioBuffer>(32);
const inflight = new Map<string, Promise<AudioBuffer>>();

/** Fetches (preview IPC) and decodes a catalog file; cached. Rejects with a readable message. */
export function fetchRemoteBuffer(url: string): Promise<AudioBuffer> {
  const hit = decoded.get(url);
  if (hit) return Promise.resolve(hit);
  const pending = inflight.get(url);
  if (pending) return pending;
  const api = typeof window !== 'undefined' ? window.electronAPI : undefined;
  if (!api || typeof api.packsFetchPreview !== 'function') return Promise.reject(new Error('Previews need the desktop app'));
  const p = (async () => {
    let data: ArrayBuffer;
    try {
      data = await api.packsFetchPreview(url);
    } catch (err) {
      throw new Error(ipcErrorMessage(err), { cause: err });
    }
    const ctx = Tone.getContext().rawContext as BaseAudioContext;
    const buffer = await ctx.decodeAudioData(data);
    decoded.set(url, buffer);
    return buffer;
  })().finally(() => inflight.delete(url));
  inflight.set(url, p);
  return p;
}

/** Pad loader for kitPreview.play: the plan's remote files. */
export function remotePadLoader(plan: RemoteKitPlan, fetch: (url: string) => Promise<AudioBuffer> = fetchRemoteBuffer): (pad: PadIndex) => Promise<AudioBuffer | null> {
  return (pad) => {
    const url = plan.urls[pad];
    return url ? fetch(url) : Promise.resolve(null);
  };
}

/** Tests only. */
export function clearRemoteBuffers(): void {
  decoded.clear();
  inflight.clear();
}
