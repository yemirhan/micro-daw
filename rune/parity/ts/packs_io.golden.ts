// @vitest-environment node
/**
 * Golden scenarios for rune/crates/mdaw-packs (src/main/packs/{download,installer,catalog}.ts):
 *
 * - `pack_json`: the bytes the installer writes as pack.json (`JSON.stringify(installed, null, 2)`)
 *   for every real catalog pack and a few test manifests;
 * - `download`: scripted HTTP responses (status, headers, body, stalls, per-hit sequences) fed to
 *   the real `downloadToBuffer` / `downloadToFileWithRetry`; outcome + requested URLs;
 * - `catalog`: remote catalog bodies fed to the real `fetchCatalog` over the real bundled catalog;
 * - `installer`: folder seeds + steps (install / cleanupStale / uninstall / pausedIds /
 *   listInstalled) on the real `PackInstaller` (concurrency 1, no throttle); outcome, state
 *   events (runs of `downloading` with one fileIndex collapsed to the last), URLs and the folder
 *   tree after each step.
 *
 * The Rust test replays the same specs on a local HTTP server and real folders.
 */
import { createHash } from 'node:crypto';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { it } from 'vitest';
import catalogJson from '../../../packs/catalog.json';
import { validateCatalog, validateManifest } from '@/shared/packValidate';
import { downloadToBuffer, downloadToFileWithRetry, type FetchLike } from '@/main/packs/download';
import { fetchCatalog } from '@/main/packs/catalog';
import { PackInstaller } from '@/main/packs/installer';
import { manifest, rawTestUrl } from '@/main/packs/testUtils';
import type { PackInstallState, PackManifest } from '@/types/packs';
import { writeFixture } from './fixture';

if (typeof globalThis.localStorage?.clear !== 'function') {
  Object.defineProperty(globalThis, 'localStorage', { value: { clear() {} }, configurable: true, writable: true });
}

interface RouteSpec {
  status?: number;
  headers?: [string, string][];
  body?: string;
  /** 'auto': content-length = body length (default); 'none': no header. */
  cl?: 'auto' | 'none';
  /** Send the body, then never end (until the request is aborted). */
  stall?: boolean;
  /** Per-hit responses (the last one repeats). */
  seq?: RouteSpec[];
}

type Routes = Record<string, RouteSpec>;

function makeFetch(routes: Routes, calls: string[]): FetchLike {
  const hits: Record<string, number> = {};
  return async (url, init) => {
    calls.push(url);
    if (init.signal?.aborted) throw new DOMException('aborted', 'AbortError');
    let spec = routes[url];
    if (!spec) return new Response('nope', { status: 404, headers: { 'content-length': '4' } });
    if (spec.seq) {
      const i = (hits[url] = (hits[url] ?? -1) + 1);
      spec = spec.seq[Math.min(i, spec.seq.length - 1)];
    }
    const status = spec.status ?? 200;
    const body = spec.body ?? '';
    const headers = new Headers(spec.headers ?? []);
    if ((spec.cl ?? 'auto') === 'auto') headers.set('content-length', String(Buffer.byteLength(body)));
    if (spec.stall) {
      const stream = new ReadableStream<Uint8Array>({
        start(c) {
          if (body) c.enqueue(new TextEncoder().encode(body));
          init.signal?.addEventListener('abort', () => c.error(new DOMException('aborted', 'AbortError')));
        },
      });
      return new Response(stream, { status, headers });
    }
    return new Response([204, 205, 304].includes(status) ? null : body, { status, headers });
  };
}

const sha = (s: string) => createHash('sha256').update(s).digest('hex');

function errorOutcome(e: unknown) {
  const err = e as Error & { retryable?: boolean };
  return { error: err.message, retryable: err.retryable ?? null, name: err.name };
}

// --- pack.json -----------------------------------------------------------------------------

