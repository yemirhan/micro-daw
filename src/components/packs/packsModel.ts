/**
 * Pure helpers for the Packs page: one card model for built-in classic kits
 * and downloadable packs, search / tag filtering, sizes, sound grouping and
 * the install-button state. No React, no audio.
 */
import type { InstalledPack, PackFile, PackInstallState, PackManifest } from '@/types/packs';
import type { ClassicMachineId } from '@/types/sampler';

/** Shape of one entry of P1's `CLASSIC_MACHINES` (structural, so this file has no audio imports). */
export interface ClassicMachineInfo {
  id: ClassicMachineId;
  name: string;
  tagline: string;
  color: string;
  description?: string;
  voices: readonly { id: string; label: string; gm?: number }[];
  /** Default 16-pad layout (voice ids). */
  pads?: readonly string[];
}

export type PackItem =
  | {
      kind: 'builtin';
      id: string;
      machine: ClassicMachineInfo;
      name: string;
      tagline: string;
      description: string;
      tags: string[];
      color: string;
      soundCount: number;
    }
  | {
      kind: 'remote';
      id: string;
      manifest: PackManifest;
      name: string;
      tagline: string;
      description: string;
      tags: string[];
      color: string;
      soundCount: number;
      /** Sum of known file sizes (bytes); null when no file has a size. */
      sizeBytes: number | null;
    };

const MACHINE_TAG: Record<ClassicMachineId, string> = {
  tr808: '808',
  tr909: '909',
  tr707: '707',
  tr606: '606',
  linn: 'linn',
  cr78: 'cr-78',
};

export function builtinItemId(machine: ClassicMachineId): string {
  return `builtin:${machine}`;
}

export function builtinItem(machine: ClassicMachineInfo): PackItem {
  return {
    kind: 'builtin',
    id: builtinItemId(machine.id),
    machine,
    name: machine.name,
    tagline: machine.tagline,
    description:
      machine.description ||
      `${machine.name} sounds synthesized right here in the app: an original take on the classic machine's character, not samples of it.`,
    tags: [MACHINE_TAG[machine.id] ?? machine.id, 'built-in', 'drum machine'],
    color: machine.color,
    soundCount: machine.voices.length,
  };
}

export function packSizeBytes(files: readonly PackFile[]): number | null {
  let total = 0;
  let known = false;
  for (const f of files) {
    if (typeof f.size === 'number' && Number.isFinite(f.size) && f.size >= 0) {
      total += f.size;
      known = true;
    }
  }
  return known ? total : null;
}

const TEXT_EXTENSIONS = new Set(['txt', 'md']);

/** True for a playable pack file (packs also ship LICENSE / README text files). */
export function isSoundFile(path: string): boolean {
  const dot = path.lastIndexOf('.');
  return !TEXT_EXTENSIONS.has(dot >= 0 ? path.slice(dot + 1).toLowerCase() : '');
}

/** The pack's playable sounds (licence / readme files left out). */
export function soundFiles(files: readonly PackFile[]): PackFile[] {
  return files.filter((f) => isSoundFile(f.path));
}

export function remoteItem(manifest: PackManifest): PackItem {
  return {
    kind: 'remote',
    id: manifest.id,
    manifest,
    name: manifest.name,
    tagline: manifest.tagline,
    description: manifest.description,
    tags: manifest.tags,
    color: manifest.color,
    soundCount: soundFiles(manifest.files).length,
    sizeBytes: packSizeBytes(manifest.files),
  };
}

/**
 * Downloadable pack items: the catalog's packs, plus installed packs the
 * catalog no longer lists (offline, or removed upstream) so they stay usable.
 */
export function remoteItems(catalogPacks: readonly PackManifest[] | null, installed: readonly InstalledPack[]): PackItem[] {
  const seen = new Set<string>();
  const items: PackItem[] = [];
  for (const m of catalogPacks ?? []) {
    if (seen.has(m.id)) continue;
    seen.add(m.id);
    items.push(remoteItem(m));
  }
  for (const p of installed) {
    if (seen.has(p.id)) continue;
    seen.add(p.id);
    items.push(remoteItem(p.manifest));
  }
  return items;
}

const norm = (s: string) => s.toLowerCase().normalize('NFKD').replace(/[̀-ͯ]/g, '');

/** Every whitespace-separated query word must appear in the item's text (name, tagline, tags, description, author). */
export function matchesQuery(item: PackItem, query: string): boolean {
  const words = norm(query).split(/\s+/).filter(Boolean);
  if (words.length === 0) return true;
  const hay = norm(
    [
      item.name,
      item.tagline,
      item.tags.join(' '),
      item.description,
      item.kind === 'remote' ? `${item.manifest.author} ${item.manifest.license}` : '',
    ].join(' '),
  );
  return words.every((w) => hay.includes(w));
}

export function matchesTag(item: PackItem, tag: string | null): boolean {
  if (!tag) return true;
  const t = norm(tag);
  return item.tags.some((x) => norm(x) === t);
}

export function filterItems(items: readonly PackItem[], query: string, tag: string | null): PackItem[] {
  return items.filter((i) => matchesTag(i, tag) && matchesQuery(i, query));
}

