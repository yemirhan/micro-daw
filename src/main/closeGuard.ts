/**
 * The window close guard (`main.ts`): a close is held and the renderer asked
 * (`project:before-close`) unless the renderer already allowed it
 * (`project:force-close`).
 *
 * Electron bug fixed in both apps (Rune port phase 10, user decision 2026-11-18): the
 * "allowed" flag was a module global that was never reset. On macOS the app keeps running
 * after its window closes, and the window `activate` recreates closed without asking: a
 * dirty project there was lost. Each window now gets its own guard, armed again when the
 * window is created.
 */
export interface CloseGuard {
  /** The window's `close` event: true when the close goes ahead. */
  onClose(event: { preventDefault(): void }, askRenderer: () => void): boolean;
  /** `project:force-close`: the next close goes ahead. */
  allowNextClose(): void;
}

export function createCloseGuard(): CloseGuard {
  let allowed = false;
  return {
    onClose(event, askRenderer) {
      if (allowed) {
        allowed = false;
        return true;
      }
      event.preventDefault();
      askRenderer();
      return false;
    },
    allowNextClose() {
      allowed = true;
    },
  };
}
