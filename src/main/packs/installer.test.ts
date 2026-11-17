// @vitest-environment node
import { createHash } from 'node:crypto';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PackInstallState } from '../../types/packs';
import { InstallPausedError, PackInstaller, PARTIAL_MAX_AGE_MS, resolveInside } from './installer';
import { downloadToBuffer, downloadToFileWithRetry, DownloadError } from './download';
import { PreviewFetcher } from './preview';
import { fetchCatalog } from './catalog';
import { BD_URL, fakeFetch, hangingResponse, manifest, rawTestUrl, sha256Of } from './testUtils';

// Node environment (real fs + streams); the shared setup clears localStorage after each test.
if (typeof globalThis.localStorage?.clear !== 'function') {
  Object.defineProperty(globalThis, 'localStorage', { value: { clear() {} }, configurable: true, writable: true });
}

const BD = BD_URL;
const SD = rawTestUrl('sd.wav');
const STAGE = '.partial-test-pack';

let root: string;
let states: { id: string; state: PackInstallState }[];

function installer(fetchImpl: ReturnType<typeof fakeFetch>) {
  return new PackInstaller({
    packsDir: root,
    fetchImpl,
    onState: (id, state) => states.push({ id, state }),
    progressIntervalMs: 0,
    retry: { baseDelayMs: 1 },
  });
}

/** Manifest whose files match the bodies `kick` / `snare` given. */
function manifestFor(kick: string, snare: string, version = 1) {
  const m = manifest({ version });
  m.files[0] = { ...m.files[0], size: Buffer.byteLength(kick), sha256: sha256Of(kick) };
  m.files[1] = { ...m.files[1], size: Buffer.byteLength(snare), sha256: sha256Of(snare) };
  return m;
}

async function listDir(): Promise<string[]> {
  return (await fs.readdir(root).catch(() => [] as string[])).sort();
}

beforeEach(async () => {
  root = await fs.mkdtemp(path.join(os.tmpdir(), 'mdaw-packs-'));
  states = [];
});

afterEach(async () => {
  await fs.rm(root, { recursive: true, force: true });
});

