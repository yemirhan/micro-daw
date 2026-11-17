import { describe, expect, it, vi } from 'vitest';
import { act, renderHook } from '@testing-library/react';
import { arrangementEngine } from '@/services/ArrangementEngine';
import { kitStore } from '@/services/KitStore';
import { useBeat } from './useBeat';

describe('useBeat', () => {
  it('creates the drum track on first edit and reflects kit changes', () => {
    const { result } = renderHook(() => useBeat());
    act(() => result.current.actions.assignFile(2, { path: '/s/Break.wav', name: 'Break' }));
    const id = arrangementEngine.getActiveDrumTrackId();
    expect(id).not.toBeNull();
    expect(result.current.trackId).toBe(id);
    expect(result.current.kit.pads[2].sample).toEqual({ source: { kind: 'file', path: '/s/Break.wav' }, name: 'Break' });
  });

  it('chops a sample to consecutive pads as one undo step', () => {
    const { result } = renderHook(() => useBeat());
    act(() => result.current.actions.assignFile(4, { path: '/s/Loop.wav', name: 'Loop' }));
    const sample = result.current.kit.pads[4].sample!;
    const begin = vi.spyOn(arrangementEngine, 'beginUndoGroup');
    const end = vi.spyOn(arrangementEngine, 'endUndoGroup');
    act(() =>
      result.current.actions.chopToPads(4, sample, [
        { startSec: 0, endSec: 0.5 },
        { startSec: 0.5, endSec: 1 },
        { startSec: 1, endSec: 2 },
      ]),
    );
    const pads = result.current.kit.pads;
    expect(pads[4]).toMatchObject({ startSec: 0, endSec: 0.5, sample: { name: 'Loop' } });
    expect(pads[5]).toMatchObject({ startSec: 0.5, endSec: 1, sample: { source: { kind: 'file', path: '/s/Loop.wav' }, name: 'Loop 2' } });
    expect(pads[6]).toMatchObject({ startSec: 1, endSec: 2, sample: { name: 'Loop 3' } });
    expect(begin).toHaveBeenCalledTimes(1);
    expect(end).toHaveBeenCalledTimes(1);
  });

  it('copies and pastes a pad with its settings', () => {
    const { result } = renderHook(() => useBeat());
    act(() => result.current.actions.updatePad(0, { pitch: 7, gainDb: -3 }));
    act(() => result.current.actions.copyPad(0));
    expect(result.current.hasClipboard).toBe(true);
    act(() => result.current.actions.pastePad(9));
    expect(result.current.kit.pads[9]).toMatchObject({ pitch: 7, gainDb: -3, sample: result.current.kit.pads[0].sample });
  });

  it('selects pads and toggles steps in the pattern', () => {
    const { result } = renderHook(() => useBeat());
    act(() => result.current.actions.selectPad(7));
    expect(result.current.selectedPad).toBe(7);
    act(() => result.current.actions.clearPattern());
    act(() => result.current.actions.toggleStep(7, 3, 0.25));
    const region = result.current.patternRegion;
    expect(region?.notes.some((n) => n.note === 36 + 7)).toBe(true);
    const version = result.current.patternVersion;
    act(() => result.current.actions.toggleStep(7, 3, 0.25));
    expect(result.current.patternVersion).not.toBe(version);
  });

  it('saves the current kit to the kit store', () => {
    const { result } = renderHook(() => useBeat());
    act(() => result.current.actions.saveKit('Dusty'));
    expect(kitStore.list().some((k) => k.name === 'Dusty')).toBe(true);
    expect(result.current.userKits.some((k) => k.name === 'Dusty')).toBe(true);
  });
});
