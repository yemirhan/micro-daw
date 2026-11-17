import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { createFactoryKit } from '@/audio/sampler/factoryKit';
import { padToNote } from '@/types/sampler';
import { StepSequencer } from './StepSequencer';

beforeEach(() => {
  HTMLCanvasElement.prototype.getContext = (() => null) as typeof HTMLCanvasElement.prototype.getContext;
});

// jsdom has no layout: rows get the minimum height (14px), cells the minimum width (18px).
const CELL_W = 18;
const ROW_H = 14;

function setup(notes: { pad: number; beat: number; velocity?: number }[] = [], extra: { onClear?: () => void; onNudgeVelocity?: (pad: number, step: number, stepBeats: number, velocity: number) => void } = {}) {
  const props = {
    pads: createFactoryKit().pads,
    selectedPad: 0,
    region: {
      startBeat: 0,
      notes: notes.map((n) => ({ note: padToNote(n.pad), velocity: n.velocity ?? 0.8, startBeat: n.beat, durationBeats: 0.25, isDrum: true })),
    },
    patternStartBeat: 0,
    bars: 1,
    swing: 0,
    grid: '1/16' as const,
    onGridChange: vi.fn(),
    onToggleStep: vi.fn(),
    onSetVelocity: vi.fn(),
    onSelectPad: vi.fn(),
    onGestureStart: vi.fn(),
    onGestureEnd: vi.fn(),
    playing: false,
    ...extra,
  };
  render(<StepSequencer {...props} />);
  const canvas = screen.getByRole('grid', { name: /pattern grid/i }) as HTMLCanvasElement;
  canvas.getBoundingClientRect = () => ({ left: 0, top: 0, width: 16 * CELL_W, height: 16 * ROW_H, right: 0, bottom: 0, x: 0, y: 0, toJSON: () => ({}) });
  return { props, canvas };
}

/** Client point at the centre of a cell (pad 0 = bottom row). */
const at = (pad: number, step: number) => ({ clientX: step * CELL_W + CELL_W / 2, clientY: (15 - pad) * ROW_H + ROW_H / 2 });

