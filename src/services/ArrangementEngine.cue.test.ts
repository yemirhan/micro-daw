/**
 * Phase-8 review (user decisions 2026-11-18), the Rust side in rune/crates/mdaw-engine
 * (tests_cue.rs) and rune/crates/mdaw-app (t_song_cue.rs):
 * - behaviour change in both apps: the Song cue (edit cursor). While stopped, ruler / marker
 *   clicks, `[` / `]` and the marker list set it; getPosition() returns it; Play starts there,
 *   Stop returns to it, Home sends it to 0;
 * - Electron bug fixed in both apps: "Split at playhead" outside the region pushed a no-op
 *   "Split Region" undo entry.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { toneMock } from '@/test/toneMock';

async function load() {
  vi.resetModules();
  const { arrangementEngine } = await import('@/services/ArrangementEngine');
  const { undoManager } = await import('@/services/UndoManager');
  undoManager.clear();
  return { arrangementEngine, undoManager };
}

beforeEach(() => {
  vi.spyOn(console, 'warn').mockImplementation(() => undefined);
});

describe('Song cue', () => {
  it('is the stopped playhead: located while stopped, Play starts there, Stop returns to it', async () => {
    const { arrangementEngine } = await load();
    const seen: number[] = [];
    arrangementEngine.onPositionChange((b) => seen.push(b));
    expect(arrangementEngine.getPosition()).toBe(0);

    arrangementEngine.locate(12);
    expect(arrangementEngine.getCue()).toBe(12);
    expect(arrangementEngine.getPosition()).toBe(12);
    expect(seen).toEqual([12]); // the stopped playhead redraws

    arrangementEngine.play();
    expect(toneMock.transport.seconds).toBeCloseTo(6); // beat 12 at 120 bpm
    expect(arrangementEngine.getPosition()).toBe(12);

    // Rolling: locate seeks, the cue stays.
    arrangementEngine.locate(20);
    expect(arrangementEngine.getCue()).toBe(12);

    arrangementEngine.stop();
    expect(arrangementEngine.getState()).toBe('stopped');
    expect(arrangementEngine.getPosition()).toBe(12);
    expect(seen.at(-1)).toBe(12);

    arrangementEngine.locate(-3);
    expect(arrangementEngine.getPosition()).toBe(0);
  });

  it('marker jumps move the cue while stopped and a new project starts at 0', async () => {
    const { arrangementEngine } = await load();
    arrangementEngine.addMarker('A', 8);
    arrangementEngine.addMarker('B', 16);
    arrangementEngine.seekToNextMarker();
    expect(arrangementEngine.getPosition()).toBe(8);
    arrangementEngine.seekToNextMarker();
    expect(arrangementEngine.getPosition()).toBe(16);
    arrangementEngine.seekToPreviousMarker();
    expect(arrangementEngine.getPosition()).toBe(8);
    const id = arrangementEngine.getMarkers().find((m) => m.name === 'B')!.id;
    arrangementEngine.seekToMarker(id);
    expect(arrangementEngine.getCue()).toBe(16);

    arrangementEngine.loadProject(structuredClone(arrangementEngine.getArrangement()));
    expect(arrangementEngine.getCue()).toBe(0);
  });

  it('add marker / paste / split at the playhead use the cue', async () => {
    const { arrangementEngine } = await load();
    const t = arrangementEngine.addTrack('synth', 0);
    arrangementEngine.addRegion(t.id, { id: 'r1', startBeat: 0, lengthBeats: 8, notes: [] });
    arrangementEngine.locate(4);
    arrangementEngine.splitRegion(t.id, 'r1', arrangementEngine.getPosition());
    const regions = arrangementEngine.getTracks().find((x) => x.id === t.id)!.regions;
    expect(regions.map((r) => [r.startBeat, r.lengthBeats])).toEqual([
      [0, 4],
      [4, 4],
    ]);
    arrangementEngine.addMarker('Here', arrangementEngine.getPosition());
    expect(arrangementEngine.getMarkers().map((m) => m.beat)).toEqual([4]);
  });
});

describe('split outside the region', () => {
  it('pushes no undo entry', async () => {
    const { arrangementEngine, undoManager } = await load();
    const t = arrangementEngine.addTrack('synth', 0);
    arrangementEngine.addRegion(t.id, { id: 'r1', startBeat: 4, lengthBeats: 4, notes: [] });
    undoManager.clear();
    // "Split at playhead" while stopped at bar 1: the cue (0) is outside the region.
    arrangementEngine.splitRegion(t.id, 'r1', arrangementEngine.getPosition());
    arrangementEngine.splitRegion(t.id, 'r1', 8);
    arrangementEngine.splitRegion(t.id, 'missing', 5);
    expect(undoManager.canUndo()).toBe(false);
    arrangementEngine.splitRegion(t.id, 'r1', 6);
    expect(undoManager.undoLabel()).toBe('Split Region');
  });
});
