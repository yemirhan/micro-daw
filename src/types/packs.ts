/**
 * Shared contract for the in-app pack store ("Packs"): built-in synthesized
 * classic-machine kits and downloadable sample packs from a curated catalog.
 * See docs/packs-plan.md.
 */
import type { PadSettings } from './sampler';

export type PackLicense = 'CC0-1.0' | 'public-domain' | 'CC-BY-4.0';

/** One downloadable audio file in a pack. */
export interface PackFile {
  /** Path inside the installed pack folder, POSIX, no leading slash, e.g. "kicks/bd-01.wav". */
  path: string;
  /** Absolute https URL (must be on an allowed host, see PACK_ALLOWED_HOSTS). */
  url: string;
  /** Bytes, if known (used for progress + size limits). */
  size?: number;
  /** Lower-case hex sha256, optional integrity check. */
  sha256?: string;
}

/** Pad assignment for "Load on pads": pad index 0..15 → a file in the pack. */
export interface PackKitPad {
  pad: number;
  file: string; // PackFile.path
  name?: string;
  settings?: Partial<Omit<PadSettings, 'sample'>>;
}

export interface PackKit {
  id: string;
  name: string;
  pads: PackKitPad[];
}

export interface PackManifest {
  id: string; // kebab-case, unique
  name: string;
  /** e.g. "808-style", never a trademark claim ("Roland" may appear only in factual source credits). */
  tagline: string;
  description: string;
  tags: string[]; // e.g. ['808', 'drum machine', 'analog', 'hip-hop']
  author: string; // who recorded/made the sounds
  source: string; // https URL of the upstream repo/page
  license: PackLicense;
  /** Required attribution text for CC-BY; optional credit otherwise. */
  attribution?: string;
  /** Accent color for the card (CSS color). */
  color: string;
  files: PackFile[];
  /** One or more ready-made 16-pad kits built from the files. */
  kits: PackKit[];
  version: number;
}

export interface PackCatalog {
  version: 1;
  updatedAt: string; // ISO date
  packs: PackManifest[];
}

export interface InstalledPack {
  id: string;
  version: number;
  /** Absolute folder path of the installed pack. */
  dir: string;
  installedAt: string;
  manifest: PackManifest;
}

export type PackInstallState =
  | { state: 'idle' }
  | { state: 'downloading'; receivedBytes: number; totalBytes: number | null; fileIndex: number; fileCount: number }
  | { state: 'installed' }
  | { state: 'error'; message: string };

/** Hosts packs and previews may be downloaded from. */
export const PACK_ALLOWED_HOSTS = ['raw.githubusercontent.com', 'github.com', 'objects.githubusercontent.com', 'codeload.github.com'] as const;

/** Remote catalog (falls back to the copy bundled with the app). */
export const PACK_CATALOG_URL = 'https://raw.githubusercontent.com/yemirhan/micro-daw/main/packs/catalog.json';
