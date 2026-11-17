import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { RegionNote } from '@/types/arrangement';
import { getSoundingNotes, toneMock } from '@/test/toneMock';

// Fresh singletons per test (Tone nodes must be created after resetToneMock()).
async function load() {
  vi.resetModules();
  // Instruments are SynthPlayer objects (src/audio), not raw Tone nodes: record their calls too.
  const { PolyInstrument } = await import('@/audio/PolyInstrument');
  const { SampledPiano } = await import('@/audio/SampledPiano');
  playerCalls.length = 0;
  for (const proto of [PolyInstrument.prototype, SampledPiano.prototype]) {
    const original = proto.triggerAttackRelease;
    vi.spyOn(proto, 'triggerAttackRelease').mockImplementation(function (this: unknown, ...args) {
      if (callDepth === 0) playerCalls.push(args); // outermost instrument only (not its internal fallback)
      callDepth++;
      try {
        return original.apply(this as never, args);
      } finally {
        callDepth--;
      }
    });
  }
  const { arrangementEngine } = await import('@/services/ArrangementEngine');
  const { undoManager } = await import('@/services/UndoManager');
  undoManager.clear();
  return { arrangementEngine, undoManager };
}

const note = (n: number, startBeat: number, durationBeats = 1): RegionNote => ({ note: n, startBeat, durationBeats, velocity: 0.8, isDrum: false });

const playerCalls: unknown[][] = [];
let callDepth = 0;

function attackReleaseCalls(): unknown[][] {
  const calls: unknown[][] = [...playerCalls];
  for (const node of toneMock.instances) {
    const fn = (node as unknown as { triggerAttackRelease?: { mock?: { calls: unknown[][] } } }).triggerAttackRelease;
    if (fn?.mock) calls.push(...fn.mock.calls);
  }
  return calls;
}

