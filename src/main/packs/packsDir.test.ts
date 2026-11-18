// @vitest-environment node
/**
 * Packs folder protocol (docs/rune-port/packs-dir-protocol.md), phase-6 review
 * findings 2, 3, 4 and 13: Electron bugs fixed in both apps (user decision
 * 2026-11-18). The Rust port runs the same scenarios.
 */
import { spawnSync } from 'node:child_process';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PackInstallState } from '../../types/packs';
import { PackInstaller } from './installer';
import { LOCK_BUSY_MESSAGE, PacksDirLock } from './lock';
import { BD_URL, fakeFetch, manifest, rawTestUrl, sha256Of } from './testUtils';

if (typeof globalThis.localStorage?.clear !== 'function') {
  Object.defineProperty(globalThis, 'localStorage', { value: { clear() {} }, configurable: true, writable: true });
}

const BD = BD_URL;
const SD = rawTestUrl('sd.wav');
const STAGE = '.partial-test-pack';

let root: string;
let outside: string;
let states: { id: string; state: PackInstallState }[];
const restore: (() => void)[] = [];

function installer(fetchImpl: ReturnType<typeof fakeFetch>, extra: Partial<ConstructorParameters<typeof PackInstaller>[0]> = {}) {
  return new PackInstaller({
    packsDir: root,
    fetchImpl,
    onState: (id, state) => states.push({ id, state }),
    progressIntervalMs: 0,
    retry: { baseDelayMs: 1 },
    ...extra,
  });
}

function manifestFor(kick: string, snare: string, version = 1) {
  const m = manifest({ version });
  m.files[0] = { ...m.files[0], size: Buffer.byteLength(kick), sha256: sha256Of(kick) };
  m.files[1] = { ...m.files[1], size: Buffer.byteLength(snare), sha256: sha256Of(snare) };
  return m;
}

const listDir = async (d = root) => (await fs.readdir(d).catch(() => [] as string[])).sort();
const tick = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** A response that waits for `gate` before sending `body`. */
function gated(body: string, gate: Promise<void>) {
  return async () => {
    await gate;
    return new Response(body, { status: 200, headers: { 'content-length': String(Buffer.byteLength(body)) } });
  };
}

/** A pid that existed and has exited. */
function deadPid(): number {
  const r = spawnSync(process.execPath, ['-e', '0']);
  return r.pid as number;
}

async function writeLock(owner: object | null, ageMs = 0) {
  const dir = path.join(root, '.lock');
  await fs.mkdir(dir, { recursive: true });
  if (owner) await fs.writeFile(path.join(dir, 'owner.json'), JSON.stringify(owner));
  if (ageMs) {
    const t = new Date(Date.now() - ageMs);
    await fs.utimes(dir, t, t);
  }
}

beforeEach(async () => {
  root = await fs.mkdtemp(path.join(os.tmpdir(), 'mdaw-packsdir-'));
  outside = await fs.mkdtemp(path.join(os.tmpdir(), 'mdaw-outside-'));
  states = [];
});

afterEach(async () => {
  for (const r of restore.splice(0)) r();
  await fs.rm(root, { recursive: true, force: true });
  await fs.rm(outside, { recursive: true, force: true });
});

