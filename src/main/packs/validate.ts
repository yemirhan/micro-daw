/**
 * Pure validation for the pack store: catalog/manifest schema, the https host
 * allowlist and pack-relative path safety. No Node imports, so the renderer
 * and tests can use it too. Everything coming from the network or from disk
 * (catalog.json, pack.json) goes through here before it is trusted.
 */
import type { InstalledPack, PackCatalog, PackFile, PackKit, PackKitPad, PackLicense, PackManifest } from '../../types/packs';
import { PACK_ALLOWED_HOSTS } from '../../types/packs';
import { PAD_COUNT } from '../../types/sampler';

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
  const file: PackFile = { path, url };
  if (raw.size !== undefined) {
    if (typeof raw.size !== 'number' || !Number.isInteger(raw.size) || raw.size < 0) throw new PackValidationError(`Invalid size for ${path}`);
    if (raw.size > MAX_FILE_BYTES) throw new PackValidationError(`File too large: ${path}`);
    file.size = raw.size;
  }
  if (raw.sha256 !== undefined) {
    if (typeof raw.sha256 !== 'string' || !SHA256_RE.test(raw.sha256)) throw new PackValidationError(`Invalid sha256 for ${path}`);
    file.sha256 = raw.sha256;
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

/** Validates one manifest; throws PackValidationError on the first problem. */
export function validateManifest(raw: unknown): PackManifest {
  if (!isObj(raw)) throw new PackValidationError('Invalid pack');
  if (!isValidPackId(raw.id)) throw new PackValidationError(`Invalid pack id: ${String(raw.id)}`);
  const id = raw.id;
  try {
    const license = raw.license as PackLicense;
    if (!LICENSES.includes(license)) throw new PackValidationError(`Unsupported licence: ${String(raw.license)}`);
    const source = str(raw, 'source', 2000) as string;
    if (!/^https:\/\//.test(source)) throw new PackValidationError('Source must be an https URL');
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
      declared += f.size ?? 0;
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
      color: str(raw, 'color', 64) as string,
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

/** Sum of declared file sizes, or null when any size is unknown. */
export function declaredPackBytes(manifest: PackManifest): number | null {
  let total = 0;
  for (const f of manifest.files) {
    if (f.size === undefined) return null;
    total += f.size;
  }
  return total;
}
