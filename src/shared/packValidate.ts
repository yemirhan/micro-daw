/**
 * Pure validation for the pack store: catalog/manifest schema, the https host
 * allowlist, commit-pinned file URLs and pack-relative path safety. No Node
 * or DOM imports and only relative imports, so the main process, the
 * renderer (PackService, sample sources) and tests share it. Everything
 * coming from the network or from disk (catalog.json, pack.json) goes through
 * here before it is trusted.
 *
 * The runtime rules match the curation rules of scripts/pack-lib.mjs: every
 * file has a size and a sha256 and comes from a commit-pinned
 * raw.githubusercontent.com URL; `source` links to github.com; `color` is a
 * plain #hex or oklch() color.
 */
import type { InstalledPack, PackCatalog, PackFile, PackKit, PackKitPad, PackLicense, PackManifest } from '../types/packs';
import { PACK_ALLOWED_HOSTS } from '../types/packs';
import { PAD_COUNT } from '../types/sampler';

/** Max bytes of one downloaded pack file. */
export const MAX_FILE_BYTES = 50 * 1024 * 1024;
/** Max bytes of one whole pack. */
export const MAX_PACK_BYTES = 500 * 1024 * 1024;
/** Max bytes of one remote preview. */
export const MAX_PREVIEW_BYTES = 10 * 1024 * 1024;
/** Max bytes of the remote catalog JSON. */
export const MAX_CATALOG_BYTES = 5 * 1024 * 1024;
/** Sanity cap on files per pack. */
export const MAX_PACK_FILES = 5000;
/** Max HTTP redirects followed per request. */
export const MAX_REDIRECTS = 5;

/** Name of the metadata file written into every installed pack folder. */
export const PACK_META_FILE = 'pack.json';

const LICENSES: readonly PackLicense[] = ['CC0-1.0', 'public-domain', 'CC-BY-4.0'];

/** File extensions a pack may contain (audio + licence/readme text). */
const ALLOWED_EXTENSIONS = new Set(['wav', 'wave', 'aif', 'aiff', 'flac', 'mp3', 'ogg', 'm4a', 'txt', 'md']);

/** Windows reserved device names (a segment like "con.wav" is unsafe there). */
const WIN_RESERVED = /^(con|prn|aux|nul|com[0-9]|lpt[0-9])(\..*)?$/i;

const PACK_ID_RE = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;
const SHA256_RE = /^[0-9a-f]{64}$/;
const COMMIT_RE = /^[0-9a-f]{40}$/;
const GH_OWNER_RE = /^[A-Za-z0-9](?:[A-Za-z0-9-]{0,38})$/;
const GH_REPO_RE = /^[A-Za-z0-9_.-]{1,100}$/;
/** Host of every pack file URL (commit-pinned raw GitHub content). */
export const PACK_FILE_HOST = 'raw.githubusercontent.com';
/** Max length of a file's display label. */
export const MAX_LABEL_LENGTH = 80;

export class PackValidationError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'PackValidationError';
  }
}

/** kebab-case, 1..64 chars: safe as a folder name on every platform. */
export function isValidPackId(id: unknown): id is string {
  return typeof id === 'string' && id.length > 0 && id.length <= 64 && PACK_ID_RE.test(id);
}

/** True when `url` is https on one of PACK_ALLOWED_HOSTS (no credentials, default port). */
export function isAllowedUrl(url: unknown): boolean {
  if (typeof url !== 'string' || url.length > 4096) return false;
  let u: URL;
  try {
    u = new URL(url);
  } catch {
    return false;
  }
  if (u.protocol !== 'https:') return false;
  if (u.username || u.password) return false;
  if (u.port && u.port !== '443') return false;
  return (PACK_ALLOWED_HOSTS as readonly string[]).includes(u.hostname.toLowerCase());
}

/**
 * True when `url` is a pack file URL the app downloads:
 * https://raw.githubusercontent.com/<owner>/<repo>/<40-hex commit>/<path>
 * (no credentials, port, query or fragment), so the bytes can never change
 * under a pinned sha256.
 */
