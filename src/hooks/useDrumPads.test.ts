import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, renderHook } from '@testing-library/react';
import { PAD_FLASH_MS, padFlash, useActivePads } from './useDrumPads';

describe('padFlash store (review E4: pad flashes do not re-render App)', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => {
    vi.runOnlyPendingTimers();
    vi.useRealTimers();
  });

  it('lights a pad for PAD_FLASH_MS; only subscribers re-render', () => {
    let renders = 0;
    const { result } = renderHook(() => {
      renders++;
      return useActivePads();
    });
    const before = renders;
    act(() => padFlash.flash(3));
    expect(result.current.has(3)).toBe(true);
    expect(renders).toBe(before + 1);
    // A re-hit while lit extends the flash without another render.
    act(() => padFlash.flash(3));
    expect(renders).toBe(before + 1);
    act(() => vi.advanceTimersByTime(PAD_FLASH_MS + 1));
    expect(result.current.has(3)).toBe(false);
    expect(renders).toBe(before + 2);
  });

  it('flashing with no subscriber is a plain function call (nothing to re-render)', () => {
    const listener = vi.fn();
    padFlash.flash(1);
    const off = padFlash.subscribe(listener);
    vi.advanceTimersByTime(PAD_FLASH_MS + 1);
    off();
    expect(listener).toHaveBeenCalledTimes(1);
    expect(padFlash.getSnapshot().has(1)).toBe(false);
  });
});