describe('PackInstaller', () => {
  it('installs files + pack.json atomically and lists the pack', async () => {
    const inst = installer(fakeFetch({ [BD]: 'kick', [SD]: 'snare' }));
    const pack = await inst.install(manifest());
    expect(pack.dir).toBe(path.join(root, 'test-pack'));
    expect(await fs.readFile(path.join(root, 'test-pack/kicks/bd.wav'), 'utf-8')).toBe('kick');
    expect(await fs.readFile(path.join(root, 'test-pack/sd.wav'), 'utf-8')).toBe('snare');
    expect(await listDir()).toEqual(['test-pack']); // no temp folder left
    const listed = await inst.listInstalled();
    expect(listed.map((p) => p.id)).toEqual(['test-pack']);
    expect(listed[0].dir).toBe(path.join(root, 'test-pack'));
    expect(states.at(-1)).toEqual({ id: 'test-pack', state: { state: 'installed' } });
    const dl = states.filter((s) => s.state.state === 'downloading').at(-1)?.state;
    expect(dl).toMatchObject({ fileIndex: 2, fileCount: 2, receivedBytes: 9 });
  });

  it('follows redirects to allowed hosts only', async () => {
    const allowedTarget = rawTestUrl('moved/bd.wav');
    const ok = installer(
      fakeFetch({
        [BD]: () => new Response(null, { status: 302, headers: { location: allowedTarget } }),
        [allowedTarget]: 'kick',
        [SD]: 'snare',
      }),
    );
    await ok.install(manifest());
    expect(await fs.readFile(path.join(root, 'test-pack/kicks/bd.wav'), 'utf-8')).toBe('kick');
    await ok.uninstall('test-pack');

    const f = fakeFetch({
      [BD]: () => new Response(null, { status: 301, headers: { location: 'https://evil.example/bd' } }),
      [SD]: 'snare',
    });
    await expect(installer(f).install(manifest())).rejects.toThrow(/Host not allowed/);
    expect(f.calls).not.toContain('https://evil.example/bd');
    expect((await listDir()).filter((n) => n !== STAGE)).toEqual([]);
    expect(states.at(-1)?.state).toMatchObject({ state: 'error' });

    // github.com (and its object/codeload hosts) are no longer allowed redirect targets (S2).
    const gh = fakeFetch({
      [BD]: () => new Response(null, { status: 302, headers: { location: 'https://objects.githubusercontent.com/blob/bd' } }),
      [SD]: 'snare',
    });
    await expect(installer(gh).install(manifest())).rejects.toThrow(/Host not allowed/);
  });

  it('rejects oversized files, size and checksum mismatches; nothing unverified is kept', async () => {
    const big = installer(
      fakeFetch({ [BD]: () => new Response('x', { headers: { 'content-length': String(60 * 1024 * 1024) } }), [SD]: 'snare' }),
    );
    await expect(big.install(manifest())).rejects.toThrow(/too large/);
    expect((await listDir()).includes('test-pack')).toBe(false);

    const sha = createHash('sha256').update('kick').digest('hex');
    const m = manifest();
    m.files[1] = { ...m.files[1], sha256: sha }; // wrong for "snare"
    const f = fakeFetch({ [BD]: 'kick', [SD]: 'snare' });
    await expect(installer(f).install(m)).rejects.toThrow(/Checksum/);
    expect(f.calls.filter((u) => u === SD)).toHaveLength(3); // retried, then failed
    expect((await listDir()).includes('test-pack')).toBe(false);
    // Resume data: the verified kick is kept, the corrupt snare is not.
    expect(await fs.readFile(path.join(root, STAGE, 'kicks/bd.wav'), 'utf-8')).toBe('kick');
    await expect(fs.stat(path.join(root, STAGE, 'sd.wav'))).rejects.toThrow();

    // S2: the exact byte count is verified even without a content-length header.
    const short = manifestFor('kick', 'snare');
    short.files[1] = { ...short.files[1], size: 6, sha256: sha256Of('snare') };
    await expect(installer(fakeFetch({ [BD]: 'kick', [SD]: () => new Response('snare') })).install(short)).rejects.toThrow(/Size mismatch/);
  });

  it('S7: retries transient failures with backoff, but not 4xx', async () => {
    let sdHits = 0;
    const flaky = fakeFetch({
      [BD]: 'kick',
      [SD]: () => (++sdHits < 3 ? new Response('busy', { status: 503 }) : new Response('snare')),
    });
    await installer(flaky).install(manifest());
    expect(sdHits).toBe(3);
    expect(await fs.readFile(path.join(root, 'test-pack/sd.wav'), 'utf-8')).toBe('snare');

    const missing = fakeFetch({ [BD]: 'kick' });
    await expect(installer(missing).install(manifest({ version: 2 }))).rejects.toThrow(/404/);
    expect(missing.calls.filter((u) => u === SD)).toHaveLength(1);
  });

  it('S7: Retry resumes, keeping verified files of the failed attempt', async () => {
    const first = fakeFetch({ [BD]: 'kick', [SD]: () => new Response('gone', { status: 404 }) });
    await expect(installer(first).install(manifest())).rejects.toThrow(/404/);
    expect(await listDir()).toEqual([STAGE]);

    const second = fakeFetch({ [BD]: 'kick', [SD]: 'snare' });
    const pack = await installer(second).install(manifest());
    expect(second.calls).toEqual([SD]); // kick was not downloaded again
    expect(await fs.readFile(path.join(pack.dir, 'kicks/bd.wav'), 'utf-8')).toBe('kick');
    expect(await listDir()).toEqual(['test-pack']);
    const dl = states.filter((s) => s.state.state === 'downloading').at(-1)?.state;
    expect(dl).toMatchObject({ fileIndex: 2, fileCount: 2, receivedBytes: 9 });
  });

  it('S7: a stale or tampered staging file is re-downloaded, unknown files are pruned', async () => {
    await fs.mkdir(path.join(root, STAGE, 'kicks'), { recursive: true });
    await fs.writeFile(path.join(root, STAGE, 'kicks/bd.wav'), 'kicK'); // same size, wrong hash
    await fs.writeFile(path.join(root, STAGE, 'extra.wav'), 'junk');
    await fs.writeFile(path.join(root, STAGE, 'pack.json'), '{}');
    const f = fakeFetch({ [BD]: 'kick', [SD]: 'snare' });
    const pack = await installer(f).install(manifest());
    expect(f.calls.sort()).toEqual([BD, SD].sort());
    expect((await fs.readdir(pack.dir)).sort()).toEqual(['kicks', 'pack.json', 'sd.wav']);
    expect(await fs.readFile(path.join(pack.dir, 'kicks/bd.wav'), 'utf-8')).toBe('kick');
  });

  it('B2: an immediate Retry is not corrupted by the failed attempt\'s still-settling workers', async () => {
    // Sol's scenario: 2 workers; bd.wav 404s, sd.wav is mid-stream. The abort
    // reaches the sd worker, but its file handle takes a while to close, so its
    // cleanup (rm of the partial sd.wav) runs late. The first install must not
    // settle (and release the lock) before that cleanup ran, or it deletes the
    // file the Retry just wrote and the Retry reports a pack missing sd.wav.
    const realOpen = fs.open.bind(fs);
    let delayed = false;
    const openSpy = vi.spyOn(fs, 'open').mockImplementation(async (...args: Parameters<typeof fs.open>) => {
      const fh = await realOpen(...args);
      if (String(args[0]).endsWith('sd.wav') && !delayed) {
        delayed = true;
        const close = fh.close.bind(fh);
        fh.close = async () => {
          await new Promise((r) => setTimeout(r, 150));
          return close();
        };
      }
      return fh;
    });
    try {
      let attempt = 1;
      const f: ReturnType<typeof fakeFetch> = fakeFetch({
        [BD]: async () => {
          if (attempt === 1) {
            await new Promise((r) => setTimeout(r, 30));
            return new Response('gone', { status: 404 });
          }
          return new Response('kick');
        },
        [SD]: (_u, init) => {
          if (attempt === 1) return hangingResponse(init);
          return new Response(
            new ReadableStream({
              start(c) {
                c.enqueue(new TextEncoder().encode('sn'));
                setTimeout(() => {
                  c.enqueue(new TextEncoder().encode('are'));
                  c.close();
                }, 250);
              },
            }),
          );
        },
      });
      const inst = new PackInstaller({ packsDir: root, fetchImpl: f, concurrency: 2, retry: { attempts: 1 }, progressIntervalMs: 0 });
      await expect(inst.install(manifest())).rejects.toThrow(/404/);
      attempt = 2;
      const pack = await inst.install(manifest());
      await new Promise((r) => setTimeout(r, 300)); // anything still settling from attempt 1
      expect(await fs.readFile(path.join(pack.dir, 'sd.wav'), 'utf-8')).toBe('snare');
      expect(await fs.readFile(path.join(pack.dir, 'kicks/bd.wav'), 'utf-8')).toBe('kick');
    } finally {
      openSpy.mockRestore();
    }
  });

  it('rejects manifests with unsafe paths before touching the disk', async () => {
    const m = manifest({ files: [{ path: '../escape.wav', url: BD, size: 1, sha256: sha256Of('x') }], kits: [] });
    const f = fakeFetch({ [BD]: 'x' });
    let p: Promise<unknown> | undefined;
    expect(() => {
      p = installer(f).install(m);
    }).not.toThrow(); // L-e: rejects, never throws synchronously
    await expect(p).rejects.toThrow(/Unsafe/);
    await expect(installer(f).install({ id: '../x' } as never)).rejects.toThrow();
    expect(f.calls).toEqual([]);
    expect(() => resolveInside(root, 'a/../../x.wav')).toThrow();
    expect(resolveInside(root, 'a/b.wav')).toBe(path.join(root, 'a', 'b.wav'));
  });

  it('cancels a running install and removes its temp folder', async () => {
    const inst = installer(fakeFetch({ [BD]: (_u, init) => hangingResponse(init), [SD]: (_u, init) => hangingResponse(init) }));
    const p = inst.install(manifest());
    await new Promise((r) => setTimeout(r, 20));
    expect(await listDir()).toContain(STAGE);
    expect(inst.cancel('test-pack')).toBe(true);
    await expect(p).rejects.toThrow(/cancelled/);
    expect(await listDir()).toEqual([]);
    expect(states.at(-1)?.state).toEqual({ state: 'idle' });
    expect(inst.cancel('test-pack')).toBe(false);
  });

  it('re-install replaces the old version; a failed update keeps it', async () => {
    const inst = installer(fakeFetch({ [BD]: 'kick', [SD]: 'snare' }));
    await inst.install(manifest());
    const v2 = installer(fakeFetch({ [BD]: 'kick2', [SD]: 'snare2' }));
    await v2.install(manifestFor('kick2', 'snare2', 2));
    expect(await fs.readFile(path.join(root, 'test-pack/sd.wav'), 'utf-8')).toBe('snare2');
    expect((await v2.listInstalled())[0].version).toBe(2);

    await expect(installer(fakeFetch({ [BD]: 'kick3' })).install(manifestFor('kick3', 'snare3', 3))).rejects.toThrow(/404/);
    expect(await fs.readFile(path.join(root, 'test-pack/sd.wav'), 'utf-8')).toBe('snare2');
    expect(await listDir()).toEqual([STAGE, 'test-pack']);
  });

  it('S5: restores a pack whose update swap was interrupted (crash between the renames)', async () => {
    const inst = installer(fakeFetch({ [BD]: 'kick', [SD]: 'snare' }));
    await inst.install(manifest());
    // Crash after "rename <id> -> .old-<id>-<hex>", before "rename staging -> <id>".
    await fs.rename(path.join(root, 'test-pack'), path.join(root, '.old-test-pack-0badf00d'));
    await fs.mkdir(path.join(root, '.old-test-pack-12345678')); // older leftover without pack.json
    await fs.mkdir(path.join(root, '.del-other-0000aaaa')); // interrupted uninstall: never restored
    await fs.mkdir(path.join(root, '.tmp-test-pack-abc123')); // legacy temp folder
    const fresh = installer(fakeFetch({}));
    await fresh.cleanupStale();
    expect(await listDir()).toEqual(['test-pack']);
    expect((await fresh.listInstalled()).map((p) => p.id)).toEqual(['test-pack']);
    expect(await fs.readFile(path.join(root, 'test-pack/kicks/bd.wav'), 'utf-8')).toBe('kick');
  });

  it('S5: an .old- folder is dropped when the pack is present; stale staging folders expire', async () => {
    const inst = installer(fakeFetch({ [BD]: 'kick', [SD]: 'snare' }));
    await inst.install(manifest());
    await fs.mkdir(path.join(root, '.old-test-pack-0badf00d'));
    await fs.writeFile(path.join(root, '.old-test-pack-0badf00d', 'pack.json'), await fs.readFile(path.join(root, 'test-pack/pack.json')));
    await fs.mkdir(path.join(root, '.partial-old-pack'));
    await fs.mkdir(path.join(root, '.partial-new-pack'));
    const old = new Date(Date.now() - PARTIAL_MAX_AGE_MS - 60_000);
    await fs.utimes(path.join(root, '.partial-old-pack'), old, old);
    await inst.cleanupStale();
    expect(await listDir()).toEqual(['.partial-new-pack', 'test-pack']);
  });

  it('uninstalls, cleans stale temp folders and ignores invalid installs', async () => {
    const inst = installer(fakeFetch({ [BD]: 'kick', [SD]: 'snare' }));
    await inst.install(manifest());
    await fs.mkdir(path.join(root, '.tmp-test-pack-abc123'));
    await fs.mkdir(path.join(root, '.old-x-1'));
    await fs.mkdir(path.join(root, STAGE));
    await fs.mkdir(path.join(root, 'junk'));
    await fs.writeFile(path.join(root, 'junk', 'pack.json'), '{"id":"junk"}');
    await inst.cleanupStale();
    expect(await listDir()).toEqual([STAGE, 'junk', 'test-pack']); // recent staging data is kept for resume
    expect((await inst.listInstalled()).map((p) => p.id)).toEqual(['test-pack']);
    await inst.uninstall('test-pack');
    expect(await listDir()).toEqual(['junk']);
    await expect(inst.uninstall('../etc')).rejects.toThrow(/Invalid pack id/);
  });
});

