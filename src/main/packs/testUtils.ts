/** Test helpers for the pack main-process code (not shipped: only imported by *.test.ts). */
import type { PackManifest } from '../../types/packs';
import type { FetchLike } from './download';

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
    files: [
      { path: 'kicks/bd.wav', url: 'https://raw.githubusercontent.com/o/r/sha/bd.wav' },
      { path: 'sd.wav', url: 'https://raw.githubusercontent.com/o/r/sha/sd.wav' },
    ],
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
