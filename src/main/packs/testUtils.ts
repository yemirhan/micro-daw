/** Test helpers for the pack main-process code (not shipped: only imported by *.test.ts). */
import { createHash } from 'node:crypto';
import type { PackFile, PackManifest } from '../../types/packs';
import type { FetchLike } from './download';

/** A 40-hex commit for test URLs. */
export const COMMIT = 'c0ffee'.padEnd(40, '0');
/** Commit-pinned raw URL of `path` in the test repo. */
export const rawTestUrl = (path: string) => `https://raw.githubusercontent.com/o/r/${COMMIT}/${path}`;
export const BD_URL = rawTestUrl('bd.wav');
export const SD_URL = rawTestUrl('sd.wav');

export const sha256Of = (body: string) => createHash('sha256').update(body).digest('hex');

/** A valid PackFile whose size + sha256 match `body`. */
export function packFile(path: string, body: string, url = rawTestUrl(path)): PackFile {
  return { path, url, size: Buffer.byteLength(body), sha256: sha256Of(body) };
}

export function manifest(overrides: Partial<PackManifest> = {}): PackManifest {
  return {
    id: 'test-pack',
    name: 'Test Pack',
    tagline: '808-style',
    description: 'A test pack',
    tags: ['808'],
    author: 'Someone',
    source: 'https://github.com/example/repo',
    license: 'CC0-1.0',
    color: '#f80',
    // Bodies served by the tests: "kick" and "snare".
    files: [packFile('kicks/bd.wav', 'kick', BD_URL), packFile('sd.wav', 'snare', SD_URL)],
    kits: [{ id: 'main', name: 'Main', pads: [{ pad: 0, file: 'kicks/bd.wav', name: 'Kick' }, { pad: 1, file: 'sd.wav' }] }],
    version: 1,
    ...overrides,
  };
}

export type Route = (url: string, init: RequestInit) => Response | Promise<Response>;

/** A fetch that serves `routes[url]` (404 otherwise) and records requested URLs. */
export function fakeFetch(routes: Record<string, Route | string>): FetchLike & { calls: string[] } {
  const calls: string[] = [];
  const f = (async (url: string, init: RequestInit) => {
    calls.push(url);
    if (init.signal?.aborted) throw new DOMException('aborted', 'AbortError');
    const r = routes[url];
    if (r === undefined) return new Response('nope', { status: 404 });
    if (typeof r === 'string') return new Response(r, { status: 200, headers: { 'content-length': String(Buffer.byteLength(r)) } });
    return r(url, init);
  }) as FetchLike & { calls: string[] };
  f.calls = calls;
  return f;
}

/** A response whose body never ends until the request is aborted. */
export function hangingResponse(init: RequestInit): Response {
  const stream = new ReadableStream<Uint8Array>({
    start(controller) {
      controller.enqueue(new Uint8Array([1, 2, 3]));
      init.signal?.addEventListener('abort', () => controller.error(new DOMException('aborted', 'AbortError')));
    },
  });
  return new Response(stream, { status: 200 });
}