describe('cross-process lock (finding 4)', () => {
  it('a second installer on the same folder waits for the first: no shared partial files', async () => {
    let open!: () => void;
    const gate = new Promise<void>((r) => (open = r));
    const a = installer(fakeFetch({ [BD]: 'kick', [SD]: gated('snare', gate) }));
    const bFetch = fakeFetch({ [BD]: 'kick', [SD]: 'snare' });
    const b = installer(bFetch);
    const pa = a.install(manifest());
    await tick(30);
    expect(await listDir()).toEqual(['.lock', STAGE]);
    const pb = b.install(manifest());
    await tick(100);
    expect(bFetch.calls).toEqual([]); // waiting for the lock, staging untouched
    expect(b.getState('test-pack')).toMatchObject({ state: 'downloading', receivedBytes: 0 });
    open();
    await pa;
    await pb;
    expect(bFetch.calls.sort()).toEqual([BD, SD].sort()); // B ran only after A's swap: a clean staging folder of its own
    expect(await fs.readFile(path.join(root, 'test-pack/sd.wav'), 'utf-8')).toBe('snare');
    expect(await listDir()).toEqual(['test-pack']); // lock released, no backup left
  });

  it('a waiting install can be cancelled', async () => {
    await writeLock({ pid: process.ppid, app: 'rune', token: 'f'.repeat(32), acquiredAt: new Date().toISOString() });
    const f = fakeFetch({ [BD]: 'kick', [SD]: 'snare' });
    const inst = installer(f);
    const p = inst.install(manifest()).catch((e: unknown) => e);
    await tick(80);
    inst.cancel('test-pack');
    expect(String(await p)).toMatch(/Install cancelled/);
    expect(f.calls).toEqual([]);
    expect(states.at(-1)).toEqual({ id: 'test-pack', state: { state: 'idle' } });
    expect(await listDir()).toEqual(['.lock']); // the other owner's lock is untouched
  });

  it('a live foreign lock: uninstall gives up after its timeout, cleanup is skipped', async () => {
    await writeLock({ pid: process.ppid, app: 'rune', token: 'f'.repeat(32), acquiredAt: new Date().toISOString() });
    await fs.mkdir(path.join(root, '.tmp-test-pack-abc'));
    const inst = installer(fakeFetch({}), { uninstallLockWaitMs: 120 });
    const t0 = Date.now();
    await expect(inst.uninstall('test-pack')).rejects.toThrow(LOCK_BUSY_MESSAGE);
    expect(Date.now() - t0).toBeGreaterThanOrEqual(100);
    await inst.cleanupStale();
    expect(await listDir()).toEqual(['.lock', '.tmp-test-pack-abc']);
  });

  it('a lock whose owner pid is dead is stolen', async () => {
    await writeLock({ pid: deadPid(), app: 'rune', token: 'a'.repeat(32), acquiredAt: new Date().toISOString() });
    const inst = installer(fakeFetch({ [BD]: 'kick', [SD]: 'snare' }));
    await inst.install(manifest());
    expect(await listDir()).toEqual(['test-pack']); // stolen, used, released; no .lock-stale-* left
  });

  it('a lock whose heartbeat stopped (mtime > 30 s) is stolen, even from a live pid or without owner.json', async () => {
    await writeLock({ pid: process.ppid, app: 'rune', token: 'b'.repeat(32), acquiredAt: '2026-01-01T00:00:00.000Z' }, 60_000);
    const inst = installer(fakeFetch({ [BD]: 'kick', [SD]: 'snare' }));
    await inst.install(manifest());
    expect(await listDir()).toEqual(['test-pack']);
    await writeLock(null, 60_000);
    await fs.mkdir(path.join(root, '.lock-stale-0badf00d')); // leftover of an interrupted steal
    await inst.cleanupStale();
    expect(await listDir()).toEqual(['test-pack']);
  });

  it('holds are refcounted; release never removes a lock carrying another token', async () => {
    const lock = new PacksDirLock(root, { heartbeatMs: 10 });
    const r1 = await lock.acquire();
    const r2 = await lock.acquire({ timeoutMs: 0 });
    expect(r1 && r2).toBeTruthy();
    const owner = JSON.parse(await fs.readFile(path.join(root, '.lock/owner.json'), 'utf-8'));
    expect(owner).toMatchObject({ pid: process.pid, app: 'electron' });
    expect(owner.token).toMatch(/^[0-9a-f]{32}$/);
    // Heartbeat refreshes the mtime.
    const old = new Date(Date.now() - 20_000);
    await fs.utimes(path.join(root, '.lock'), old, old);
    await vi.waitFor(async () => expect(Date.now() - (await fs.stat(path.join(root, '.lock'))).mtimeMs).toBeLessThan(5_000), { timeout: 2_000 });
    await r1!();
    expect(await listDir()).toEqual(['.lock']); // still held by r2
    // A second owner object (another window) cannot take it.
    expect(await new PacksDirLock(root).acquire({ timeoutMs: 60 })).toBeNull();
    await fs.writeFile(path.join(root, '.lock/owner.json'), JSON.stringify({ ...owner, token: 'c'.repeat(32) }));
    await r2!();
    expect(await listDir()).toEqual(['.lock']); // not ours any more: left alone
  });
});

