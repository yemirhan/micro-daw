/**
 * Review round 5 N2: opening a project (even the same one again) must start a
 * new arrangement session, so a pending "Get & load" aimed at the previous
 * project is dropped. ProjectManager therefore opens via loadProject, not the
 * undo/redo restoreFromSnapshot path.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ElectronAPI } from '@/types/electron';

const engine = vi.hoisted(() => ({
  loadProject: vi.fn(),
  restoreFromSnapshot: vi.fn(),
  getArrangement: vi.fn(() => ({ id: 'p', tracks: [] })),
}));

vi.mock('./ArrangementEngine', () => ({ arrangementEngine: engine }));
vi.mock('./AudioEngine', () => ({
  audioEngine: {
    setReverbWet: vi.fn(),
    setChorusDepth: vi.fn(),
    setFilterCutoff: vi.fn(),
    setFilterResonance: vi.fn(),
    getEffectParams: vi.fn(() => ({})),
  },
}));
vi.mock('./UndoManager', () => ({ undoManager: { clear: vi.fn() } }));
vi.mock('./SampleManager', () => ({
  sampleManager: { loadFromPathWithId: vi.fn(async () => null), getSample: vi.fn() },
}));
vi.mock('@/audio/sampler/bufferCache', () => ({ preloadKitBuffers: vi.fn(async () => undefined) }));
vi.mock('@/audio/sampler/factoryKit', () => ({ normalizeKit: (k: unknown) => k }));
vi.mock('@/audio/masterBus', () => ({
  getMasterVolumeDb: () => 0,
  setMasterVolumeDb: vi.fn(),
  MASTER_REFERENCE_DB: 0,
}));

describe('ProjectManager open (N2 session)', () => {
  beforeEach(() => {
    engine.loadProject.mockClear();
    engine.restoreFromSnapshot.mockClear();
    const arrangement = { id: 'same-project', tracks: [] };
    window.electronAPI = {
      projectLoad: vi.fn(async () => JSON.stringify({ version: '1.0.0', arrangement })),
      projectSetTitle: vi.fn(),
    } as unknown as ElectronAPI;
  });

  it('opens through loadProject (new session) every time, even for the same project', async () => {
    const { projectManager } = await import('./ProjectManager');
    expect(await projectManager.openFromPath('/x/song.mdaw')).toBe(true);
    expect(await projectManager.openFromPath('/x/song.mdaw')).toBe(true);
    expect(engine.loadProject).toHaveBeenCalledTimes(2);
    expect(engine.loadProject).toHaveBeenLastCalledWith({ id: 'same-project', tracks: [] });
    expect(engine.restoreFromSnapshot).not.toHaveBeenCalled();
  });
});