function packJsonCases() {
  const manifests: unknown[] = [...(catalogJson as { packs: unknown[] }).packs];
  manifests.push(manifest());
  manifests.push(
    manifest({
      license: 'CC-BY-4.0',
      attribution: 'by "Someone" é\u0001',
      description: 'line 1\nline 2\ttab \\ /  ',
      kits: [
        {
          id: 'main',
          name: 'Main',
          pads: [{ pad: 0, file: 'kicks/bd.wav', name: 'Kick', settings: { gainDb: -3.5, pitch: 1.0, decay: 0.1, reverse: true, chokeGroup: null, mode: 'gate', tune: 1e21, x: 1e-7 } as never }],
        },
      ],
    }),
  );
  return manifests.map((m) => {
    const v = validateManifest(m);
    const installed = { id: v.id, version: v.version, dir: `/packs/${v.id}`, installedAt: '2026-11-18T12:34:56.789Z', manifest: v };
    return { manifest: m, text: JSON.stringify(installed, null, 2) };
  });
}

// --- downloads -----------------------------------------------------------------------------

interface DownloadCase {
  name: string;
  url: string;
  routes: Routes;
  mode: 'buffer' | 'file';
  maxBytes: number;
  expectedBytes?: number;
  sha256?: string;
  idleTimeoutMs?: number;
}

const BD = rawTestUrl('bd.wav');
const MOVED = rawTestUrl('moved/bd.wav');

function downloadCases(): DownloadCase[] {
  const c = (name: string, routes: Routes, extra: Partial<DownloadCase> = {}): DownloadCase => ({
    name,
    url: BD,
    routes,
    mode: 'buffer',
    maxBytes: 100,
    ...extra,
  });
  const file = { mode: 'file' as const };
  return [
    c('ok', { [BD]: { body: 'kick' } }, { expectedBytes: 4, sha256: sha('kick') }),
    c('ok, upper-case expected sha', { [BD]: { body: 'kick' } }, { sha256: sha('kick').toUpperCase() }),
    c('ok, no content-length', { [BD]: { body: 'kick', cl: 'none' } }),
    c('ok, empty body', { [BD]: { body: '' } }, { expectedBytes: 0 }),
    c('404', { [BD]: { status: 404, body: 'x' } }),
    c('403', { [BD]: { status: 403 } }, file),
    c('500 x3', { [BD]: { status: 500, body: 'x' } }, file),
    c('503, 503, ok', { [BD]: { seq: [{ status: 503 }, { status: 503 }, { body: 'kick' }] } }, file),
    c('429 then ok', { [BD]: { seq: [{ status: 429 }, { body: 'kick' }] } }, file),
    c('408 then ok', { [BD]: { seq: [{ status: 408 }, { body: 'kick' }] } }, file),
    c('302 allowed absolute', { [BD]: { status: 302, headers: [['location', MOVED]] }, [MOVED]: { body: 'kick' } }),
    c('301 allowed relative', { [BD]: { status: 301, headers: [['location', 'moved/bd.wav']] }, [MOVED]: { body: 'kick' } }),
    c('307 root-relative', { [BD]: { status: 307, headers: [['location', '/o/r/x.wav']] }, ['https://raw.githubusercontent.com/o/r/x.wav']: { body: 'k' } }),
    c('redirect to evil host', { [BD]: { status: 302, headers: [['location', 'https://evil.example/x']] } }, file),
    c('redirect to http', { [BD]: { status: 302, headers: [['location', 'http://raw.githubusercontent.com/x']] } }),
    c('redirect with port', { [BD]: { status: 302, headers: [['location', 'https://raw.githubusercontent.com:8443/x']] } }),
    c('redirect without location', { [BD]: { status: 303 } }, file),
    c('too many redirects', { [BD]: { status: 302, headers: [['location', BD]] } }, file),
    c('5 redirects is fine', {
      [BD]: { status: 302, headers: [['location', rawTestUrl('r1')]] },
      [rawTestUrl('r1')]: { status: 302, headers: [['location', rawTestUrl('r2')]] },
      [rawTestUrl('r2')]: { status: 302, headers: [['location', rawTestUrl('r3')]] },
      [rawTestUrl('r3')]: { status: 302, headers: [['location', rawTestUrl('r4')]] },
      [rawTestUrl('r4')]: { status: 302, headers: [['location', rawTestUrl('r5')]] },
      [rawTestUrl('r5')]: { body: 'end' },
    }),
    c('declared too large', { [BD]: { body: 'x', headers: [['content-length', '101']], cl: 'none' } }),
    c('streamed too large', { [BD]: { body: 'x'.repeat(101), cl: 'none' } }),
    c('exactly the cap', { [BD]: { body: 'x'.repeat(100) } }),
    c('size mismatch x3', { [BD]: { body: 'kick' } }, { ...file, expectedBytes: 5 }),
    c('checksum mismatch x3', { [BD]: { body: 'kick' } }, { ...file, sha256: sha('kicK') }),
    c('checksum then ok', { [BD]: { seq: [{ body: 'kicK' }, { body: 'kick' }] } }, { ...file, sha256: sha('kick') }),
    c('stall, idle timeout', { [BD]: { body: 'ki', stall: true, cl: 'none' } }, { idleTimeoutMs: 60 }),
    c('stall x3 then file fails', { [BD]: { body: 'ki', stall: true, cl: 'none' } }, { ...file, idleTimeoutMs: 60 }),
    c('stall then ok', { [BD]: { seq: [{ body: 'ki', stall: true, cl: 'none' }, { body: 'kick' }] } }, { ...file, idleTimeoutMs: 60, sha256: sha('kick') }),
    c('initial URL not allowed (http)', {}, { url: 'http://raw.githubusercontent.com/o/r/x.wav' }),
    c('initial URL not allowed (host)', {}, { url: 'https://evil.example/x.wav' }),
    c('initial URL not allowed (credentials)', {}, { url: 'https://a:b@raw.githubusercontent.com/x' }),
    c('initial URL not allowed (garbage)', {}, { url: 'not a url' }),
  ];
}

