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
  let replacing = false;
  return {
    isReplacingProject: vi.fn(() => replacing),
    setReplacing(v: boolean) {
      replacing = v;
    },
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
      replacing = false;
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
      // loadProject notifies while it runs, inside ProjectManager's replacement.
      pm.setReplacing(true);
      engine.emit();
      pm.setReplacing(false);
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

  it('an edit while the open dialog is still showing marks the project dirty', async () => {
    // Phase 10 review (GPT-6.1 Sol): suppression started before the dialog was awaited, so a
    // MIDI recording or pad edit made while the Open dialog was up was not marked dirty.
    let finish: (v: boolean) => void = () => undefined;
    pm.open.mockImplementationOnce(
      () =>
        new Promise<boolean>((resolve) => {
          finish = resolve;
        }),
    );
    const { result } = renderHook(() => useProject());
    let pending: Promise<void> = Promise.resolve();
    act(() => {
      pending = result.current.open();
    });
    engine.emit(); // the user records while the dialog is open
    expect(pm.markDirty).toHaveBeenCalledTimes(1);
    finish(false);
    await act(async () => {
      await pending;
    });
  });
});
