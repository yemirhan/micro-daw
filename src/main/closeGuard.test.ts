import { describe, expect, it, vi } from 'vitest';
import { createCloseGuard } from './closeGuard';

describe('close guard (main.ts)', () => {
  it('holds a close and asks the renderer until it is allowed', () => {
    const g = createCloseGuard();
    const ev = { preventDefault: vi.fn() };
    const ask = vi.fn();
    expect(g.onClose(ev, ask)).toBe(false);
    expect(ev.preventDefault).toHaveBeenCalledTimes(1);
    expect(ask).toHaveBeenCalledTimes(1);
    g.allowNextClose();
    expect(g.onClose(ev, ask)).toBe(true);
    expect(ev.preventDefault).toHaveBeenCalledTimes(1);
  });

  it('is armed again after the allowed close (bug: a reopened macOS window closed unasked)', () => {
    const g = createCloseGuard();
    g.allowNextClose();
    expect(g.onClose({ preventDefault: vi.fn() }, vi.fn())).toBe(true);
    const ask = vi.fn();
    expect(g.onClose({ preventDefault: vi.fn() }, ask)).toBe(false);
    expect(ask).toHaveBeenCalledTimes(1);
  });

  it('a new window starts with its own armed guard', () => {
    const first = createCloseGuard();
    first.allowNextClose();
    const second = createCloseGuard();
    const ask = vi.fn();
    expect(second.onClose({ preventDefault: vi.fn() }, ask)).toBe(false);
    expect(ask).toHaveBeenCalled();
  });
});