describe('durability (finding 3)', () => {
  /** Records fsyncs (by path), renames and removals in order. */
  async function recordFsOps() {
    const events: string[] = [];
    const fdPath = new Map<number, string>();
    const probe = await fs.open(path.join(root, '.probe'), 'w');
    const proto = Object.getPrototypeOf(probe) as { sync: () => Promise<void> };
    await probe.close();
    await fs.rm(path.join(root, '.probe'));
    const realOpen = fs.open.bind(fs);
    const realSync = proto.sync;
    const realRename = fs.rename.bind(fs);
    const realRm = fs.rm.bind(fs);
    const rel = (p: string) => path.relative(root, p) || '.';
    const so = vi.spyOn(fs, 'open').mockImplementation((async (...a: Parameters<typeof fs.open>) => {
      const fh = await realOpen(...a);
      fdPath.set(fh.fd, String(a[0]));
      return fh;
    }) as never);
    const ss = vi.spyOn(proto, 'sync').mockImplementation(async function (this: { fd: number }) {
      events.push(`sync ${rel(fdPath.get(this.fd) ?? '?')}`);
      return realSync.call(this);
    });
    const sr = vi.spyOn(fs, 'rename').mockImplementation((async (a: string, b: string) => {
      events.push(`rename ${rel(a).replace(/[0-9a-f]{8}$/, 'X')} ${rel(b).replace(/[0-9a-f]{8}$/, 'X')}`);
      return realRename(a, b);
    }) as never);
    const sm = vi.spyOn(fs, 'rm').mockImplementation((async (p: string, o?: object) => {
      if (rel(p).startsWith('.old-')) events.push(`rm ${rel(p).replace(/[0-9a-f]{8}$/, 'X')}`);
      return realRm(p, o);
    }) as never);
    restore.push(() => {
      so.mockRestore();
      ss.mockRestore();
      sr.mockRestore();
      sm.mockRestore();
    });
    return { events, proto };
  }

  it('files, pack.json and every staging folder are synced before the renames; the packs folder before the backup goes', async () => {
    await installer(fakeFetch({ [BD]: 'kick', [SD]: 'snare' })).install(manifest());
    const { events } = await recordFsOps();
    await installer(fakeFetch({ [BD]: 'kick2', [SD]: 'snare2' })).install(manifestFor('kick2', 'snare2', 2));
    const at = (e: string) => {
      const i = events.indexOf(e);
      expect(i, `${e} in ${events.join(' | ')}`).toBeGreaterThanOrEqual(0);
      return i;
    };
    const firstRename = at('rename test-pack .old-test-pack-X');
    const secondRename = at(`rename ${STAGE} test-pack`);
    for (const e of [`sync ${STAGE}/kicks/bd.wav`, `sync ${STAGE}/sd.wav`, `sync ${STAGE}/pack.json`, `sync ${STAGE}/kicks`, `sync ${STAGE}`]) {
      expect(at(e)).toBeLessThan(firstRename);
    }
    expect(at(`sync ${STAGE}/kicks`)).toBeLessThan(at(`sync ${STAGE}`)); // deepest first
    expect(at(`sync ${STAGE}/pack.json`)).toBeLessThan(at(`sync ${STAGE}`));
    const dirSync = events.indexOf('sync .', secondRename);
    expect(dirSync).toBeGreaterThan(secondRename);
    expect(dirSync).toBeLessThan(at('rm .old-test-pack-X'));
    expect(await listDir()).toEqual(['test-pack']);
  });

  it('a failing fsync (ENOSPC) fails the install without a retry and keeps the verified files', async () => {
    const { proto } = await recordFsOps();
    const realSync = (proto.sync as unknown as { getMockImplementation: () => (this: { fd: number }) => Promise<void> }).getMockImplementation();
    let target = -1;
    const realOpen = (fs.open as unknown as { getMockImplementation: () => typeof fs.open }).getMockImplementation();
    vi.spyOn(fs, 'open').mockImplementation((async (...a: Parameters<typeof fs.open>) => {
      const fh = await realOpen(...a);
      if (String(a[0]).endsWith('sd.wav')) target = fh.fd;
      return fh;
    }) as never);
    vi.spyOn(proto, 'sync').mockImplementation(async function (this: { fd: number }) {
      if (this.fd === target) throw Object.assign(new Error('ENOSPC: no space left on device, fsync'), { code: 'ENOSPC' });
      return realSync.call(this);
    });
    const f = fakeFetch({ [BD]: 'kick', [SD]: 'snare' });
    await expect(installer(f, { concurrency: 1 }).install(manifest())).rejects.toThrow(/ENOSPC/);
    expect(f.calls.filter((u) => u === SD)).toHaveLength(1); // not retried
    expect(await listDir()).toEqual([STAGE]);
    expect(await listDir(path.join(root, STAGE))).toEqual(['kicks']); // the unsynced file is gone, bd.wav kept
    expect(states.at(-1)?.state).toMatchObject({ state: 'error', message: expect.stringMatching(/ENOSPC/) });
  });

  it('recovery: a pack folder with a valid pack.json but a short file (power cut) gives way to its complete backup', async () => {
    await installer(fakeFetch({ [BD]: 'kick', [SD]: 'snare' })).install(manifest());
    await fs.cp(path.join(root, 'test-pack'), path.join(root, '.old-test-pack-0123abcd'), { recursive: true });
    await fs.truncate(path.join(root, 'test-pack/sd.wav'), 0);
    const inst = installer(fakeFetch({}));
    await inst.cleanupStale();
    expect(await listDir()).toEqual(['test-pack']);
    expect(await fs.readFile(path.join(root, 'test-pack/sd.wav'), 'utf-8')).toBe('snare');
  });

  it('recovery: an incomplete backup is never restored over nothing, and is kept', async () => {
    await installer(fakeFetch({ [BD]: 'kick', [SD]: 'snare' })).install(manifest());
    await fs.rename(path.join(root, 'test-pack'), path.join(root, '.old-test-pack-0123abcd'));
    await fs.rm(path.join(root, '.old-test-pack-0123abcd/sd.wav'));
    await installer(fakeFetch({})).cleanupStale();
    expect(await listDir()).toEqual(['.old-test-pack-0123abcd']);
  });
});

