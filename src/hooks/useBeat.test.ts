import { afterEach, describe, expect, it, vi } from 'vitest';
import { act, renderHook } from '@testing-library/react';
import { arrangementEngine } from '@/services/ArrangementEngine';
import { kitStore } from '@/services/KitStore';
import { STEP_BURST_IDLE_MS, listDrumTracks, useActiveDrumTrackId, useActiveKitPads, useBeat } from './useBeat';

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

  describe('step gestures (one undo step each)', () => {
    afterEach(() => {
      vi.useRealTimers();
      vi.restoreAllMocks();
    });

    it('a drag: begin/end wrap every toggle in one group', () => {
      const { result } = renderHook(() => useBeat());
      const begin = vi.spyOn(arrangementEngine, 'beginUndoGroup');
      const end = vi.spyOn(arrangementEngine, 'endUndoGroup');
      act(() => {
        result.current.actions.beginStepGesture('Paint steps');
        for (let s = 0; s < 4; s++) result.current.actions.toggleStep(11, s, 0.25);
        result.current.actions.endStepGesture();
      });
      expect(begin).toHaveBeenCalledTimes(1);
      expect(begin).toHaveBeenCalledWith('Paint steps');
      expect(end).toHaveBeenCalledTimes(1);
      act(() => result.current.actions.endStepGesture()); // idempotent
      expect(end).toHaveBeenCalledTimes(1);
    });

    it('wheel / key nudges merge into one group closed after the idle time', () => {
      vi.useFakeTimers();
      const { result } = renderHook(() => useBeat());
      act(() => result.current.actions.toggleStep(10, 2, 0.25));
      const begin = vi.spyOn(arrangementEngine, 'beginUndoGroup');
      const end = vi.spyOn(arrangementEngine, 'endUndoGroup');
      act(() => {
        for (let i = 0; i < 5; i++) {
          result.current.actions.nudgeStepVelocity(10, 2, 0.25, 0.5 + i * 0.05);
          vi.advanceTimersByTime(STEP_BURST_IDLE_MS - 100);
        }
      });
      expect(begin).toHaveBeenCalledTimes(1);
      expect(end).not.toHaveBeenCalled();
      act(() => vi.advanceTimersByTime(STEP_BURST_IDLE_MS));
      expect(end).toHaveBeenCalledTimes(1);
      // A new burst opens a new group.
      act(() => result.current.actions.nudgeStepVelocity(10, 2, 0.25, 0.4));
      expect(begin).toHaveBeenCalledTimes(2);
    });

    it('a nudge during a drag joins the drag; unmount closes an open group', () => {
      const { result, unmount } = renderHook(() => useBeat());
      act(() => result.current.actions.toggleStep(9, 1, 0.25));
      const begin = vi.spyOn(arrangementEngine, 'beginUndoGroup');
      const end = vi.spyOn(arrangementEngine, 'endUndoGroup');
      act(() => {
        result.current.actions.beginStepGesture('Step velocity');
        result.current.actions.nudgeStepVelocity(9, 1, 0.25, 0.3);
      });
      expect(begin).toHaveBeenCalledTimes(1);
      unmount();
      expect(end).toHaveBeenCalledTimes(1);
    });
  });

  describe('drum tracks', () => {
    it('lists drum tracks with numbered duplicate names', () => {
      const t = (id: string, name: string, type: 'drums' | 'synth') => ({ id, name, color: '#f00', instrument: { type, presetIndex: 0 } });
      expect(listDrumTracks([t('a', 'Drums', 'drums'), t('b', 'Bass', 'synth'), t('c', 'Drums', 'drums')]).map((d) => d.label)).toEqual([
        'Drums',
        'Drums 2',
      ]);
    });

    it('creates a new drum track, makes it active and switches back', () => {
      const { result } = renderHook(() => useBeat());
      const active = renderHook(() => useActiveDrumTrackId());
      const kitPads = renderHook(() => useActiveKitPads());
      act(() => result.current.actions.updatePad(0, { pitch: 5 }));
      const first = result.current.trackId!;
      let second = '';
      act(() => {
        second = result.current.actions.newDrumTrack();
      });
      expect(second).not.toBe(first);
      expect(result.current.trackId).toBe(second);
      expect(active.result.current).toBe(second);
      expect(result.current.drumTracks.map((d) => d.id)).toEqual(expect.arrayContaining([first, second]));
      expect(kitPads.result.current.pads[0].pitch).not.toBe(5);
      act(() => result.current.actions.setActiveDrumTrack(first));
      expect(result.current.trackId).toBe(first);
      expect(active.result.current).toBe(first);
      expect(kitPads.result.current.pads[0].pitch).toBe(5);
    });
  });
});
