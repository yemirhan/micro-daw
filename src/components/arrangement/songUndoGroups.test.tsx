/**
 * Electron bugs fixed in both apps during the phase-8 port (user decision 2026-11-18):
 * - a region move / resize drag, a loop-handle drag and a track-FX knob drag are ONE undo
 *   step each (each pointer move used to push its own entry);
 * - the rubber band tests regions against the rows as drawn (group header rows, collapsed
 *   groups, render order) and no longer loses its running top for rows above the band.
 * The Rust side: rune/crates/mdaw-app/src/tests/t_song_*.rs.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render } from '@testing-library/react';
import type { Track, TrackGroup } from '@/types/arrangement';
import { arrangementEngine } from '@/services/ArrangementEngine';
import { undoManager } from '@/services/UndoManager';
import { RegionBlock } from './RegionBlock';
import { TimelineRuler } from './TimelineRuler';
import { TrackEffectsPanel } from './TrackEffectsPanel';
import { regionsInBand } from './ArrangementView';
import { endOpenGestures, openGestureCount } from './gestureOwner';

const depth = () => (undoManager as unknown as { undoStack: unknown[] }).undoStack.length;

beforeEach(() => {
  vi.spyOn(console, 'warn').mockImplementation(() => undefined);
  arrangementEngine.resetToStarter();
  undoManager.clear();
  HTMLElement.prototype.setPointerCapture ??= () => undefined;
  SVGElement.prototype.setPointerCapture ??= () => undefined;
});

afterEach(() => {
  vi.restoreAllMocks();
});

function synthWithRegion() {
  const track = arrangementEngine.addTrack('synth', 0);
  arrangementEngine.addRegion(track.id, { id: 'r1', startBeat: 0, lengthBeats: 4, notes: [] });
  undoManager.clear();
  return track;
}

describe('one undo step per drag', () => {
  it('a region move drag', () => {
    const track = synthWithRegion();
    const region = arrangementEngine.getTracks().find((t) => t.id === track.id)!.regions[0];
    render(
      <RegionBlock
        region={region}
        trackColor="#f00"
        pxPerBeat={40}
        snapValue={1}
        tool="pointer"
        onMove={(id, beat) => arrangementEngine.moveRegion(track.id, id, beat)}
        onContextMenu={vi.fn()}
      />,
    );
    const block = document.querySelector<HTMLElement>('[data-region-id="r1"]')!;
    fireEvent.pointerDown(block, { button: 0, clientX: 20, pointerId: 1 });
    for (const x of [60, 100, 140]) fireEvent.pointerMove(block, { clientX: x, pointerId: 1 });
    fireEvent.pointerUp(block, { clientX: 140, pointerId: 1 });
    expect(arrangementEngine.getTracks().find((t) => t.id === track.id)!.regions[0].startBeat).toBe(3);
    expect(depth()).toBe(1);
    expect(undoManager.undoLabel()).toBe('Move Region');
    // A press without movement leaves no step.
    fireEvent.pointerDown(block, { button: 0, clientX: 20, pointerId: 1 });
    fireEvent.pointerUp(block, { clientX: 20, pointerId: 1 });
    expect(depth()).toBe(1);
  });

  it('a loop handle drag', () => {
    arrangementEngine.setLoopMarkers(8, 16);
    undoManager.clear();
    const { container } = render(
      <TimelineRuler
        lengthBeats={64}
        pxPerBeat={10}
        snapValue={1}
        loopEnabled
        loopMarkers={{ startBeat: 8, endBeat: 16 }}
        onSeek={vi.fn()}
        onLoopMarkersChange={(a, b) => arrangementEngine.setLoopMarkers(a, b)}
      />,
    );
    const handle = container.querySelector<HTMLElement>('[title="Drag loop end"]')!;
    fireEvent.pointerDown(handle, { button: 0, clientX: 160, pointerId: 1 });
    for (const x of [170, 180, 200]) fireEvent.pointerMove(window, { clientX: x });
    fireEvent.pointerUp(window, { clientX: 200 });
    expect(arrangementEngine.getLoopMarkers()).toEqual({ startBeat: 8, endBeat: 20 });
    expect(depth()).toBe(1);
    expect(undoManager.undoLabel()).toBe('Set Loop Markers');
  });

  it('a track-FX knob drag', async () => {
    const track = synthWithRegion();
    const t = arrangementEngine.getTracks().find((x) => x.id === track.id)!;
    arrangementEngine.setTrackEffect(track.id, 'eq', { enabled: true });
    undoManager.clear();
    const { getByRole } = render(<TrackEffectsPanel track={structuredClone(t)} onClose={vi.fn()} />);
    const knob = getByRole('slider', { name: 'Low' });
    fireEvent.pointerDown(knob, { button: 0, clientY: 200, pointerId: 1 });
    for (const y of [190, 180, 170, 160]) {
      fireEvent.pointerMove(knob, { clientY: y, pointerId: 1 });
      await act(() => new Promise((r) => requestAnimationFrame(() => r(undefined))));
    }
    fireEvent.pointerUp(knob, { clientY: 160, pointerId: 1 });
    expect(arrangementEngine.getTracks().find((x) => x.id === track.id)!.effects!.eq.low).toBe(5); // 40 px of 200 × 24 dB = 4.8, the 0.5 step
    expect(depth()).toBe(1);
    expect(undoManager.undoLabel()).toBe('Change Track Effect');
  });
});

/**
 * Phase-8 review #1 / #11 (Electron bugs fixed in both apps, user decision 2026-11-18): a drag
 * whose target goes away mid-gesture closed nothing, so every later edit was swallowed into
 * its open undo group; a knob released before its animation frame dropped the last movement.
 */
