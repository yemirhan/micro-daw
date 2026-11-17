import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import type { RegionNote, Track } from '@/types/arrangement';
import { DEFAULT_TRACK_EFFECTS } from '@/types/effects';
import { useState } from 'react';
import { PianoRollEditor, type PianoRollEditorProps } from './PianoRollEditor';

const note = (n: number, startBeat: number, durationBeats = 1, velocity = 0.8): RegionNote => ({ note: n, startBeat, durationBeats, velocity, isDrum: false });

function makeTrack(notes: RegionNote[], type: 'synth' | 'drums' = 'synth'): Track {
  return {
    id: 't1',
    name: 'Lead',
    instrument: { type, presetIndex: 0 },
    regions: [{ id: 'r1', startBeat: 4, lengthBeats: 8, notes, name: 'Riff' }],
    volume: 0,
    pan: 0,
    muted: false,
    solo: false,
    color: 'oklch(0.65 0.2 250)',
    effects: structuredClone(DEFAULT_TRACK_EFFECTS),
  };
}

/** Controlled like App: onEditNotes writes the notes back into the tracks. */
function Harness({ initial, ...props }: PianoRollEditorProps & { initial: Track[] }) {
  const [tracks, setTracks] = useState(initial);
  return (
    <PianoRollEditor
      {...props}
      tracks={tracks}
      onEditNotes={(trackId, regionId, notes, label, len) => {
        props.onEditNotes(trackId, regionId, notes, label, len);
        setTracks((ts) =>
          ts.map((t) =>
            t.id !== trackId
              ? t
              : { ...t, regions: t.regions.map((r) => (r.id === regionId ? { ...r, notes, lengthBeats: Math.max(r.lengthBeats, len ?? 0) } : r)) },
          ),
        );
      }}
    />
  );
}

function setup(notes: RegionNote[], overrides: Partial<PianoRollEditorProps> = {}) {
  const props: PianoRollEditorProps = {
    trackId: 't1',
    regionId: 'r1',
    tracks: [makeTrack(notes)],
    positionBeats: 0,
    transportState: 'stopped',
    loopEnabled: false,
    onClose: vi.fn(),
    onEditNotes: vi.fn(),
    onPreviewNote: vi.fn(),
    onPlayFrom: vi.fn(),
    onStop: vi.fn(),
    onSeek: vi.fn(),
    onSetLoop: vi.fn(),
    onToggleLoop: vi.fn(),
    ...overrides,
  };
  const utils = render(<Harness {...props} initial={props.tracks} />);
  const root = screen.getByTestId('piano-roll-editor');
  const grid = screen.getByTestId('piano-roll-grid').firstElementChild as HTMLElement;
  return { props, root, grid, ...utils };
}

// Grid geometry in jsdom: rect at 0,0, no scroll, default zoom.
const PX = 64; // PR_DEFAULT_PX_PER_BEAT
const ROW = 16; // PR_DEFAULT_ROW_HEIGHT
const at = (beat: number, pitch: number) => ({ clientX: beat * PX, clientY: (127 - pitch) * ROW + ROW / 2 });

beforeAll(() => {
  // jsdom lacks pointer capture.
  Element.prototype.setPointerCapture ??= () => undefined;
  Element.prototype.releasePointerCapture ??= () => undefined;
  HTMLCanvasElement.prototype.getContext = (() => null) as unknown as HTMLCanvasElement['getContext'];
  // Radix Slider measures itself.
  globalThis.ResizeObserver ??= class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver;
});

beforeEach(() => localStorage.clear());
afterEach(cleanup);

