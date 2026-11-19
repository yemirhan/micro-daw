import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, renderHook } from '@testing-library/react';
import '@/test/toneMock';

vi.mock('@/services/SampleProcessor', () => ({
  generateSignal: vi.fn(async () => ({
    duration: 1,
    length: 48000,
    sampleRate: 48000,
    numberOfChannels: 1,
    getChannelData: () => new Float32Array(48000),
  })),
  processAudio: vi.fn(),
}));

import { resetSampleLibrarySession, useSampleLibrary } from './useSampleLibrary';
import { DEFAULT_SIGNAL_PARAMS } from '@/types/samples';

describe('useSampleLibrary session', () => {
  beforeEach(() => {
    localStorage.clear();
    resetSampleLibrarySession();
  });

  it('keeps generated samples when the Samples view unmounts and mounts again', async () => {
    const first = renderHook(() => useSampleLibrary());
    await act(async () => {
      await first.result.current.generateSample({ ...DEFAULT_SIGNAL_PARAMS });
    });
    expect(first.result.current.entries.map((e) => e.sample.name)).toEqual(['sine 440Hz 1s']);
    first.unmount();

    // Before the fix the list came back empty: the in-memory sample has no path, so it is
    // not in localStorage, and the hook's state started over.
    const second = renderHook(() => useSampleLibrary());
    expect(second.result.current.entries.map((e) => e.sample.name)).toEqual(['sine 440Hz 1s']);
    // The selection is the view's own state: it starts empty on a new mount.
    expect(second.result.current.selectedId).toBeNull();
    // Still not persisted (empty path).
    expect(JSON.parse(localStorage.getItem('micro-daw-sample-library') ?? '[]')).toEqual([]);
  });
});

describe('useSampleLibrary restore', () => {
  beforeEach(() => {
    localStorage.clear();
    resetSampleLibrarySession();
  });

  it('keeps samples added while the stored library is still loading', async () => {
    const { sampleManager } = await import('@/services/SampleManager');
    localStorage.setItem(
      'micro-daw-sample-library',
      JSON.stringify([{ sampleId: 'stored-1', samplePath: '/x/kick.wav', sampleName: 'Kick', addedAt: 1 }]),
    );
    let release: () => void = () => {};
    const gate = new Promise<void>((r) => {
      release = r;
    });
    const spy = vi.spyOn(sampleManager, 'loadFromPathWithId').mockImplementation(async (id, path) => {
      await gate;
      return { id, name: 'kick', path, durationSeconds: 1, sampleRate: 48000, channels: 1 };
    });
    const h = renderHook(() => useSampleLibrary());
    // A sample generated before the restore finished.
    await act(async () => {
      await h.result.current.generateSample({ ...DEFAULT_SIGNAL_PARAMS });
    });
    await act(async () => {
      release();
      await gate;
      await Promise.resolve();
    });
    // Before the fix `setEntries(loaded)` replaced the list: only "Kick" was left.
    expect(h.result.current.entries.map((e) => e.sample.name)).toEqual(['Kick', 'sine 440Hz 1s']);
    spy.mockRestore();
  });
});