/** Tags by frequency (ties alphabetical), case-insensitive, at most `limit`. */
export function topTags(items: readonly PackItem[], limit = 10): { tag: string; count: number }[] {
  const counts = new Map<string, { tag: string; count: number }>();
  for (const item of items) {
    const seen = new Set<string>();
    for (const tag of item.tags) {
      const key = norm(tag);
      if (!key || seen.has(key)) continue;
      seen.add(key);
      const e = counts.get(key);
      if (e) e.count++;
      else counts.set(key, { tag: tag.toLowerCase(), count: 1 });
    }
  }
  return [...counts.values()]
    .sort((a, b) => b.count - a.count || a.tag.localeCompare(b.tag))
    .slice(0, limit);
}

export function formatBytes(bytes: number | null | undefined): string {
  if (bytes == null || !Number.isFinite(bytes) || bytes < 0) return '';
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  const mb = bytes / (1024 * 1024);
  return `${mb < 10 ? mb.toFixed(1) : Math.round(mb)} MB`;
}

export const LICENSE_LABEL: Record<PackManifest['license'], string> = {
  'CC0-1.0': 'CC0',
  'public-domain': 'Public domain',
  'CC-BY-4.0': 'CC BY 4.0',
};

export const LICENSE_URL: Record<PackManifest['license'], string | null> = {
  'CC0-1.0': 'https://creativecommons.org/publicdomain/zero/1.0/',
  'public-domain': null,
  'CC-BY-4.0': 'https://creativecommons.org/licenses/by/4.0/',
};

export interface SoundGroup {
  /** Folder path ('' = pack root). */
  folder: string;
  files: PackFile[];
}

/** Files grouped by their folder, folders sorted, files sorted by natural name order. */
export function groupFilesByFolder(files: readonly PackFile[]): SoundGroup[] {
  const groups = new Map<string, PackFile[]>();
  for (const f of files) {
    const i = f.path.lastIndexOf('/');
    const folder = i >= 0 ? f.path.slice(0, i) : '';
    const list = groups.get(folder);
    if (list) list.push(f);
    else groups.set(folder, [f]);
  }
  const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });
  return [...groups.entries()]
    .sort(([a], [b]) => (a === '' ? -1 : b === '' ? 1 : collator.compare(a, b)))
    .map(([folder, list]) => ({ folder, files: [...list].sort((a, b) => collator.compare(a.path, b.path)) }));
}

/** "kicks/BD 01.wav" -> "BD 01". */
export function soundLabel(path: string): string {
  const base = path.split('/').pop() ?? path;
  return base.replace(/\.[a-z0-9]{2,5}$/i, '');
}

/** What the card / detail install button shows. */
export type InstallButtonState =
  | { kind: 'builtin' }
  | { kind: 'get' }
  | { kind: 'downloading'; progress: number | null; label: string }
  | { kind: 'installed' }
  | { kind: 'error'; message: string };

/** 0..1 overall progress of a download, or null when the total is unknown. */
export function downloadProgress(s: Extract<PackInstallState, { state: 'downloading' }>): number | null {
  if (s.totalBytes && s.totalBytes > 0) return Math.min(1, Math.max(0, s.receivedBytes / s.totalBytes));
  if (s.fileCount > 0) return Math.min(1, Math.max(0, s.fileIndex / s.fileCount));
  return null;
}

export function installButtonState(item: PackItem, state: PackInstallState | undefined, installed: boolean): InstallButtonState {
  if (item.kind === 'builtin') return { kind: 'builtin' };
  if (state?.state === 'downloading') {
    const progress = downloadProgress(state);
    const label =
      progress != null
        ? `${Math.round(progress * 100)}%`
        : state.fileCount > 0
          ? `${Math.min(state.fileIndex + 1, state.fileCount)}/${state.fileCount}`
          : '…';
    return { kind: 'downloading', progress, label };
  }
  if (state?.state === 'error') return { kind: 'error', message: state.message };
  if (installed || state?.state === 'installed') return { kind: 'installed' };
  return { kind: 'get' };
}

/** Which of 16 pads a pack's first kit (or a classic machine) fills, for the faceplate. */
export function litPads(item: PackItem): boolean[] {
  const lit = new Array<boolean>(16).fill(false);
  if (item.kind === 'builtin') {
    const pads = item.machine.pads;
    if (pads) pads.slice(0, 16).forEach((v, i) => (lit[i] = !!v));
    else for (let i = 0; i < Math.min(16, item.machine.voices.length); i++) lit[i] = true;
    return lit;
  }
  const kit = item.manifest.kits[0];
  if (!kit) {
    for (let i = 0; i < Math.min(16, item.soundCount); i++) lit[i] = true;
    return lit;
  }
  for (const p of kit.pads) if (p.pad >= 0 && p.pad < 16) lit[p.pad] = true;
  return lit;
}

/** A playable sound in the detail panel: a classic voice or a pack file. */
export type SoundRef = { kind: 'voice'; voice: string } | { kind: 'file'; file: PackFile };

/** Key of a sound preview (packService's `previewing`) for one sound of a pack / machine. */
export const soundKey = (itemId: string, sound: SoundRef): string =>
  `${itemId}::${sound.kind === 'voice' ? sound.voice : sound.file.path}`;
