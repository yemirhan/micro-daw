import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { createFactoryKit } from '@/audio/sampler/factoryKit';
import { SAMPLE_DRAG_MIME } from '@/components/browser/browserHelpers';
import { noteEventBus } from '@/services/NoteEventBus';
import { PadGrid } from './PadGrid';

// Thumbnails would render factory voices through Tone.Offline; not needed here.
vi.mock('@/audio/sampler/bufferCache', () => ({
  getCachedPadBuffer: () => null,
  loadPadBuffer: () => new Promise(() => undefined),
  padSourceKey: (s: { kind: string; voice?: string; path?: string }) => `${s.kind}:${s.voice ?? s.path}`,
}));

beforeEach(() => {
  // jsdom has no canvas.
  HTMLCanvasElement.prototype.getContext = (() => null) as typeof HTMLCanvasElement.prototype.getContext;
});

function setup(overrides: Partial<React.ComponentProps<typeof PadGrid>> = {}) {
  const kit = createFactoryKit();
  kit.pads[5] = { ...kit.pads[5], sample: null };
  const props = {
    pads: kit.pads,
    selectedPad: 0,
    hardwareNotes: Array.from({ length: 16 }, (_, i) => 36 + i),
    onHit: vi.fn(),
    onRelease: vi.fn(),
    onSelect: vi.fn(),
    onDropSample: vi.fn(),
    onClear: vi.fn(),
    onCopy: vi.fn(),
    onPaste: vi.fn(),
    ...overrides,
  };
  render(<PadGrid {...props} />);
  return props;
}

const pad = (i: number) => document.querySelector<HTMLElement>(`[data-pad="${i}"]`)!;

describe('PadGrid', () => {
  it('renders 16 pads with names, hardware notes and the selection', () => {
    setup({ selectedPad: 3 });
    expect(screen.getAllByRole('gridcell')).toHaveLength(16);
    expect(pad(0)).toHaveAccessibleName('Pad A1: Kick');
    expect(pad(5)).toHaveAccessibleName('Pad A6, empty');
    expect(pad(0)).toHaveTextContent('C1');
    expect(pad(3)).toHaveAttribute('aria-selected', 'true');
    expect(pad(0)).toHaveAttribute('aria-selected', 'false');
  });

  it('lays pads out MPC-style: pad 0 bottom-left', () => {
    setup();
    const cells = screen.getAllByRole('gridcell').map((el) => Number(el.dataset.pad));
    expect(cells.slice(0, 4)).toEqual([12, 13, 14, 15]);
    expect(cells.slice(12)).toEqual([0, 1, 2, 3]);
  });

  it('click hits + selects with velocity from the click height, release on pointer up', () => {
    const p = setup();
    const el = pad(9);
    el.getBoundingClientRect = () => ({ top: 100, height: 100, left: 0, width: 100, bottom: 200, right: 100, x: 0, y: 100, toJSON: () => ({}) });
    fireEvent.pointerDown(el, { button: 0, clientY: 100, pointerId: 1 });
    expect(p.onSelect).toHaveBeenCalledWith(9);
    expect(p.onHit).toHaveBeenCalledWith(9, 1);
    fireEvent.pointerUp(el, { pointerId: 1 });
    expect(p.onRelease).toHaveBeenCalledWith(9);
    expect(p.onRelease).toHaveBeenCalledTimes(1);

    fireEvent.pointerDown(el, { button: 0, clientY: 200, pointerId: 1 });
    expect(p.onHit).toHaveBeenLastCalledWith(9, 0.3);
  });

  it('ignores right-button pointer downs (context menu instead)', () => {
    const p = setup();
    fireEvent.pointerDown(pad(1), { button: 2 });
    expect(p.onHit).not.toHaveBeenCalled();
    fireEvent.contextMenu(pad(1), { clientX: 10, clientY: 10 });
    expect(p.onSelect).toHaveBeenCalledWith(1);
    expect(screen.getByRole('menu')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('menuitem', { name: 'Clear pad' }));
    expect(p.onClear).toHaveBeenCalledWith(1);
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
  });

  it('paste is disabled until something was copied', () => {
    setup({ canPaste: false });
    fireEvent.contextMenu(pad(2));
    expect(screen.getByRole('menuitem', { name: 'Paste pad' })).toBeDisabled();
  });

  it('accepts a sample dragged from the browser', () => {
    const p = setup();
    const payload = JSON.stringify({ path: '/samples/Snare 2.wav', name: 'Snare 2' });
    const dataTransfer = {
      types: [SAMPLE_DRAG_MIME],
      getData: (t: string) => (t === SAMPLE_DRAG_MIME ? payload : ''),
      files: [],
      dropEffect: 'none',
    };
    fireEvent.dragOver(pad(4), { dataTransfer });
    fireEvent.drop(pad(4), { dataTransfer });
    expect(p.onDropSample).toHaveBeenCalledWith(4, { path: '/samples/Snare 2.wav', name: 'Snare 2' });
    expect(p.onSelect).toHaveBeenCalledWith(4);
  });

  it('accepts an audio file dropped from Finder', () => {
    const getPathForFile = vi.fn((f: File) => `/Users/me/${f.name}`);
    const p = setup({ getPathForFile });
    const file = new File(['x'], 'Clap.aif');
    const dataTransfer = { types: ['Files'], getData: () => '', files: [file], dropEffect: 'none' };
    fireEvent.drop(pad(7), { dataTransfer });
    expect(getPathForFile).toHaveBeenCalledWith(file);
    expect(p.onDropSample).toHaveBeenCalledWith(7, { path: '/Users/me/Clap.aif', name: 'Clap' });
  });

  it('ignores drops without a usable sample', () => {
    const p = setup();
    const dataTransfer = { types: ['text/plain'], getData: () => 'hello', files: [], dropEffect: 'none' };
    fireEvent.drop(pad(7), { dataTransfer });
    expect(p.onDropSample).not.toHaveBeenCalled();
  });

  it('flashes the pad for hits from the note bus (MIDI or screen)', () => {
    setup();
    const light = pad(6).querySelector<HTMLElement>('[data-pad-light]')!;
    const animate = vi.fn();
    light.animate = animate as unknown as HTMLElement['animate'];
    act(() => {
      noteEventBus.emit({ type: 'drum', padId: 6, note: 42, velocity: 0.9, time: 0 });
      noteEventBus.emit({ type: 'noteon', note: 60, velocity: 1, channel: 1, time: 0 });
    });
    expect(animate).toHaveBeenCalledTimes(1);
  });
});
