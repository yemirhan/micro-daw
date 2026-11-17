import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render } from '@testing-library/react';
import { DockPads } from './InstrumentDock';

// Thumbnails would render factory voices through Tone.Offline; not needed here.
vi.mock('@/audio/sampler/bufferCache', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/audio/sampler/bufferCache')>()),
  getCachedPadBuffer: () => null,
  loadPadBuffer: () => new Promise(() => undefined),
  padSourceKey: (s: { kind: string; voice?: string; path?: string }) => `${s.kind}:${s.voice ?? s.path}`,
}));

beforeEach(() => {
  HTMLCanvasElement.prototype.getContext = (() => null) as typeof HTMLCanvasElement.prototype.getContext;
  try {
    localStorage.removeItem('micro-daw:dock-pad-bank');
  } catch {
    // ignore
  }
});

describe('Song dock pads (#8)', () => {
  it('pair press and release like the Beat view (gates end, note repeat stops)', () => {
    const onPress = vi.fn();
    const onRelease = vi.fn();
    render(<DockPads onPress={onPress} onRelease={onRelease} />);
    const el = document.querySelector<HTMLElement>('[data-pad="2"]')!;
    expect(el).not.toBeNull();
    fireEvent.pointerDown(el, { button: 0, clientY: 0, pointerId: 1 });
    expect(onPress).toHaveBeenCalledTimes(1);
    expect(onPress.mock.calls[0][0]).toBe(2);
    expect(onRelease).not.toHaveBeenCalled();
    fireEvent.pointerUp(el, { pointerId: 1 });
    expect(onRelease).toHaveBeenCalledWith(2);
    expect(onRelease).toHaveBeenCalledTimes(1);
  });
});
