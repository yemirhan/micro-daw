import { PAD_COUNT, type PadIndex } from '@/types/sampler';

/** Keyboard model for the step grid (the canvas has no DOM cells, so focus is a roving cell). */

export interface StepCell {
  pad: PadIndex;
  step: number;
}

export type StepKeyAction =
  | { type: 'move'; cell: StepCell }
  | { type: 'toggle' }
  | { type: 'velocity'; delta: number };

/** Velocity change per +/- (or ] / [) press. */
export const VELOCITY_KEY_STEP = 0.1;

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/**
 * What a key does on the focused step cell, or null when the key isn't ours.
 * Rows run from pad 15 (top) to pad 0 (bottom), so ArrowUp = next pad up.
 * Shift+←/→ jumps a beat (to the previous/next beat line), Home/End go to the
 * first/last step of the row (Ctrl/⌘+Home/End: also the top/bottom pad).
 */
export function stepKeyAction(
  e: Pick<KeyboardEvent, 'key' | 'shiftKey' | 'altKey' | 'metaKey' | 'ctrlKey'>,
  cell: StepCell,
  steps: number,
  stepsPerBeat: number,
): StepKeyAction | null {
  const last = Math.max(0, steps - 1);
  const perBeat = Math.max(1, stepsPerBeat);
  const move = (pad: number, step: number): StepKeyAction => ({
    type: 'move',
    cell: { pad: clamp(pad, 0, PAD_COUNT - 1), step: clamp(step, 0, last) },
  });
  const mod = e.metaKey || e.ctrlKey;
  switch (e.key) {
    case 'ArrowRight':
      if (e.altKey || mod) return null;
      return move(cell.pad, e.shiftKey ? (Math.floor(cell.step / perBeat) + 1) * perBeat : cell.step + 1);
    case 'ArrowLeft':
      if (e.altKey || mod) return null;
      return move(cell.pad, e.shiftKey ? (Math.ceil(cell.step / perBeat) - 1) * perBeat : cell.step - 1);
    case 'ArrowUp':
      if (e.altKey || mod) return null;
      return move(cell.pad + (e.shiftKey ? 4 : 1), cell.step);
    case 'ArrowDown':
      if (e.altKey || mod) return null;
      return move(cell.pad - (e.shiftKey ? 4 : 1), cell.step);
    case 'Home':
      return move(mod ? PAD_COUNT - 1 : cell.pad, 0);
    case 'End':
      return move(mod ? 0 : cell.pad, last);
    case ' ':
    case 'Enter':
      if (e.altKey || mod || e.shiftKey) return null;
      return { type: 'toggle' };
    case '+':
    case '=':
    case ']':
      if (e.altKey || mod) return null;
      return { type: 'velocity', delta: VELOCITY_KEY_STEP };
    case '-':
    case '_':
    case '[':
      if (e.altKey || mod) return null;
      return { type: 'velocity', delta: -VELOCITY_KEY_STEP };
  }
  return null;
}

/** New velocity after a keyboard nudge: 5%..100%, rounded to whole percent. */
export function nudgeVelocity(v: number, delta: number): number {
  return Math.round(clamp(v + delta, 0.05, 1) * 100) / 100;
}

/** Screen-reader text for a cell: "Kick, step 5, on, velocity 80%" / "Snare, step 2, off". */
export function describeStep(padName: string, step: number, velocity: number): string {
  const base = `${padName}, step ${step + 1}`;
  return velocity > 0 ? `${base}, on, velocity ${Math.round(velocity * 100)}%` : `${base}, off`;
}
