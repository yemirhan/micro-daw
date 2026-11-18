// @vitest-environment node
import { afterEach, describe, expect, it, vi } from 'vitest';
import { pauseInstallsOnQuit, QUIT_PAUSE_TIMEOUT_MS, settleWithin } from './quit';

// Node environment; the shared setup clears localStorage after each test.
if (typeof globalThis.localStorage?.clear !== 'function') {
  Object.defineProperty(globalThis, 'localStorage', { value: { clear() {} }, configurable: true, writable: true });
}

function fakeApp() {
  let listener: ((e: { preventDefault: () => void }) => void) | null = null;
  const app = {
    on: vi.fn((_ev: 'will-quit', cb: (e: { preventDefault: () => void }) => void) => {
      listener = cb;
    }),
    quit: vi.fn(),
  };
  const fire = () => {
    const e = { preventDefault: vi.fn() };
    listener?.(e);
    return e;
  };
  return { app, fire };
}

describe('pauseInstallsOnQuit (N7)', () => {
  afterEach(() => vi.useRealTimers());

  it('lets the quit through when nothing installs', () => {
    const { app, fire } = fakeApp();
    pauseInstallsOnQuit(app, { busy: false, pauseAll: vi.fn() });
    expect(fire().preventDefault).not.toHaveBeenCalled();
  });

  it('pauses running installs and quits once they settled', async () => {
    const { app, fire } = fakeApp();
    let done!: () => void;
    const installer = { busy: true, pauseAll: vi.fn(() => new Promise<void>((r) => (done = r))) };
    pauseInstallsOnQuit(app, installer);
    expect(fire().preventDefault).toHaveBeenCalled();
    expect(installer.pauseAll).toHaveBeenCalledTimes(1);
    await Promise.resolve();
    expect(app.quit).not.toHaveBeenCalled();
    done();
    await vi.waitFor(() => expect(app.quit).toHaveBeenCalledTimes(1));
    // The second will-quit (from app.quit) is not intercepted again.
    expect(fire().preventDefault).not.toHaveBeenCalled();
  });

  it('quits after at most 3 s when an install never settles', async () => {
    vi.useFakeTimers();
    const { app, fire } = fakeApp();
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    pauseInstallsOnQuit(app, { busy: true, pauseAll: () => new Promise<void>(() => undefined) });
    fire();
    await vi.advanceTimersByTimeAsync(QUIT_PAUSE_TIMEOUT_MS - 1);
    expect(app.quit).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(app.quit).toHaveBeenCalledTimes(1);
    expect(QUIT_PAUSE_TIMEOUT_MS).toBe(3000);
    warn.mockRestore();
  });

  it('a failing pause still quits', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    expect(await settleWithin(Promise.reject(new Error('boom')), 1000)).toBe('settled');
    warn.mockRestore();
  });
});
