/**
 * Allowlisted, size-capped https downloads for the pack store (main process).
 *
 * - Every hop (initial URL + each redirect target) must pass `isAllowedUrl`:
 *   redirects are followed manually so an allowed host cannot bounce us to an
 *   arbitrary one.
 * - Size is capped from Content-Length up front and again while streaming.
 * - Optional sha256 and exact byte count are verified on the streamed bytes.
 * - A stalled connection aborts after `idleTimeoutMs`; a timeout (idle or the
 *   caller's AbortSignal.timeout) is a DownloadTimeoutError, never reported as
 *   a user cancel.
 * - Errors carry `retryable` (network / timeout / 5xx / 429 / truncated body),
 *   see `downloadToFileWithRetry`.
 */
import { createHash } from 'node:crypto';
import fs from 'node:fs/promises';
import { isAllowedUrl, MAX_REDIRECTS } from './validate';

export type FetchLike = (url: string, init: RequestInit) => Promise<Response>;

export class DownloadError extends Error {
  /** Worth another attempt (network hiccup, timeout, 5xx/429, truncated or corrupted body). */
  readonly retryable: boolean;
  constructor(message: string, retryable = false) {
    super(message);
    this.name = 'DownloadError';
    this.retryable = retryable;
  }
}

export class DownloadTimeoutError extends DownloadError {
  constructor() {
    super('Download timed out: the server stopped responding', true);
    this.name = 'DownloadTimeoutError';
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
  /** Expected exact byte count of the body. */
  expectedBytes?: number;
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
      throw new DownloadError(`HTTP ${res.status} for ${safeHost(current)}`, res.status >= 500 || res.status === 429 || res.status === 408);
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
  // Aborting the signal errors a real fetch body, but a body stream that does
  // not observe the signal would leave reader.read() pending forever: cancel
  // the reader too, so a stalled or cancelled download always settles.
  let reader: ReadableStreamDefaultReader<Uint8Array> | null = null;
  const abortAll = () => {
    ctrl.abort();
    reader?.cancel().catch(() => undefined);
  };
  const onOuterAbort = () => abortAll();
  if (opts.signal?.aborted) throw new DownloadAbortedError();
  opts.signal?.addEventListener('abort', onOuterAbort, { once: true });
  const idleMs = opts.idleTimeoutMs ?? DEFAULT_IDLE_MS;
  let timedOut = false;
  let timer: ReturnType<typeof setTimeout> | null = null;
  const armIdle = () => {
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => {
      timedOut = true;
      abortAll();
    }, idleMs);
  };
  try {
    armIdle();
    const res = await openAllowed(url, ctrl.signal, opts.fetchImpl ?? defaultFetch);
    const declared = Number(res.headers.get('content-length') ?? NaN);
    if (Number.isFinite(declared) && declared > opts.maxBytes) {
      await res.body?.cancel().catch(() => undefined);
      throw new DownloadError(`File too large (${declared} bytes)`);
    }
    if (!res.body) throw new DownloadError('Empty response');
    const hash = createHash('sha256');
    reader = res.body.getReader();
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
    // A cancelled reader ends like a finished body: never return a truncated file.
    if (timedOut) throw new DownloadTimeoutError();
    if (opts.signal?.aborted) throw new DownloadAbortedError();
    if (opts.expectedBytes !== undefined && bytes !== opts.expectedBytes) {
      throw new DownloadError(`Size mismatch (${bytes} bytes, expected ${opts.expectedBytes})`, true);
    }
    const sha256 = hash.digest('hex');
    if (opts.sha256 && opts.sha256.toLowerCase() !== sha256) throw new DownloadError('Checksum mismatch', true);
    return { bytes, sha256 };
  } catch (err) {
    if (opts.signal?.aborted) {
      // AbortSignal.timeout() (catalog / preview) is a timeout, not a cancel.
      if (isTimeoutReason(opts.signal.reason)) throw new DownloadTimeoutError();
      throw new DownloadAbortedError();
    }
    if (timedOut) throw new DownloadTimeoutError();
    if (err instanceof DownloadError) throw err;
    // fetch() network failures (TypeError: fetch failed, ECONNRESET …) are transient.
    throw new DownloadError(err instanceof Error ? err.message : String(err), true);
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

/** An AbortSignal.timeout() reason (DOMException "TimeoutError"). */
export function isTimeoutReason(reason: unknown): boolean {
  return !!reason && typeof reason === 'object' && (reason as { name?: unknown }).name === 'TimeoutError';
}

export interface RetryOptions {
  /** Total attempts (default 3). */
  attempts?: number;
  /** Delay before attempt n+1 is `baseDelayMs * 3^(n-1)` (default 500 ms → 500, 1500). */
  baseDelayMs?: number;
  /** Called before each retry (attempt = the one about to start, 2-based). */
  onRetry?: (attempt: number, err: DownloadError) => void;
}

function sleep(ms: number, signal?: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    if (signal?.aborted) return reject(new DownloadAbortedError());
    const t = setTimeout(() => {
      signal?.removeEventListener('abort', onAbort);
      resolve();
    }, ms);
    const onAbort = () => {
      clearTimeout(t);
      reject(new DownloadAbortedError());
    };
    signal?.addEventListener('abort', onAbort, { once: true });
  });
}

/**
 * `downloadToFile` with retries (backoff) for retryable errors. Each attempt
 * starts from an empty `dest` (the partial file of a failed attempt is
 * removed by downloadToFile). Cancels and non-retryable errors (4xx, host
 * not allowed, too large) are thrown at once.
 */
export async function downloadToFileWithRetry(
  url: string,
  dest: string,
  opts: DownloadOptions,
  retry: RetryOptions = {},
): Promise<{ bytes: number; sha256: string }> {
  const attempts = Math.max(1, retry.attempts ?? 3);
  const base = retry.baseDelayMs ?? 500;
  for (let attempt = 1; ; attempt++) {
    try {
      return await downloadToFile(url, dest, opts);
    } catch (err) {
      const retryable = err instanceof DownloadError && err.retryable && !opts.signal?.aborted;
      if (!retryable || attempt >= attempts) throw err;
      retry.onRetry?.(attempt + 1, err as DownloadError);
      await sleep(base * 3 ** (attempt - 1), opts.signal);
    }
  }
}
