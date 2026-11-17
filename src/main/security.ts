/**
 * Main-process hardening (see docs/review-round-3.md S1):
 *
 * - Every webContents: new windows are denied (https links open in the
 *   system browser instead), navigation / redirects of any frame away from
 *   the app renderer are blocked (this also stops a Finder file dropped
 *   outside a drop zone from replacing the app), <webview> is refused.
 * - Every IPC handler checks its sender is the app renderer's top frame
 *   (`createTrustedIpc`), so a foreign page can never reach the file system
 *   or pack store even if it were loaded somehow.
 *
 * The policy functions are pure (URL strings only) and unit tested; the
 * Electron objects are passed in.
 */
import type { IpcMain, IpcMainEvent, IpcMainInvokeEvent, WebContents } from 'electron';
import { assertTrustedSender, isTrustedRendererUrl, type SenderEventLike } from './packs/sender';

/** Whether a frame may navigate to `url` (only the app renderer itself: reload, hash routes, HMR). */
export function isAllowedNavigation(url: string, rendererUrl: string): boolean {
  return isTrustedRendererUrl(url, rendererUrl);
}

/**
 * URL to hand to the system browser for a window.open / target=_blank
 * request, or null to just drop it. Only plain https links (no credentials)
 * leave the app; the new Electron window itself is always denied.
 */
export function externalUrlFor(url: string): string | null {
  let u: URL;
  try {
    u = new URL(url);
  } catch {
    return null;
  }
  if (u.protocol !== 'https:' || u.username || u.password) return null;
  return u.toString();
}

export interface GuardOptions {
  rendererUrl: string;
  /** shell.openExternal in the app; injectable for tests. */
  openExternal: (url: string) => unknown;
  log?: (message: string) => void;
}

/** Window-open / navigation / webview guards for one webContents. */
export function installWebContentsGuards(contents: WebContents, opts: GuardOptions): void {
  const log = opts.log ?? ((m: string) => console.warn(m));
  contents.setWindowOpenHandler(({ url }) => {
    const external = externalUrlFor(url);
    if (external) {
      Promise.resolve(opts.openExternal(external)).catch(() => undefined);
    } else {
      log(`[security] blocked window.open(${truncate(url)})`);
    }
    return { action: 'deny' };
  });
  const block = (event: { preventDefault: () => void }, url: string, what: string) => {
    if (isAllowedNavigation(url, opts.rendererUrl)) return;
    event.preventDefault();
    log(`[security] blocked ${what} to ${truncate(url)}`);
  };
  contents.on('will-navigate', (event, url) => block(event, url, 'navigation'));
  contents.on('will-frame-navigate', (event) => block(event, event.url, 'frame navigation'));
  contents.on('will-redirect', (event, url) => block(event, url, 'redirect'));
  contents.on('will-attach-webview', (event) => {
    event.preventDefault();
    log('[security] blocked <webview>');
  });
}

function truncate(url: string): string {
  return url.length > 200 ? `${url.slice(0, 200)}…` : url;
}

type InvokeHandler = (event: IpcMainInvokeEvent, ...args: never[]) => unknown;
type EventHandler = (event: IpcMainEvent, ...args: never[]) => void;

export interface TrustedIpc {
  /** ipcMain.handle that rejects ("Untrusted IPC sender") unless the sender is the app renderer. */
  handle: (channel: string, fn: InvokeHandler) => void;
  /** ipcMain.on that ignores messages from anything but the app renderer. */
  on: (channel: string, fn: EventHandler) => void;
}

/** Sender-checked ipcMain wrappers (`ipc` is ipcMain; injectable for tests). */
export function createTrustedIpc(
  ipc: Pick<IpcMain, 'handle' | 'on'>,
  rendererUrl: string,
  log: (message: string) => void = (m) => console.warn(m),
): TrustedIpc {
  return {
    handle(channel, fn) {
      ipc.handle(channel, (event, ...args) => {
        assertTrustedSender(event as SenderEventLike, rendererUrl);
        return (fn as (e: IpcMainInvokeEvent, ...a: unknown[]) => unknown)(event, ...args);
      });
    },
    on(channel, fn) {
      ipc.on(channel, (event, ...args) => {
        try {
          assertTrustedSender(event as SenderEventLike, rendererUrl);
        } catch {
          log(`[security] ignored ${channel} from an untrusted sender`);
          return;
        }
        (fn as (e: IpcMainEvent, ...a: unknown[]) => void)(event, ...args);
      });
    },
  };
}