describe('downloads, previews and catalog', () => {
  it('caps in-memory downloads while streaming', async () => {
    const stream = new ReadableStream<Uint8Array>({
      start(c) {
        c.enqueue(new Uint8Array(6));
        c.enqueue(new Uint8Array(6));
        c.close();
      },
    });
    const f = fakeFetch({ [BD]: () => new Response(stream) });
    await expect(downloadToBuffer(BD, { maxBytes: 10, fetchImpl: f })).rejects.toThrow(/too large/);
  });

  it('times out a stalled response', async () => {
    const f = fakeFetch({ [BD]: (_u, init) => hangingResponse(init) });
    await expect(downloadToBuffer(BD, { maxBytes: 100, fetchImpl: f, idleTimeoutMs: 30 })).rejects.toThrow(/timed out/);
  });

  it('S6: an AbortSignal.timeout() is reported as a timeout, a real abort as a cancel', async () => {
    const f = fakeFetch({ [BD]: (_u, init) => hangingResponse(init) });
    const err = await downloadToBuffer(BD, { maxBytes: 100, fetchImpl: f, signal: AbortSignal.timeout(30) }).catch((e: unknown) => e);
    expect(err).toBeInstanceOf(DownloadError);
    expect((err as Error).message).toMatch(/timed out/);
    expect((err as Error).message).not.toMatch(/cancel/i);

    const ctrl = new AbortController();
    const p = downloadToBuffer(BD, { maxBytes: 100, fetchImpl: f, signal: ctrl.signal });
    setTimeout(() => ctrl.abort(), 10);
    await expect(p).rejects.toThrow(/cancelled/);
  });

  it('S6: a stalled body that ignores the abort signal still times out (no hang, no truncated file)', async () => {
    // Sends 2 of 4 bytes, then stalls; the stream never observes the AbortSignal.
    const stalled = () =>
      new Response(new ReadableStream<Uint8Array>({ start: (c) => c.enqueue(new Uint8Array([1, 2])) }), { status: 200 });
    const f = fakeFetch({ [BD]: stalled });
    const err = await downloadToBuffer(BD, { maxBytes: 100, fetchImpl: f, idleTimeoutMs: 30 }).catch((e: unknown) => e);
    expect((err as Error).message).toMatch(/timed out/);
    // Same without a declared size: a cancelled reader must not look like a complete body.
    const err2 = await downloadToBuffer(BD, { maxBytes: 100, fetchImpl: f, idleTimeoutMs: 30 }).catch((e: unknown) => e);
    expect(err2).toBeInstanceOf(DownloadError);
    const ctrl = new AbortController();
    const p = downloadToBuffer(BD, { maxBytes: 100, fetchImpl: f, signal: ctrl.signal, idleTimeoutMs: 10_000 });
    setTimeout(() => ctrl.abort(), 10);
    await expect(p).rejects.toThrow(/cancelled/);
  });

  it('S7: downloadToFileWithRetry gives up after N attempts and stops on cancel', async () => {
    let hits = 0;
    const f = fakeFetch({ [BD]: () => (hits++, new Response('x', { status: 500 })) });
    const dest = path.join(root, 'a.wav');
    await expect(downloadToFileWithRetry(BD, dest, { maxBytes: 10, fetchImpl: f }, { attempts: 3, baseDelayMs: 1 })).rejects.toThrow(/500/);
    expect(hits).toBe(3);
    await expect(fs.stat(dest)).rejects.toThrow();
  });

  it('previews: allowlist, size + sha256 check, cache and LRU eviction', async () => {
    const f = fakeFetch({ [BD]: 'kick', [SD]: 'snare' });
    const pf = new PreviewFetcher({ fetchImpl: f, maxEntries: 1 });
    await expect(pf.fetch('https://evil.example/x.wav')).rejects.toThrow(/not allowed/);
    const a = await pf.fetch(BD, { size: 4, sha256: sha256Of('kick') });
    expect(new TextDecoder().decode(a)).toBe('kick');
    await pf.fetch(BD);
    expect(f.calls).toEqual([BD]);
    await pf.fetch(SD);
    expect(pf.keys()).toEqual([SD]);
    const bad = new PreviewFetcher({ fetchImpl: fakeFetch({ [BD]: 'kick' }) });
    await expect(bad.fetch(BD, { size: 4, sha256: sha256Of('nope') })).rejects.toThrow(/Checksum/);
  });

  it('falls back to the bundled catalog when the remote one fails, is invalid or not newer', async () => {
    const offline = await fetchCatalog({ fetchImpl: fakeFetch({}) });
    expect(offline.source).toBe('bundled');
    expect(offline.catalog.version).toBe(1);

    const url = 'https://raw.githubusercontent.com/x/y/main/catalog.json';
    const bad = await fetchCatalog({ url, fetchImpl: fakeFetch({ [url]: '{"version":9}' }) });
    expect(bad.source).toBe('bundled');

    const bundled = { version: 1 as const, updatedAt: '2026-11-17', packs: [manifest()] };
    const same = JSON.stringify({ version: 1, updatedAt: '2026-11-17', packs: [manifest({ version: 5 })] });
    expect((await fetchCatalog({ url, bundled, fetchImpl: fakeFetch({ [url]: same }) })).source).toBe('bundled');

    const newer = JSON.stringify({ version: 1, updatedAt: '2026-12-01', packs: [manifest({ version: 2 })] });
    const remote = await fetchCatalog({ url, bundled, fetchImpl: fakeFetch({ [url]: newer }) });
    expect(remote.source).toBe('remote');
    expect(remote.catalog.packs.map((p) => `${p.id}@${p.version}`)).toEqual(['test-pack@2']);
  });
});

