/**
 * Shared, dependency-free helpers for the pack catalog scripts
 * (scripts/pack-build.mjs, scripts/pack-check.mjs) and the catalog unit test.
 *
 * Sources of truth:
 *   packs/sources/<id>.json  curated pack specs (metadata, upstream repo + commit,
 *                            licence evidence, file mapping, kits)
 *   packs/catalog.json       generated PackCatalog (src/types/packs.ts) served to the app
 *
 * See docs/packs.md for the rules these checks enforce.
 */

export const RAW_HOST = 'raw.githubusercontent.com';
/** Must match PACK_ALLOWED_HOSTS in src/types/packs.ts. */
export const ALLOWED_HOSTS = ['raw.githubusercontent.com'];
/** Max length of a file's display label (MAX_LABEL_LENGTH in src/shared/packValidate.ts). */
export const MAX_LABEL_LENGTH = 80;
export const LICENSES = ['CC0-1.0', 'public-domain', 'CC-BY-4.0'];
export const PAD_COUNT = 16;
/** Curation limit per pack (the app itself allows 500 MB). */
export const MAX_PACK_BYTES = 60 * 1024 * 1024;
/** App limit per file (src/main/packs/validate.ts). */
export const MAX_FILE_BYTES = 50 * 1024 * 1024;
export const AUDIO_EXTENSIONS = ['wav', 'wave', 'aif', 'aiff', 'flac', 'mp3', 'ogg', 'm4a'];
export const TEXT_EXTENSIONS = ['txt', 'md'];
/** Pad settings a kit may set (PadSettings minus `sample`). */
export const PAD_SETTING_KEYS = ['gainDb', 'pan', 'pitch', 'startSec', 'endSec', 'decay', 'filterCutoff', 'reverse', 'mode', 'chokeGroup', 'color'];
/** Words that must not appear as a product claim in pack names (factual credits in description/attribution are fine). */
export const TRADEMARKS = ['roland', 'linn', 'yamaha', 'korg', 'akai', 'e-mu', 'emu ', 'oberheim', 'sequential', 'boss'];

const PACK_ID_RE = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;
const SHA_RE = /^[0-9a-f]{40}$/;
const SHA256_RE = /^[0-9a-f]{64}$/;
const REPO_RE = /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/;
const WIN_RESERVED = /^(con|prn|aux|nul|com[0-9]|lpt[0-9])(\..*)?$/i;

const HEX_COLOR_RE = /^#(?:[0-9a-f]{3,4}|[0-9a-f]{6}|[0-9a-f]{8})$/i;
const NUM = String.raw`[+-]?(?:\d+(?:\.\d*)?|\.\d+)`;
const OKLCH_RE = new RegExp(String.raw`^oklch\(\s*${NUM}%?\s+${NUM}%?\s+${NUM}(?:deg)?\s*(?:/\s*${NUM}%?\s*)?\)$`, 'i');

/** Mirrors isSafePackColor in src/shared/packValidate.ts: #hex or oklch() only. */
export const isSafeColor = (c) => typeof c === 'string' && c.length <= 64 && (HEX_COLOR_RE.test(c) || OKLCH_RE.test(c));

/** Optional display label of a pack file; returns an error string or null. */
export function labelError(label) {
  if (label === undefined) return null;
  // eslint-disable-next-line no-control-regex
  if (typeof label !== 'string' || label.trim() === '' || label.length > MAX_LABEL_LENGTH || /[\u0000-\u001f\u007f]/.test(label)) {
    return `label must be a non-empty string of at most ${MAX_LABEL_LENGTH} characters`;
  }
  return null;
}

const isObj = (v) => !!v && typeof v === 'object' && !Array.isArray(v);
const nonEmpty = (v) => typeof v === 'string' && v.trim().length > 0;

/** Pinned raw URL for a file in a GitHub repo at a commit; every path segment is percent-encoded. */
export function rawUrl(repo, commit, path) {
  const encoded = path.split('/').map((s) => encodeURIComponent(s)).join('/');
  return `https://${RAW_HOST}/${repo}/${commit}/${encoded}`;
}

/** True when `url` is https://raw.githubusercontent.com/<owner>/<repo>/<40-hex sha>/<path>. */
export function isPinnedRawUrl(url) {
  let u;
  try {
    u = new URL(url);
  } catch {
    return false;
  }
  if (u.protocol !== 'https:' || u.hostname !== RAW_HOST) return false;
  const parts = u.pathname.split('/');
  // ['', owner, repo, sha, ...path]
  return parts.length >= 5 && SHA_RE.test(parts[3]) && parts.slice(4).every((p) => p.length > 0);
}

