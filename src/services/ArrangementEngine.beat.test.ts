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

    // Note repeat hands the sink a look-ahead time: it is passed through
    // (sample accurate); a time already passed is clamped to now.
    toneMock.clock.now = 7;
    sink.hit(4, 0.6, 7.08);
    expect(sampler.trigger).toHaveBeenLastCalledWith(4, 0.6, 7.08);
    sink.release(4, 7.2);
    expect(sampler.release).toHaveBeenLastCalledWith(4, 7.2);
    sink.hit(4, 0.6, 6.5);
    expect(sampler.trigger).toHaveBeenLastCalledWith(4, 0.6, 7);
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

  it('stores steps on the straight grid (swing is a playback setting) and finds them again', async () => {
    const { engine } = await load();
    const id = engine.ensureDrumTrack();
    engine.setLoopSettings({ bars: 1, swing: 0.5 });
    engine.toggleStep(id, 2, 1, 0.25, 0.6);
    expect(notesOf(engine, id)).toEqual([{ pad: 2, beat: 0.25, v: 0.6 }]);
    // changing swing does not move or orphan the stored step
    engine.setLoopSettings({ swing: 0 });
    engine.setStepVelocity(id, 2, 1, 0.25, 0.4);
    expect(notesOf(engine, id)[0].v).toBe(0.4);
    engine.setLoopSettings({ swing: 0.5 });
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
    engine.setLoopSettings({ bars: 1 });
    expect(engine.getLoopMarkers()).toEqual({ startBeat: 8, endBeat: 12 });
    engine.setLoopSettings({ bars: 2 });
    expect(engine.getLoopMarkers()).toEqual({ startBeat: 8, endBeat: 16 });
    engine.toggleStep(id, 0, 0, 0.25);
    expect(engine.getPatternRegion(id)).toMatchObject({ startBeat: 8, lengthBeats: 8 });
  });

  it('only a change of bars rewrites the loop markers', async () => {
    const { engine } = await load();
    engine.ensureDrumTrack();
    engine.setLoopMarkers(4, 7);
    engine.setLoopSettings({ swing: 0.4, quantize: '1/8', metronome: false, overdub: false, countInBars: 2 });
    expect(engine.getLoopMarkers()).toEqual({ startBeat: 4, endBeat: 7 });
    engine.setLoopSettings({ bars: 4 });
    expect(engine.getLoopMarkers()).toEqual({ startBeat: 4, endBeat: 20 });
  });

  it('a note in the last half-step is found on the last step', async () => {
    const { engine } = await load();
    const id = engine.ensureDrumTrack();
    engine.setLoopSettings({ bars: 1 });
    const region = engine.ensurePatternRegion(id);
    engine.editRegionNotes(id, region.id, [{ note: padToNote(3), velocity: 0.8, startBeat: 3.9, durationBeats: 0.1, isDrum: true }]);
    engine.toggleStep(id, 3, 15, 0.25); // the grid shows it on step 15: toggling removes it
    expect(notesOf(engine, id)).toEqual([]);
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

  it('quantizes against the swung grid but stores straight; triplet grids', async () => {
    const { engine, id } = await recording({ swing: 0.6 });
    hitAt(engine, 0.4, 2); // played on the swung position of step 1 (0.25 + 0.15)
    expect(notesOf(engine, id)[0].beat).toBe(0.25);
    engine.setLoopSettings({ quantize: '1/8T' });
    hitAt(engine, 0.7, 3);
    expect(notesOf(engine, id)[1].beat).toBeCloseTo(2 / 3, 6);
  });

  it('note-repeat hits on a finer grid than the quantize keep their own grid', async () => {
    const { engine, id } = await recording({ quantize: '1/16' });
    const repeatAt = (beat: number, pad: number) => {
      toneMock.transport.seconds = beat * SEC_PER_BEAT;
      engine.captureNoteOn(padToNote(pad), 0.8, true, undefined, { gridBeats: 0.125 });
    };
    // A 1/32 roll: every hit is kept (a 1/16 quantize would merge pairs).
    for (const b of [1, 1.125, 1.25, 1.375]) repeatAt(b, 0);
    const notes = engine.getPatternRegion(id)!.notes.filter((n) => n.note === padToNote(0));
    expect(notes.map((n) => n.startBeat)).toEqual([1, 1.125, 1.25, 1.375]);
    expect(notes.every((n) => n.durationBeats === 0.125)).toBe(true);
    // A coarser hint than the quantize changes nothing: quantize wins.
    toneMock.transport.seconds = 2.2 * SEC_PER_BEAT;
    engine.captureNoteOn(padToNote(1), 0.8, true, undefined, { gridBeats: 0.5 });
    expect(notesOf(engine, id).find((n) => n.pad === 1)!.beat).toBe(2.25);
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
    expect(toneMock.transport.start).toHaveBeenCalledWith(expect.closeTo(4 * SEC_PER_BEAT, 1));
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

describe('review regressions: timing, count-in, stop, gate, swing, undo, active track', () => {
  const PPQ = 192;
  /** Make the mock transport report ticks (beats × PPQ) from an audio time. */
  function tickClock(beatAt: (time: number) => number) {
    const fn = vi.fn((time: number) => beatAt(time) * PPQ);
    Object.assign(toneMock.transport, { PPQ, getTicksAtTime: fn });
    return fn;
  }

  it('position comes from transport ticks, so a tempo change does not corrupt it', async () => {
    const { engine } = await load();
    engine.ensureDrumTrack();
    engine.play();
    // 3 s played at 120 bpm = beat 6; then the tempo drops to 60.
    toneMock.transport.seconds = 3;
    tickClock(() => 6);
    engine.setBpm(60);
    expect(engine.getPosition()).toBe(6); // seconds × new bpm would say 3
    engine.stop();
  });

  it('captureNoteOn/Off take an optional audio time for the position', async () => {
    const { engine } = await load();
    const id = engine.ensureDrumTrack();
    engine.setLoopSettings({ bars: 1, quantize: '1/16', countInBars: 0, swing: 0, metronome: false });
    engine.startLoopRecording();
    const ticks = tickClock((t) => t * 2); // 120 bpm from time 0
    toneMock.clock.now = 0.1;
    engine.captureNoteOn(padToNote(1), 0.8, true, 1.5); // a scheduled (note repeat) hit at beat 3
    expect(ticks).toHaveBeenCalledWith(1.5);
    expect(notesOf(engine, id)).toEqual([{ pad: 1, beat: 3, v: 0.8 }]);
    engine.captureNoteOff(padToNote(1), 1.6);
    engine.stopLoopRecording();
  });

  it('count-in clicks live on their own synth, disposed on stop; re-recording works', async () => {
    const { engine } = await load();
    engine.ensureDrumTrack();
    engine.setLoopSettings({ bars: 1, countInBars: 2, metronome: true });
    engine.startLoopRecording();
    const countIn = instancesOf('Synth').at(-1)!;
    expect(countIn.triggerAttackRelease).toHaveBeenCalledTimes(8);
    engine.stop();
    expect(countIn.dispose).toHaveBeenCalled();
    expect(engine.getState()).toBe('stopped');

    engine.startLoopRecording();
    const second = instancesOf('Synth').at(-1)!;
    expect(second).not.toBe(countIn);
    expect(engine.isLoopRecording()).toBe(true);
    engine.stop();
  });

  it('a failing start rolls back to stopped and notifies', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const { engine } = await load();
    engine.ensureDrumTrack();
    engine.setLoopSettings({ bars: 1, countInBars: 1 });
    const markers = engine.getLoopMarkers();
    const onState = vi.fn();
    engine.onStateChange(onState);
    toneMock.transport.start.mockImplementationOnce(() => {
      throw new Error('The time must be greater than or equal to the last scheduled time');
    });
    expect(() => engine.startLoopRecording()).not.toThrow();
    expect(engine.getState()).toBe('stopped');
    expect(engine.isLoopRecording()).toBe(false);
    expect(engine.isLoopEnabled()).toBe(false);
    expect(engine.getLoopMarkers()).toEqual(markers);
    expect(onState).toHaveBeenCalled();
    // and it can start again
    engine.startLoopRecording();
    expect(engine.getState()).toBe('recording');
    engine.stop();
  });

  it('the metronome never clicks at/before its last click and never throws from the clock', async () => {
    const { engine } = await load();
    engine.ensureDrumTrack();
    engine.setMetronomeEnabled(true);
    engine.play();
    const loop = instancesOf('Loop').at(-1)!;
    const tick = loop.__args[0] as (time: number) => void;
    const synth = instancesOf('Synth')[0];
    tick(1);
    tick(1); // same time again (tempo change / loop wrap)
    tick(0.5);
    expect(synth.triggerAttackRelease).toHaveBeenCalledTimes(1);
    synth.triggerAttackRelease.mockImplementationOnce(() => {
      throw new Error('The time must be greater than or equal to the last scheduled time');
    });
    expect(() => tick(2)).not.toThrow();
    tick(2.5);
    expect(synth.triggerAttackRelease).toHaveBeenCalledTimes(3);
    engine.stop();
  });

  it('stop silences the sampler voices of every drum track (and loop-record stop too)', async () => {
    const { engine } = await load();
    engine.addTrack('drums');
    engine.addTrack('drums');
    const live = () => samplers.filter((s) => !s.disposed) as unknown as { stopAll: ReturnType<typeof vi.fn> }[];
    engine.play();
    engine.stop();
    for (const s of live()) expect(s.stopAll).toHaveBeenCalledTimes(1);
    engine.setLoopSettings({ countInBars: 0 });
    engine.startLoopRecording();
    engine.stopLoopRecording();
    for (const s of live()) expect(s.stopAll.mock.calls.length).toBeGreaterThanOrEqual(2);
  });

  it('loop recording: gate pads get their held length (wrapped, capped); one-shots a fixed one', async () => {
    const { engine } = await load();
    const id = engine.ensureDrumTrack();
    engine.updatePad(id, 2, { mode: 'gate' });
    engine.setLoopSettings({ bars: 1, quantize: '1/16', countInBars: 0, swing: 0, metronome: false });
    engine.startLoopRecording();
    const dur = (pad: number) => engine.getPatternRegion(id)!.notes.find((n) => n.note === padToNote(pad))!.durationBeats;

    // gate pad: hit at beat 3.5, released 1 beat later (across the loop wrap at 4)
    toneMock.clock.now = 10;
    hitAt(engine, 3.5, 2);
    toneMock.clock.now = 10 + SEC_PER_BEAT;
    toneMock.transport.seconds = 0.5 * SEC_PER_BEAT;
    engine.captureNoteOff(padToNote(2));
    expect(dur(2)).toBeCloseTo(1, 6);

    // one-shot: fixed grid length regardless of how long it is held
    toneMock.clock.now = 20;
    hitAt(engine, 1, 0);
    engine.captureNoteOff(padToNote(0), 20 + 3 * SEC_PER_BEAT);
    expect(dur(0)).toBe(0.25);

    // held longer than the loop: capped at the loop length
    toneMock.clock.now = 30;
    hitAt(engine, 2, 2);
    engine.captureNoteOff(padToNote(2), 30 + 9 * SEC_PER_BEAT);
    expect(engine.getPatternRegion(id)!.notes.find((n) => n.note === padToNote(2) && n.startBeat === 2)!.durationBeats).toBe(4);
    engine.stopLoopRecording();
  });

  it('swing is applied at playback to odd steps of drum patterns and re-applied when it changes', async () => {
    const { engine } = await load();
    const id = engine.ensureDrumTrack();
    engine.setLoopSettings({ bars: 1, quantize: '1/16', swing: 0 });
    engine.toggleStep(id, 0, 0, 0.25);
    engine.toggleStep(id, 0, 1, 0.25);
    engine.play();
    const times = () => {
      const part = instancesOf('Part').at(-1)!;
      return (part.__args[1] as { time: number; beat: number }[]).map((e) => [e.beat, e.time]);
    };
    expect(times()).toEqual([[0, 0], [0.25, 0.25 * SEC_PER_BEAT]]);
    engine.setLoopSettings({ swing: 0.5 }); // rebuilt live: audible on the recorded loop
    expect(times()).toEqual([[0, 0], [0.25, 0.375 * SEC_PER_BEAT]]);
    engine.setLoopSettings({ quantize: '1/8T' }); // the record quantize does not change playback
    expect(times()).toEqual([[0, 0], [0.25, 0.375 * SEC_PER_BEAT]]);
    engine.setLoopSettings({ swingGrid: '1/8' }); // 0.25 is not on the 1/8 swing grid: straight
    expect(times()).toEqual([[0, 0], [0.25, 0.25 * SEC_PER_BEAT]]);
    engine.stop();
  });

  it('undo groups nest: one entry with the outermost label; no clone per edit inside', async () => {
    const { engine, undoManager } = await load();
    const id = engine.ensureDrumTrack();
    const depth = () => (undoManager as unknown as { undoStack: unknown[] }).undoStack.length;
    const start = depth();
    engine.beginUndoGroup('Paste pad');
    engine.beginUndoGroup('Pad Pitch'); // e.g. a knob gesture inside
    engine.updatePad(id, 0, { pitch: 3 });
    engine.endUndoGroup(); // inner end must not commit
    expect(depth()).toBe(start);
    engine.toggleStep(id, 0, 0, 0.25);
    engine.setStepVelocity(id, 0, 0, 0.25, 0.3);
    engine.endUndoGroup();
    expect(depth()).toBe(start + 1);
    expect(undoManager.undoLabel()).toBe('Paste pad');
    engine.endUndoGroup(); // unbalanced end: ignored
    expect(depth()).toBe(start + 1);
    engine.performUndo();
    expect(engine.getKit(id).pads[0].pitch).toBe(0);
    expect(notesOf(engine, id)).toEqual([]);
  });

  it('an empty undo group adds no entry', async () => {
    const { engine, undoManager } = await load();
    engine.ensureDrumTrack();
    const before = undoManager.undoLabel();
    const n = (undoManager as unknown as { undoStack: unknown[] }).undoStack.length;
    engine.beginUndoGroup('Nothing');
    engine.endUndoGroup();
    expect((undoManager as unknown as { undoStack: unknown[] }).undoStack.length).toBe(n);
    expect(undoManager.undoLabel()).toBe(before);
  });

  it('redo twice after undo twice restores all changes', async () => {
    const { engine } = await load();
    const id = engine.ensureDrumTrack();
    engine.toggleStep(id, 0, 0, 0.25);
    engine.toggleStep(id, 0, 4, 0.25);
    engine.toggleStep(id, 0, 8, 0.25);
    engine.performUndo();
    engine.performUndo();
    expect(notesOf(engine, id)).toHaveLength(1);
    engine.performRedo();
    expect(notesOf(engine, id)).toHaveLength(2);
    engine.performRedo();
    expect(notesOf(engine, id)).toHaveLength(3);
    engine.performUndo();
    expect(notesOf(engine, id)).toHaveLength(2);
  });

  it('arming a drum track makes it the active drum track (pads play it)', async () => {
    const { engine } = await load();
    const a = engine.addTrack('drums');
    const b = engine.addTrack('drums');
    expect(engine.getActiveDrumTrackId()).toBe(a.id);
    engine.armTrackForRecording(b.id);
    expect(engine.getActiveDrumTrackId()).toBe(b.id);
    const sink = fakeDrumEngine.setSink.mock.calls.at(-1)![0] as { hit: (pad: number, v: number) => void };
    sink.hit(2, 1);
    const samplerB = samplers.filter((s) => !s.disposed).at(-1)!;
    expect(samplerB.trigger).toHaveBeenCalledWith(2, 1, expect.any(Number));
    const synth = engine.addTrack('synth');
    engine.armTrackForRecording(synth.id);
    expect(engine.getActiveDrumTrackId()).toBe(b.id);
  });
});

describe('review round 2 regressions (engine, undo, timing)', () => {
  const partEvents = () =>
    (instancesOf('Part').at(-1)!.__args[1] as { time: number; beat: number; note: RegionNote }[]).map((e) => [e.beat, e.time]);

  it('#1 a template without loop markers gets [0, bars*4); playPattern loops the pattern range', async () => {
    const { engine } = await load();
    const arr = legacyArrangement();
    arr.tracks[0].regions[0].lengthBeats = 16;
    arr.tracks[0].regions[0].notes = [{ note: 36, startBeat: 2.5, durationBeats: 0.25, velocity: 0.8, isDrum: true }, { note: 36, startBeat: 10.5, durationBeats: 0.25, velocity: 0.8, isDrum: true }];
    engine.loadFromTemplate(arr);
    expect(engine.getLoopMarkers()).toEqual({ startBeat: 0, endBeat: 8 });
    engine.playPattern();
    expect(engine.getState()).toBe('playing');
    expect(engine.isLoopEnabled()).toBe(true);
    expect(toneMock.transport.loop).toBe(true);
    expect(toneMock.transport.loopStart).toBe(0);
    expect(toneMock.transport.loopEnd).toBe(8 * SEC_PER_BEAT);
    engine.stop();

    // Song-view markers elsewhere: the pattern starts at the loop start, length = bars
    engine.setLoopMarkers(16, 20);
    engine.setLoopSettings({ bars: 1 });
    engine.setLoopMarkers(16, 30);
    engine.playPattern();
    expect(engine.getLoopMarkers()).toEqual({ startBeat: 16, endBeat: 20 });
    expect(toneMock.transport.loopStart).toBe(16 * SEC_PER_BEAT);
    expect(toneMock.transport.loopEnd).toBe(20 * SEC_PER_BEAT);
    expect(engine.getPosition()).toBeCloseTo(16, 6);
    engine.stop();
  });

  it('#1 loading saved data without markers migrates them too', async () => {
    const { engine } = await load();
    const { ARRANGEMENT_STORAGE_KEY } = await import('@/utils/constants');
    localStorage.setItem(ARRANGEMENT_STORAGE_KEY, JSON.stringify(legacyArrangement()));
    expect(engine.loadFromLocalStorage()).toBe(true);
    expect(engine.getLoopMarkers()).toEqual({ startBeat: 0, endBeat: 8 });
  });

  it('#2 undo during the count-in of an empty take: the take ends, the last step is undone, redo kept', async () => {
    vi.useFakeTimers();
    const { engine, undoManager } = await load();
    const id = engine.ensureDrumTrack();
    engine.toggleStep(id, 0, 0, 0.25);
    engine.setLoopSettings({ bars: 4, countInBars: 1, metronome: false }); // the take grows the region
    engine.startLoopRecording();
    expect(engine.getCountInRemaining()).toBeGreaterThan(0);
    engine.performUndo();
    expect(engine.isLoopRecording()).toBe(false);
    expect(engine.getState()).toBe('stopped');
    expect(notesOf(engine, id)).toEqual([]);
    expect(undoManager.canRedo()).toBe(true);
    expect(undoManager.undoLabel()).not.toBe('Loop Record');
    engine.performRedo();
    expect(notesOf(engine, id)).toEqual([{ pad: 0, beat: 0, v: 0.8 }]);
  });

  it('#2 undo mid-take with hits undoes the take (one step); redo brings it back', async () => {
    const { engine, undoManager } = await load();
    const id = engine.ensureDrumTrack();
    engine.setLoopSettings({ bars: 1, countInBars: 0, metronome: false, quantize: '1/16', swing: 0 });
    engine.toggleStep(id, 5, 0, 0.25);
    engine.startLoopRecording();
    hitAt(engine, 1, 1);
    hitAt(engine, 2, 2);
    engine.performUndo();
    expect(engine.isLoopRecording()).toBe(false);
    expect(notesOf(engine, id)).toEqual([{ pad: 5, beat: 0, v: 0.8 }]);
    expect(undoManager.undoLabel()).toBe('Toggle Step');
    engine.performRedo();
    expect(notesOf(engine, id)).toHaveLength(3);
  });

  it('#2 undo inside an open gesture group undoes the gesture only; its end adds nothing', async () => {
    const { engine, undoManager } = await load();
    const id = engine.ensureDrumTrack();
    engine.toggleStep(id, 0, 0, 0.25);
    engine.toggleStep(id, 0, 4, 0.25);
    engine.beginUndoGroup('Pad Cutoff'); // knob gesture still open (400 ms window)
    engine.updatePad(id, 0, { filterCutoff: 2000 });
    engine.updatePad(id, 0, { filterCutoff: 1500 });
    engine.performUndo();
    expect(engine.getKit(id).pads[0].filterCutoff).toBe(20000);
    expect(notesOf(engine, id)).toHaveLength(2);
    engine.endUndoGroup();
    expect(undoManager.undoLabel()).toBe('Toggle Step');
    expect(undoManager.canRedo()).toBe(true);
    engine.performRedo();
    expect(engine.getKit(id).pads[0].filterCutoff).toBe(1500);
    expect(notesOf(engine, id)).toHaveLength(2);
  });

  it('#2 a redo inside an open gesture that changed something is a no-op (the gesture is the newest step)', async () => {
    const { engine, undoManager } = await load();
    const id = engine.ensureDrumTrack();
    engine.toggleStep(id, 0, 0, 0.25);
    engine.performUndo();
    engine.beginUndoGroup('Pad Gain');
    engine.updatePad(id, 1, { gainDb: -6 });
    engine.performRedo();
    expect(notesOf(engine, id)).toEqual([]);
    engine.updatePad(id, 1, { gainDb: -9 });
    engine.endUndoGroup();
    expect(undoManager.undoLabel()).toBe('Pad Gain');
    engine.performUndo();
    expect(engine.getKit(id).pads[1].gainDb).toBe(-6);
    engine.performUndo();
    expect(engine.getKit(id).pads[1].gainDb).toBe(0);
  });

  it('#2 restoring a snapshot never writes history', async () => {
    const { engine, undoManager } = await load();
    const id = engine.ensureDrumTrack();
    engine.toggleStep(id, 0, 0, 0.25);
    engine.setLoopSettings({ bars: 4, countInBars: 0, metronome: false });
    const snap = structuredClone(engine.getArrangement());
    engine.startLoopRecording(); // grows the pattern (structure change)
    const depth = (undoManager as unknown as { undoStack: unknown[] }).undoStack.length;
    engine.restoreFromSnapshot(snap);
    expect((undoManager as unknown as { undoStack: unknown[] }).undoStack.length).toBe(depth);
    expect(engine.isLoopRecording()).toBe(false);
  });

  it('#3 switching the active drum track releases the pads held on the old sink', async () => {
    const { engine } = await load();
    const a = engine.addTrack('drums');
    const b = engine.addTrack('drums');
    const sinkA = fakeDrumEngine.setSink.mock.calls.at(-1)![0] as { hit: (p: number, v: number) => void; release: (p: number) => void };
    const samplerA = samplers[0];
    sinkA.hit(6, 1);
    sinkA.hit(7, 1);
    sinkA.release(7);
    samplerA.release.mockClear();
    engine.setActiveDrumTrack(b.id);
    expect(samplerA.release).toHaveBeenCalledTimes(1);
    expect(samplerA.release).toHaveBeenCalledWith(6, expect.any(Number));
    // nothing held any more: switching back and forth releases nothing new
    engine.setActiveDrumTrack(a.id);
    expect(samplerA.release).toHaveBeenCalledTimes(1);
  });

  it('#5 a gate pad held while the tempo changes gets its musical (tick) length', async () => {
    const { engine } = await load();
    const id = engine.ensureDrumTrack();
    engine.updatePad(id, 2, { mode: 'gate' });
    engine.setLoopSettings({ bars: 1, quantize: '1/16', countInBars: 0, swing: 0, metronome: false });
    engine.startLoopRecording();
    // Transport position in beats from a tick clock (as real Tone).
    let beatNow = 0;
    const t = toneMock.transport as unknown as { getTicksAtTime: (time: number) => number; PPQ: number };
    t.getTicksAtTime = () => beatNow * t.PPQ;
    toneMock.clock.now = 10;
    beatNow = 1;
    engine.captureNoteOn(padToNote(2), 0.8, true);
    // half a beat at 120 BPM (0.25 s), then the tempo halves: the next half beat takes 0.5 s
    engine.setBpm(60);
    toneMock.clock.now = 10.75;
    beatNow = 2;
    engine.captureNoteOff(padToNote(2));
    const note = engine.getPatternRegion(id)!.notes.find((n) => n.note === padToNote(2))!;
    expect(note.durationBeats).toBeCloseTo(1, 6); // seconds × final BPM would say 0.75
    engine.stopLoopRecording();
  });

  it('#7 swing uses its own grid: quantize 1/4 + swing does not delay beats 2 and 4', async () => {
    const { engine } = await load();
    const id = engine.ensureDrumTrack();
    engine.setLoopSettings({ bars: 1, quantize: '1/4', swing: 0.5 });
    expect(engine.getLoopSettings().swingGrid).toBe('1/16');
    for (const step of [0, 1, 2, 3]) engine.toggleStep(id, 0, step, 1);
    engine.play();
    expect(partEvents()).toEqual([[0, 0], [1, 1 * SEC_PER_BEAT], [2, 2 * SEC_PER_BEAT], [3, 3 * SEC_PER_BEAT]]);
    engine.stop();
  });

  it('#7 loop recording snaps to the quantize grid against the sounding swing of the swing grid', async () => {
    const { engine, id } = await (async () => {
      const ctx = await load();
      ctx.engine.setLoopSettings({ bars: 1, quantize: '1/8', swing: 0.5, swingGrid: '1/16', countInBars: 0, metronome: false });
      ctx.engine.startLoopRecording();
      return { ...ctx, id: ctx.engine.getActiveDrumTrackId()! };
    })();
    hitAt(engine, 0.5, 1); // on an 8th (even 16th): plays straight
    hitAt(engine, 1.45, 2);
    expect(notesOf(engine, id).map((n) => n.beat)).toEqual([0.5, 1.5]);
    engine.stopLoopRecording();
  });

  it('#11 note lengths follow the tempo at play time, not when the Part was built', async () => {
    const { engine } = await load();
    const id = engine.ensureDrumTrack();
    engine.setLoopSettings({ bars: 1 });
    engine.toggleStep(id, 0, 0, 0.25);
    const r = engine.getPatternRegion(id)!;
    engine.updateRegionNotes(id, r.id, r.notes.map((n) => ({ ...n, durationBeats: 0.5 })));
    engine.updatePad(id, 0, { mode: 'gate' });
    engine.play();
    const part = instancesOf('Part').at(-1)!;
    const [callback, events] = part.__args as [(time: number, ev: unknown) => void, unknown[]];
    engine.setBpm(60);
    const sampler = samplers.filter((s) => !s.disposed).at(-1)!;
    callback(toneMock.clock.now + 1, events[0]);
    const [, , at] = sampler.trigger.mock.calls.at(-1)!;
    const [, releaseAt] = sampler.release.mock.calls.at(-1)!;
    expect(at).toBeCloseTo(toneMock.clock.now + 1, 6);
    expect(releaseAt - at).toBeCloseTo(0.5, 6); // 0.5 beat at 60 BPM (was 0.25 s at the old 120)
    engine.stop();
  });

  it('#12 the beat-view metronome setting clicks during plain pattern playback', async () => {
    const { engine } = await load();
    engine.ensureDrumTrack();
    engine.setLoopSettings({ metronome: true });
    engine.playPattern();
    expect(instancesOf('Loop')).toHaveLength(1);
    engine.setLoopSettings({ metronome: false });
    expect(instancesOf('Loop')[0].dispose).toHaveBeenCalled();
    engine.stop();
    // song-view play: only the global metronome toggle clicks
    engine.setLoopSettings({ metronome: true });
    engine.play();
    expect(instancesOf('Loop')).toHaveLength(1);
    engine.stop();
  });

  it('#13 a new recorded hit is added to the running Part instead of rebuilding it', async () => {
    const { engine } = await load();
    const id = engine.ensureDrumTrack();
    engine.setLoopSettings({ bars: 1, quantize: '1/16', countInBars: 0, swing: 0.5, metronome: false });
    engine.toggleStep(id, 0, 0, 0.25);
    engine.startLoopRecording();
    const parts = instancesOf('Part').length;
    const part = instancesOf('Part').at(-1)!;
    hitAt(engine, 1.25, 3);
    expect(instancesOf('Part')).toHaveLength(parts);
    const add = (part as unknown as { add: ReturnType<typeof vi.fn> }).add;
    expect(add).toHaveBeenCalledTimes(1);
    expect(add.mock.calls[0][0]).toMatchObject({ beat: 1.25, time: 1.375 * SEC_PER_BEAT, note: { note: padToNote(3) } });
    // a louder hit on the same step merges (velocity): the Part is rebuilt
    hitAt(engine, 1.25, 3, 1);
    expect(instancesOf('Part').length).toBe(parts + 1);
    engine.stopLoopRecording();
  });

  it('#14 new drum tracks are numbered', async () => {
    const { engine } = await load();
    expect(engine.addTrack('drums').name).toBe('Drums');
    expect(engine.addTrack('drums').name).toBe('Drums 2');
    const synth = engine.addTrack('synth');
    engine.setTrackInstrument(synth.id, { type: 'drums', presetIndex: 0 });
    expect(engine.getTracks().find((t) => t.id === synth.id)!.name).toBe('Drums 3');
  });

  it('#10 setTrackEffect(compressor) puts the compressor in the path only while enabled', async () => {
    const { engine } = await load();
    engine.loadFromTemplate(legacyArrangement());
    const id = engine.getTracks()[0].id;
    const eq = instancesOf('EQ3').at(-1)!;
    const comp = instancesOf('Compressor').at(-1)!;
    const chorus = instancesOf('Chorus').at(-1)!;
    const target = () => eq.connect.mock.calls.at(-1)?.[0];
    expect(target()).toBe(chorus);
    engine.setTrackEffect(id, 'compressor', { enabled: true, ratio: 4 });
    expect(target()).toBe(comp);
    engine.setTrackEffect(id, 'compressor', { enabled: false });
    expect(target()).toBe(chorus);
  });
});
