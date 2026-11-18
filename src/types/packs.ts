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
  /**
   * Commit-pinned https://raw.githubusercontent.com/<owner>/<repo>/<40-hex sha>/… URL
   * (enforced by the validator, see src/shared/packValidate.ts).
   */
  url: string;
  /** Exact byte count (required; the installer verifies it). */
  size: number;
  /** Lower-case hex sha256 (required; the installer verifies it). */
  sha256: string;
  /**
   * Optional human-friendly sound name for lists ("Kick · tone 0 · decay 10");
   * UIs fall back to the file name. At most 80 characters.
   */
  label?: string;
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
  source: string; // https://github.com/… URL of the upstream repo/page
  license: PackLicense;
  /** Required attribution text for CC-BY; optional credit otherwise. */
  attribution?: string;
  /** Accent color for the card: `#hex` or `oklch(L C H)` only (validated). */
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

/**
 * Hosts the pack store may download from (catalog, pack files, previews),
 * including every redirect hop. Commit-pinned raw.githubusercontent.com URLs
 * are served directly (no redirect), so no other host is needed.
 */
export const PACK_ALLOWED_HOSTS = ['raw.githubusercontent.com'] as const;

/** Remote catalog (falls back to the copy bundled with the app). */
export const PACK_CATALOG_URL = 'https://raw.githubusercontent.com/yemirhan/micro-daw/main/packs/catalog.json';