async function runDownload(dc: DownloadCase, dir: string) {
  const calls: string[] = [];
  const fetchImpl = makeFetch(dc.routes, calls);
  const opts = { maxBytes: dc.maxBytes, expectedBytes: dc.expectedBytes, sha256: dc.sha256, idleTimeoutMs: dc.idleTimeoutMs, fetchImpl };
  let outcome: unknown;
  try {
    if (dc.mode === 'buffer') {
      const bytes = await downloadToBuffer(dc.url, opts);
      outcome = { ok: { bytes: bytes.byteLength, sha256: createHash('sha256').update(bytes).digest('hex') } };
    } else {
      const dest = path.join(dir, `f-${Math.random().toString(16).slice(2)}.bin`);
      const r = await downloadToFileWithRetry(dc.url, dest, opts, { attempts: 3, baseDelayMs: 1 });
      outcome = { ok: { bytes: r.bytes, sha256: r.sha256, file: await fs.readFile(dest, 'latin1') } };
    }
  } catch (e) {
    outcome = errorOutcome(e);
  }
  return { ...dc, outcome, calls };
}

// --- catalog -------------------------------------------------------------------------------

function catalogCases() {
  const real = catalogJson as { version: number; updatedAt: string; packs: Record<string, unknown>[] };
  const clone = <T>(v: T): T => JSON.parse(JSON.stringify(v)) as T;
  const bump = (p: Record<string, unknown>, by: number) => ({ ...clone(p), version: (p.version as number) + by });
  const newer = '2099-01-01';
  return [
    { name: 'offline (404)', body: null },
    { name: 'invalid JSON', body: '{"version":1,' },
    { name: 'envelope invalid', body: JSON.stringify({ version: 2, updatedAt: newer, packs: [] }) },
    { name: 'not newer', body: JSON.stringify({ ...clone(real), packs: real.packs.map((p) => bump(p, 1)) }) },
    { name: 'unparseable date', body: JSON.stringify({ ...clone(real), updatedAt: 'soon', packs: real.packs.map((p) => bump(p, 1)) }) },
    { name: 'newer, bumped versions', body: JSON.stringify({ ...clone(real), updatedAt: newer, packs: real.packs.map((p) => bump(p, 1)) }) },
    { name: 'newer, older versions keep bundled', body: JSON.stringify({ ...clone(real), updatedAt: newer, packs: real.packs.map((p) => bump(p, -1)) }) },
    { name: 'newer but empty', body: JSON.stringify({ version: 1, updatedAt: newer, packs: [] }) },
    { name: 'newer, one withdrawn cleanly', body: JSON.stringify({ version: 1, updatedAt: newer, packs: real.packs.slice(1) }) },
    { name: 'newer, one invalid keeps bundled', body: JSON.stringify({ version: 1, updatedAt: newer, packs: [{ ...clone(real.packs[0]), license: 'GPL' }, ...real.packs.slice(1)] }) },
    { name: 'newer, a new pack', body: JSON.stringify({ version: 1, updatedAt: newer, packs: [...real.packs, manifest()] }) },
    { name: 'too big', body: 'x'.repeat(5 * 1024 * 1024 + 1) },
  ];
}