describe('PianoRollEditor keyboard', () => {
  it('select all + arrow up transposes as one undoable edit', () => {
    const { props, root } = setup([note(60, 0), note(64, 1)]);
    fireEvent.keyDown(root, { code: 'KeyA', key: 'a', metaKey: true });
    fireEvent.keyDown(root, { code: 'ArrowUp', key: 'ArrowUp' });
    expect(props.onEditNotes).toHaveBeenCalledTimes(1);
    const [trackId, regionId, notes, label] = (props.onEditNotes as ReturnType<typeof vi.fn>).mock.calls[0];
    expect([trackId, regionId, label]).toEqual(['t1', 'r1', 'Transpose Notes']);
    expect((notes as RegionNote[]).map((n) => n.note)).toEqual([61, 65]);
  });

  it('shift+arrow moves an octave, left/right nudges by the grid, delete removes', () => {
    const { props, root } = setup([note(60, 1)]);
    const edit = props.onEditNotes as ReturnType<typeof vi.fn>;
    fireEvent.keyDown(root, { code: 'KeyA', metaKey: true });
    fireEvent.keyDown(root, { code: 'ArrowDown', shiftKey: true });
    expect(edit.mock.calls[0][2][0].note).toBe(48);
    fireEvent.keyDown(root, { code: 'ArrowRight' });
    expect(edit.mock.calls[1][2][0].startBeat).toBe(1.25);
    fireEvent.keyDown(root, { code: 'Delete' });
    expect(edit.mock.calls[2][2]).toEqual([]);
    expect(edit.mock.calls[2][3]).toBe('Delete Note');
  });

  it('copy/paste pastes at the cue (region start by default) and duplicate appends after the selection', () => {
    const { props, root } = setup([note(60, 2, 1), note(62, 3, 1)]);
    const edit = props.onEditNotes as ReturnType<typeof vi.fn>;
    fireEvent.keyDown(root, { code: 'KeyA', ctrlKey: true });
    fireEvent.keyDown(root, { code: 'KeyC', ctrlKey: true });
    fireEvent.keyDown(root, { code: 'KeyV', ctrlKey: true });
    const pasted = edit.mock.calls[0][2] as RegionNote[];
    expect(pasted.slice(2).map((n) => [n.note, n.startBeat])).toEqual([
      [60, 0],
      [62, 1],
    ]);
    fireEvent.keyDown(root, { code: 'KeyD', metaKey: true });
    // Selection is now the pasted notes (span 0..2): duplicates land at 2..4
    const dup = edit.mock.calls[1][2] as RegionNote[];
    expect(dup.slice(4).map((n) => [n.note, n.startBeat])).toEqual([
      [60, 2],
      [62, 3],
    ]);
  });

  it('Space plays from the cue, or stops when playing', () => {
    const a = setup([]);
    fireEvent.keyDown(a.root, { code: 'Space', key: ' ' });
    expect(a.props.onPlayFrom).toHaveBeenCalledWith(4);
    cleanup();
    const b = setup([], { transportState: 'playing', positionBeats: 6 });
    fireEvent.keyDown(b.root, { code: 'Space', key: ' ' });
    expect(b.props.onStop).toHaveBeenCalled();
  });

  it('consumes its own shortcuts but lets global ones (undo) reach the app', () => {
    const { root } = setup([note(60, 0)]);
    const seen: string[] = [];
    const onKey = (e: KeyboardEvent) => seen.push(e.code);
    window.addEventListener('keydown', onKey);
    fireEvent.keyDown(root, { code: 'KeyA', metaKey: true });
    fireEvent.keyDown(root, { code: 'Space' });
    fireEvent.keyDown(root, { code: 'KeyZ', metaKey: true });
    window.removeEventListener('keydown', onKey);
    expect(seen).toEqual(['KeyZ']);
  });

  it('ignores keys typed into inputs', () => {
    const { props, root } = setup([note(60, 0)]);
    const input = document.createElement('input');
    root.appendChild(input);
    fireEvent.keyDown(input, { code: 'KeyA', metaKey: true });
    fireEvent.keyDown(input, { code: 'Delete' });
    expect(props.onEditNotes).not.toHaveBeenCalled();
  });
});

