/**
 * Review round 5, engine side:
 *  N1 Stop during a count-in cancels the pending transport start.
 *  N4 A seek before the scheduled start took effect restarts from the new beat.
 *  N2 Project session id + whole-kit selection events (PackService drops a stale "Get & load").
 *  N5 Which Song audio regions use an unavailable sample; change signal on in-place reload.
 * (Real-Tone timing: scripts/probes/n1-countin-abort.mjs, n4-seek-pending.mjs via npm run test:audio.)
 */
import * as Tone from 'tone';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ElectronAPI } from '@/types/electron';
import type { KitState } from '@/types/sampler';
import { advanceClock, createFakeAudioBuffer, toneMock } from '@/test/toneMock';

vi.mock('@/services/DrumEngine', async () => (await import('@/test/fakeAudioEngine')).drumEngineModule());

vi.mock('@/audio/sampler/SamplerKit', () => {
  class SamplerKit {
    kit: KitState;
    setKit = vi.fn((kit: KitState) => {
      this.kit = kit;
    });
    trigger = vi.fn();
    release = vi.fn();
    stopAll = vi.fn();
    constructor(kit: KitState) {
      this.kit = kit;
    }
    get ready() {
      return Promise.resolve();
    }
    connect() {
      return this;
    }
    getKit() {
      return this.kit;
    }
    dispose() {}
  }
  return { SamplerKit };
});

type EngineInternals = { transportStart: { time: number; beat: number } | null };

async function load() {
  vi.resetModules();
  const { arrangementEngine } = await import('@/services/ArrangementEngine');
  const { sampleManager } = await import('@/services/SampleManager');
  const { undoManager } = await import('@/services/UndoManager');
  undoManager.clear();
  arrangementEngine.resetToStarter();
  arrangementEngine.setBpm(120);
  return { engine: arrangementEngine, sampleManager, undoManager };
}

const transport = () => Tone.getTransport() as unknown as { state: string; stop: ReturnType<typeof vi.fn>; start: ReturnType<typeof vi.fn> };

beforeEach(() => {
  vi.spyOn(console, 'warn').mockImplementation(() => undefined);
});

afterEach(() => {
  delete window.electronAPI;
  vi.restoreAllMocks();
});

describe('N1: Stop during a count-in', () => {
  it('cancels the pending transport start even though Tone reports the transport stopped', async () => {
    const { engine } = await load();
    engine.setLoopSettings({ bars: 1, countInBars: 1, metronome: false });
    engine.startLoopRecording();
    const start = (engine as unknown as EngineInternals).transportStart!;
    expect(start.time).toBeGreaterThan(toneMock.clock.now + 1); // count-in: start ~2 s ahead
    // Real Tone reads 'stopped' until the scheduled start: the old code skipped stop() here.
    transport().state = 'stopped';
    transport().stop.mockClear();
    advanceClock(0.5);
    engine.stop();
    expect(transport().stop).toHaveBeenCalled();
    expect(engine.getState()).toBe('stopped');
    expect((engine as unknown as EngineInternals).transportStart).toBeNull();
  });

  it('a stopped transport with no pending start is not stopped again (B1: keeps the earlier stop event)', async () => {
    const { engine } = await load();
    engine.play();
    advanceClock(1);
    engine.stop();
    transport().stop.mockClear();
    advanceClock(1);
    engine.stop();
    expect(transport().stop).not.toHaveBeenCalled();
  });
});

describe('N4: seek before the scheduled start took effect', () => {
  it('restarts playback from the new beat (no frame / step from the old position)', async () => {
    const { engine } = await load();
    engine.play();
    const internals = engine as unknown as EngineInternals;
    // Real Tone starts at now + look-ahead: still pending right after Play.
    internals.transportStart!.time = toneMock.clock.now + 0.1;
    advanceClock(0.03);
    transport().stop.mockClear();
    transport().start.mockClear();
    engine.seek(3);
    expect(transport().stop).toHaveBeenCalled();
    expect(transport().start).toHaveBeenCalledTimes(1);
    expect(engine.getState()).toBe('playing');
    expect(internals.transportStart!.beat).toBe(3);
    expect(Tone.getTransport().seconds).toBeCloseTo(1.5); // 3 beats at 120 BPM
    expect(engine.getPosition()).toBe(3);
  });

  it('keeps pattern playback (transient pattern loop) across the restart', async () => {
    const { engine } = await load();
    engine.playPattern();
    (engine as unknown as EngineInternals).transportStart!.time = toneMock.clock.now + 0.1;
    engine.seek(1);
    expect(engine.isPatternPlayback()).toBe(true);
    expect(engine.getState()).toBe('playing');
  });

  it('a seek after the start took effect just moves the transport (no restart)', async () => {
    const { engine } = await load();
    engine.play();
    advanceClock(0.5);
    transport().start.mockClear();
    engine.seek(2);
    expect(transport().start).not.toHaveBeenCalled();
    expect(Tone.getTransport().seconds).toBeCloseTo(1);
  });
});

