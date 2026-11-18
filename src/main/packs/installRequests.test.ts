// @vitest-environment node
import { describe, expect, it, vi } from 'vitest';
import type { InstalledPack, PackManifest } from '../../types/packs';
import { InstallPausedError } from './installer';
import { InstallRequests, type RequestInstaller } from './installRequests';
import { pauseInstallsOnQuit } from './quit';
import { manifest } from './testUtils';

if (typeof globalThis.localStorage?.clear !== 'function') {
  Object.defineProperty(globalThis, 'localStorage', { value: { clear() {} }, configurable: true, writable: true });
}

function fakeInstaller() {
  const inst = {
    busy: false,
    install: vi.fn(async (m: PackManifest) => ({ id: m.id }) as unknown as InstalledPack),
    cancel: vi.fn(() => false),
    pauseAll: vi.fn(async () => undefined),
    reportCancelled: vi.fn(async () => undefined),
  } satisfies RequestInstaller;
  return inst;
}

function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}

describe('install requests waiting for startup / the catalog (finding 10)', () => {
  it('a cancel while the request waits is not lost: it never reaches the installer', async () => {
    const inst = fakeInstaller();
    const reqs = new InstallRequests(inst);
    const wait = deferred<PackManifest>();
    const p = reqs.install('test-pack', () => wait.promise).catch((e: unknown) => e);
    expect(reqs.busy).toBe(true);
    expect(reqs.cancel('test-pack')).toBe(true); // before the fix: false (nothing installing yet)
    expect(reqs.cancel('other-pack')).toBe(false);
    wait.resolve(manifest());
    const err = await p;
    expect(String(err)).toMatch(/Install cancelled/);
    expect(inst.install).not.toHaveBeenCalled();
    expect(inst.reportCancelled).toHaveBeenCalledWith('test-pack');
    expect(reqs.busy).toBe(false);
  });

  it('an uncancelled request installs; a cancel then goes to the installer', async () => {
    const inst = fakeInstaller();
    const reqs = new InstallRequests(inst);
    await expect(reqs.install('test-pack', async () => manifest())).resolves.toMatchObject({ id: 'test-pack' });
    inst.cancel.mockReturnValue(true);
    expect(reqs.cancel('test-pack')).toBe(true);
    expect(inst.cancel).toHaveBeenCalledWith('test-pack');
  });

  it('quit: queued requests count as busy and are paused (never started)', async () => {
    const inst = fakeInstaller();
    const reqs = new InstallRequests(inst);
    let listener!: (e: { preventDefault: () => void }) => void;
    const app = { on: vi.fn((_: 'will-quit', cb: typeof listener) => (listener = cb)), quit: vi.fn() };
    pauseInstallsOnQuit(app, reqs);
    const wait = deferred<PackManifest>();
    const p = reqs.install('test-pack', () => wait.promise).catch((e: unknown) => e);
    const e = { preventDefault: vi.fn() };
    listener(e);
    expect(e.preventDefault).toHaveBeenCalled(); // before the fix the quit went through
    await vi.waitFor(() => expect(app.quit).toHaveBeenCalled());
    expect(inst.pauseAll).toHaveBeenCalled();
    wait.resolve(manifest());
    expect(await p).toBeInstanceOf(InstallPausedError);
    expect(inst.install).not.toHaveBeenCalled();
  });
});
