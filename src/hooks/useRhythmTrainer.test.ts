import { describe, expect, it, vi } from 'vitest';
import { act, renderHook } from '@testing-library/react';
import { useRhythmTrainer } from '@/hooks/useRhythmTrainer';
import { noteEventBus } from '@/services/NoteEventBus';
import { advanceClock, instancesOf, toneMock } from '@/test/toneMock';
import type { DrumPadId } from '@/types/drums';

type TickFn = (time: number) => void;

function clockOf() {
  const clocks = instancesOf('Clock');
  const clock = clocks[clocks.length - 1];
  return {
    node: clock,
    tick: clock.__args[0] as TickFn,
    frequency: clock.__args[1] as number,
    startTime: (clock.start as unknown as { mock: { calls: unknown[][] } }).mock.calls[0][0] as number,
  };
}

const hitKey = () => noteEventBus.emit({ type: 'noteon', note: 60, velocity: 0.8, channel: 1, time: 0 });
const hitPad = (padId: DrumPadId) => noteEventBus.emit({ type: 'drum', padId, note: 36, velocity: 0.8, time: 0 });

describe('useRhythmTrainer', () => {
  it('runs on its own clock and never touches the arrangement transport', () => {
    const { result } = renderHook(() => useRhythmTrainer({ bpm: 100 }));
    act(() => result.current.start());
    expect(result.current.isRunning).toBe(true);
    expect(toneMock.transport.start).not.toHaveBeenCalled();
    expect(clockOf().frequency).toBeCloseTo(100 / 60);
    act(() => result.current.stop());
    expect(toneMock.transport.stop).not.toHaveBeenCalled();
    expect(clockOf().node.disposed).toBe(true);
  });

  it('eighth-note subdivision doubles the tick rate', () => {
    const { result } = renderHook(() => useRhythmTrainer({ bpm: 90, subdivision: '8n' }));
    act(() => result.current.start());
    expect(clockOf().frequency).toBeCloseTo(3);
  });

  it('scores MIDI/on-screen hits from the note bus and completes after the last beat', () => {
    const onComplete = vi.fn();
    const { result } = renderHook(() => useRhythmTrainer({ bpm: 120, totalBeats: 4, onComplete }));
    act(() => result.current.start());
    const { tick, startTime } = clockOf();
    const interval = 0.5;
    for (let b = 0; b < 4; b++) {
      const beatTime = startTime + b * interval;
      act(() => tick(beatTime));
      expect(result.current.currentBeat).toBe(b);
      // hit 10ms late on beats 0-2, skip beat 3
      if (b < 3) {
        advanceClock(beatTime + 0.01 - toneMock.clock.now);
        act(() => hitKey());
      }
    }
    expect(result.current.feedbackHistory.map((f) => f.timing)).toEqual(['perfect', 'perfect', 'perfect']);
    expect(onComplete).not.toHaveBeenCalled();
    act(() => tick(startTime + 4 * interval)); // one tick after the last beat
    expect(onComplete).toHaveBeenCalledTimes(1);
    expect(onComplete.mock.calls[0][0]).toBeCloseTo(0.75); // the skipped beat counts
    expect(result.current.isRunning).toBe(false);
    // the metronome clicked on the 4 beats but not on the closing tick
    expect(instancesOf('Synth')[0].triggerAttackRelease).toHaveBeenCalledTimes(4);
  });

  it('ignores input while stopped and after completion', () => {
    const { result } = renderHook(() => useRhythmTrainer({ bpm: 120 }));
    act(() => hitKey());
    expect(result.current.feedbackHistory).toEqual([]);
  });

  it('drum mode judges the pad against the pattern', () => {
    const { result } = renderHook(() =>
      useRhythmTrainer({ bpm: 120, totalBeats: 2, pattern: [[0], [1]], input: 'drums' }),
    );
    act(() => result.current.start());
    const { tick, startTime } = clockOf();
    act(() => tick(startTime));
    advanceClock(startTime - toneMock.clock.now);
    act(() => hitPad(1)); // wrong pad on beat 1
    act(() => hitPad(0));
    expect(result.current.feedbackHistory.map((f) => f.timing)).toEqual(['miss', 'perfect']);
    act(() => hitKey()); // keys don't count in drum mode (not a pad note)
    expect(result.current.feedbackHistory).toHaveLength(2);
  });

  it('cleans up the clock and click synth on unmount', () => {
    const { result, unmount } = renderHook(() => useRhythmTrainer({ bpm: 120 }));
    act(() => result.current.start());
    const { node } = clockOf();
    unmount();
    expect(node.disposed).toBe(true);
    expect(instancesOf('Synth')[0].disposed).toBe(true);
  });
});
