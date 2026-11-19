/**
 * Electron bug fixed in both apps (Rune port phase 10, user decision 2026-11-18): `useProject`
 * set `skipDirtyRef` before an open / new and never cleared it when that was cancelled, so the
 * next real edit did not mark the project dirty and the close guard let the window close
 * without asking.
 */
import { act, renderHook } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const pm = vi.hoisted(() => {
  let dirty = false;
  return {
    getFilePath: vi.fn(() => null),
    getIsDirty: vi.fn(() => dirty),
    getProjectName: vi.fn(() => 'Untitled'),
    getRecentProjects: vi.fn(() => []),
    onStateChange: vi.fn(() => () => undefined),
    markDirty: vi.fn(() => {
      dirty = true;
    }),
    open: vi.fn(async () => false),
    newProject: vi.fn(async () => false),
    openFromPath: vi.fn(async () => false),
    newProjectFromTemplate: vi.fn(async () => false),
    save: vi.fn(),
    saveAs: vi.fn(),
    reset() {
      dirty = false;
    },
  };
});

const engine = vi.hoisted(() => {
  const cbs: Array<() => void> = [];
  return {
    onStateChange: vi.fn((cb: () => void) => {
      cbs.push(cb);
      return () => cbs.splice(cbs.indexOf(cb), 1);
    }),
    emit() {
      for (const cb of [...cbs]) cb();
    },
  };
});

vi.mock('@/services/ProjectManager', () => ({ projectManager: pm }));
vi.mock('@/services/ArrangementEngine', () => ({ arrangementEngine: engine }));

import { useProject } from './useProject';

describe('useProject dirty tracking', () => {
  beforeEach(() => {
    pm.reset();
    pm.markDirty.mockClear();
  });

  it.each(['open', 'newProject'] as const)('a cancelled %s does not swallow the next edit', async (op) => {
    const { result } = renderHook(() => useProject());
    await act(async () => {
      await result.current[op]();
    });
    engine.emit(); // the user edits
    expect(pm.markDirty).toHaveBeenCalledTimes(1);
  });

  it('a cancelled open of a recent project does not either', async () => {
    const { result } = renderHook(() => useProject());
    await act(async () => {
      await result.current.openRecent('/x/a.mdaw');
    });
    engine.emit();
    expect(pm.markDirty).toHaveBeenCalledTimes(1);
  });

  it("the open's own state change is still skipped", async () => {
    pm.open.mockImplementationOnce(async () => {
      engine.emit(); // loadProject notifies while it runs
      return true;
    });
    const { result } = renderHook(() => useProject());
    await act(async () => {
      await result.current.open();
    });
    expect(pm.markDirty).not.toHaveBeenCalled();
    engine.emit();
    expect(pm.markDirty).toHaveBeenCalledTimes(1);
  });
});
