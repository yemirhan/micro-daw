import { describe, expect, it } from 'vitest';
import { act, render, renderHook } from '@testing-library/react';
import { padToNote } from '@/types/sampler';
import { arrangementEngine } from '@/services/ArrangementEngine';
import { toneMock } from '@/test/toneMock';
import {
  arrangementActions,
  useArrangement,
  useArrangementTransport,
  usePlaybackPosition,
  type UseArrangementOptions,
} from './useArrangement';
import { useBeat, useBeatActions } from './useBeat';

/** Fire the engine's playhead callbacks (what its rAF loop does every frame). */
function positionFrame(beat: number) {
  const cbs = (arrangementEngine as unknown as { positionCallbacks: ((b: number) => void)[] }).positionCallbacks;
  for (const cb of cbs) cb(beat);
}

/** Loop-record and hit `n` pads (each one merged into the pattern, i.e. an engine state change). */
let hitCount = 0;
function recordHits(n: number) {
  for (let i = 0; i < n; i++, hitCount++) {
    // a new step (and pad) each time: every hit adds a note
    toneMock.transport.seconds = (hitCount % 8) * 0.125;
    arrangementEngine.captureNoteOn(padToNote(hitCount % 4), 0.9, true);
    arrangementEngine.captureNoteOff(padToNote(hitCount % 4));
  }
}

function setup() {
  hitCount = 0;
  arrangementEngine.stop();
  arrangementEngine.resetToStarter();
  arrangementEngine.setLoopSettings({ bars: 1, countInBars: 0, metronome: false, overdub: true, quantize: '1/16' });
}

describe('fine-grained arrangement subscriptions (review E4)', () => {
  it('a host using actions + transport only does not re-render on recorded hits or playhead frames', () => {
    setup();
    let renders = 0;
    function Host({ opts }: { opts: UseArrangementOptions }) {
      renders++;
      useArrangement(opts);
      useBeatActions();
      return null;
    }
    render(<Host opts={{ position: false, tracks: false, beat: false }} />);
    act(() => arrangementEngine.startLoopRecording());
    const before = renders;
    for (let i = 0; i < 10; i++) act(() => recordHits(1));
    for (let f = 0; f < 30; f++) act(() => positionFrame(f / 10));
    expect(renders - before).toBe(0);
    act(() => arrangementEngine.stopLoopRecording());
    expect(renders - before).toBe(1); // the transport state changed: one render
  });

  it('the legacy default (everything) still re-renders on hits and frames', () => {
    setup();
    let renders = 0;
    function Host() {
      renders++;
      useArrangement();
      return null;
    }
    render(<Host />);
    act(() => arrangementEngine.startLoopRecording());
    const before = renders;
    for (let i = 0; i < 3; i++) act(() => recordHits(1));
    expect(renders - before).toBeGreaterThanOrEqual(3);
    const afterHits = renders;
    toneMock.transport.seconds = 0.75;
    act(() => positionFrame(1.5));
    expect(renders - afterHits).toBe(1);
    act(() => arrangementEngine.stopLoopRecording());
  });

  it('opted-out slices still read fresh values whenever the host renders', () => {
    setup();
    const { result, rerender } = renderHook(() => useArrangement({ tracks: false, beat: false, position: false }));
    const id = arrangementEngine.getActiveDrumTrackId()!;
    act(() => arrangementEngine.toggleStep(id, 0, 0, 0.25));
    rerender();
    expect(result.current.patternRegion?.notes).toHaveLength(1);
    expect(result.current.tracks.length).toBeGreaterThan(0);
  });

  it('usePlaybackPosition follows the playhead; transport snapshots keep identity on hits', () => {
    setup();
    const pos = renderHook(() => usePlaybackPosition());
    const transport = renderHook(() => useArrangementTransport());
    act(() => arrangementEngine.startLoopRecording());
    const snap = transport.result.current;
    act(() => recordHits(4));
    expect(transport.result.current).toBe(snap);
    act(() => positionFrame(2.25));
    toneMock.transport.seconds = 2.25 * 0.5;
    act(() => positionFrame(2.25));
    expect(pos.result.current).toBeCloseTo(2.25, 6);
    act(() => arrangementEngine.stopLoopRecording());
    expect(pos.result.current).toBe(0);
  });

  it('metronome toggle is engine state (no local copy): every subscriber sees it', () => {
    setup();
    const a = renderHook(() => useArrangement({ position: false, tracks: false, beat: false }));
    const b = renderHook(() => useArrangementTransport());
    expect(a.result.current.metronomeOn).toBe(false);
    act(() => arrangementActions.toggleMetronome());
    expect(a.result.current.metronomeOn).toBe(true);
    expect(b.result.current.metronomeOn).toBe(true);
    act(() => arrangementActions.toggleMetronome());
  });

  it('useBeat re-renders once per recorded hit and never on playhead frames', () => {
    setup();
    let renders = 0;
    renderHook(() => {
      renders++;
      return useBeat();
    });
    act(() => arrangementEngine.startLoopRecording());
    let before = renders;
    for (let f = 0; f < 30; f++) act(() => positionFrame(f / 10));
    expect(renders - before).toBe(0);
    before = renders;
    for (let i = 0; i < 5; i++) act(() => recordHits(1));
    expect(renders - before).toBe(5);
    act(() => arrangementEngine.stopLoopRecording());
  });
});