const CATALOG_URL = 'https://raw.githubusercontent.com/yemirhan/micro-daw/main/packs/catalog.json';

async function runCatalog(cc: { name: string; body: string | null }) {
  const calls: string[] = [];
  const routes: Routes = cc.body === null ? {} : { [CATALOG_URL]: { body: cc.body } };
  const r = await fetchCatalog({ fetchImpl: makeFetch(routes, calls) });
  return {
    name: cc.name,
    // Huge bodies are stored as a repeat marker (the Rust test rebuilds them).
    body: cc.body !== null && cc.body.length > 100_000 ? { repeat: cc.body[0], count: cc.body.length } : cc.body,
    source: r.source,
    updatedAt: r.catalog.updatedAt,
    packs: r.catalog.packs.map((p) => `${p.id}@${p.version}`),
  };
}

// --- installer -----------------------------------------------------------------------------

interface Seed {
  path: string;
  /** File content (utf8); absent: a folder. */
  content?: string;
  /** mtime = now - ageMs. */
  ageMs?: number;
}

type Step =
  | { op: 'install'; manifest: unknown; routes: Routes; concurrency?: number }
  | { op: 'cleanup' }
  | { op: 'uninstall'; id: string }
  | { op: 'paused' }
  | { op: 'list' };

interface InstallerCase {
  name: string;
  seeds: Seed[];
  steps: Step[];
}

const SD = rawTestUrl('sd.wav');
const DAY = 24 * 60 * 60 * 1000;

function mfor(kick: string, snare: string, version = 1, over: Partial<PackManifest> = {}): PackManifest {
  const m = manifest({ version, ...over });
  m.files[0] = { ...m.files[0], size: Buffer.byteLength(kick), sha256: sha(kick) };
  m.files[1] = { ...m.files[1], size: Buffer.byteLength(snare), sha256: sha(snare) };
  return m;
}

/** A pack.json body (what an installer of either app wrote). */
function meta(m: PackManifest, installedAt: string, dir = '/somewhere/else'): string {
  const v = validateManifest(m);
  return JSON.stringify({ id: v.id, version: v.version, dir, installedAt, manifest: v }, null, 2);
}

const ok = (kick = 'kick', snare = 'snare'): Routes => ({ [BD]: { body: kick }, [SD]: { body: snare } });