describe('symlinks are never followed (finding 2)', () => {
  it('a symlinked staging folder is replaced: nothing outside is deleted or written', async () => {
    await fs.writeFile(path.join(outside, 'victim.txt'), 'keep me');
    await fs.writeFile(path.join(outside, 'sd.wav'), 'snare'); // would even pass verification
    await fs.symlink(outside, path.join(root, STAGE), 'dir');
    const f = fakeFetch({ [BD]: 'kick', [SD]: 'snare' });
    await installer(f).install(manifest());
    expect(await listDir(outside)).toEqual(['sd.wav', 'victim.txt']);
    expect(await fs.readFile(path.join(outside, 'victim.txt'), 'utf-8')).toBe('keep me');
    expect(f.calls.sort()).toEqual([BD, SD].sort()); // nothing was "resumed" from the outside folder
    expect((await fs.lstat(path.join(root, 'test-pack'))).isDirectory()).toBe(true);
    expect(await listDir()).toEqual(['test-pack']);
  });

  it('a symlinked staging subfolder is pruned, not followed', async () => {
    await fs.writeFile(path.join(outside, 'victim.txt'), 'keep me');
    await fs.mkdir(path.join(root, STAGE), { recursive: true });
    await fs.symlink(outside, path.join(root, STAGE, 'kicks'), 'dir');
    await installer(fakeFetch({ [BD]: 'kick', [SD]: 'snare' })).install(manifest());
    expect(await listDir(outside)).toEqual(['victim.txt']);
    expect(await fs.readFile(path.join(root, 'test-pack/kicks/bd.wav'), 'utf-8')).toBe('kick');
  });

  it('a symlinked .old-<id>-* is no backup and a symlinked <id> no install; removal unlinks the link only', async () => {
    await installer(fakeFetch({ [BD]: 'kick', [SD]: 'snare' })).install(manifest());
    await fs.cp(path.join(root, 'test-pack'), outside, { recursive: true });
    await fs.rm(path.join(root, 'test-pack'), { recursive: true });
    await fs.symlink(outside, path.join(root, '.old-test-pack-0badf00d'), 'dir');
    const inst = installer(fakeFetch({}));
    await inst.cleanupStale();
    expect(await listDir()).toEqual([]); // not restored; the link was removed
    expect(await listDir(outside)).toEqual(['kicks', 'pack.json', 'sd.wav']);

    await fs.symlink(outside, path.join(root, 'test-pack'), 'dir');
    expect(await inst.listInstalled()).toEqual([]);
    await fs.symlink(outside, path.join(root, STAGE), 'dir');
    await inst.uninstall('test-pack');
    expect(await listDir()).toEqual([]);
    expect(await listDir(outside)).toEqual(['kicks', 'pack.json', 'sd.wav']);
  });
});

describe('per-pack operations (finding 13)', () => {
  it('an install started during an uninstall of the same pack waits for it, then installs from scratch', async () => {
    await installer(fakeFetch({ [BD]: 'kick', [SD]: 'snare' })).install(manifest());
    const order: string[] = [];
    const realRm = fs.rm.bind(fs);
    const sm = vi.spyOn(fs, 'rm').mockImplementation((async (p: string, o?: object) => {
      if (path.basename(p).startsWith('.del-')) await tick(80); // a slow delete
      const r = await realRm(p, o);
      if (path.basename(p) === STAGE) order.push('uninstall removed staging');
      return r;
    }) as never);
    restore.push(() => sm.mockRestore());
    const f = fakeFetch({ [BD]: 'kick', [SD]: 'snare' });
    const inst = installer(f);
    const u = inst.uninstall('test-pack').then(() => order.push('uninstalled'));
    const i = inst.install(manifest()).then(() => order.push('installed'));
    await tick(30);
    expect(f.calls).toEqual([]);
    await Promise.all([u, i]);
    expect(order).toEqual(['uninstall removed staging', 'uninstalled', 'installed']);
    expect(f.calls.sort()).toEqual([BD, SD].sort());
    expect(await listDir()).toEqual(['test-pack']);
    expect((await inst.listInstalled()).map((p) => p.id)).toEqual(['test-pack']);
  });
});