/** Mirrors safePackPathSegments in src/main/packs/validate.ts; returns an error string or null. */
export function packPathError(rel) {
  if (typeof rel !== 'string' || rel.length === 0 || rel.length > 512) return 'invalid path';
  // eslint-disable-next-line no-control-regex
  if (/[\u0000-\u001f\u007f\\:*?"<>|]/.test(rel)) return 'unsafe characters';
  if (rel.startsWith('/')) return 'absolute path';
  if (/\s/.test(rel)) return 'paths must not contain whitespace';
  const segs = rel.split('/');
  if (segs.length > 8) return 'too deep';
  for (const s of segs) {
    if (s === '' || s === '.' || s === '..') return 'empty or relative segment';
    if (s.length > 128) return 'segment too long';
    if (s.startsWith('.')) return 'hidden file';
    if (/[. ]$/.test(s) || WIN_RESERVED.test(s)) return 'unsafe file name';
  }
  const name = segs[segs.length - 1];
  if (segs.length === 1 && name === 'pack.json') return 'reserved name';
  const ext = name.includes('.') ? name.slice(name.lastIndexOf('.') + 1) : '';
  if (!AUDIO_EXTENSIONS.includes(ext) && !TEXT_EXTENSIONS.includes(ext)) return `extension .${ext} not allowed`;
  // Audio paths are lower-case so they read the same on case-insensitive file systems (licence/readme files may keep their usual names).
  if (AUDIO_EXTENSIONS.includes(ext) && rel !== rel.toLowerCase()) return 'audio paths must be lower-case';
  return null;
}

export const isAudioPath = (p) => AUDIO_EXTENSIONS.includes(p.slice(p.lastIndexOf('.') + 1));

function checkPadSettings(where, settings, errors) {
  if (settings === undefined) return;
  if (!isObj(settings)) {
    errors.push(`${where}: settings must be an object`);
    return;
  }
  for (const [k, v] of Object.entries(settings)) {
    if (!PAD_SETTING_KEYS.includes(k)) errors.push(`${where}: unknown setting "${k}"`);
    else if (k === 'mode' && v !== 'oneshot' && v !== 'gate') errors.push(`${where}: mode must be oneshot|gate`);
    else if (k === 'reverse' && typeof v !== 'boolean') errors.push(`${where}: reverse must be boolean`);
    else if (k === 'color' && typeof v !== 'string') errors.push(`${where}: color must be a string`);
    else if (k === 'endSec' && v !== null && typeof v !== 'number') errors.push(`${where}: endSec must be number|null`);
    else if (!['mode', 'reverse', 'color', 'endSec'].includes(k) && (typeof v !== 'number' || !Number.isFinite(v))) {
      errors.push(`${where}: ${k} must be a finite number`);
    }
  }
  const s = settings;
  const range = (k, lo, hi) => {
    if (typeof s[k] === 'number' && (s[k] < lo || s[k] > hi)) errors.push(`${where}: ${k} out of range ${lo}..${hi}`);
  };
  range('gainDb', -60, 12);
  range('pan', -1, 1);
  range('pitch', -24, 24);
  range('decay', 0.005, 4);
  range('filterCutoff', 20, 20000);
  range('startSec', 0, 600);
  if (typeof s.chokeGroup === 'number' && (!Number.isInteger(s.chokeGroup) || s.chokeGroup < 0)) {
    errors.push(`${where}: chokeGroup must be a non-negative integer`);
  }
}

function checkKits(where, kits, filePaths, errors) {
  if (!Array.isArray(kits) || kits.length === 0) {
    errors.push(`${where}: needs at least one kit`);
    return;
  }
  if (kits.length > 4) errors.push(`${where}: at most 4 kits per pack`);
  const ids = new Set();
  for (const kit of kits) {
    const kw = `${where} kit ${kit?.id}`;
    if (!isObj(kit) || !nonEmpty(kit.id) || !nonEmpty(kit.name)) {
      errors.push(`${kw}: needs id and name`);
      continue;
    }
    if (!PACK_ID_RE.test(kit.id)) errors.push(`${kw}: id must be kebab-case`);
    if (ids.has(kit.id)) errors.push(`${kw}: duplicate kit id`);
    ids.add(kit.id);
    if (!Array.isArray(kit.pads)) {
      errors.push(`${kw}: pads must be an array`);
      continue;
    }
    if (kit.pads.length !== PAD_COUNT) errors.push(`${kw}: curated kits fill all ${PAD_COUNT} pads (has ${kit.pads.length})`);
    const used = new Set();
    for (const p of kit.pads) {
      const pw = `${kw} pad ${p?.pad}`;
      if (!isObj(p) || !Number.isInteger(p.pad) || p.pad < 0 || p.pad >= PAD_COUNT) {
        errors.push(`${kw}: invalid pad index`);
        continue;
      }
      if (used.has(p.pad)) errors.push(`${pw}: assigned twice`);
      used.add(p.pad);
      if (!filePaths.has(p.file)) errors.push(`${pw}: file "${p.file}" is not in files`);
      else if (!isAudioPath(p.file)) errors.push(`${pw}: file "${p.file}" is not audio`);
      if (!nonEmpty(p.name)) errors.push(`${pw}: needs a display name`);
      else if (p.name.length > 24) errors.push(`${pw}: name "${p.name}" too long for a pad (max 24)`);
      checkPadSettings(pw, p.settings, errors);
    }
  }
}

/** Fields shared by the generated manifest and the source spec. */
function checkMeta(where, m, errors) {
  if (!PACK_ID_RE.test(m.id ?? '')) errors.push(`${where}: id must be kebab-case`);
  for (const k of ['name', 'tagline', 'description', 'author', 'source', 'color']) {
    if (!nonEmpty(m[k])) errors.push(`${where}: missing ${k}`);
  }
  if (nonEmpty(m.source) && !m.source.startsWith('https://github.com/')) errors.push(`${where}: source must be an https://github.com/ URL`);
  if (nonEmpty(m.color) && !isSafeColor(m.color)) errors.push(`${where}: color must be #hex or oklch(L C H)`);
  if (!LICENSES.includes(m.license)) errors.push(`${where}: licence must be one of ${LICENSES.join(', ')}`);
  if (m.license === 'CC-BY-4.0' && !nonEmpty(m.attribution)) errors.push(`${where}: CC-BY needs attribution`);
  if (!Array.isArray(m.tags) || m.tags.length === 0 || !m.tags.every((t) => nonEmpty(t) && t === t.toLowerCase())) {
    errors.push(`${where}: tags must be non-empty lower-case strings`);
  }
  if (!Number.isInteger(m.version) || m.version < 1) errors.push(`${where}: version must be an integer >= 1`);
  for (const key of ['name', 'tagline']) {
    const text = String(m[key] ?? '').toLowerCase();
    for (const tm of TRADEMARKS) {
      if (text.includes(tm)) errors.push(`${where}: ${key} must not present a trademark ("${tm.trim()}"); credit it in description/attribution`);
    }
  }
}

/**
 * Validates a generated catalog against the PackCatalog contract and the
 * curation rules (pinned URLs, sizes + sha256 present, < 60 MB per pack,
 * 16-pad kits). Returns a list of human-readable errors (empty = OK).
 */
export function checkCatalog(catalog) {
  const errors = [];
  if (!isObj(catalog)) return ['catalog is not an object'];
  if (catalog.version !== 1) errors.push('catalog.version must be 1');
  if (!nonEmpty(catalog.updatedAt) || Number.isNaN(Date.parse(catalog.updatedAt))) errors.push('catalog.updatedAt must be an ISO date');
  if (!Array.isArray(catalog.packs)) return [...errors, 'catalog.packs must be an array'];
  const ids = new Set();
  for (const m of catalog.packs) {
    const where = `pack ${m?.id}`;
    if (!isObj(m)) {
      errors.push('pack entry is not an object');
      continue;
    }
    if (ids.has(m.id)) errors.push(`${where}: duplicate id`);
    ids.add(m.id);
    checkMeta(where, m, errors);
    if ('upstream' in m || 'licenseEvidence' in m) errors.push(`${where}: source-only fields leaked into the catalog`);
    if (!Array.isArray(m.files) || m.files.length === 0) {
      errors.push(`${where}: no files`);
      continue;
    }
    const paths = new Set();
    let total = 0;
    for (const f of m.files) {
      const fw = `${where} file ${f?.path}`;
      const pathErr = packPathError(f?.path);
      if (pathErr) errors.push(`${fw}: ${pathErr}`);
      if (paths.has(f.path)) errors.push(`${fw}: duplicate path`);
      paths.add(f.path);
      if (!isPinnedRawUrl(f.url)) errors.push(`${fw}: url must be a raw.githubusercontent.com URL pinned to a commit SHA`);
      if (!Number.isInteger(f.size) || f.size <= 0) errors.push(`${fw}: size missing`);
      else if (f.size > MAX_FILE_BYTES) errors.push(`${fw}: larger than 50 MB`);
      else total += f.size;
      if (!SHA256_RE.test(f.sha256 ?? '')) errors.push(`${fw}: sha256 missing`);
      const le = labelError(f.label);
      if (le) errors.push(`${fw}: ${le}`);
    }
    for (const p of paths) {
      const segs = p.split('/');
      for (let i = 1; i < segs.length; i++) {
        if (paths.has(segs.slice(0, i).join('/'))) errors.push(`${where}: file/folder clash at ${p}`);
      }
    }
    if (total > MAX_PACK_BYTES) errors.push(`${where}: ${(total / 1048576).toFixed(1)} MB exceeds the 60 MB curation limit`);
    checkKits(where, m.kits, paths, errors);
  }
  return errors;
}

/** Validates one source spec (packs/sources/<id>.json). */
export function checkSource(src, fileName) {
  const errors = [];
  const where = `source ${fileName ?? src?.id}`;
  if (!isObj(src)) return [`${where}: not an object`];
  if (fileName && fileName !== `${src.id}.json`) errors.push(`${where}: file name must be <id>.json`);
  checkMeta(where, src, errors);
  if (src.order !== undefined && !Number.isInteger(src.order)) errors.push(`${where}: order must be an integer`);
  const up = src.upstream;
  if (!isObj(up) || !REPO_RE.test(up.repo ?? '') || !SHA_RE.test(up.commit ?? '')) {
    errors.push(`${where}: upstream needs repo "owner/name" and a full 40-hex commit SHA`);
  }
  if (!Array.isArray(src.licenseEvidence) || src.licenseEvidence.length === 0) {
    errors.push(`${where}: licenseEvidence (url + quote) is required`);
  } else {
    for (const ev of src.licenseEvidence) {
      if (!isObj(ev) || !nonEmpty(ev.quote) || !nonEmpty(ev.url) || !ev.url.startsWith('https://')) {
        errors.push(`${where}: each licenseEvidence entry needs an https url and a quote`);
      }
    }
  }
  if (!Array.isArray(src.files) || src.files.length === 0) {
    errors.push(`${where}: no files`);
    return errors;
  }
  const paths = new Set();
  for (const f of src.files) {
    const err = packPathError(f?.path);
    if (err) errors.push(`${where} file ${f?.path}: ${err}`);
    if (!nonEmpty(f?.from)) errors.push(`${where} file ${f?.path}: missing "from"`);
    const le = labelError(f?.label);
    if (le) errors.push(`${where} file ${f?.path}: ${le}`);
    if (paths.has(f.path)) errors.push(`${where} file ${f.path}: duplicate path`);
    paths.add(f.path);
  }
  checkKits(where, src.kits, paths, errors);
  return errors;
}

/** Manifest (PackManifest) for a source spec; `info(path)` returns { size, sha256 } for each pack file. */
export function manifestFromSource(src, info) {
  const m = {
    id: src.id,
    name: src.name,
    tagline: src.tagline,
    description: src.description,
    tags: src.tags,
    author: src.author,
    source: src.source,
    license: src.license,
  };
  if (src.attribution) m.attribution = src.attribution;
  m.color = src.color;
  m.files = src.files.map((f) => {
    const { size, sha256 } = info(f.path) ?? {};
    const file = { path: f.path, url: rawUrl(src.upstream.repo, src.upstream.commit, f.from) };
    if (size !== undefined) file.size = size;
    if (sha256 !== undefined) file.sha256 = sha256;
    if (f.label !== undefined) file.label = f.label;
    return file;
  });
  m.kits = src.kits;
  m.version = src.version;
  return m;
}

/**
 * Checks that the catalog was generated from the current sources (same
 * packs, URLs and kits). Returns errors; empty when in sync.
 */
export function checkSync(sources, catalog) {
  const errors = [];
  const byId = new Map((catalog?.packs ?? []).map((p) => [p.id, p]));
  for (const src of sources) {
    const built = byId.get(src.id);
    if (!built) {
      errors.push(`pack ${src.id}: in packs/sources but not in catalog.json (run node scripts/pack-build.mjs)`);
      continue;
    }
    byId.delete(src.id);
    const sizes = new Map(built.files.map((f) => [f.path, f]));
    const expected = manifestFromSource(src, (p) => sizes.get(p));
    if (JSON.stringify(expected) !== JSON.stringify(built)) {
      errors.push(`pack ${src.id}: catalog.json is out of date with packs/sources/${src.id}.json (run node scripts/pack-build.mjs)`);
    }
  }
  for (const id of byId.keys()) errors.push(`pack ${id}: in catalog.json but has no packs/sources/${id}.json`);
  return errors;
}

/** Catalog order: `order` ascending (default 1000), then id. Items are `{ spec }` wrappers. */
export function sortSources(items) {
  return [...items].sort((a, b) => (a.spec.order ?? 1000) - (b.spec.order ?? 1000) || String(a.spec.id).localeCompare(String(b.spec.id)));
}

/** Total declared bytes of a manifest. */
export const packBytes = (m) => m.files.reduce((n, f) => n + (f.size ?? 0), 0);