function installerCases(): InstallerCase[] {
  const m1 = mfor('kick', 'snare', 1);
  const m2 = mfor('kick2', 'snare2', 2);
  const install = (manifest: unknown, routes: Routes, concurrency?: number): Step => ({ op: 'install', manifest, routes, concurrency });
  const packSeeds = (dir: string, m: PackManifest, at: string, kick = 'kick', snare = 'snare'): Seed[] => [
    { path: dir },
    { path: `${dir}/kicks/bd.wav`, content: kick },
    { path: `${dir}/sd.wav`, content: snare },
    { path: `${dir}/pack.json`, content: meta(m, at) },
  ];
  return [
    { name: 'fresh install', seeds: [], steps: [install(m1, ok()), { op: 'list' }] },
    { name: 'resume after 404', seeds: [], steps: [install(m1, { [BD]: { body: 'kick' }, [SD]: { status: 404 } }), { op: 'paused' }, install(m1, ok()), { op: 'paused' }] },
    {
      name: 'tampered staging, junk and stale pack.json',
      seeds: [
        { path: '.partial-test-pack/kicks/bd.wav', content: 'kicK' },
        { path: '.partial-test-pack/extra.wav', content: 'junk' },
        { path: '.partial-test-pack/deep/er/x.txt', content: 'junk' },
        { path: '.partial-test-pack/pack.json', content: '{}' },
        { path: '.partial-test-pack/sd.wav', content: 'snare' },
      ],
      steps: [install(m1, ok())],
    },
    { name: 'checksum mismatch retried', seeds: [], steps: [install(mfor('kick', 'snarE'), ok())] },
    { name: '503 twice then ok', seeds: [], steps: [install(m1, { [BD]: { body: 'kick' }, [SD]: { seq: [{ status: 503 }, { status: 503 }, { body: 'snare' }] } })] },
    { name: 'oversized declared', seeds: [], steps: [install(m1, { [BD]: { body: 'kick', headers: [['content-length', '999']], cl: 'none' }, [SD]: { body: 'snare' } })] },
    { name: 'redirect allowed', seeds: [], steps: [install(m1, { [BD]: { status: 302, headers: [['location', MOVED]] }, [MOVED]: { body: 'kick' }, [SD]: { body: 'snare' } })] },
    { name: 'redirect evil', seeds: [], steps: [install(m1, { [BD]: { status: 302, headers: [['location', 'https://evil.example/bd']] }, [SD]: { body: 'snare' } })] },
    { name: 'update then failed update', seeds: [], steps: [install(m1, ok()), install(m2, ok('kick2', 'snare2')), { op: 'list' }, install(mfor('kick3', 'snare3', 3), { [BD]: { body: 'kick3' } }), { op: 'list' }, { op: 'paused' }] },
    { name: 'invalid manifest', seeds: [], steps: [install({ ...m1, files: [{ path: '../escape.wav', url: BD, size: 1, sha256: sha('x') }], kits: [] }, ok()), install({ id: '../x' }, ok())] },
    { name: 'concurrency 4', seeds: [], steps: [install(m1, ok(), 4)] },
    {
      name: 'S5 restore after an interrupted swap',
      seeds: [
        ...packSeeds('.old-test-pack-0badf00d', m1, '2026-11-18T00:00:00.000Z'),
        { path: '.old-test-pack-12345678' },
        { path: '.del-other-0000aaaa/x.wav', content: 'x' },
        { path: '.tmp-test-pack-abc123' },
      ],
      steps: [{ op: 'cleanup' }, { op: 'list' }],
    },
    {
      name: 'newest backup wins',
      seeds: [
        ...packSeeds('.old-test-pack-00000001', m1, '2026-11-18T00:00:00.000Z'),
        ...packSeeds('.old-test-pack-00000002', m2, '2026-11-19T00:00:00.000Z', 'kick2', 'snare2'),
        ...packSeeds('.old-test-pack-00000003', m1, 'not a date'),
      ],
      steps: [{ op: 'cleanup' }, { op: 'list' }],
    },
    {
      name: 'M1 present but invalid folder replaced',
      seeds: [...packSeeds('.old-test-pack-0123abcd', m1, '2026-11-18T00:00:00.000Z'), { path: 'test-pack/half.wav', content: 'x' }],
      steps: [{ op: 'cleanup' }, { op: 'list' }],
    },
    {
      name: 'backup of a valid install is dropped, staging expiry',
      seeds: [
        ...packSeeds('test-pack', m2, '2026-11-19T00:00:00.000Z', 'kick2', 'snare2'),
        ...packSeeds('.old-test-pack-0badf00d', m1, '2026-11-18T00:00:00.000Z'),
        { path: '.partial-old-pack/a.wav', content: 'a', ageMs: 8 * DAY },
        { path: '.partial-new-pack/a.wav', content: 'a', ageMs: 6 * DAY },
        { path: '.old-x-1' },
        { path: '.old-wrong-id-0badf00d/pack.json', content: meta(m1, '2026-11-18T00:00:00.000Z') },
        { path: 'junk/pack.json', content: '{"id":"junk"}' },
        { path: 'Bad_Name/pack.json', content: meta(m1, '2026-11-18T00:00:00.000Z') },
        { path: 'loose-file.wav', content: 'x' },
      ],
      steps: [{ op: 'cleanup' }, { op: 'list' }, { op: 'paused' }],
    },
    {
      name: 'uninstall',
      seeds: [...packSeeds('test-pack', m1, '2026-11-18T00:00:00.000Z'), { path: '.partial-test-pack/a.wav', content: 'a' }],
      steps: [{ op: 'uninstall', id: 'test-pack' }, { op: 'uninstall', id: 'test-pack' }, { op: 'uninstall', id: '../etc' }, { op: 'list' }],
    },
    {
      name: 'pausedIds layouts',
      seeds: [
        { path: '.partial-empty-pack/kicks' },
        { path: '.partial-Bad_ID/x.wav', content: 'x' },
        { path: '.partial-nested-pack/a/b/c.wav', content: 'c' },
        { path: '.partial-flat/x.wav', content: 'x' },
        { path: '.partialnot/x.wav', content: 'x' },
      ],
      steps: [{ op: 'paused' }],
    },
    {
      name: 'listInstalled order and validity',
      seeds: [
        ...['zeta', 'Alpha', 'beta', 'Äpfel', 'alpha 2', 'Beta'].flatMap((name, i) => {
          const id = `p${i}`;
          const m = mfor('kick', 'snare', 1, { id, name });
          return [{ path: `${id}/pack.json`, content: meta(m, '2026-11-18T00:00:00.000Z') }];
        }),
        { path: 'mismatch/pack.json', content: meta(mfor('kick', 'snare', 1, { id: 'other-id' }), 'x') },
        { path: 'bad-json/pack.json', content: '{nope' },
        { path: 'no-meta/a.wav', content: 'a' },
        { path: 'old-version/pack.json', content: JSON.stringify({ id: 'old-version', manifest: validateManifest(mfor('kick', 'snare', 7, { id: 'old-version', name: 'Old' })) }) },
      ],
      steps: [{ op: 'list' }],
    },
  ];
}

