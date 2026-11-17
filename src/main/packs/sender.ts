/**
 * IPC sender check for the pack store: only the app's own renderer (the main
 * window's top frame, loaded from the Vite dev server or the packaged
 * index.html) may call `packs:*`. Pure (URL strings only), unit tested.
 */

/**
 * Whether `senderUrl` is the app's renderer at `rendererUrl` (the URL the main
 * window loads). http(s): same origin (the dev server serves the whole app).
 * file: the same file (query / hash ignored). Anything else is rejected.
 */
export function isTrustedRendererUrl(senderUrl: string | null | undefined, rendererUrl: string): boolean {
  if (!senderUrl) return false;
  let sender: URL;
  let renderer: URL;
  try {
    sender = new URL(senderUrl);
    renderer = new URL(rendererUrl);
  } catch {
    return false;
  }
  if (sender.protocol !== renderer.protocol) return false;
  if (renderer.protocol === 'http:' || renderer.protocol === 'https:') return sender.origin === renderer.origin;
  if (renderer.protocol === 'file:') return sender.host === renderer.host && sender.pathname === renderer.pathname;
  return false;
}

/** The parts of an IPC event the check reads (Electron's IpcMainInvokeEvent). */
export interface SenderEventLike {
  senderFrame?: { url: string; parent: unknown } | null;
}

/** Throws unless the event comes from the renderer's top frame at `rendererUrl`. */
export function assertTrustedSender(event: SenderEventLike, rendererUrl: string): void {
  const frame = event.senderFrame;
  if (!frame || frame.parent || !isTrustedRendererUrl(frame.url, rendererUrl)) {
    throw new Error('Untrusted IPC sender');
  }
}