describe('StepSequencer', () => {
  it('shows one column per step for the loop length', () => {
    const { canvas } = setup();
    expect(canvas.dataset.steps).toBe('16');
  });

  it('click toggles a step with the grid step length and selects the row', () => {
    const { props, canvas } = setup();
    fireEvent.pointerDown(canvas, { button: 0, pointerId: 1, ...at(3, 5) });
    fireEvent.pointerUp(canvas, { pointerId: 1 });
    expect(props.onToggleStep).toHaveBeenCalledWith(3, 5, 0.25);
    expect(props.onSelectPad).toHaveBeenCalledWith(3);
  });

  it('drag paints empty cells and skips filled ones (draw mode)', () => {
    const { props, canvas } = setup([{ pad: 0, beat: 0.5 }]);
    fireEvent.pointerDown(canvas, { button: 0, pointerId: 1, ...at(0, 0) });
    fireEvent.pointerMove(canvas, { pointerId: 1, ...at(0, 1) });
    fireEvent.pointerMove(canvas, { pointerId: 1, ...at(0, 1) }); // same cell twice
    fireEvent.pointerMove(canvas, { pointerId: 1, ...at(0, 2) }); // already on: no toggle
    fireEvent.pointerMove(canvas, { pointerId: 1, ...at(0, 3) });
    fireEvent.pointerUp(canvas, { pointerId: 1 });
    expect(props.onToggleStep.mock.calls.map((c) => c[1])).toEqual([0, 1, 3]);
  });

  it('starting on a filled cell erases while dragging', () => {
    const { props, canvas } = setup([{ pad: 2, beat: 0 }, { pad: 2, beat: 0.5 }]);
    fireEvent.pointerDown(canvas, { button: 0, pointerId: 1, ...at(2, 0) });
    fireEvent.pointerMove(canvas, { pointerId: 1, ...at(2, 1) }); // empty: untouched
    fireEvent.pointerMove(canvas, { pointerId: 1, ...at(2, 2) });
    expect(props.onToggleStep.mock.calls.map((c) => c[1])).toEqual([0, 2]);
  });

  it('alt-drag on a filled cell sets its velocity instead of toggling', () => {
    const { props, canvas } = setup([{ pad: 1, beat: 1, velocity: 0.5 }]);
    const p = at(1, 4);
    fireEvent.pointerDown(canvas, { button: 0, pointerId: 1, altKey: true, ...p });
    fireEvent.pointerMove(canvas, { pointerId: 1, clientX: p.clientX, clientY: p.clientY - 24 });
    expect(props.onToggleStep).not.toHaveBeenCalled();
    expect(props.onSetVelocity).toHaveBeenLastCalledWith(1, 4, 0.25, 0.7);
  });

  it('wheel over a filled cell nudges its velocity', () => {
    const { props, canvas } = setup([{ pad: 0, beat: 0, velocity: 0.5 }]);
    fireEvent.wheel(canvas, { deltaY: -10, ...at(0, 0) });
    expect(props.onSetVelocity).toHaveBeenCalledWith(0, 0, 0.25, 0.55);
    fireEvent.wheel(canvas, { deltaY: -10, ...at(0, 1) }); // empty cell: nothing
    expect(props.onSetVelocity).toHaveBeenCalledTimes(1);
  });

  it('switches step size', () => {
    const { props } = setup();
    fireEvent.click(screen.getByRole('radio', { name: '1/32' }));
    expect(props.onGridChange).toHaveBeenCalledWith('1/32');
  });

  it('Clear is disabled without a handler', () => {
    setup();
    expect(screen.getByRole('button', { name: /clear/i })).toBeDisabled();
  });

  it('Clear calls onClear', () => {
    const onClear = vi.fn();
    setup([{ pad: 0, beat: 0 }], { onClear });
    fireEvent.click(screen.getByRole('button', { name: /clear/i }));
    expect(onClear).toHaveBeenCalledTimes(1);
  });

  it('a paint drag is one gesture (one undo step): start before the first toggle, end on pointer up', () => {
    const { props, canvas } = setup();
    fireEvent.pointerDown(canvas, { button: 0, pointerId: 1, ...at(4, 0) });
    expect(props.onGestureStart).toHaveBeenCalledTimes(1);
    expect(props.onGestureStart.mock.invocationCallOrder[0]).toBeLessThan(props.onToggleStep.mock.invocationCallOrder[0]);
    fireEvent.pointerMove(canvas, { pointerId: 1, ...at(4, 1) });
    fireEvent.pointerMove(canvas, { pointerId: 1, ...at(4, 2) });
    expect(props.onGestureEnd).not.toHaveBeenCalled();
    fireEvent.pointerUp(canvas, { pointerId: 1 });
    expect(props.onToggleStep).toHaveBeenCalledTimes(3);
    expect(props.onGestureStart).toHaveBeenCalledTimes(1);
    expect(props.onGestureEnd).toHaveBeenCalledTimes(1);
  });

  it('pointer cancel also closes the gesture; an alt-click without change opens none', () => {
    const { props, canvas } = setup([{ pad: 1, beat: 0, velocity: 0.5 }]);
    fireEvent.pointerDown(canvas, { button: 0, pointerId: 1, ...at(2, 0) });
    fireEvent.pointerCancel(canvas, { pointerId: 1 });
    expect(props.onGestureEnd).toHaveBeenCalledTimes(1);
    fireEvent.pointerDown(canvas, { button: 0, pointerId: 2, altKey: true, ...at(1, 0) });
    fireEvent.pointerUp(canvas, { pointerId: 2 });
    expect(props.onGestureStart).toHaveBeenCalledTimes(1);
    expect(props.onGestureEnd).toHaveBeenCalledTimes(1);
  });

  it('a velocity drag is one gesture', () => {
    const { props, canvas } = setup([{ pad: 1, beat: 1, velocity: 0.5 }]);
    const p = at(1, 4);
    fireEvent.pointerDown(canvas, { button: 0, pointerId: 1, altKey: true, ...p });
    for (let i = 1; i <= 4; i++) fireEvent.pointerMove(canvas, { pointerId: 1, clientX: p.clientX, clientY: p.clientY - i * 12 });
    fireEvent.pointerUp(canvas, { pointerId: 1 });
    expect(props.onSetVelocity).toHaveBeenCalledTimes(4);
    expect(props.onGestureStart).toHaveBeenCalledTimes(1);
    expect(props.onGestureStart).toHaveBeenCalledWith('Step velocity');
    expect(props.onGestureEnd).toHaveBeenCalledTimes(1);
  });

  it('wheel nudges go through onNudgeVelocity (burst-grouped by the caller)', () => {
    const onNudgeVelocity = vi.fn();
    const { props, canvas } = setup([{ pad: 0, beat: 0, velocity: 0.5 }], { onNudgeVelocity });
    fireEvent.wheel(canvas, { deltaY: -10, ...at(0, 0) });
    expect(onNudgeVelocity).toHaveBeenCalledWith(0, 0, 0.25, 0.55);
    expect(props.onSetVelocity).not.toHaveBeenCalled();
  });

  it('is keyboard operable: arrows move the focus cell, Space toggles, +/- set velocity, live region announces', () => {
    const onNudgeVelocity = vi.fn();
    const { props, canvas } = setup([{ pad: 1, beat: 1, velocity: 0.8 }], { onNudgeVelocity });
    expect(canvas).toHaveAttribute('tabindex', '0');
    canvas.focus();
    for (let i = 0; i < 4; i++) fireEvent.keyDown(canvas, { key: 'ArrowRight' });
    fireEvent.keyDown(canvas, { key: 'ArrowUp' });
    expect(canvas.dataset.focusStep).toBe('4');
    expect(canvas.dataset.focusPad).toBe('1');
    expect(props.onSelectPad).toHaveBeenLastCalledWith(1);
    expect(screen.getByRole('status')).toHaveTextContent(/step 5, on, velocity 80%/);
    fireEvent.keyDown(canvas, { key: '-' });
    expect(onNudgeVelocity).toHaveBeenCalledWith(1, 4, 0.25, 0.7);
    fireEvent.keyDown(canvas, { key: 'ArrowLeft' });
    const space = new KeyboardEvent('keydown', { key: ' ', bubbles: true, cancelable: true });
    canvas.dispatchEvent(space);
    expect(space.defaultPrevented).toBe(true); // never reaches the app's play/stop
    expect(props.onToggleStep).toHaveBeenCalledWith(1, 3, 0.25);
    fireEvent.keyDown(canvas, { key: 'End' });
    expect(canvas.dataset.focusStep).toBe('15');
    fireEvent.keyDown(canvas, { key: 'ArrowRight', shiftKey: true });
    expect(canvas.dataset.focusStep).toBe('15');
    fireEvent.keyDown(canvas, { key: 'Home' });
    fireEvent.keyDown(canvas, { key: 'ArrowRight', shiftKey: true });
    expect(canvas.dataset.focusStep).toBe('4');
  });
});
