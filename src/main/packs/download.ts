/**
 * Allowlisted, size-capped https downloads for the pack store (main process).
 *
 * - Every hop (initial URL + each redirect target) must pass `isAllowedUrl`:
 *   redirects are followed manually so an allowed host cannot bounce us to an
 *   arbitrary one.
 * - Size is capped from Content-Length up front and again while streaming.
 * - Optional sha256 is verified on the streamed bytes.
 * - A stalled connection aborts after `idleTimeoutMs`.
 */
import { createHash } from 'node:crypto';
import fs from 'node:fs/promises';
import { isAllowedUrl, MAX_REDIRECTS } from './validate';

export type FetchLike = (url: string, init: RequestInit) => Promise<Response>;

export class DownloadError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'DownloadError';
  }
}

export class DownloadAbortedError extends Error {
  constructor() {
    super('Download cancelled');
    this.name = 'AbortError';
  }
}

export interface DownloadOptions {
  maxBytes: number;
  signal?: AbortSignal;
  /** Expected lower-case hex sha256 of the body. */
  sha256?: string;
  /** Called with the bytes received so far for this request. */
  onProgress?: (received: number) => void;
  /** Aborts when no data arrives for this long (default 30 s; also bounds the wait for headers). */
  idleTimeoutMs?: number;
  fetchImpl?: FetchLike;
}

const DEFAULT_IDLE_MS = 30_000;
const USER_AGENT = 'micro-daw-packs';

function defaultFetch(url: string, init: RequestInit): Promise<Response> {
  return fetch(url, init);
}

/** Opens a response for `url`, following up to MAX_REDIRECTS redirects to allowed hosts only. */
async function openAllowed(url: string, signal: AbortSignal, fetchImpl: FetchLike): Promise<Response> {
  let current = url;
  for (let hop = 0; hop <= MAX_REDIRECTS; hop++) {
    if (!isAllowedUrl(current)) throw new DownloadError(`Host not allowed: ${safeHost(current)}`);
    const res = await fetchImpl(current, {
      redirect: 'manual',
      signal,
      credentials: 'omit',
      headers: { 'User-Agent': USER_AGENT, Accept: '*/*' },
    });
    if (res.status >= 300 && res.status < 400) {
      const loc = res.headers.get('location');
      await res.body?.cancel().catch(() => undefined);
      if (!loc) throw new DownloadError(`Redirect without location (${res.status})`);
      current = new URL(loc, current).toString();
      continue;
    }
    // An opaque redirect (fetch implementations that hide 3xx) cannot be checked: refuse it.
    if (res.type === 'opaqueredirect') throw new DownloadError('Unverifiable redirect');
    if (!res.ok) {
      await res.body?.cancel().catch(() => undefined);
      throw new DownloadError(`HTTP ${res.status} for ${safeHost(current)}`);
    }
    return res;
  }
  throw new DownloadError('Too many redirects');
}

function safeHost(url: string): string {
  try {
    return new URL(url).host || 'invalid URL';
  } catch {
    return 'invalid URL';
  }
}

/** Streams the body, enforcing the cap; calls `sink` per chunk. Returns the byte count and hex sha256. */
async function streamBody(
  url: string,
  opts: DownloadOptions,
  sink: (chunk: Uint8Array) => Promise<void> | void,
): Promise<{ bytes: number; sha256: string }> {
  const ctrl = new AbortController();
  const onOuterAbort = () => ctrl.abort();
  if (opts.signal?.aborted) throw new DownloadAbortedError();
  opts.signal?.addEventListener('abort', onOuterAbort, { once: true });
  const idleMs = opts.idleTimeoutMs ?? DEFAULT_IDLE_MS;
  let timedOut = false;
  let timer: ReturnType<typeof setTimeout> | null = null;
  const armIdle = () => {
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => {
      timedOut = true;
      ctrl.abort();
    }, idleMs);
  };
  try {
    armIdle();
    const res = await openAllowed(url, ctrl.signal, opts.fetchImpl ?? defaultFetch);
    const declared = Number(res.headers.get('content-length'));
    if (Number.isFinite(declared) && declared > opts.maxBytes) {
      await res.body?.cancel().catch(() => undefined);
      throw new DownloadError(`File too large (${declared} bytes)`);
    }
    if (!res.body) throw new DownloadError('Empty response');
    const hash = createHash('sha256');
    const reader = res.body.getReader();
    let bytes = 0;
    for (;;) {
      armIdle();
      const { done, value } = await reader.read();
      if (done) break;
      if (!value) continue;
      bytes += value.byteLength;
      if (bytes > opts.maxBytes) {
        await reader.cancel().catch(() => undefined);
        throw new DownloadError(`File too large (over ${opts.maxBytes} bytes)`);
      }
      hash.update(value);
      await sink(value);
      opts.onProgress?.(bytes);
    }
    const sha256 = hash.digest('hex');
    if (opts.sha256 && opts.sha256.toLowerCase() !== sha256) throw new DownloadError('Checksum mismatch');
    return { bytes, sha256 };
  } catch (err) {
    if (opts.signal?.aborted) throw new DownloadAbortedError();
    if (timedOut) throw new DownloadError('Download timed out');
    if (err instanceof DownloadError) throw err;
    throw new DownloadError(err instanceof Error ? err.message : String(err));
  } finally {
    if (timer) clearTimeout(timer);
    opts.signal?.removeEventListener('abort', onOuterAbort);
  }
}

/**
 * Downloads `url` to `dest` (created exclusively; the caller owns cleanup of
 * the containing temp folder). On any failure the partial file is removed.
 */
export async function downloadToFile(url: string, dest: string, opts: DownloadOptions): Promise<{ bytes: number; sha256: string }> {
  const fh = await fs.open(dest, 'wx');
  let ok = false;
  try {
    const result = await streamBody(url, opts, async (chunk) => {
      await fh.write(chunk);
    });
    ok = true;
    return result;
  } finally {
    await fh.close().catch(() => undefined);
    if (!ok) await fs.rm(dest, { force: true }).catch(() => undefined);
  }
}

/** Downloads `url` into memory (catalog, previews). */
export async function downloadToBuffer(url: string, opts: DownloadOptions): Promise<Uint8Array> {
  const chunks: Uint8Array[] = [];
  const { bytes } = await streamBody(url, opts, (chunk) => {
    chunks.push(chunk);
  });
  const out = new Uint8Array(bytes);
  let offset = 0;
  for (const c of chunks) {
    out.set(c, offset);
    offset += c.byteLength;
  }
  return out;
}

export function isAbortError(err: unknown): boolean {
  return err instanceof DownloadAbortedError || (err instanceof Error && err.name === 'AbortError');
}
