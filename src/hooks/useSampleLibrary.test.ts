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