describe('interrupted drags close their undo group', () => {
  const grouping = () => undoManager.isGrouping();
  /** One undoable edit after the gesture: it must be its own entry. */
  const laterEditIsItsOwnStep = () => {
    const before = depth();
    arrangementEngine.addMarker('After', 2);
    expect(depth()).toBe(before + 1);
    expect(undoManager.undoLabel()).not.toBe('Move Region');
  };

  function regionBlock(track: { id: string }) {
    const region = arrangementEngine.getTracks().find((t) => t.id === track.id)!.regions[0];
    return render(
      <RegionBlock
        region={region}
        trackColor="#f00"
        pxPerBeat={40}
        snapValue={1}
        tool="pointer"
        onMove={(id, beat) => arrangementEngine.moveRegion(track.id, id, beat)}
        onContextMenu={vi.fn()}
      />,
    );
  }

  it('a region deleted (or the view switched) mid-drag: unmount ends the drag', () => {
    const track = synthWithRegion();
    const { unmount } = regionBlock(track);
    const block = document.querySelector<HTMLElement>('[data-region-id="r1"]')!;
    fireEvent.pointerDown(block, { button: 0, clientX: 20, pointerId: 1 });
    fireEvent.pointerMove(block, { clientX: 100, pointerId: 1 });
    expect(grouping()).toBe(true);
    unmount();
    expect(grouping()).toBe(false);
    expect(openGestureCount()).toBe(0);
    expect(depth()).toBe(1);
    expect(undoManager.undoLabel()).toBe('Move Region');
    laterEditIsItsOwnStep();
  });

  it('pointercancel / lost capture end a region drag', () => {
    const track = synthWithRegion();
    regionBlock(track);
    const block = document.querySelector<HTMLElement>('[data-region-id="r1"]')!;
    fireEvent.pointerDown(block, { button: 0, clientX: 20, pointerId: 1 });
    fireEvent.pointerMove(block, { clientX: 100, pointerId: 1 });
    fireEvent.pointerCancel(block, { pointerId: 1 });
    expect(grouping()).toBe(false);
    fireEvent.pointerDown(block, { button: 0, clientX: 20, pointerId: 1 });
    fireEvent.lostPointerCapture(block, { pointerId: 1 });
    expect(grouping()).toBe(false);
  });

  it('a destructive key (Delete) ends the drag first: the delete is its own step', () => {
    const track = synthWithRegion();
    regionBlock(track);
    const block = document.querySelector<HTMLElement>('[data-region-id="r1"]')!;
    fireEvent.pointerDown(block, { button: 0, clientX: 20, pointerId: 1 });
    fireEvent.pointerMove(block, { clientX: 100, pointerId: 1 });
    act(() => endOpenGestures()); // ArrangementView's Delete / Backspace, App's L
    arrangementEngine.removeRegion(track.id, 'r1');
    expect(grouping()).toBe(false);
    expect(depth()).toBe(2);
    expect(undoManager.undoLabel()).not.toBe('Move Region');
    // The rest of the drag moves nothing.
    fireEvent.pointerMove(block, { clientX: 140, pointerId: 1 });
    fireEvent.pointerUp(block, { clientX: 140, pointerId: 1 });
    expect(depth()).toBe(2);
  });

  function loopRuler(loopEnabled: boolean) {
    return (
      <TimelineRuler
        lengthBeats={64}
        pxPerBeat={10}
        snapValue={1}
        loopEnabled={loopEnabled}
        loopMarkers={arrangementEngine.getLoopMarkers()}
        onSeek={vi.fn()}
        onLoopMarkersChange={(a, b) => arrangementEngine.setLoopMarkers(a, b)}
      />
    );
  }

  it('L mid loop-handle drag: the handles go and the drag ends', () => {
    arrangementEngine.setLoopMarkers(8, 16);
    undoManager.clear();
    const { container, rerender } = render(loopRuler(true));
    const handle = container.querySelector<HTMLElement>('[title="Drag loop end"]')!;
    fireEvent.pointerDown(handle, { button: 0, clientX: 160, pointerId: 1 });
    fireEvent.pointerMove(window, { clientX: 200 });
    expect(grouping()).toBe(true);
    rerender(loopRuler(false));
    expect(grouping()).toBe(false);
    expect(depth()).toBe(1);
    // The window listeners went with it.
    fireEvent.pointerMove(window, { clientX: 300 });
    expect(arrangementEngine.getLoopMarkers()).toEqual({ startBeat: 8, endBeat: 20 });
    laterEditIsItsOwnStep();
  });

  it('pointercancel and unmount end a loop-handle drag', () => {
    arrangementEngine.setLoopMarkers(8, 16);
    undoManager.clear();
    const { container, unmount } = render(loopRuler(true));
    const handle = () => container.querySelector<HTMLElement>('[title="Drag loop end"]')!;
    fireEvent.pointerDown(handle(), { button: 0, clientX: 160, pointerId: 1 });
    fireEvent.pointerMove(window, { clientX: 180 });
    fireEvent.pointerCancel(window);
    expect(grouping()).toBe(false);
    fireEvent.pointerDown(handle(), { button: 0, clientX: 180, pointerId: 1 });
    fireEvent.pointerMove(window, { clientX: 200 });
    unmount();
    expect(grouping()).toBe(false);
    expect(depth()).toBe(2);
  });

  async function fxKnob() {
    const track = synthWithRegion();
    const t = arrangementEngine.getTracks().find((x) => x.id === track.id)!;
    arrangementEngine.setTrackEffect(track.id, 'eq', { enabled: true });
    undoManager.clear();
    const view = render(<TrackEffectsPanel track={structuredClone(t)} onClose={vi.fn()} />);
    return { track, view, knob: view.getByRole('slider', { name: 'Low' }) };
  }
  const low = (id: string) => arrangementEngine.getTracks().find((x) => x.id === id)!.effects!.eq.low;
  const frame = () => act(() => new Promise((r) => requestAnimationFrame(() => r(undefined))));

  it('the FX drawer closed mid knob drag: the drag ends with its last value', async () => {
    const { track, view, knob } = await fxKnob();
    fireEvent.pointerDown(knob, { button: 0, clientY: 200, pointerId: 1 });
    fireEvent.pointerMove(knob, { clientY: 180, pointerId: 1 });
    await frame();
    fireEvent.pointerMove(knob, { clientY: 160, pointerId: 1 }); // still waiting for its frame
    expect(grouping()).toBe(true);
    view.unmount();
    expect(grouping()).toBe(false);
    expect(low(track.id)).toBe(5);
    expect(depth()).toBe(1);
    expect(undoManager.undoLabel()).toBe('Change Track Effect');
    laterEditIsItsOwnStep();
  });

  it('a knob released before its animation frame keeps the final movement (#11)', async () => {
    const { track, knob } = await fxKnob();
    fireEvent.pointerDown(knob, { button: 0, clientY: 200, pointerId: 1 });
    fireEvent.pointerMove(knob, { clientY: 160, pointerId: 1 });
    fireEvent.pointerUp(knob, { clientY: 160, pointerId: 1 }); // before the frame
    expect(low(track.id)).toBe(5);
    expect(depth()).toBe(1);
    expect(undoManager.undoLabel()).toBe('Change Track Effect');
    await frame(); // the cancelled frame sends nothing more
    expect(depth()).toBe(1);
  });
});