export function isPinnedPackUrl(url: unknown): url is string {
  if (!isAllowedUrl(url)) return false;
  const u = new URL(url as string);
  // Canonical form only (no dot segments, default port, lower-case host), so what is checked is what is fetched.
  if (u.href !== url || u.hostname !== PACK_FILE_HOST || u.search || u.hash) return false;
  const parts = u.pathname.split('/');
  // ['', owner, repo, commit, ...path]
  if (parts.length < 5 || !GH_OWNER_RE.test(parts[1]) || !GH_REPO_RE.test(parts[2]) || !COMMIT_RE.test(parts[3])) return false;
  return parts.slice(4).every((p) => p.length > 0 && p !== '.' && p !== '..' && !/%2f|%5c|%2e%2e/i.test(p));
}

/** True for https://github.com/... links (the pack's upstream `source`). */
export function isGithubSourceUrl(url: unknown): url is string {
  if (typeof url !== 'string' || url.length > 2000) return false;
  let u: URL;
  try {
    u = new URL(url);
  } catch {
    return false;
  }
  return u.protocol === 'https:' && u.hostname.toLowerCase() === 'github.com' && !u.username && !u.password && (!u.port || u.port === '443');
}

const NUM = String.raw`[+-]?(?:\d+(?:\.\d*)?|\.\d+)`;
const HEX_COLOR_RE = /^#(?:[0-9a-f]{3,4}|[0-9a-f]{6}|[0-9a-f]{8})$/i;
const OKLCH_RE = new RegExp(String.raw`^oklch\(\s*${NUM}%?\s+${NUM}%?\s+${NUM}(?:deg)?\s*(?:/\s*${NUM}%?\s*)?\)$`, 'i');

/**
 * A pack accent color: `#rgb[a]`, `#rrggbb[aa]` or `oklch(L C H [/ A])` with
 * plain numbers. Anything else (url(), var(), expressions, other CSS) is
 * rejected, so a catalog value can never inject CSS.
 */
export function isSafePackColor(color: unknown): color is string {
  return typeof color === 'string' && color.length <= 64 && (HEX_COLOR_RE.test(color) || OKLCH_RE.test(color));
}

/**
 * Validates a pack-relative file path ("kicks/bd-01.wav") and returns its
 * segments. Rejects absolute paths, backslashes, "."/".." and empty segments,
 * control characters, drive letters, Windows-reserved names, the pack meta
 * file and non-audio extensions. Throws PackValidationError.
 */
