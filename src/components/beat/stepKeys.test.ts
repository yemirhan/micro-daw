import { describe, expect, it } from 'vitest';
import { describeStep, nudgeVelocity, stepKeyAction } from './stepKeys';

const k = (key: string, mods: Partial<{ shiftKey: boolean; altKey: boolean; metaKey: boolean; ctrlKey: boolean }> = {}) => ({
  key,
  shiftKey: false,
  altKey: false,
  metaKey: false,
  ctrlKey: false,
  ...mods,
});

describe('stepKeyAction', () => {
  const cell = { pad: 5, step: 6 };
  it('arrows move one cell, clamped to the grid; up = higher pad', () => {
    expect(stepKeyAction(k('ArrowRight'), cell, 16, 4)).toEqual({ type: 'move', cell: { pad: 5, step: 7 } });
    expect(stepKeyAction(k('ArrowLeft'), { pad: 5, step: 0 }, 16, 4)).toEqual({ type: 'move', cell: { pad: 5, step: 0 } });
    expect(stepKeyAction(k('ArrowUp'), cell, 16, 4)).toEqual({ type: 'move', cell: { pad: 6, step: 6 } });
    expect(stepKeyAction(k('ArrowDown'), { pad: 0, step: 6 }, 16, 4)).toEqual({ type: 'move', cell: { pad: 0, step: 6 } });
  });
  it('Shift+arrows jump to the next / previous beat line', () => {
    expect(stepKeyAction(k('ArrowRight', { shiftKey: true }), cell, 16, 4)).toEqual({ type: 'move', cell: { pad: 5, step: 8 } });
    expect(stepKeyAction(k('ArrowLeft', { shiftKey: true }), cell, 16, 4)).toEqual({ type: 'move', cell: { pad: 5, step: 4 } });
    expect(stepKeyAction(k('ArrowLeft', { shiftKey: true }), { pad: 5, step: 8 }, 16, 4)).toEqual({ type: 'move', cell: { pad: 5, step: 4 } });
  });
  it('Home / End go to the row ends', () => {
    expect(stepKeyAction(k('Home'), cell, 32, 4)).toEqual({ type: 'move', cell: { pad: 5, step: 0 } });
    expect(stepKeyAction(k('End'), cell, 32, 4)).toEqual({ type: 'move', cell: { pad: 5, step: 31 } });
  });
  it('Space / Enter toggle; + - [ ] change velocity; other keys and chords are not ours', () => {
    expect(stepKeyAction(k(' '), cell, 16, 4)).toEqual({ type: 'toggle' });
    expect(stepKeyAction(k('Enter'), cell, 16, 4)).toEqual({ type: 'toggle' });
    expect(stepKeyAction(k('+'), cell, 16, 4)).toEqual({ type: 'velocity', delta: 0.1 });
    expect(stepKeyAction(k(']'), cell, 16, 4)).toEqual({ type: 'velocity', delta: 0.1 });
    expect(stepKeyAction(k('-'), cell, 16, 4)).toEqual({ type: 'velocity', delta: -0.1 });
    expect(stepKeyAction(k('['), cell, 16, 4)).toEqual({ type: 'velocity', delta: -0.1 });
    expect(stepKeyAction(k('z'), cell, 16, 4)).toBeNull();
    expect(stepKeyAction(k(' ', { metaKey: true }), cell, 16, 4)).toBeNull();
    expect(stepKeyAction(k('ArrowRight', { altKey: true }), cell, 16, 4)).toBeNull();
  });
});

describe('nudgeVelocity / describeStep', () => {
  it('clamps to 5%..100%', () => {
    expect(nudgeVelocity(0.95, 0.1)).toBe(1);
    expect(nudgeVelocity(0.1, -0.1)).toBe(0.05);
    expect(nudgeVelocity(0.8, -0.1)).toBe(0.7);
  });
  it('announces pad, 1-based step, state and velocity', () => {
    expect(describeStep('Kick', 4, 0.8)).toBe('Kick, step 5, on, velocity 80%');
    expect(describeStep('Snare', 0, 0)).toBe('Snare, step 1, off');
  });
});