async function seed(root: string, seeds: Seed[]) {
  const now = Date.now();
  for (const s of seeds) {
    const full = path.join(root, s.path);
    if (s.content === undefined) await fs.mkdir(full, { recursive: true });
    else {
      await fs.mkdir(path.dirname(full), { recursive: true });
      await fs.writeFile(full, s.content);
    }
  }
  for (const s of seeds) {
    if (s.ageMs === undefined) continue;
    const t = new Date(now - s.ageMs);
    const top = path.join(root, s.path.split('/')[0]);
    await fs.utimes(path.join(root, s.path), t, t);
    await fs.utimes(top, t, t);
  }
}

/** Sorted tree; pack.json parsed with dir / installedAt replaced; random hex suffixes masked. */
async function tree(root: string, rel = ''): Promise<unknown[]> {
  const out: unknown[] = [];
  const names = (await fs.readdir(path.join(root, rel)).catch(() => [] as string[])).sort();
  for (const n of names) {
    const r = rel ? `${rel}/${n}` : n;
    const masked = r.replace(/^\.(old|del)-(.+)-[0-9a-f]{8}/, '.$1-$2-XXXXXXXX');
    const st = await fs.lstat(path.join(root, r));
    if (st.isDirectory()) {
      out.push({ dir: masked });
      out.push(...(await tree(root, r)));
    } else {
      const text = await fs.readFile(path.join(root, r), 'utf-8');
      let content: unknown = text;
      if (n === 'pack.json') {
        try {
          const j = JSON.parse(text) as Record<string, unknown>;
          if ('dir' in j) j.dir = '<DIR>';
          if ('installedAt' in j) j.installedAt = maskAt(j.installedAt);
          content = j;
        } catch {
          content = text;
        }
      }
      out.push({ file: masked, content });
    }
  }
  return out;
}

