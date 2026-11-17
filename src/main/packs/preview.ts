/**
 * Remote sound previews (main process): allowlisted hosts only, ≤ 10 MB each,
 * size + sha256 verified when the catalog gives them, a small byte-bounded
 * in-memory LRU, and in-flight de-duplication. The IPC layer only passes URLs
 * of the loaded catalog (see ipc.ts).
 */
import { downloadToBuffer, DownloadError, type FetchLike } from './download';
import { isAllowedUrl, MAX_PREVIEW_BYTES } from './validate';

export interface PreviewCacheOptions {
  maxEntries?: number;
  maxTotalBytes?: number;
  fetchImpl?: FetchLike;
  timeoutMs?: number;
}

export class PreviewFetcher {
  private readonly cache = new Map<string, Uint8Array>();
  private readonly pending = new Map<string, Promise<Uint8Array>>();
  private totalBytes = 0;
  private readonly maxEntries: number;
  private readonly maxTotalBytes: number;
  private readonly opts: PreviewCacheOptions;

  constructor(opts: PreviewCacheOptions = {}) {
    this.opts = opts;
    this.maxEntries = opts.maxEntries ?? 24;
    this.maxTotalBytes = opts.maxTotalBytes ?? 48 * 1024 * 1024;
  }

  /** Bytes of the file at `url` (a fresh ArrayBuffer per call, safe to transfer over IPC). */
  async fetch(url: string, expect: { size?: number; sha256?: string } = {}): Promise<ArrayBuffer> {
    if (!isAllowedUrl(url)) throw new DownloadError('Preview URL not allowed');
    const bytes = await this.get(url, expect);
    return bytes.slice().buffer as ArrayBuffer;
  }

  private get(url: string, expect: { size?: number; sha256?: string }): Promise<Uint8Array> {
    const hit = this.cache.get(url);
    if (hit) {
      this.cache.delete(url);
      this.cache.set(url, hit);
      return Promise.resolve(hit);
    }
    const inflight = this.pending.get(url);
    if (inflight) return inflight;
    const p = downloadToBuffer(url, {
      maxBytes: MAX_PREVIEW_BYTES,
      expectedBytes: expect.size,
      sha256: expect.sha256,
      signal: AbortSignal.timeout(this.opts.timeoutMs ?? 20_000),
      fetchImpl: this.opts.fetchImpl,
    })
      .then((bytes) => {
        this.put(url, bytes);
        return bytes;
      })
      .finally(() => this.pending.delete(url));
    this.pending.set(url, p);
    return p;
  }

  private put(url: string, bytes: Uint8Array): void {
    const prev = this.cache.get(url);
    if (prev) {
      this.totalBytes -= prev.byteLength;
      this.cache.delete(url);
    }
    this.cache.set(url, bytes);
    this.totalBytes += bytes.byteLength;
    while (this.cache.size > this.maxEntries || (this.totalBytes > this.maxTotalBytes && this.cache.size > 1)) {
      const oldest = this.cache.keys().next().value as string;
      this.totalBytes -= this.cache.get(oldest)?.byteLength ?? 0;
      this.cache.delete(oldest);
    }
  }

  /** Cached URLs, least recently used first (tests / diagnostics). */
  keys(): string[] {
    return [...this.cache.keys()];
  }

  clear(): void {
    this.cache.clear();
    this.totalBytes = 0;
  }
}