describe('PianoRollEditor mouse editing', () => {
  it('a drag-move commits once (one undo entry) and previews the new pitch', () => {
    const { props, grid } = setup([note(60, 0, 1)]);
    const edit = props.onEditNotes as ReturnType<typeof vi.fn>;
    fireEvent.pointerDown(grid, { button: 0, pointerId: 1, detail: 1, ...at(0.5, 60) });
    expect(props.onPreviewNote).toHaveBeenLastCalledWith('t1', 60, 0.8, expect.any(Number));
    fireEvent.pointerMove(grid, { pointerId: 1, ...at(1.0, 61) });
    fireEvent.pointerMove(grid, { pointerId: 1, ...at(1.5, 62) });
    fireEvent.pointerMove(grid, { pointerId: 1, ...at(2.5, 62) });
    expect(edit).not.toHaveBeenCalled();
    fireEvent.pointerUp(grid, { pointerId: 1, ...at(2.5, 62) });
    expect(edit).toHaveBeenCalledTimes(1);
    const moved = edit.mock.calls[0][2][0] as RegionNote;
    expect(moved).toMatchObject({ note: 62, startBeat: 2 });
    expect(edit.mock.calls[0][3]).toBe('Move Note');
    expect(props.onPreviewNote).toHaveBeenLastCalledWith('t1', 62, 0.8, expect.any(Number));
  });

  it('draw tool: click adds a note with the current length, drag sets the length', () => {
    const { props, grid, root } = setup([]);
    const edit = props.onEditNotes as ReturnType<typeof vi.fn>;
    fireEvent.keyDown(root, { code: 'KeyD' });
    fireEvent.pointerDown(grid, { button: 0, pointerId: 1, detail: 1, ...at(1.1, 64) });
    fireEvent.pointerUp(grid, { pointerId: 1, ...at(1.1, 64) });
    expect(edit.mock.calls[0][2]).toEqual([{ note: 64, startBeat: 1, durationBeats: 1, velocity: 0.8, isDrum: false }]);
    expect(edit.mock.calls[0][3]).toBe('Draw Note');

    fireEvent.pointerDown(grid, { button: 0, pointerId: 1, detail: 1, ...at(3, 67) });
    fireEvent.pointerMove(grid, { pointerId: 1, ...at(4, 67) });
    fireEvent.pointerMove(grid, { pointerId: 1, ...at(5.5, 67) });
    fireEvent.pointerUp(grid, { pointerId: 1, ...at(5.5, 67) });
    expect(edit).toHaveBeenCalledTimes(2);
    expect(edit.mock.calls[1][2][1]).toMatchObject({ note: 67, startBeat: 3, durationBeats: 2.5 });
  });

  it('resizes from the right edge and grows the region when drawing past its end', () => {
    const { props, grid } = setup([note(60, 6, 1)]);
    const edit = props.onEditNotes as ReturnType<typeof vi.fn>;
    fireEvent.pointerDown(grid, { button: 0, pointerId: 1, detail: 1, ...at(6.98, 60) });
    fireEvent.pointerMove(grid, { pointerId: 1, ...at(8, 60) });
    fireEvent.pointerMove(grid, { pointerId: 1, ...at(10, 60) });
    fireEvent.pointerUp(grid, { pointerId: 1, ...at(10, 60) });
    expect(edit).toHaveBeenCalledTimes(1);
    const [, , notes, label, newLength] = edit.mock.calls[0];
    expect((notes as RegionNote[])[0]).toMatchObject({ startBeat: 6, durationBeats: 4 });
    expect(label).toBe('Resize Note');
    expect(newLength).toBe(12); // 10 beats rounded up to the bar
  });

  it('right-click deletes a note', () => {
    const { props, grid } = setup([note(60, 0), note(62, 2)]);
    const edit = props.onEditNotes as ReturnType<typeof vi.fn>;
    fireEvent.pointerDown(grid, { button: 2, pointerId: 1, detail: 1, ...at(2.5, 62) });
    fireEvent.pointerUp(grid, { pointerId: 1, ...at(2.5, 62) });
    expect(edit.mock.calls[0][2]).toEqual([note(60, 0)]);
  });
});
