import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { synthesizeClassicVoice } from './synth';

/** Fake worker (jsdom has none): answers like classic.worker.ts, or fails to load. */
const g = globalThis as typeof globalThis & { __fakeWorkers?: FakeWorker[]; __workerMode?: 'ok' | 'error' };
class FakeWorker {
  onmessage: ((e: { data: unknown }) => void) | null = null;
  onerror: ((e: { message: string; preventDefault(): void }) => void) | null = null;
  onmessageerror: (() => void) | null = null;
  posted: { id: number; machine: 'tr808'; voice: string }[] = [];
  terminated = false;
  constructor() {
    (g.__fakeWorkers ??= []).push(this);
  }
  postMessage(msg: { id: number; machine: 'tr808'; voice: string }) {
    this.posted.push(msg);
    setTimeout(() => {
      if (g.__workerMode === 'error') {
        this.onerror?.({ message: 'failed to load', preventDefault() {} });
        return;
      }
      this.onmessage?.({ data: { id: msg.id, samples: synthesizeClassicVoice(msg.machine, msg.voice) } });
    }, 0);
  }
  terminate() {
    this.terminated = true;
  }
}

vi.mock('./classic.worker?worker&inline', () => ({ default: FakeWorker }));

async function load() {
  vi.resetModules();
  const render = await import('./render');
  render.resetClassicWorkerForTests();
  return render;
}

beforeEach(() => {
  g.__fakeWorkers = [];
  g.__workerMode = 'ok';
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('classic synthesis off the main thread (review E2)', () => {
  it('without Worker support (tests, old runtimes) it synthesizes on the main thread', async () => {
    vi.stubGlobal('Worker', undefined);
    const { synthesizeClassicVoiceAsync } = await load();
    const data = await synthesizeClassicVoiceAsync('tr808', 'kick');
    expect(data).toEqual(synthesizeClassicVoice('tr808', 'kick'));
    expect(g.__fakeWorkers).toHaveLength(0);
  });

  it('renders in the worker (one worker for every voice) with identical samples', async () => {
    vi.stubGlobal('Worker', FakeWorker);
    const { synthesizeClassicVoiceAsync } = await load();
    const [kick, snare] = await Promise.all([synthesizeClassicVoiceAsync('tr808', 'kick'), synthesizeClassicVoiceAsync('tr808', 'snare')]);
    expect(g.__fakeWorkers).toHaveLength(1);
    expect(g.__fakeWorkers![0].posted.map((m) => m.voice)).toEqual(['kick', 'snare']);
    expect(kick).toEqual(synthesizeClassicVoice('tr808', 'kick'));
    expect(snare).toEqual(synthesizeClassicVoice('tr808', 'snare'));
  });

  it('a worker that fails to load: pending and later voices fall back to the main thread', async () => {
    vi.stubGlobal('Worker', FakeWorker);
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    g.__workerMode = 'error';
    const { synthesizeClassicVoiceAsync } = await load();
    const data = await synthesizeClassicVoiceAsync('tr808', 'clap');
    expect(data).toEqual(synthesizeClassicVoice('tr808', 'clap'));
    expect(g.__fakeWorkers![0].terminated).toBe(true);
    await synthesizeClassicVoiceAsync('tr808', 'kick');
    expect(g.__fakeWorkers).toHaveLength(1); // not retried
  });

  it('unknown voices reject (worker error message passed through)', async () => {
    vi.stubGlobal('Worker', FakeWorker);
    const { synthesizeClassicVoiceAsync } = await load();
    const w = () => g.__fakeWorkers![0];
    const p = synthesizeClassicVoiceAsync('tr808', 'kick');
    w().onmessage?.({ data: { id: w().posted[0].id, error: 'Unknown classic voice: tr808/nope' } });
    await expect(p).rejects.toThrow(/Unknown classic voice/);
  });
});
