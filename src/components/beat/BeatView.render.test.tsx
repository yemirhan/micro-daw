import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, render } from '@testing-library/react';
import { memo } from 'react';

// Count renders of the heavy children; their props are what BeatView must keep stable (review L-a).
const renders = { transport: 0, pads: 0, editor: 0, steps: 0 };
vi.mock('./BeatTransport', () => ({ BeatTransport: memo(() => (renders.transport++, null)) }));
vi.mock('./PadGrid', () => ({ PadGrid: memo(() => (renders.pads++, null)) }));
vi.mock('./PadEditor', () => ({ PadEditor: memo(() => (renders.editor++, null)) }));
vi.mock('./StepSequencer', () => ({ StepSequencer: memo(() => (renders.steps++, null)) }));

import { arrangementEngine } from '@/services/ArrangementEngine';
import { BeatView } from './BeatView';

const onPadHit = vi.fn();
const onPadRelease = vi.fn();
const onExport = vi.fn();

beforeEach(() => {
  HTMLCanvasElement.prototype.getContext = (() => null) as typeof HTMLCanvasElement.prototype.getContext;
  arrangementEngine.ensureDrumTrack();
  for (const k of Object.keys(renders) as (keyof typeof renders)[]) renders[k] = 0;
});
afterEach(() => vi.restoreAllMocks());

describe('BeatView re-render scope (review L-a)', () => {
  it('a recorded hit / step edit re-renders only the step sequencer', () => {
    render(<BeatView onPadHit={onPadHit} onPadRelease={onPadRelease} onExport={onExport} />);
    const before = { ...renders };
    expect(before.steps).toBeGreaterThan(0);
    const track = arrangementEngine.getActiveDrumTrackId()!;
    act(() => arrangementEngine.toggleStep(track, 0, 0, 0.25));
    expect(renders.steps).toBeGreaterThan(before.steps);
    expect(renders.transport).toBe(before.transport);
    expect(renders.pads).toBe(before.pads);
    expect(renders.editor).toBe(before.editor);
  });

  it('re-rendering the view with the same props does not re-render its memoised children', () => {
    const { rerender } = render(<BeatView onPadHit={onPadHit} onPadRelease={onPadRelease} onExport={onExport} />);
    const before = { ...renders };
    rerender(<BeatView onPadHit={onPadHit} onPadRelease={onPadRelease} onExport={onExport} />);
    expect(renders).toEqual(before);
  });
});
