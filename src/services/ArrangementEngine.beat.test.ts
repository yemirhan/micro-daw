import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Arrangement, RegionNote } from '@/types/arrangement';
import { padToNote, type KitState } from '@/types/sampler';
import { advanceClock, instancesOf, toneMock } from '@/test/toneMock';
import { fakeDrumEngine, resetFakeEngines } from '@/test/fakeAudioEngine';

vi.mock('@/services/DrumEngine', async () => (await import('@/test/fakeAudioEngine')).drumEngineModule());

/** Records what drum tracks do with their sampler (no buffers, no audio). */
interface FakeSampler {
  kit: KitState;
  setKit: ReturnType<typeof vi.fn>;
  trigger: ReturnType<typeof vi.fn>;
  release: ReturnType<typeof vi.fn>;
  disposed: boolean;
}
const g = globalThis as typeof globalThis & { __fakeSamplers?: FakeSampler[] };
const samplers = (g.__fakeSamplers ??= []);

vi.mock('@/audio/sampler/SamplerKit', () => {
  class SamplerKit {
    kit: KitState;
    disposed = false;
    setKit = vi.fn((kit: KitState) => {
      this.kit = kit;
    });
    trigger = vi.fn();
    release = vi.fn();
    stopAll = vi.fn();
    constructor(kit: KitState) {
      this.kit = kit;
      (globalThis as typeof globalThis & { __fakeSamplers: unknown[] }).__fakeSamplers.push(this);
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
    dispose() {
      this.disposed = true;
    }
  }
  return { SamplerKit };
});

const BPM = 120;
const SEC_PER_BEAT = 60 / BPM;

async function load() {
  vi.resetModules();
  samplers.length = 0;
  const { arrangementEngine } = await import('@/services/ArrangementEngine');
  const { undoManager } = await import('@/services/UndoManager');
  undoManager.clear();
  return { engine: arrangementEngine, undoManager };
}

type Engine = Awaited<ReturnType<typeof load>>['engine'];

const notesOf = (engine: Engine, trackId: string) =>
  engine.getPatternRegion(trackId)?.notes.map((n) => ({ pad: n.note - 36, beat: n.startBeat, v: n.velocity })) ?? [];

/** Put the (mock) transport at an absolute beat and hit a pad as useMidiNotes does. */
function hitAt(engine: Engine, beat: number, pad: number, velocity = 0.8) {
  toneMock.transport.seconds = beat * SEC_PER_BEAT;
  engine.captureNoteOn(padToNote(pad), velocity, true);
}

const legacyArrangement = (): Arrangement => ({
  id: 'a',
  name: 'Old',
  bpm: BPM,
  lengthBeats: 16,
  tracks: [
    {
      id: 'old-drums',
      name: 'Drums',
      instrument: { type: 'drums', presetIndex: 0 },
      regions: [
        {
          id: 'r',
          startBeat: 0,
          lengthBeats: 4,
          notes: [36, 38, 42, 46, 99].map<RegionNote>((note, i) => ({ note, startBeat: i * 0.5, durationBeats: 0.25, velocity: 100, isDrum: true })),
        },
      ],
      volume: -6,
      pan: 0,
      muted: false,
      solo: false,
      color: '#f00',
    },
  ],
});

beforeEach(() => {
  resetFakeEngines();
  vi.spyOn(console, 'warn').mockImplementation(() => undefined);
});

afterEach(() => {
  vi.useRealTimers();
});

describe('drum tracks and the active drum track', () => {
  it('a new drum track gets the factory kit, becomes active and receives live pad hits', async () => {
    const { engine } = await load();
    expect(engine.getActiveDrumTrackId()).toBeNull();
    const id = engine.ensureDrumTrack();
    expect(engine.getActiveDrumTrackId()).toBe(id);
    expect(engine.getKit(id).pads).toHaveLength(16);
    expect(engine.getKit(id).pads[0].sample?.source).toEqual({ kind: 'factory', voice: 'kick' });
    expect(engine.ensureDrumTrack()).toBe(id);

    const sink = fakeDrumEngine.setSink.mock.calls.at(-1)?.[0];
    expect(sink).toBeTruthy();
    sink.hit(3, 0.5);
    sink.release(3);
    const sampler = samplers.at(-1)!;
    expect(sampler.trigger).toHaveBeenCalledWith(3, 0.5, expect.any(Number));
    expect(sampler.release).toHaveBeenCalledWith(3, expect.any(Number));
  });

  it('re-registers the sink when the active track changes or is removed', async () => {
    const { engine } = await load();
    const a = engine.addTrack('drums');
    const b = engine.addTrack('drums');
    expect(engine.getActiveDrumTrackId()).toBe(a.id);
    const sinkA = fakeDrumEngine.setSink.mock.calls.at(-1)?.[0];

    engine.setActiveDrumTrack(b.id);
    const sinkB = fakeDrumEngine.setSink.mock.calls.at(-1)?.[0];
    expect(sinkB).not.toBe(sinkA);
    sinkB.hit(0, 1);
    expect(samplers[1].trigger).toHaveBeenCalled();
    expect(samplers[0].trigger).not.toHaveBeenCalled();

    engine.removeTrack(b.id);
    expect(engine.getActiveDrumTrackId()).toBe(a.id);
    engine.removeTrack(a.id);
    expect(engine.getActiveDrumTrackId()).toBeNull();
    expect(fakeDrumEngine.setSink).toHaveBeenLastCalledWith(null);
  });

  it('re-registers the sink after undo rebuilds the track audio', async () => {
    const { engine } = await load();
    const id = engine.ensureDrumTrack();
    engine.updatePad(id, 0, { gainDb: -3 });
    const before = fakeDrumEngine.setSink.mock.calls.at(-1)?.[0];
    engine.performUndo();
    const after = fakeDrumEngine.setSink.mock.calls.at(-1)?.[0];
    expect(after).not.toBe(before);
    after.hit(1, 1);
    expect(samplers.at(-1)!.trigger).toHaveBeenCalledWith(1, 1, expect.any(Number));
  });
});

describe('kit edits', () => {
  it('updatePad / setPadSample update the sampler and are undoable', async () => {
    const { engine, undoManager } = await load();
    const id = engine.ensureDrumTrack();
    const sampler = samplers.at(-1)!;
    const onKit = vi.fn();
    engine.onKitChange(onKit);

    engine.updatePad(id, 2, { gainDb: -12, pitch: 5 });
    expect(engine.getKit(id).pads[2]).toMatchObject({ gainDb: -12, pitch: 5 });
    expect(sampler.setKit).toHaveBeenLastCalledWith(engine.getKit(id));
    expect(onKit).toHaveBeenCalled();
    expect(undoManager.undoLabel()).toBe('Edit Pad');

    engine.updatePad(id, 4, { startSec: 0.2, endSec: 0.4 });
    engine.setPadSample(id, 4, { name: 'Clap 2', source: { kind: 'file', path: '/s/clap2.wav' } });
    expect(engine.getKit(id).pads[4]).toMatchObject({ sample: { name: 'Clap 2' }, startSec: 0, endSec: null });

    engine.performUndo();
    expect(engine.getKit(id).pads[4]).toMatchObject({ startSec: 0.2, endSec: 0.4, sample: { name: 'Clap' } });
    engine.performUndo();
    engine.performUndo();
    expect(engine.getKit(id).pads[2]).toMatchObject({ gainDb: 0, pitch: 0 });
    // the rebuilt track sampler got the restored kit
    expect(samplers.at(-1)!.kit.pads[2].gainDb).toBe(0);
  });

  it('a knob drag inside an undo group is one undo step', async () => {
    const { engine, undoManager } = await load();
    const id = engine.ensureDrumTrack();
    const depth = () => (undoManager as unknown as { undoStack: unknown[] }).undoStack.length;
    const start = depth();
    engine.beginUndoGroup('Pad Pitch');
    for (const pitch of [1, 2, 3, 4]) engine.updatePad(id, 0, { pitch });
    engine.endUndoGroup();
    expect(depth()).toBe(start + 1);
    expect(undoManager.undoLabel()).toBe('Pad Pitch');
    engine.performUndo();
    expect(engine.getKit(id).pads[0].pitch).toBe(0);
  });

  it('setKit replaces the whole kit (validated)', async () => {
    const { engine } = await load();
    const id = engine.ensureDrumTrack();
    engine.setKit(id, { id: 'k', name: 'Mine', pads: [] });
    expect(engine.getKit(id)).toMatchObject({ id: 'k', name: 'Mine' });
    expect(engine.getKit(id).pads).toHaveLength(16);
    engine.performUndo();
    expect(engine.getKit(id).name).toBe('Factory');
  });
});

describe('step pattern editing', () => {
  it('toggleStep adds and removes notes at the step position', async () => {
    const { engine, undoManager } = await load();
    const id = engine.ensureDrumTrack();
    engine.setLoopSettings({ bars: 1 });
    expect(engine.getLoopMarkers()).toEqual({ startBeat: 0, endBeat: 4 });

    engine.toggleStep(id, 1, 4, 0.25);
    expect(notesOf(engine, id)).toEqual([{ pad: 1, beat: 1, v: 0.8 }]);
    expect(engine.getPatternRegion(id)).toMatchObject({ startBeat: 0, lengthBeats: 4 });
    expect(undoManager.undoLabel()).toBe('Toggle Step');

    engine.toggleStep(id, 1, 4, 0.25);
    expect(notesOf(engine, id)).toEqual([]);
    engine.performUndo();
    expect(notesOf(engine, id)).toEqual([{ pad: 1, beat: 1, v: 0.8 }]);

    // steps past the loop are ignored
    engine.toggleStep(id, 1, 16, 0.25);
    expect(notesOf(engine, id)).toHaveLength(1);
  });

  it('stores swung positions and finds them again', async () => {
    const { engine } = await load();
    const id = engine.ensureDrumTrack();
    engine.setLoopSettings({ bars: 1, swing: 0.5 });
    engine.toggleStep(id, 2, 1, 0.25, 0.6);
    expect(notesOf(engine, id)).toEqual([{ pad: 2, beat: 0.375, v: 0.6 }]);
    engine.setStepVelocity(id, 2, 1, 0.25, 0.3);
    expect(notesOf(engine, id)[0].v).toBe(0.3);
    engine.toggleStep(id, 2, 1, 0.25);
    expect(notesOf(engine, id)).toEqual([]);
  });

  it('setStepVelocity is a no-op on empty steps; clearPattern clears one pad or all', async () => {
    const { engine, undoManager } = await load();
    const id = engine.ensureDrumTrack();
    engine.setLoopSettings({ bars: 1 });
    engine.toggleStep(id, 0, 0, 0.25);
    engine.toggleStep(id, 0, 8, 0.25);
    engine.toggleStep(id, 3, 2, 0.25);
    const label = undoManager.undoLabel();
    engine.setStepVelocity(id, 0, 1, 0.25, 1);
    expect(undoManager.undoLabel()).toBe(label);

    engine.clearPattern(id, 0);
    expect(notesOf(engine, id)).toEqual([{ pad: 3, beat: 0.5, v: 0.8 }]);
    engine.clearPattern(id);
    expect(notesOf(engine, id)).toEqual([]);
    engine.performUndo();
    engine.performUndo();
    expect(notesOf(engine, id)).toHaveLength(3);
  });

  it('pattern follows the loop start', async () => {
    const { engine } = await load();
    const id = engine.ensureDrumTrack();
    engine.setLoopMarkers(8, 12);
    engine.setLoopSettings({ bars: 2 });
    expect(engine.getLoopMarkers()).toEqual({ startBeat: 8, endBeat: 16 });
    engine.toggleStep(id, 0, 0, 0.25);
    expect(engine.getPatternRegion(id)).toMatchObject({ startBeat: 8, lengthBeats: 8 });
  });
});

describe('GM migration on load', () => {
  it('restoreFromSnapshot migrates kitless drum tracks', async () => {
    const { engine } = await load();
    engine.restoreFromSnapshot(legacyArrangement());
    const track = engine.getTracks()[0];
    expect(track.instrument.kit?.pads).toHaveLength(16);
    expect(track.regions[0].notes.map((n) => n.note)).toEqual([36, 37, 38, 39]);
    expect(track.regions[0].notes.every((n) => n.velocity <= 1)).toBe(true);
    expect(engine.getActiveDrumTrackId()).toBe('old-drums');
    // a second restore (undo snapshot) must not migrate again
    engine.restoreFromSnapshot(structuredClone(engine.getArrangement()));
    expect(engine.getTracks()[0].regions[0].notes.map((n) => n.note)).toEqual([36, 37, 38, 39]);
  });

  it('templates load onto the factory kit with remapped beat state', async () => {
    const { engine } = await load();
    const { ALL_TEMPLATES } = await import('@/data/templates');
    const hipHop = ALL_TEMPLATES.find((t) => t.id.includes('hip'))!;
    engine.loadFromTemplate(hipHop.arrangement);
    const drums = engine.getTracks().find((t) => t.instrument.type === 'drums')!;
    expect(engine.getActiveDrumTrackId()).toBe(drums.id);
    expect(drums.instrument.kit?.id).toBe('factory');
    const pads = new Set(drums.regions.flatMap((r) => r.notes.map((n) => n.note - 36)));
    // kick, snare, closed hat, open hat, clap
    expect([...pads].sort()).toEqual([0, 1, 2, 3, 4]);
    expect(console.warn).not.toHaveBeenCalled();
  });
});

describe('loop recording', () => {
  async function recording(settings: Parameters<Engine['setLoopSettings']>[0] = {}) {
    const ctx = await load();
    ctx.engine.setLoopSettings({ bars: 1, quantize: '1/16', countInBars: 0, swing: 0, metronome: false, ...settings });
    ctx.engine.startLoopRecording();
    const id = ctx.engine.getActiveDrumTrackId()!;
    return { ...ctx, id };
  }

  it('creates the drum track + pattern region and loops the pattern range', async () => {
    const { engine, id } = await recording({ bars: 2 });
    expect(engine.isLoopRecording()).toBe(true);
    expect(engine.getState()).toBe('recording');
    expect(engine.getPatternRegion(id)).toMatchObject({ startBeat: 0, lengthBeats: 8 });
    expect(engine.isLoopEnabled()).toBe(true);
    expect(toneMock.transport.loop).toBe(true);
    expect(toneMock.transport.loopEnd).toBe(8 * SEC_PER_BEAT);
    expect(toneMock.transport.start).toHaveBeenCalled();

    engine.stop();
    expect(engine.isLoopRecording()).toBe(false);
    expect(engine.getState()).toBe('stopped');
  });

  it('quantizes hits and wraps them into the loop', async () => {
    const { engine, id } = await recording();
    hitAt(engine, 0.52, 1, 0.9); // → 0.5
    hitAt(engine, 3.97, 0, 0.7); // → 4 → wraps to 0
    hitAt(engine, 6.1, 2, 0.5); // second pass position 2.1 → 2
    expect(notesOf(engine, id)).toEqual([
      { pad: 1, beat: 0.5, v: 0.9 },
      { pad: 0, beat: 0, v: 0.7 },
      { pad: 2, beat: 2, v: 0.5 },
    ]);
  });

  it('applies swing and triplet grids when recording', async () => {
    const { engine, id } = await recording({ swing: 0.5 });
    hitAt(engine, 0.3, 2);
    expect(notesOf(engine, id)[0].beat).toBe(0.375);
    engine.setLoopSettings({ quantize: '1/8T' });
    hitAt(engine, 0.7, 3);
    expect(notesOf(engine, id)[1].beat).toBeCloseTo(2 / 3, 6);
  });

  it('dedupes hits on the same step and pad (louder wins)', async () => {
    const { engine, id } = await recording();
    hitAt(engine, 1.02, 0, 0.4);
    hitAt(engine, 4.98, 0, 0.9); // next pass, same step
    hitAt(engine, 1.0, 0, 0.6);
    expect(notesOf(engine, id)).toEqual([{ pad: 0, beat: 1, v: 0.9 }]);
  });

  it('overdub keeps the existing pattern', async () => {
    const { engine } = await load();
    const id = engine.ensureDrumTrack();
    engine.setLoopSettings({ bars: 1, countInBars: 0, overdub: true, metronome: false });
    engine.toggleStep(id, 5, 0, 0.25);
    engine.startLoopRecording();
    hitAt(engine, 2, 1);
    expect(notesOf(engine, id).map((n) => n.pad).sort()).toEqual([1, 5]);
  });

  it('replace (overdub off) clears the pattern on the first hit only', async () => {
    const { engine } = await load();
    const id = engine.ensureDrumTrack();
    engine.setLoopSettings({ bars: 1, countInBars: 0, overdub: false, metronome: false });
    engine.toggleStep(id, 5, 0, 0.25);
    engine.toggleStep(id, 6, 4, 0.25);
    engine.startLoopRecording();
    expect(notesOf(engine, id)).toHaveLength(2); // nothing cleared until the first hit
    hitAt(engine, 2, 1);
    hitAt(engine, 5, 2); // second pass: kept
    expect(notesOf(engine, id)).toEqual([
      { pad: 1, beat: 2, v: 0.8 },
      { pad: 2, beat: 1, v: 0.8 },
    ]);
  });

  it('the whole take is one undo step', async () => {
    const { engine, undoManager } = await load();
    const id = engine.ensureDrumTrack();
    engine.setLoopSettings({ bars: 1, countInBars: 0, metronome: false });
    engine.toggleStep(id, 5, 0, 0.25);
    engine.startLoopRecording();
    hitAt(engine, 1, 1);
    hitAt(engine, 2, 2);
    engine.stopLoopRecording();
    expect(undoManager.undoLabel()).toBe('Loop Record');
    engine.performUndo();
    expect(notesOf(engine, id)).toEqual([{ pad: 5, beat: 0, v: 0.8 }]);
  });

  it('ignores synth notes and note-offs', async () => {
    const { engine, id } = await recording();
    toneMock.transport.seconds = 1;
    engine.captureNoteOn(60, 0.8, false);
    engine.captureNoteOff(60);
    engine.captureNoteOff(padToNote(0));
    expect(notesOf(engine, id)).toEqual([]);
  });

  it('merges hits into playback live and does not replay a hit that was snapped forward', async () => {
    const { engine, id } = await recording();
    hitAt(engine, 0.95, 4); // snapped forward to beat 1: the pattern must not retrigger it this pass
    const part = instancesOf('Part').at(-1)!;
    const [callback, events] = part.__args as [(time: number, ev: unknown) => void, { beat: number; note: RegionNote }[]];
    const ev = events.find((e) => e.note.note === padToNote(4))!;
    expect(ev.beat).toBe(1);
    const sampler = samplers.find((s) => s.trigger.mock.calls.length === 0 && !s.disposed)!;
    callback(toneMock.clock.now + 0.03, ev);
    expect(sampler.trigger).not.toHaveBeenCalled();
    callback(toneMock.clock.now + 2, ev); // next pass plays it
    expect(sampler.trigger).toHaveBeenCalledWith(4, 0.8, expect.any(Number));
    expect(engine.getPatternRegion(id)?.notes).toHaveLength(1);
  });

  it('count-in: remaining beats, early hits ignored, pre-roll hits land on beat 1', async () => {
    vi.useFakeTimers();
    const { engine, id } = await recording({ countInBars: 1, metronome: true });
    const onState = vi.fn();
    engine.onStateChange(onState);
    expect(engine.getCountInRemaining()).toBe(4);
    // 4 count-in clicks scheduled before the transport starts
    const click = instancesOf('Synth').at(-1)!;
    expect(click.triggerAttackRelease).toHaveBeenCalledTimes(4);
    expect(toneMock.transport.start).toHaveBeenCalledWith(4 * SEC_PER_BEAT);
    // metronome keeps clicking during the loop (loop setting)
    expect(instancesOf('Loop')).toHaveLength(1);

    engine.captureNoteOn(padToNote(1), 0.8, true); // during count-in: ignored
    expect(notesOf(engine, id)).toEqual([]);

    advanceClock(1.1);
    vi.advanceTimersByTime(1100);
    expect(engine.getCountInRemaining()).toBe(2);
    expect(onState).toHaveBeenCalled();

    advanceClock(0.75); // 0.15 s (0.3 beat) before the downbeat
    engine.captureNoteOn(padToNote(2), 0.8, true);
    expect(notesOf(engine, id)).toEqual([{ pad: 2, beat: 0, v: 0.8 }]);

    advanceClock(0.2);
    vi.advanceTimersByTime(1000);
    expect(engine.getCountInRemaining()).toBe(0);
    engine.stopLoopRecording();
  });

  it('starts the metronome during loop play only when the setting is on', async () => {
    const { engine } = await recording({ metronome: false });
    expect(instancesOf('Loop')).toHaveLength(0);
    engine.setLoopSettings({ metronome: true });
    expect(instancesOf('Loop')).toHaveLength(1);
    engine.stopLoopRecording();
    expect(instancesOf('Loop')[0].dispose).toHaveBeenCalled();
  });
});

describe('starter project', () => {
  it('resetToStarter: one active "Drums" track with the factory kit, default loop, no undo history', async () => {
    const { engine, undoManager } = await load();
    engine.addTrack('synth');
    engine.resetToStarter();

    const tracks = engine.getTracks();
    expect(tracks).toHaveLength(1);
    expect(tracks[0]).toMatchObject({ name: 'Drums', instrument: { type: 'drums' } });
    expect(engine.getActiveDrumTrackId()).toBe(tracks[0].id);
    expect(engine.getKit(tracks[0].id).id).toBe('factory');
    expect(engine.getLoopSettings()).toMatchObject({ bars: 2, quantize: '1/16', metronome: true });
    expect(engine.getLoopMarkers()).toEqual({ startBeat: 0, endBeat: 8 });
    expect(undoManager.canUndo()).toBe(false);
    expect(fakeDrumEngine.setSink).toHaveBeenLastCalledWith(expect.objectContaining({ hit: expect.any(Function) }));
  });
});
