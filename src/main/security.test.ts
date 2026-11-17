import { describe, expect, it, vi } from 'vitest';
import type { WebContents } from 'electron';
import { createTrustedIpc, externalUrlFor, installWebContentsGuards, isAllowedNavigation } from './security';

const DEV = 'http://localhost:5173';
const PROD = 'file:///Applications/Micro%20DAW.app/Contents/Resources/app.asar/.vite/renderer/main_window/index.html';

describe('navigation / window-open policy (S1)', () => {
  it('allows only the app renderer itself', () => {
    expect(isAllowedNavigation(`${DEV}/`, DEV)).toBe(true);
    expect(isAllowedNavigation(`${PROD}#/beat`, PROD)).toBe(true);
    expect(isAllowedNavigation('https://github.com/yemirhan/micro-daw', DEV)).toBe(false);
    // A Finder file dropped outside a drop zone navigates to its file: URL.
    expect(isAllowedNavigation('file:///Users/me/Samples/kick.wav', PROD)).toBe(false);
    expect(isAllowedNavigation('file:///Users/me/Samples/kick.wav', DEV)).toBe(false);
  });

  it('hands only plain https links to the system browser', () => {
    expect(externalUrlFor('https://creativecommons.org/licenses/by/4.0/')).toBe('https://creativecommons.org/licenses/by/4.0/');
    expect(externalUrlFor('http://example.com/')).toBeNull();
    expect(externalUrlFor('file:///etc/passwd')).toBeNull();
    expect(externalUrlFor('javascript:alert(1)')).toBeNull();
    expect(externalUrlFor('https://user:pw@example.com/')).toBeNull();
    expect(externalUrlFor('not a url')).toBeNull();
  });
});

type Listener = (...args: unknown[]) => void;

function fakeContents() {
  const listeners = new Map<string, Listener>();
  let openHandler: ((d: { url: string }) => { action: string }) | null = null;
  const contents = {
    setWindowOpenHandler: (h: typeof openHandler) => {
      openHandler = h;
    },
    on: (event: string, l: Listener) => {
      listeners.set(event, l);
      return contents;
    },
  };
  const fire = (event: string, ...args: unknown[]) => {
    const e = { preventDefault: vi.fn(), url: args[0] };
    listeners.get(event)?.(e, ...args);
    return e.preventDefault.mock.calls.length > 0;
  };
  return { contents: contents as unknown as WebContents, fire, open: (url: string) => openHandler!({ url }) };
}

describe('installWebContentsGuards', () => {
  it('denies every new window and opens https links externally', () => {
    const openExternal = vi.fn(() => Promise.resolve());
    const { contents, open } = fakeContents();
    installWebContentsGuards(contents, { rendererUrl: DEV, openExternal, log: () => undefined });
    expect(open('https://github.com/yemirhan/micro-daw')).toEqual({ action: 'deny' });
    expect(openExternal).toHaveBeenCalledWith('https://github.com/yemirhan/micro-daw');
    expect(open('http://localhost:9999/remote')).toEqual({ action: 'deny' });
    expect(open(`${DEV}/`)).toEqual({ action: 'deny' });
    expect(openExternal).toHaveBeenCalledTimes(1);
  });

  it('blocks navigation, frame navigation and redirects away from the renderer, and <webview>', () => {
    const { contents, fire } = fakeContents();
    installWebContentsGuards(contents, { rendererUrl: PROD, openExternal: vi.fn(), log: () => undefined });
    expect(fire('will-navigate', 'file:///Users/me/kick.wav')).toBe(true);
    expect(fire('will-navigate', `${PROD}#/packs`)).toBe(false);
    expect(fire('will-frame-navigate', 'https://evil.example/')).toBe(true);
    expect(fire('will-redirect', 'https://evil.example/')).toBe(true);
    expect(fire('will-attach-webview')).toBe(true);
  });
});

describe('createTrustedIpc', () => {
  function fakeIpc() {
    const handlers = new Map<string, (e: unknown, ...a: unknown[]) => unknown>();
    const ons = new Map<string, (e: unknown, ...a: unknown[]) => void>();
    return {
      ipc: {
        handle: (c: string, h: (e: unknown, ...a: unknown[]) => unknown) => handlers.set(c, h),
        on: (c: string, h: (e: unknown, ...a: unknown[]) => void) => ons.set(c, h),
      },
      handlers,
      ons,
    };
  }
  const frame = (url: string, parent: unknown = null) => ({ senderFrame: { url, parent } });

  it('runs handlers only for the renderer top frame', async () => {
    const { ipc, handlers } = fakeIpc();
    const t = createTrustedIpc(ipc as never, DEV, () => undefined);
    const read = vi.fn((_e: unknown, p: string) => `read ${p}`);
    t.handle('sample:read-file', read as never);
    const h = handlers.get('sample:read-file')!;
    expect(h(frame(`${DEV}/`), '/etc/hosts')).toBe('read /etc/hosts');
    expect(() => h(frame('http://localhost:9999/remote'), '/etc/hosts')).toThrow('Untrusted IPC sender');
    expect(() => h(frame(`${DEV}/`, {}), '/etc/hosts')).toThrow('Untrusted IPC sender');
    expect(() => h({ senderFrame: null }, '/etc/hosts')).toThrow('Untrusted IPC sender');
    expect(read).toHaveBeenCalledTimes(1);
  });

  it('ignores fire-and-forget messages from untrusted senders', () => {
    const { ipc, ons } = fakeIpc();
    const t = createTrustedIpc(ipc as never, PROD, () => undefined);
    const close = vi.fn();
    t.on('project:force-close', close as never);
    ons.get('project:force-close')!(frame('https://evil.example/'));
    expect(close).not.toHaveBeenCalled();
    ons.get('project:force-close')!(frame(PROD));
    expect(close).toHaveBeenCalledTimes(1);
  });
});
