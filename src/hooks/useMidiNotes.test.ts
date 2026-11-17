import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, renderHook } from '@testing-library/react';
import { getSoundingNotes } from '@/test/toneMock';

// Services are singletons: load fresh copies per test so Tone nodes are
// created after resetToneMock() and show up in the mock registry.
async function load() {
  vi.resetModules();
  const { audioEngine } = await import('@/services/AudioEngine');
  const { useMidiNotes } = await import('@/hooks/useMidiNotes');
  await audioEngine.start();
  return { audioEngine, useMidiNotes };
}

describe('useMidiNotes (MIDI keyboard path)', () => {
  beforeEach(() => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
  });

  it('plays and releases a keyboard note', async () => {
    const { useMidiNotes } = await load();
    const { result } = renderHook(() => useMidiNotes());

    act(() => result.current.callbacks.onNoteOn(60, 0.8, 1));
    expect([...result.current.activeNotes.keys()]).toEqual([60]);
    expect(result.current.getSoundingNotes()).toEqual([60]);
    // Real AudioEngine underneath: something is sounding (engine internals may vary).
    expect(getSoundingNotes().length).toBeGreaterThan(0);

    act(() => result.current.callbacks.onNoteOff(60, 1));
    expect(result.current.activeNotes.size).toBe(0);
    expect(result.current.getSoundingNotes()).toEqual([]);
    expect(getSoundingNotes()).toHaveLength(0);
  });
});