describe('installer fault injection (L-f)', () => {
  const restoreSpies: (() => void)[] = [];
  afterEach(() => {
    for (const r of restoreSpies.splice(0)) r();
  });
  /** Makes `fs[name]` throw `code` for calls whose first argument matches `when`. */
  function inject<K extends 'rename' | 'open' | 'writeFile'>(name: K, when: (a: string, b?: unknown) => boolean, code = 'EIO') {
    const real = (fs[name] as (...a: unknown[]) => Promise<unknown>).bind(fs);
    const spy = vi.spyOn(fs, name).mockImplementation((async (...args: unknown[]) => {
      if (when(String(args[0]), args[1])) throw Object.assign(new Error(`injected ${code}`), { code });
      return real(...args);
    }) as never);
    restoreSpies.push(() => spy.mockRestore());
    return spy;
  }

  it('M1: a failed crash recovery keeps every backup for the next start', async () => {
    const inst = installer(fakeFetch({ [BD]: 'kick', [SD]: 'snare' }));
    await inst.install(manifest());
    await fs.rename(path.join(root, 'test-pack'), path.join(root, '.old-test-pack-0123abcd'));
    inject('rename', (a) => a.includes('.old-test-pack-'));
    await installer(fakeFetch({})).cleanupStale();
    expect(await listDir()).toEqual(['.old-test-pack-0123abcd']);
    for (const r of restoreSpies.splice(0)) r();
    // Next start: the restore works.
    const fresh = installer(fakeFetch({}));
    await fresh.cleanupStale();
    expect(await listDir()).toEqual(['test-pack']);
    expect((await fresh.listInstalled()).map((p) => p.id)).toEqual(['test-pack']);
  });

  it('M1: a present but invalid pack folder is replaced by the valid backup', async () => {
    const inst = installer(fakeFetch({ [BD]: 'kick', [SD]: 'snare' }));
    await inst.install(manifest());
    await fs.rename(path.join(root, 'test-pack'), path.join(root, '.old-test-pack-0123abcd'));
    await fs.mkdir(path.join(root, 'test-pack')); // half-written, no pack.json
    await inst.cleanupStale();
    expect(await listDir()).toEqual(['test-pack']);
    expect(await fs.readFile(path.join(root, 'test-pack/sd.wav'), 'utf-8')).toBe('snare');
  });

  it('a failed swap (rename staging -> pack) keeps the old version and the resumable staging folder', async () => {
    const inst = installer(fakeFetch({ [BD]: 'kick', [SD]: 'snare' }));
    await inst.install(manifest());
    inject('rename', (a, b) => a.endsWith(STAGE) && String(b).endsWith('test-pack'));
    const v2 = installer(fakeFetch({ [BD]: 'kick2', [SD]: 'snare2' }));
    await expect(v2.install(manifestFor('kick2', 'snare2', 2))).rejects.toThrow(/injected/);
    expect(await listDir()).toEqual([STAGE, 'test-pack']);
    expect((await v2.listInstalled())[0].version).toBe(1);
    expect(v2.getState('test-pack')).toMatchObject({ state: 'error' });
    for (const r of restoreSpies.splice(0)) r();
    const f = fakeFetch({ [BD]: 'kick2', [SD]: 'snare2' });
    await installer(f).install(manifestFor('kick2', 'snare2', 2));
    expect(f.calls).toEqual([]); // both files resumed from staging
    expect(await fs.readFile(path.join(root, 'test-pack/sd.wav'), 'utf-8')).toBe('snare2');
  });

  it('a disk error while writing a file fails the install, keeps verified files, Retry resumes', async () => {
    inject('open', (a) => a.endsWith('sd.wav'), 'ENOSPC');
    const first = fakeFetch({ [BD]: 'kick', [SD]: 'snare' });
    const one = new PackInstaller({ packsDir: root, fetchImpl: first, concurrency: 1, retry: { baseDelayMs: 1 } });
    await expect(one.install(manifest())).rejects.toThrow(/ENOSPC/);
    expect(await listDir()).toEqual([STAGE]);
    for (const r of restoreSpies.splice(0)) r();
    const second = fakeFetch({ [BD]: 'kick', [SD]: 'snare' });
    await installer(second).install(manifest());
    expect(second.calls).toEqual([SD]);
  });

  it('a failed pack.json write leaves no pack.json in staging and nothing installed', async () => {
    inject('writeFile', (a) => a.endsWith(`${STAGE}/pack.json`));
    const inst = installer(fakeFetch({ [BD]: 'kick', [SD]: 'snare' }));
    await expect(inst.install(manifest())).rejects.toThrow(/injected/);
    expect(await listDir()).toEqual([STAGE]);
    expect(await inst.listInstalled()).toEqual([]);
    expect(await fs.readdir(path.join(root, STAGE))).not.toContain('pack.json');
  });

  it('L-e: pauseAll (app quit) stops installs, keeps the staging folder and awaits settlement', async () => {
    const f = fakeFetch({ [BD]: 'kick', [SD]: (_u, init) => hangingResponse(init) });
    const inst = installer(f);
    const p = inst.install(manifest());
    const settled = p.catch((e: unknown) => e);
    await new Promise((r) => setTimeout(r, 30));
    expect(inst.busy).toBe(true);
    await inst.pauseAll();
    expect(inst.busy).toBe(false);
    const err = await settled;
    expect(err).toBeInstanceOf(InstallPausedError);
    expect(await listDir()).toEqual([STAGE]);
    expect(await fs.readFile(path.join(root, STAGE, 'kicks/bd.wav'), 'utf-8')).toBe('kick');
    expect(await fs.readdir(path.join(root, STAGE))).not.toContain('sd.wav'); // partial file removed
    expect(states.at(-1)).toEqual({ id: 'test-pack', state: { state: 'idle' } });
    // Next start resumes.
    const second = fakeFetch({ [BD]: 'kick', [SD]: 'snare' });
    await installer(second).install(manifest());
    expect(second.calls).toEqual([SD]);
  });

  it('cancelAll discards staging (explicit cancel semantics)', async () => {
    const inst = installer(fakeFetch({ [BD]: (_u, init) => hangingResponse(init), [SD]: (_u, init) => hangingResponse(init) }));
    const p = inst.install(manifest()).catch((e: unknown) => e);
    await new Promise((r) => setTimeout(r, 20));
    await inst.cancelAll();
    expect(String(await p)).toMatch(/cancelled/);
    expect(await listDir()).toEqual([]);
  });
});