export function safePackPathSegments(rel: unknown): string[] {
  if (typeof rel !== 'string' || rel.length === 0 || rel.length > 512) throw new PackValidationError('Invalid file path');
  // eslint-disable-next-line no-control-regex
  if (/[\u0000-\u001f\u007f\\:*?"<>|]/.test(rel)) throw new PackValidationError(`Unsafe characters in file path: ${rel}`);
  if (rel.startsWith('/')) throw new PackValidationError(`Absolute file path: ${rel}`);
  const segments = rel.split('/');
  for (const seg of segments) {
    if (seg === '' || seg === '.' || seg === '..') throw new PackValidationError(`Unsafe file path: ${rel}`);
    if (seg.length > 128) throw new PackValidationError(`File path segment too long: ${rel}`);
    if (seg.startsWith('.')) throw new PackValidationError(`Hidden file in pack: ${rel}`);
    if (/[. ]$/.test(seg) || WIN_RESERVED.test(seg)) throw new PackValidationError(`Unsafe file name: ${rel}`);
  }
  if (segments.length > 8) throw new PackValidationError(`File path too deep: ${rel}`);
  const name = segments[segments.length - 1];
  if (segments.length === 1 && name.toLowerCase() === PACK_META_FILE) throw new PackValidationError(`Reserved file name: ${rel}`);
  const dot = name.lastIndexOf('.');
  const ext = dot > 0 ? name.slice(dot + 1).toLowerCase() : '';
  if (!ALLOWED_EXTENSIONS.has(ext)) throw new PackValidationError(`File type not allowed: ${rel}`);
  return segments;
}

export function isSafePackPath(rel: unknown): rel is string {
  try {
    safePackPathSegments(rel);
    return true;
  } catch {
    return false;
  }
}

// ---------------------------------------------------------------------------
// Schema
// ---------------------------------------------------------------------------

type Obj = Record<string, unknown>;
const isObj = (v: unknown): v is Obj => !!v && typeof v === 'object' && !Array.isArray(v);

function str(o: Obj, key: string, max = 2000, required = true): string | undefined {
  const v = o[key];
  if (v === undefined && !required) return undefined;
  if (typeof v !== 'string' || (required && v.trim() === '') || v.length > max) {
    throw new PackValidationError(`Invalid "${key}"`);
  }
  return v;
}

function validateFile(raw: unknown): PackFile {
  if (!isObj(raw)) throw new PackValidationError('Invalid file entry');
  const path = str(raw, 'path') as string;
  safePackPathSegments(path);
  const url = str(raw, 'url', 4096) as string;
  if (!isAllowedUrl(url)) throw new PackValidationError(`File URL not allowed: ${url}`);
  if (!isPinnedPackUrl(url)) throw new PackValidationError(`File URL not pinned to a commit: ${url}`);
  if (typeof raw.size !== 'number' || !Number.isInteger(raw.size) || raw.size <= 0) throw new PackValidationError(`Invalid size for ${path}`);
  if (raw.size > MAX_FILE_BYTES) throw new PackValidationError(`File too large: ${path}`);
  if (typeof raw.sha256 !== 'string' || !SHA256_RE.test(raw.sha256)) throw new PackValidationError(`Invalid sha256 for ${path}`);
  const file: PackFile = { path, url, size: raw.size, sha256: raw.sha256 };
  if (raw.label !== undefined) {
    // eslint-disable-next-line no-control-regex
    if (typeof raw.label !== 'string' || raw.label.trim() === '' || raw.label.length > MAX_LABEL_LENGTH || /[\u0000-\u001f\u007f]/.test(raw.label)) {
      throw new PackValidationError(`Invalid label for ${path}`);
    }
    file.label = raw.label;
  }
  return file;
}

function validateKit(raw: unknown, files: ReadonlySet<string>): PackKit {
  if (!isObj(raw)) throw new PackValidationError('Invalid kit');
  const id = str(raw, 'id', 64) as string;
  const name = str(raw, 'name', 200) as string;
  if (!Array.isArray(raw.pads)) throw new PackValidationError(`Kit ${id} has no pads`);
  const used = new Set<number>();
  const pads: PackKitPad[] = [];
  for (const p of raw.pads) {
    if (!isObj(p)) throw new PackValidationError(`Kit ${id}: invalid pad`);
    const pad = p.pad;
    if (typeof pad !== 'number' || !Number.isInteger(pad) || pad < 0 || pad >= PAD_COUNT) throw new PackValidationError(`Kit ${id}: invalid pad index`);
    if (used.has(pad)) throw new PackValidationError(`Kit ${id}: pad ${pad} assigned twice`);
    used.add(pad);
    const file = str(p, 'file') as string;
    if (!files.has(file)) throw new PackValidationError(`Kit ${id}: unknown file ${file}`);
    const out: PackKitPad = { pad, file };
    const padName = str(p, 'name', 200, false);
    if (padName) out.name = padName;
    if (p.settings !== undefined) {
      if (!isObj(p.settings)) throw new PackValidationError(`Kit ${id}: invalid pad settings`);
      // Values are clamped later by normalizePad; only plain JSON scalars pass.
      const settings: Obj = {};
      for (const [k, v] of Object.entries(p.settings)) {
        if (k === 'sample' || k === '__proto__') continue;
        if (typeof v === 'number' || typeof v === 'string' || typeof v === 'boolean' || v === null) settings[k] = v;
      }
      out.settings = settings as PackKitPad['settings'];
    }
    pads.push(out);
  }
  return { id, name, pads };
}

function safeColor(v: unknown): string {
  if (!isSafePackColor(v)) throw new PackValidationError('Invalid "color" (#hex or oklch() only)');
  return v;
}

/** Validates one manifest; throws PackValidationError on the first problem. */
export function validateManifest(raw: unknown): PackManifest {
  if (!isObj(raw)) throw new PackValidationError('Invalid pack');
  if (!isValidPackId(raw.id)) throw new PackValidationError(`Invalid pack id: ${String(raw.id)}`);
  const id = raw.id;
  try {
    const license = raw.license as PackLicense;
    if (!LICENSES.includes(license)) throw new PackValidationError(`Unsupported licence: ${String(raw.license)}`);
    const source = str(raw, 'source', 2000) as string;
    if (!isGithubSourceUrl(source)) throw new PackValidationError('Source must be an https://github.com/ URL');
    if (!Array.isArray(raw.tags) || !raw.tags.every((t) => typeof t === 'string' && t.length <= 64)) {
      throw new PackValidationError('Invalid tags');
    }
    if (!Array.isArray(raw.files) || raw.files.length === 0) throw new PackValidationError('Pack has no files');
    if (raw.files.length > MAX_PACK_FILES) throw new PackValidationError('Too many files');
    const files = raw.files.map(validateFile);
    const seen = new Set<string>();
    let declared = 0;
    for (const f of files) {
      // Case-insensitive: macOS/Windows file systems would merge these.
      const key = f.path.toLowerCase();
      if (seen.has(key)) throw new PackValidationError(`Duplicate file path: ${f.path}`);
      seen.add(key);
      declared += f.size;
    }
    if (declared > MAX_PACK_BYTES) throw new PackValidationError('Pack too large');
    // A file and a folder must not share a name ("a.wav" and "a.wav/b.wav").
    for (const f of files) {
      const segs = f.path.toLowerCase().split('/');
      for (let i = 1; i < segs.length; i++) {
        if (seen.has(segs.slice(0, i).join('/'))) throw new PackValidationError(`File/folder name clash: ${f.path}`);
      }
    }
    const filePaths = new Set(files.map((f) => f.path));
    if (!Array.isArray(raw.kits)) throw new PackValidationError('Invalid kits');
    const kits = raw.kits.map((k) => validateKit(k, filePaths));
    const kitIds = new Set<string>();
    for (const k of kits) {
      if (kitIds.has(k.id)) throw new PackValidationError(`Duplicate kit id: ${k.id}`);
      kitIds.add(k.id);
    }
    const version = raw.version;
    if (typeof version !== 'number' || !Number.isInteger(version) || version < 0) throw new PackValidationError('Invalid version');
    const manifest: PackManifest = {
      id,
      name: str(raw, 'name', 200) as string,
      tagline: str(raw, 'tagline', 200) as string,
      description: str(raw, 'description', 5000) as string,
      tags: raw.tags as string[],
      author: str(raw, 'author', 500) as string,
      source,
      license,
      color: safeColor(raw.color),
      files,
      kits,
      version,
    };
    const attribution = str(raw, 'attribution', 2000, false);
    if (attribution) manifest.attribution = attribution;
    if (license === 'CC-BY-4.0' && !attribution) throw new PackValidationError('CC-BY pack needs attribution');
    return manifest;
  } catch (err) {
    if (err instanceof PackValidationError) throw new PackValidationError(`Pack ${id}: ${err.message}`);
    throw err;
  }
}

export interface CatalogValidation {
  catalog: PackCatalog;
  /** Packs that were dropped, with the reason. */
  errors: string[];
}

/**
 * Validates a catalog. The envelope must be valid (throws otherwise); invalid
 * or duplicate packs are dropped and reported in `errors`, so one bad entry
 * does not hide the rest.
 */
export function validateCatalog(raw: unknown): CatalogValidation {
  if (!isObj(raw)) throw new PackValidationError('Catalog is not an object');
  if (raw.version !== 1) throw new PackValidationError(`Unsupported catalog version: ${String(raw.version)}`);
  if (typeof raw.updatedAt !== 'string' || raw.updatedAt.length > 64) throw new PackValidationError('Invalid catalog updatedAt');
  if (!Array.isArray(raw.packs)) throw new PackValidationError('Catalog has no packs array');
  const errors: string[] = [];
  const packs: PackManifest[] = [];
  const ids = new Set<string>();
  for (const p of raw.packs) {
    try {
      const m = validateManifest(p);
      if (ids.has(m.id)) throw new PackValidationError(`Duplicate pack id: ${m.id}`);
      ids.add(m.id);
      packs.push(m);
    } catch (err) {
      errors.push(err instanceof Error ? err.message : String(err));
    }
  }
  return { catalog: { version: 1, updatedAt: raw.updatedAt, packs }, errors };
}

/** Validates a stored pack.json; `dir` is replaced by the folder it was read from. */
export function validateInstalled(raw: unknown, dir: string): InstalledPack {
  if (!isObj(raw)) throw new PackValidationError('Invalid pack.json');
  const manifest = validateManifest(raw.manifest);
  if (raw.id !== manifest.id) throw new PackValidationError('pack.json id mismatch');
  const version = typeof raw.version === 'number' && Number.isInteger(raw.version) ? raw.version : manifest.version;
  const installedAt = typeof raw.installedAt === 'string' ? raw.installedAt : new Date(0).toISOString();
  return { id: manifest.id, version, dir, installedAt, manifest };
}

/** Sum of declared file sizes (every validated file has one). */
export function declaredPackBytes(manifest: PackManifest): number {
  let total = 0;
  for (const f of manifest.files) total += f.size;
  return total;
}

/** The catalog file with exactly this URL, or null (the preview IPC fetches only these). */
export function findCatalogFile(catalog: PackCatalog, url: unknown): PackFile | null {
  if (typeof url !== 'string') return null;
  for (const p of catalog.packs) {
    for (const f of p.files) if (f.url === url) return f;
  }
  return null;
}

/** Date.parse of a catalog's updatedAt; NaN when unparsable. */
function catalogTime(c: PackCatalog): number {
  return Date.parse(c.updatedAt);
}

/**
 * Chooses between a validated remote catalog and the bundled one so the remote
 * copy can never silently replace good data with weaker data:
 * - a remote catalog that is not strictly newer (`updatedAt`) than the bundled
 *   one, or undated, loses to it;
 * - a pack the remote catalog dropped as invalid (`remoteErrors` names it)
 *   keeps the bundled entry;
 * - per pack, a lower remote `version` keeps the bundled entry.
 * Packs the remote catalog removes cleanly stay removed (that is how a pack is
 * withdrawn), and remote-only packs are added.
 */
export function mergeCatalogs(
  bundled: PackCatalog,
  remote: PackCatalog,
  remoteErrors: readonly string[] = [],
): { catalog: PackCatalog; source: 'remote' | 'bundled' } {
  const rt = catalogTime(remote);
  const bt = catalogTime(bundled);
  if (!Number.isFinite(rt) || (Number.isFinite(bt) && rt <= bt)) return { catalog: bundled, source: 'bundled' };
  if (remote.packs.length === 0 && bundled.packs.length > 0) return { catalog: bundled, source: 'bundled' };
  const bundledById = new Map(bundled.packs.map((p) => [p.id, p]));
  const packs = remote.packs.map((p) => {
    const b = bundledById.get(p.id);
    return b && b.version > p.version ? b : p;
  });
  const have = new Set(packs.map((p) => p.id));
  for (const b of bundled.packs) {
    if (have.has(b.id)) continue;
    if (remoteErrors.some((e) => e.startsWith(`Pack ${b.id}:`) || e === `Duplicate pack id: ${b.id}`)) packs.push(b);
  }
  return { catalog: { version: 1, updatedAt: remote.updatedAt, packs }, source: 'remote' };
}
