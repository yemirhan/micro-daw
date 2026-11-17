// @vitest-environment node
import { createHash } from 'node:crypto';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import type { PackInstallState } from '../../types/packs';
import { PackInstaller, resolveInside } from './installer';
import { downloadToBuffer } from './download';
import { PreviewFetcher } from './preview';
import { fetchCatalog } from './catalog';
import { fakeFetch, hangingResponse, manifest } from './testUtils';

// Node environment (real fs + streams); the shared setup clears localStorage after each test.
if (typeof globalThis.localStorage?.clear !== 'function') {
  Object.defineProperty(globalThis, 'localStorage', { value: { clear() {} }, configurable: true, writable: true });
}

const BD = 'https://raw.githubusercontent.com/o/r/sha/bd.wav';
const SD = 'https://raw.githubusercontent.com/o/r/sha/sd.wav';

let root: string;
let states: { id: string; state: PackInstallState }[];

function installer(fetchImpl: ReturnType<typeof fakeFetch>) {
  return new PackInstaller({ packsDir: root, fetchImpl, onState: (id, state) => states.push({ id, state }), progressIntervalMs: 0 });
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
    const allowedTarget = 'https://objects.githubusercontent.com/blob/bd';
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
    expect(await listDir()).toEqual([]);
    expect(states.at(-1)?.state).toMatchObject({ state: 'error' });
  });

  it('rejects oversized files from content-length and checksum mismatches, cleaning up', async () => {
    const big = installer(
      fakeFetch({ [BD]: () => new Response('x', { headers: { 'content-length': String(60 * 1024 * 1024) } }), [SD]: 'snare' }),
    );
    await expect(big.install(manifest())).rejects.toThrow(/too large/);
    expect(await listDir()).toEqual([]);

    const sha = createHash('sha256').update('kick').digest('hex');
    const m = manifest();
    m.files[0].sha256 = sha;
    m.files[1].sha256 = sha; // wrong for "snare"
    await expect(installer(fakeFetch({ [BD]: 'kick', [SD]: 'snare' })).install(m)).rejects.toThrow(/Checksum/);
    expect(await listDir()).toEqual([]);
  });

  it('rejects manifests with unsafe paths before touching the disk', async () => {
    const m = manifest({ files: [{ path: '../escape.wav', url: BD }], kits: [] });
    const f = fakeFetch({ [BD]: 'x' });
    expect(() => installer(f).install(m)).toThrow(/Unsafe/);
    expect(f.calls).toEqual([]);
    expect(() => resolveInside(root, 'a/../../x.wav')).toThrow();
    expect(resolveInside(root, 'a/b.wav')).toBe(path.join(root, 'a', 'b.wav'));
  });

  it('cancels a running install and removes its temp folder', async () => {
    const inst = installer(fakeFetch({ [BD]: (_u, init) => hangingResponse(init), [SD]: (_u, init) => hangingResponse(init) }));
    const p = inst.install(manifest());
    await new Promise((r) => setTimeout(r, 20));
    expect((await listDir()).some((n) => n.startsWith('.tmp-test-pack-'))).toBe(true);
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
    await v2.install(manifest({ version: 2 }));
    expect(await fs.readFile(path.join(root, 'test-pack/sd.wav'), 'utf-8')).toBe('snare2');
    expect((await v2.listInstalled())[0].version).toBe(2);

    await expect(installer(fakeFetch({ [BD]: 'kick3' })).install(manifest({ version: 3 }))).rejects.toThrow(/404/);
    expect(await fs.readFile(path.join(root, 'test-pack/sd.wav'), 'utf-8')).toBe('snare2');
    expect(await listDir()).toEqual(['test-pack']);
  });

  it('uninstalls, cleans stale temp folders and ignores invalid installs', async () => {
    const inst = installer(fakeFetch({ [BD]: 'kick', [SD]: 'snare' }));
    await inst.install(manifest());
    await fs.mkdir(path.join(root, '.tmp-test-pack-abc123'));
    await fs.mkdir(path.join(root, '.old-x-1'));
    await fs.mkdir(path.join(root, 'junk'));
    await fs.writeFile(path.join(root, 'junk', 'pack.json'), '{"id":"junk"}');
    await inst.cleanupStale();
    expect(await listDir()).toEqual(['junk', 'test-pack']);
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

  it('previews: allowlist, cache and LRU eviction', async () => {
    const f = fakeFetch({ [BD]: 'kick', [SD]: 'snare' });
    const pf = new PreviewFetcher({ fetchImpl: f, maxEntries: 1 });
    await expect(pf.fetch('https://evil.example/x.wav')).rejects.toThrow(/not allowed/);
    const a = await pf.fetch(BD);
    expect(new TextDecoder().decode(a)).toBe('kick');
    await pf.fetch(BD);
    expect(f.calls).toEqual([BD]);
    await pf.fetch(SD);
    expect(pf.keys()).toEqual([SD]);
  });

  it('falls back to the bundled catalog when the remote one fails or is invalid', async () => {
    const offline = await fetchCatalog({ fetchImpl: fakeFetch({}) });
    expect(offline.source).toBe('bundled');
    expect(offline.catalog.version).toBe(1);

    const url = 'https://raw.githubusercontent.com/x/y/main/catalog.json';
    const bad = await fetchCatalog({ url, fetchImpl: fakeFetch({ [url]: '{"version":9}' }) });
    expect(bad.source).toBe('bundled');

    const good = JSON.stringify({ version: 1, updatedAt: '2026-11-17', packs: [manifest()] });
    const remote = await fetchCatalog({ url, fetchImpl: fakeFetch({ [url]: good }) });
    expect(remote.source).toBe('remote');
    expect(remote.catalog.packs.map((p) => p.id)).toEqual(['test-pack']);
  });
});