/** installedAt values the seeds use; anything else was written now by the installer. */
const SEEDED_AT = new Set(['2026-11-18T00:00:00.000Z', '2026-11-19T00:00:00.000Z', 'x', 'not a date', '1970-01-01T00:00:00.000Z']);
const maskAt = (v: unknown) => (typeof v === 'string' && SEEDED_AT.has(v) ? v : '<NOW>');

function collapse(states: PackInstallState[]): PackInstallState[] {
  const out: PackInstallState[] = [];
  for (const s of states) {
    const prev = out.at(-1);
    if (s.state === 'downloading' && prev?.state === 'downloading' && prev.fileIndex === s.fileIndex) out[out.length - 1] = s;
    else out.push(s);
  }
  return out;
}

async function runInstaller(ic: InstallerCase) {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'mdaw-golden-'));
  try {
    await seed(root, ic.seeds);
    const results: unknown[] = [];
    for (const step of ic.steps) {
      const states: PackInstallState[] = [];
      const calls: string[] = [];
      const inst = new PackInstaller({
        packsDir: root,
        fetchImpl: makeFetch(step.op === 'install' ? step.routes : {}, calls),
        onState: (_id, s) => states.push(s),
        progressIntervalMs: 0,
        concurrency: step.op === 'install' ? (step.concurrency ?? 1) : 1,
        retry: { baseDelayMs: 1 },
      });
      let outcome: unknown;
      try {
        if (step.op === 'install') {
          const p = await inst.install(step.manifest as PackManifest);
          outcome = { ok: { id: p.id, version: p.version, dir: path.relative(root, p.dir) } };
        } else if (step.op === 'cleanup') outcome = { ok: await inst.cleanupStale() };
        else if (step.op === 'uninstall') outcome = { ok: await inst.uninstall(step.id) };
        else if (step.op === 'paused') outcome = { ok: await inst.pausedIds() };
        else outcome = { ok: (await inst.listInstalled()).map((p) => ({ id: p.id, version: p.version, name: p.manifest.name, installedAt: maskAt(p.installedAt), dir: path.relative(root, p.dir) })) };
      } catch (e) {
        outcome = { error: (e as Error).message };
      }
      if (outcome && typeof outcome === 'object' && 'ok' in outcome && (outcome as { ok: unknown }).ok === undefined) outcome = { ok: null };
      results.push({ outcome, states: collapse(states), calls, tree: await tree(root) });
    }
    return { ...ic, results };
  } finally {
    await fs.rm(root, { recursive: true, force: true });
  }
}

it('packs_io golden', async () => {
  const tmp = await fs.mkdtemp(path.join(os.tmpdir(), 'mdaw-golden-dl-'));
  try {
    const download = [];
    for (const dc of downloadCases()) download.push(await runDownload(dc, tmp));
    const catalog = [];
    for (const cc of catalogCases()) catalog.push(await runCatalog(cc));
    const installer = [];
    for (const ic of installerCases()) installer.push(await runInstaller(ic));
    // Sanity: the bundled catalog validates.
    if (validateCatalog(catalogJson).errors.length) throw new Error('bundled catalog has errors');
    writeFixture('packs_io', { pack_json: packJsonCases(), download, catalog, installer });
  } finally {
    await fs.rm(tmp, { recursive: true, force: true });
  }
}, 60_000);