describe('regionsInBand (rubber band rows as drawn)', () => {
  const track = (id: string, regions: [number, number][], automation = 0): Track =>
    ({
      id,
      name: id,
      color: '#fff',
      volume: 0,
      pan: 0,
      muted: false,
      solo: false,
      instrument: { type: 'synth', presetIndex: 0 },
      regions: regions.map(([startBeat, lengthBeats], i) => ({ id: `${id}-r${i}`, startBeat, lengthBeats, notes: [] })),
      automation: Array.from({ length: automation }, () => ({ parameter: 'volume', points: [], visible: true })),
    }) as unknown as Track;
  const tracks = [track('a', [[0, 4]]), track('b', [[0, 4]], 1), track('c', [[0, 4]])];

  it('selects a region on a row below a row above the band', () => {
    // Rows: a 0–64, b 64–176 (one lane), c 176–240. A band over c's region only.
    expect(regionsInBand(tracks, [], { x1: 10, x2: 50, y1: 190, y2: 200 }, 30)).toEqual([
      { regionId: 'c-r0', trackId: 'c' },
    ]);
  });

  it('counts group header rows, uses render order and skips collapsed groups', () => {
    const groups: TrackGroup[] = [{ id: 'g', name: 'G', color: '#fff', trackIds: ['c'], collapsed: false }];
    // Rows: group 0–28, c 28–92, a 92–156, b 156–268.
    expect(regionsInBand(tracks, groups, { x1: 0, x2: 10, y1: 30, y2: 40 }, 30)).toEqual([{ regionId: 'c-r0', trackId: 'c' }]);
    expect(regionsInBand(tracks, groups, { x1: 0, x2: 10, y1: 100, y2: 110 }, 30)).toEqual([{ regionId: 'a-r0', trackId: 'a' }]);
    const collapsed = [{ ...groups[0], collapsed: true }];
    // Rows: group 0–28, a 28–92, b 92–204; c hidden.
    expect(regionsInBand(tracks, collapsed, { x1: 0, x2: 10, y1: 30, y2: 40 }, 30)).toEqual([{ regionId: 'a-r0', trackId: 'a' }]);
  });
});