describe('N2: project session + kit selection events', () => {
  it('the session changes on new / template / open, not on edits or undo', async () => {
    const { engine, undoManager } = await load();
    const s0 = engine.getSessionId();
    engine.addTrack('synth');
    engine.setBpm(100);
    expect(engine.getSessionId()).toBe(s0);
    expect(undoManager.canUndo()).toBe(true);
    engine.performUndo();
    engine.performRedo();
    expect(engine.getSessionId()).toBe(s0);

    engine.resetToStarter();
    const s1 = engine.getSessionId();
    expect(s1).not.toBe(s0);
    engine.loadFromTemplate(structuredClone(engine.getArrangement()));
    const s2 = engine.getSessionId();
    expect(s2).not.toBe(s1);
    // Re-opening the same project is still a new session.
    engine.loadProject(structuredClone(engine.getArrangement()));
    const s3 = engine.getSessionId();
    expect(s3).not.toBe(s2);
    // A restore of ANOTHER project (open via restoreFromSnapshot) too.
    engine.restoreFromSnapshot({ ...structuredClone(engine.getArrangement()), id: 'other-project' });
    expect(engine.getSessionId()).not.toBe(s3);
  });

  it('setKit reports the selection with its source; pad edits and undo do not', async () => {
    const { engine } = await load();
    const trackId = engine.getActiveDrumTrackId()!;
    const seen: unknown[] = [];
    const off = engine.onKitSelected((e) => seen.push(e));
    const kit = { ...engine.getKit(trackId), id: 'k1', name: 'K1' };
    engine.setKit(trackId, kit);
    engine.setKit(trackId, { ...kit, id: 'k2' }, 'pack-load');
    engine.setKit(trackId, { ...kit, id: 'k1' }); // same kit again: still an explicit choice
    engine.updatePad(trackId, 0, { gainDb: -6 });
    off();
    engine.setKit(trackId, kit);
    expect(seen).toEqual([
      { trackId, kitId: 'k1', source: 'user' },
      { trackId, kitId: 'k2', source: 'pack-load' },
      { trackId, kitId: 'k1', source: 'user' },
    ]);
  });

  it('isLoopRecording reflects a take (PackService defers a kit swap until it stops)', async () => {
    const { engine } = await load();
    engine.setLoopSettings({ countInBars: 0, metronome: false });
    engine.startLoopRecording();
    expect(engine.isLoopRecording()).toBe(true);
    engine.stop();
    expect(engine.isLoopRecording()).toBe(false);
  });
});

describe('N5: Song audio regions and unavailable / reloaded samples', () => {
  const FILE = '/data/packs/uzu-drumkit/kick/10_bd.wav';
  let read: ReturnType<typeof vi.fn>;
  beforeEach(() => {
    read = vi.fn(async () => new ArrayBuffer(8));
    window.electronAPI = { sampleReadFile: read } as unknown as ElectronAPI;
    const raw = Tone.getContext().rawContext as unknown as { decodeAudioData: ReturnType<typeof vi.fn> };
    raw.decodeAudioData.mockImplementation(async () => createFakeAudioBuffer(1, 44100, 44100));
  });

  it('lists the audio regions whose pack sample became unavailable and signals the change', async () => {
    const { engine, sampleManager } = await load();
    const ref = await sampleManager.loadFromPath(FILE);
    const other = await sampleManager.loadFromPath('/Users/me/loop.wav');
    const track = engine.addTrack('audio');
    const region = engine.addAudioRegion(track.id, ref.id, 0)!;
    engine.addAudioRegion(track.id, other.id, 8);
    expect(engine.getUnavailableAudioRegions()).toEqual([]);
    const changed: string[][] = [];
    const off = engine.onSamplesChanged((ids) => changed.push(ids));
    const rev0 = engine.getSampleRevision(ref.id);

    read.mockRejectedValueOnce(new Error('ENOENT'));
    await sampleManager.reloadByPathPrefix('/data/packs/uzu-drumkit/');
    expect(engine.getUnavailableAudioRegions()).toEqual([{ trackId: track.id, regionId: region.id, sampleId: ref.id }]);
    expect(engine.isAudioRegionUnavailable(region)).toBe(true);
    expect(changed).toEqual([[ref.id]]);
    const rev1 = engine.getSampleRevision(ref.id);
    expect(rev1).not.toBe(rev0);

    // The pack is back (reinstalled): audio reloaded in place, warning gone, waveform revision bumps.
    await sampleManager.reloadByPathPrefix('/data/packs/uzu-drumkit/');
    off();
    expect(engine.getUnavailableAudioRegions()).toEqual([]);
    expect(changed).toEqual([[ref.id], [ref.id]]);
    expect(engine.getSampleRevision(ref.id)).not.toBe(rev1);
  });
});
