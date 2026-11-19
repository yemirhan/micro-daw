/**
 * Rune port phase 10: project files cross between the two apps.
 *
 * - `rune/parity/fixtures/project-saved-by-rust.mdaw` is written by the Rust app's writer
 *   (`t_p10_project::golden_the_rust_writer_output_is_the_committed_fixture`); this test opens it
 *   with the real ProjectManager + ArrangementEngine and checks nothing was lost.
 * - `rune/parity/fixtures/project-saved-by-electron.mdaw` is written here by the real
 *   `projectManager.saveAs` (deterministic ids and clock); the Rust test
 *   `golden_a_project_saved_by_electron_opens_unchanged` opens it. `UPDATE_GOLDENS=1` rewrites it.
 */
import { readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import * as Tone from 'tone';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ElectronAPI } from '@/types/electron';
import { createFakeAudioBuffer } from '@/test/toneMock';
import hipHop from '@/data/templates/hip-hop-beat.json';
import type { Arrangement } from '@/types/arrangement';

vi.mock('@/services/DrumEngine', async () => (await import('@/test/fakeAudioEngine')).drumEngineModule());

const FIXTURES = resolve(__dirname, '../../rune/parity/fixtures');
const RUST_GOLDEN = resolve(FIXTURES, 'project-saved-by-rust.mdaw');
const ELECTRON_GOLDEN = resolve(FIXTURES, 'project-saved-by-electron.mdaw');

async function load() {
  vi.resetModules();
  const { projectManager } = await import('@/services/ProjectManager');
  const { arrangementEngine } = await import('@/services/ArrangementEngine');
  const { audioEngine } = await import('@/services/AudioEngine');
  const { sampleManager } = await import('@/services/SampleManager');
  const bus = await import('@/audio/masterBus');
  return { projectManager, arrangementEngine, audioEngine, sampleManager, bus };
}

beforeEach(() => {
  vi.spyOn(console, 'warn').mockImplementation(() => undefined);
  const raw = Tone.getContext().rawContext as unknown as { decodeAudioData: ReturnType<typeof vi.fn> };
  raw.decodeAudioData.mockImplementation(async () => createFakeAudioBuffer(2, 66150, 44100));
});

afterEach(() => {
  delete window.electronAPI;
  vi.useRealTimers();
});

describe('phase 10 goldens: .mdaw files cross between the apps', () => {
  it('opens a project saved by the Rust app without losing anything', async () => {
    const raw = readFileSync(RUST_GOLDEN, 'utf8');
    const file = JSON.parse(raw) as { arrangement: Arrangement; audioConfig: { masterVolume: number } };
    window.electronAPI = {
      projectLoad: vi.fn(async () => raw),
      projectSetTitle: vi.fn(),
      sampleReadFile: vi.fn(async () => new ArrayBuffer(8)),
    } as unknown as ElectronAPI;
    const { projectManager, arrangementEngine, audioEngine, sampleManager, bus } = await load();
    expect(await projectManager.openFromPath('/songs/project-saved-by-rust.mdaw')).toBe(true);
    // The migration is a no-op on what the Rust app writes: the same arrangement comes back.
    expect(arrangementEngine.getArrangement()).toEqual(file.arrangement);
    expect(bus.getMasterVolumeDb()).toBe(file.audioConfig.masterVolume);
    const fx = audioEngine.getEffectParams();
    expect([fx.reverbWet, fx.chorusDepth, fx.filterCutoff, fx.filterResonance]).toEqual([0.25, 0.4, 9000, 2]);
    // The manifest's sample is loaded under its id.
    expect(sampleManager.getSample('sample-1')).toBeTruthy();
    expect(projectManager.getProjectName()).toBe('project-saved-by-rust');
    expect(projectManager.getIsDirty()).toBe(false);
    expect(projectManager.getRecentProjects()[0]).toEqual({
      filePath: '/songs/project-saved-by-rust.mdaw',
      name: 'project-saved-by-rust',
      modifiedAt: '2026-11-19T10:30:00.000Z',
    });
  });

  it('saves a project the Rust app opens (the committed golden)', async () => {
    let n = 0;
    vi.spyOn(crypto, 'randomUUID').mockImplementation(
      () => `00000000-0000-4000-8000-${String(++n).padStart(12, '0')}` as `${string}-${string}-${string}-${string}-${string}`,
    );
    vi.useFakeTimers({ toFake: ['Date'] });
    vi.setSystemTime(new Date('2026-11-19T10:30:00.000Z'));
    let written = '';
    window.electronAPI = {
      projectSave: vi.fn(async (_p: string, data: string) => {
        written = data;
      }),
      projectShowSaveDialog: vi.fn(async () => '/songs/project-saved-by-electron.mdaw'),
      projectSetTitle: vi.fn(),
      sampleReadFile: vi.fn(async () => new ArrayBuffer(8)),
    } as unknown as ElectronAPI;
    const { projectManager, arrangementEngine, audioEngine, sampleManager, bus } = await load();
    arrangementEngine.loadFromTemplate((hipHop as unknown as { arrangement: Arrangement }).arrangement);
    arrangementEngine.addMarker('Hook', 16);
    const ids = arrangementEngine.getArrangement().tracks.slice(0, 2).map((t) => t.id);
    arrangementEngine.createGroup('Rhythm', ids);
    const audio = arrangementEngine.addTrack('audio');
    const ref = await sampleManager.loadFromPath('/Users/someone/Samples/vox chop.wav');
    expect(arrangementEngine.addAudioRegion(audio.id, ref.id, 4)).toBeTruthy();
    bus.setMasterVolumeDb(-9);
    audioEngine.setReverbWet(0.25);
    audioEngine.setChorusDepth(0.4);
    audioEngine.setFilterCutoff(9000);
    audioEngine.setFilterResonance(2);
    expect(await projectManager.saveAs()).toBe(true);
    expect(projectManager.getProjectName()).toBe('project-saved-by-electron');
    if (process.env.UPDATE_GOLDENS) writeFileSync(ELECTRON_GOLDEN, written);
    expect(written).toBe(readFileSync(ELECTRON_GOLDEN, 'utf8'));
  });
});