describe('ArrangementEngine piano roll support', () => {
  beforeEach(() => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
  });

  it('editRegionNotes is a single undo step and can grow the region', async () => {
    const { arrangementEngine, undoManager } = await load();
    const track = arrangementEngine.addTrack('synth', 0);
    arrangementEngine.addRegion(track.id, { id: 'r1', startBeat: 4, lengthBeats: 4, notes: [note(60, 0)] });
    const before = structuredClone(arrangementEngine.getArrangement().tracks[0].regions[0].notes);

    arrangementEngine.editRegionNotes(track.id, 'r1', [note(60, 0), note(64, 6, 2)], 'Draw Note', 8);
    const region = arrangementEngine.getArrangement().tracks[0].regions[0];
    expect(region.notes).toHaveLength(2);
    expect(region.lengthBeats).toBe(8);
    expect(undoManager.undoLabel()).toBe('Draw Note');

    arrangementEngine.performUndo();
    const restored = arrangementEngine.getArrangement().tracks[0].regions[0];
    expect(restored.notes).toEqual(before);
    expect(restored.lengthBeats).toBe(4);
  });

  it('never shrinks the region through editRegionNotes', async () => {
    const { arrangementEngine } = await load();
    const track = arrangementEngine.addTrack('synth', 0);
    arrangementEngine.addRegion(track.id, { id: 'r1', startBeat: 0, lengthBeats: 16, notes: [] });
    arrangementEngine.editRegionNotes(track.id, 'r1', [note(60, 0)], 'Edit', 4);
    expect(arrangementEngine.getArrangement().tracks[0].regions[0].lengthBeats).toBe(16);
  });

  it('previewNote is a self-releasing one-shot (no stuck notes)', async () => {
    const { arrangementEngine } = await load();
    const track = arrangementEngine.addTrack('synth', 0);
    arrangementEngine.previewNote(track.id, 64, 0.9, 0.3);
    arrangementEngine.previewNote(track.id, 64, 0.9, 0.3); // re-trigger
    const calls = attackReleaseCalls().filter((c) => c[0] === 'E4');
    expect(calls).toHaveLength(2);
    expect(calls[0][1]).toBeCloseTo(0.3);
    expect(calls[0][3]).toBeCloseTo(0.9);
    expect(getSoundingNotes()).toHaveLength(0);
  });

  it('previewNote ignores unknown tracks and clamps silly input', async () => {
    const { arrangementEngine } = await load();
    const track = arrangementEngine.addTrack('synth', 0);
    expect(() => arrangementEngine.previewNote('nope', 60)).not.toThrow();
    arrangementEngine.previewNote(track.id, 200, 5, 99);
    const call = attackReleaseCalls().at(-1)!;
    expect(call[0]).toBe('G9'); // clamped to MIDI 127
    expect(call[1]).toBeLessThanOrEqual(2);
    expect(call[3]).toBeLessThanOrEqual(1);
  });

  it('reschedules only the edited track while playing (transport keeps running)', async () => {
    const { arrangementEngine } = await load();
    const track = arrangementEngine.addTrack('synth', 0);
    arrangementEngine.addRegion(track.id, { id: 'r1', startBeat: 0, lengthBeats: 4, notes: [note(60, 0)] });
    arrangementEngine.play();
    const partsBefore = toneMock.instances.filter((n) => n.__className === 'Part');
    expect(partsBefore).toHaveLength(1);
    const transportStop = toneMock.transport.stop as unknown as { mock: { calls: unknown[] } };
    const stopsBefore = transportStop.mock.calls.length;

    arrangementEngine.editRegionNotes(track.id, 'r1', [note(60, 0), note(67, 2)], 'Draw Note');

    const parts = toneMock.instances.filter((n) => n.__className === 'Part');
    expect(parts).toHaveLength(2);
    expect(parts[0].disposed).toBe(true);
    expect(parts[1].disposed).toBeFalsy();
    expect(transportStop.mock.calls.length).toBe(stopsBefore);
    expect(arrangementEngine.getState()).toBe('playing');
    arrangementEngine.stop();
  });

  it('undo while playing keeps every track scheduled and resumes from the same beat', async () => {
    const { arrangementEngine } = await load();
    const synth = arrangementEngine.addTrack('synth', 0);
    const drums = arrangementEngine.addTrack('drums', 0);
    arrangementEngine.addRegion(synth.id, { id: 'r1', startBeat: 0, lengthBeats: 8, notes: [note(60, 0)] });
    arrangementEngine.addRegion(drums.id, { id: 'r2', startBeat: 0, lengthBeats: 8, notes: [{ ...note(36, 0), isDrum: true }] });
    arrangementEngine.play();
    toneMock.transport.seconds = 2; // beat 4 at 120 bpm
    arrangementEngine.editRegionNotes(synth.id, 'r1', [note(60, 0), note(67, 2)], 'Draw Note');

    arrangementEngine.performUndo();

    expect(arrangementEngine.getState()).toBe('playing');
    expect(toneMock.transport.state).toBe('started');
    expect(arrangementEngine.getPosition()).toBeCloseTo(4);
    const liveParts = toneMock.instances.filter((n) => n.__className === 'Part' && !n.disposed);
    expect(liveParts).toHaveLength(2); // one per track, on the freshly created nodes
    expect(arrangementEngine.getArrangement().tracks[0].regions[0].notes).toHaveLength(1);
    arrangementEngine.stop();
  });

  it('changing a track instrument while playing reschedules it', async () => {
    const { arrangementEngine } = await load();
    const track = arrangementEngine.addTrack('synth', 0);
    arrangementEngine.addRegion(track.id, { id: 'r1', startBeat: 0, lengthBeats: 4, notes: [note(60, 0)] });
    arrangementEngine.play();
    arrangementEngine.setTrackInstrument(track.id, { type: 'synth', presetIndex: 1 });
    expect(arrangementEngine.getState()).toBe('playing');
    expect(toneMock.instances.filter((n) => n.__className === 'Part' && !n.disposed)).toHaveLength(1);
    arrangementEngine.stop();
  });

  it('normalises legacy 0..127 note velocities when loading a template or snapshot', async () => {
    const { arrangementEngine } = await load();
    const { ALL_TEMPLATES } = await import('@/data/templates');
    for (const t of ALL_TEMPLATES) {
      for (const tr of t.arrangement.tracks) {
        for (const r of tr.regions) for (const n of r.notes) expect(n.velocity).toBeLessThanOrEqual(1);
      }
    }
    const legacy = structuredClone(arrangementEngine.getArrangement());
    legacy.tracks = [
      {
        id: 't1', name: 'Old', instrument: { type: 'synth', presetIndex: 0 }, volume: 0, muted: false, solo: false,
        regions: [{ id: 'r1', startBeat: 0, lengthBeats: 4, notes: [{ ...note(60, 0), velocity: 127 }, { ...note(62, 1), velocity: 0.5 }] }],
      } as never,
    ];
    arrangementEngine.loadFromTemplate(legacy);
    const notes = arrangementEngine.getArrangement().tracks[0].regions[0].notes;
    expect(notes.map((n) => n.velocity)).toEqual([1, 0.5]);
  });
});
