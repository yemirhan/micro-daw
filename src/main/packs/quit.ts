/**
 * Quitting mid-install (N7, review L-e). 'will-quit' fires once every window
 * closed, i.e. after the unsaved-changes prompt could still cancel the quit,
 * so a cancelled quit never pauses. Running installs are paused (aborted, the
 * staging folder with its verified files is KEPT) and the quit waits for them
 * to settle, at most `timeoutMs`, so a stuck download can never block quitting.
 *
 * Nothing resumes on its own at the next start: `packs:states` reports a pack
 * with a kept staging folder as paused, its card shows "Resume", and Get then
 * only downloads the files that are missing.
 */
export const QUIT_PAUSE_TIMEOUT_MS = 3000;

interface QuitApp {
  on(event: 'will-quit', listener: (event: { preventDefault: () => void }) => void): unknown;
  quit(): void;
}

interface PausableInstaller {
  readonly busy: boolean;
  pauseAll(): Promise<void>;
}

/** Resolves when `p` settles or after `ms`, whichever comes first (never rejects). */
export function settleWithin(p: Promise<unknown>, ms: number): Promise<'settled' | 'timeout'> {
  return new Promise((resolve) => {
    const timer = setTimeout(() => resolve('timeout'), ms);
    p.then(
      () => {
        clearTimeout(timer);
        resolve('settled');
      },
      (err: unknown) => {
        clearTimeout(timer);
        console.warn('[packs] pausing installs failed', err);
        resolve('settled');
      },
    );
  });
}

export function pauseInstallsOnQuit(app: QuitApp, installer: PausableInstaller, timeoutMs = QUIT_PAUSE_TIMEOUT_MS): void {
  let quitPaused = false;
  app.on('will-quit', (event) => {
    if (quitPaused || !installer.busy) return;
    event.preventDefault();
    quitPaused = true;
    void settleWithin(installer.pauseAll(), timeoutMs).then((r) => {
      if (r === 'timeout') console.warn(`[packs] installs did not settle within ${timeoutMs} ms; quitting anyway`);
      app.quit();
    });
  });
}
