/**
 * ArrangementDoc parity: drives the REAL `arrangementEngine` (Tone mock, fake DrumEngine) through
 * long scripted edit sessions and records, after every step, the arrangement JSON, the undo/redo
 * labels, canUndo/canRedo, the session id, a few engine flags and the op's return value. The Rust
 * test (`rune/crates/mdaw-model/src/doc/golden_tests.rs`) replays the same ops on `ArrangementDoc`.
 *
 * Ids from `crypto.randomUUID` are a deterministic `uuid-<n>` counter reset at the start of each
 * session (the Rust side installs the same counter). One engine instance runs every session in
 * order, like the singleton (clipboard, colour index and session id carry over).
 *
 * To keep the fixture small the arrangement is delta-encoded: `arr` holds only the top-level keys
 * whose JSON changed since the previous step (`null` value = key removed); `tracks` is either the
 * full array or, when the count is unchanged, an array with `"="` for unchanged tracks.
 */
import { it, vi } from 'vitest';
import { writeFixture } from './fixture';

vi.mock('@/services/DrumEngine', async () => (await import('@/test/fakeAudioEngine')).drumEngineModule());

type Json = unknown;
type Arr = Record<string, Json> & { tracks: Json[] };

const note = (n: number, startBeat: number, durationBeats = 1, velocity = 0.8, isDrum = false) => ({
  note: n,
  velocity,
  startBeat,
  durationBeats,
  isDrum,
});

const legacyArrangement = () => ({
  id: 'legacy',
  name: 'Old',
  bpm: 110,
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
          lengthBeats: 16,
          notes: [36, 38, 42, 46, 99, 36].map((n, i) => ({ note: n, startBeat: i * 0.5, durationBeats: 0.25, velocity: 100, isDrum: true })),
        },
      ],
      volume: -6,
      pan: 0,
      muted: false,
      solo: false,
      color: '#f00',
    },
    {
      id: 'old-synth',
      name: 'Keys',
      instrument: { type: 'synth', presetIndex: 3 },
      regions: [{ id: 'k', startBeat: 4, lengthBeats: 8, notes: [note(60, 0, 2, 127), note(64, 1, 1, 0.5)] }],
      volume: 0,
      muted: false,
      solo: false,
      color: '#0f0',
    },
  ],
});

it('dumps ArrangementDoc golden sessions', async () => {
  let n = 0;
  vi.spyOn(crypto, 'randomUUID').mockImplementation(() => `uuid-${++n}` as `${string}-${string}-${string}-${string}-${string}`);
  vi.spyOn(console, 'warn').mockImplementation(() => undefined);
  const { arrangementEngine: e } = await import('@/services/ArrangementEngine');
  const { undoManager } = await import('@/services/UndoManager');
  const { ALL_TEMPLATES } = await import('@/data/templates');
  const { SYNTH_PRESETS } = await import('@/audio/presets');
  const engine = e as unknown as Record<string, (...args: unknown[]) => unknown>;

  const sessions: { name: string; steps: Json[] }[] = [];
  let prev: Arr | null = null;

  const delta = (cur: Arr): Record<string, Json> => {
    const out: Record<string, Json> = {};
    const keys = new Set([...Object.keys(cur), ...Object.keys(prev ?? {})]);
    for (const k of keys) {
      const a = prev?.[k];
      const b = cur[k];
      if (JSON.stringify(a) === JSON.stringify(b)) continue;
      if (b === undefined) {
        out[k] = null;
      } else if (k === 'tracks' && prev && Array.isArray(a) && a.length === (b as Json[]).length) {
        out[k] = (b as Json[]).map((t, i) => (JSON.stringify(t) === JSON.stringify(a[i]) ? '=' : t));
      } else {
        out[k] = b;
      }
    }
    return out;
  };

  function session(name: string) {
    n = 0;
    prev = null;
    const steps: Json[] = [];
    sessions.push({ name, steps });
    return (op: string, ...args: unknown[]) => {
      const callArgs = structuredClone(args);
      if (op === 'quantizeSelectedNotes') callArgs[2] = new Set(callArgs[2] as number[]);
      let ret: unknown;
      if (op === 'loadFromLocalStorage') {
        localStorage.setItem('micro-daw-arrangement', args[0] as string);
        ret = e.loadFromLocalStorage();
      } else {
        ret = engine[op](...callArgs);
      }
      const cur = JSON.parse(JSON.stringify(e.getArrangement())) as Arr;
      steps.push({
        op,
        args: JSON.parse(JSON.stringify(args)),
        ret: ret === undefined ? null : JSON.parse(JSON.stringify(ret)),
        arr: delta(cur),
        undoLabel: undoManager.undoLabel() ?? null,
        redoLabel: undoManager.redoLabel() ?? null,
        canUndo: undoManager.canUndo(),
        canRedo: undoManager.canRedo(),
        sessionId: e.getSessionId(),
        loopEnabled: e.isLoopEnabled(),
        hasClipboard: e.hasClipboard(),
        loopRecording: e.isLoopRecording(),
      });
      prev = cur;
      return ret as never;
    };
  }

  const tracks = () => e.getTracks();
  const lastRegion = (ti: number) => tracks()[ti].regions.at(-1)!.id;

  // --- 1: Song editing: tracks, regions, notes, clipboard, effects, instruments, undo/redo ---
  {
    const s = session('song-editing');
    s('resetToStarter');
    const t1 = (s('addTrack', 'synth', 3) as { id: string }).id;
    const t2 = (s('addTrack', 'synth', 99) as { id: string }).id;
    const ta = (s('addTrack', 'audio', 5) as { id: string }).id;
    const td = (s('addTrack', 'drums') as { id: string }).id;
    s('addTrack', 'synth', -1);
    s('setTrackVolume', t1, -12);
    s('setTrackPan', t1, 0.25);
    s('setTrackMute', t2, true);
    s('setTrackSolo', t2, true);
    s('setTrackSolo', t2, false);
    s('setTrackVolume', 'nope', 3);
    s('addRegion', t1, { id: 'r1', startBeat: 0, lengthBeats: 8, name: 'Verse', color: '#123', notes: [note(60, 0, 1), note(62, 2.5, 2), note(64, 5, 4), note(67, 7.75, 0.25)] });
    s('addRegion', t1, { id: 'r2', startBeat: 70, lengthBeats: 4, notes: [note(48, 0.13, 0.5)] });
    s('addRegion', 'missing', { id: 'rx', startBeat: 0, lengthBeats: 4, notes: [] });
    s('moveRegion', t1, 'r2', -3);
    s('moveRegion', t1, 'r2', 101.5);
    s('splitRegion', t1, 'r1', 3);
    s('splitRegion', t1, 'r1', 0);
    s('splitRegion', t1, 'r1', 3);
    const right = lastRegion(0 + tracks().findIndex((t) => t.id === t1));
    s('duplicateRegion', t1, 'r1');
    s('duplicateRegion', t1, right);
    s('resizeRegion', t1, right, 3.5, undefined);
    s('resizeRegion', t1, right, 1, 3);
    s('resizeRegion', t1, right, undefined, 0.1);
    s('quantizeRegionNotes', t1, 'r1', 0.5);
    s('quantizeRegionNotes', t1, 'r2', 0);
    s('quantizeSelectedNotes', t1, 'r2', [0], 0.25);
    s('updateRegionNotes', t1, 'r2', [note(50, 0.3, 0.7, 0.9), note(52, 1.1, 0.2)]);
    s('editRegionNotes', t1, 'r2', [note(50, 0, 1), note(55, 6, 2)], 'Draw Note', 8);
    s('editRegionNotes', t1, 'r2', [note(50, 0, 1)], 'Delete Note', 2);
    s('editRegionNotes', t1, 'nope', [note(50, 0, 1)]);
    s('editRegionNotes', t1, 'r2', [note(51, 0, 1)]);
    s('pasteRegion', t2, 4);
    s('copyRegion', t1, 'r1');
    s('pasteRegion', t2, 16);
    s('pasteRegion', 'missing', 16);
    s('moveRegionToTrack', t1, t2, 'r2');
    s('moveRegionToTrack', t1, t2, 'r2');
    s('removeRegion', t2, 'r2');
    s('performUndo');
    s('performUndo');
    s('performUndo');
    s('performRedo');
    s('performRedo');
    s('setTrackEffect', t1, 'reverb', { enabled: true, wet: 0.5 });
    s('setTrackEffect', t1, 'filter', { type: 'highpass', cutoff: 800, lfoDepth: 0.4 });
    s('setTrackEffect', t1, 'compressor', { enabled: true, ratio: 6 });
    s('setTrackEffect', 'nope', 'delay', { wet: 1 });
    s('setTrackSynthOverrides', t1, { a: 1, env: { attack: 0.1 } });
    s('setTrackSynthOverrides', t1, { b: 2, a: 3 });
    s('setTrackSynthOverrides', td, { a: 1 });
    s('setTrackInstrument', t1, { type: 'drums', presetIndex: 0 });
    s('setTrackInstrument', t1, { type: 'drums', presetIndex: 4 });
    s('setTrackInstrument', t1, { type: 'synth', presetIndex: 2 });
    s('setTrackInstrument', ta, { type: 'audio', presetIndex: 7 });
    s('setTrackInstrument', 'missing', { type: 'synth', presetIndex: 1 });
    s('setBpm', 97);
    s('addRegion', ta, {
      id: 'a1',
      startBeat: 4,
      lengthBeats: 8,
      notes: [],
      name: 'Loop',
      audio: { sampleId: 's1', samplePath: '/s.wav', offsetSeconds: 0.5, gainDb: -3, originalDuration: 6 },
    });
    s('splitRegion', ta, 'a1', 6.5);
    s('resizeRegion', ta, 'a1', 5, undefined);
    s('resizeRegion', ta, 'a1', -2, 0.1);
    s('resizeRegion', ta, 'a1', undefined, 12);
    s('duplicateRegion', ta, 'a1');
    s('removeTrack', 'missing');
    s('removeTrack', t2);
    s('performUndo');
    s('performUndo');
    s('performUndo');
    s('performRedo');
    s('setTrackVolume', t1, -3);
    s('performRedo');
    s('performUndo');
    s('addTrack', 'drums');
    s('performRedo');
  }

  // --- 2: markers, groups, automation, loop markers, undo groups ---
  {
    const s = session('markers-groups-automation');
    s('loadProject', ALL_TEMPLATES[1].arrangement);
    const ids = tracks().map((t) => t.id);
    s('addMarker', 'Intro 2', 4);
    s('addMarker', 'Neg', -4, '#fff');
    s('addMarker', 'Mid', 20);
    const markers = () => e.getMarkers();
    s('updateMarker', markers()[0].id, { beat: 40 });
    s('updateMarker', markers()[1].id, { name: 'V2', color: '#000' });
    s('updateMarker', 'nope', { name: 'x' });
    s('removeMarker', markers()[2].id);
    s('removeMarker', 'nope');
    s('createGroup', 'G1', [ids[0], ids[1]]);
    s('createGroup', 'G2', [ids[1], ids[2]]);
    const groups = () => e.getGroups();
    s('addTrackToGroup', groups()[1].id, ids[0]);
    s('removeTrackFromGroup', groups()[0]?.id ?? 'none', ids[0]);
    s('createGroup', 'G3', [ids[3]]);
    s('renameGroup', groups()[0].id, 'Renamed');
    s('toggleGroupCollapsed', groups()[0].id);
    s('setGroupMute', groups()[0].id, true);
    s('setGroupSolo', groups()[0].id, true);
    s('setGroupSolo', groups()[0].id, false);
    s('removeTrack', ids[3]);
    s('removeGroup', groups()[0].id);
    s('addAutomationLane', ids[1], 'volume');
    s('addAutomationLane', ids[1], 'volume');
    s('addAutomationLane', ids[1], 'filterCutoff');
    s('setAutomationPoint', ids[1], 'volume', 4.1, 0.7, 0.25);
    s('setAutomationPoint', ids[1], 'volume', 1, 1.5, 0);
    s('setAutomationPoint', ids[1], 'volume', 4.0004, 0.2, 0.25);
    s('setAutomationPoint', ids[1], 'volume', 8.37, -1, 0.5);
    s('setAutomationPoint', ids[1], 'pan', 1, 0.5, 0.25);
    s('deleteAutomationPoint', ids[1], 'volume', 1);
    s('toggleAutomationLaneVisibility', ids[1], 'volume');
    s('removeAutomationLane', ids[1], 'filterCutoff');
    s('setLoopEnabled', true);
    s('setLoopMarkers', 0, 1000);
    s('setLoopEnabled', false);
    s('setLoopEnabled', true);
    s('setLoopMarkers', -2, 0.5);
    s('setLoopMarkers', 10, 6);
    s('beginUndoGroup', 'Group A');
    s('addMarker', 'In group', 12);
    s('setLoopMarkers', 4, 12);
    s('beginUndoGroup', 'Inner');
    s('setTrackVolume', ids[0], -20);
    s('endUndoGroup');
    s('endUndoGroup');
    s('endUndoGroup');
    s('beginUndoGroup', 'Empty');
    s('endUndoGroup');
    s('beginUndoGroup', 'No-undo change');
    s('setTrackPan', ids[0], -0.5);
    s('endUndoGroup');
    s('performUndo');
    s('performUndo');
    s('performRedo');
    s('beginUndoGroup', 'Gesture');
    s('setLoopMarkers', 2, 6);
    s('performUndo');
    s('setLoopMarkers', 3, 7);
    s('endUndoGroup');
    s('performUndo');
    s('performRedo');
    s('performRedo');
  }

  // --- 3: beat mode: kits, pads, steps, loop settings, active drum track, gestures ---
  {
    const s = session('beat');
    s('resetToStarter');
    const d = s('getActiveDrumTrackId') as string;
    s('getKit', d);
    s('setLoopSettings', { bars: 1 });
    s('toggleStep', d, 1, 4, 0.25);
    s('toggleStep', d, 1, 4, 0.25);
    s('toggleStep', d, 1, 4, 0.25, 1.5);
    s('toggleStep', d, 0, 0, 0.25);
    s('toggleStep', d, 0, 8, 0.25, 0.3);
    s('toggleStep', d, 3, 2, 0.25);
    s('toggleStep', d, 3, 16, 0.25);
    s('toggleStep', d, 3, -1, 0.25);
    s('toggleStep', d, 3, 1.5, 0.25);
    s('toggleStep', d, 16, 1, 0.25);
    s('toggleStep', d, 2, 1, 0);
    s('setStepVelocity', d, 0, 1, 0.25, 1);
    s('setStepVelocity', d, 0, 8, 0.25, 0.45);
    s('setStepVelocity', d, 3, 2, 0.25, -2);
    s('clearPattern', d, 0);
    s('clearPattern', d, 0);
    s('clearPattern', d);
    s('performUndo');
    s('performUndo');
    s('updatePad', d, 2, { gainDb: -12, pitch: 5 });
    s('updatePad', d, 3, { gainDb: 99, decay: 0, mode: 'gate', chokeGroup: 2.6 });
    s('updatePad', d, 20, { gainDb: 1 });
    s('updatePad', 'nope', 1, { gainDb: 1 });
    s('updatePad', d, 4, { startSec: 0.2, endSec: 0.4, reverse: true });
    s('setPadSample', d, 4, { name: 'Clap 2', source: { kind: 'file', path: '/s/clap2.wav' } });
    s('updatePad', d, 4, { startSec: 0.1, endSec: 0.3 });
    s('setPadSample', d, 4, { name: 'Clap 2b', source: { kind: 'file', path: '/s/clap2.wav', sampleId: 'x' } });
    s('setPadSample', d, 4, { name: '', source: { kind: 'factory', voice: 'snare' } });
    s('setPadSample', d, 4, null);
    s('setPadSample', d, 99, null);
    s('setKit', d, { id: 'k', name: 'Mine', pads: [] });
    s('setKit', d, { id: '', name: '', pads: [{ gainDb: -3 }] }, 'pack-load');
    s('setKit', 'nope', { id: 'k', name: 'Mine', pads: [] });
    s('performUndo');
    s('setSelectedPad', 7);
    s('setSelectedPad', 7);
    s('setSelectedPad', 99);
    s('getSelectedPad');
    s('setLoopMarkers', 8, 12);
    s('setLoopSettings', { bars: 2 });
    s('toggleStep', d, 0, 0, 0.25);
    s('getPatternRegion', d);
    s('ensurePatternRegion', d);
    s('setLoopSettings', { bars: 4 });
    s('ensurePatternRegion', d);
    s('setLoopSettings', { swing: 0.4, quantize: '1/8', swingGrid: '1/8', countInBars: 2, metronome: false, overdub: false });
    s('setLoopSettings', { bars: 3, swing: 5 });
    s('setLoopSettings', { quantize: '1/16T', countInBars: 7 });
    s('getLoopSettings');
    s('setLoopMarkers', 0, 4);
    const d2 = (s('addTrack', 'drums') as { id: string }).id;
    s('setActiveDrumTrack', d2);
    s('setActiveDrumTrack', d2);
    s('toggleStep', d2, 5, 3, 0.5);
    s('ensurePatternRegion', 'missing');
    s('setActiveDrumTrack', d);
    s('armTrackForRecording', d2);
    s('armTrackForRecording', null);
    s('beginUndoGroup', 'Paste pad');
    s('beginUndoGroup', 'Pad Pitch');
    s('updatePad', d, 0, { pitch: 3 });
    s('endUndoGroup');
    s('toggleStep', d, 0, 2, 0.25);
    s('setStepVelocity', d, 0, 2, 0.25, 0.3);
    s('endUndoGroup');
    s('endUndoGroup');
    s('performUndo');
    s('performRedo');
    s('beginUndoGroup', 'Pad Cutoff');
    s('updatePad', d, 0, { filterCutoff: 2000 });
    s('updatePad', d, 0, { filterCutoff: 1500 });
    s('performUndo');
    s('endUndoGroup');
    s('performRedo');
    s('toggleStep', d, 1, 0, 0.25);
    s('performUndo');
    s('beginUndoGroup', 'Pad Gain');
    s('updatePad', d, 1, { gainDb: -6 });
    s('performRedo');
    s('updatePad', d, 1, { gainDb: -9 });
    s('endUndoGroup');
    s('performUndo');
    s('performUndo');
    s('removeTrack', d);
    s('getActiveDrumTrackId');
    s('ensureDrumTrack');
    s('setTrackInstrument', d2, { type: 'drums', presetIndex: 0, kit: { id: 'given', name: 'Given', pads: [{ pitch: 2 }] } });
    s('setTrackInstrument', d2, { type: 'synth', presetIndex: 0 });
    s('getActiveDrumTrackId');
    s('performUndo');
    s('getKit', d2);
  }

  // --- 4: projects: templates, legacy load, snapshots, storage, loop takes ---
  {
    const s = session('projects-and-takes');
    for (const t of ALL_TEMPLATES) {
      s('loadFromTemplate', t.arrangement);
      const first = tracks()[0];
      if (first) s('addRegion', first.id, { id: `x-${t.id}`, startBeat: 2, lengthBeats: 3, notes: [note(70, 0, 1)] });
    }
    s('performUndo');
    s('performUndo');
    s('performRedo');
    s('loadProject', legacyArrangement());
    s('restoreFromSnapshot', { ...legacyArrangement(), id: 'other-project', loopMarkers: { startBeat: 4, endBeat: 12 } });
    s('restoreFromSnapshot', { ...legacyArrangement(), id: 'other-project', name: 'Same id' });
    s('loadFromLocalStorage', JSON.stringify({ ...legacyArrangement(), id: 'stored' }));
    s('loadFromLocalStorage', '{not json');
    s('setLoopEnabled', true);
    s('setLoopMarkers', 0, 64);
    s('setLoopEnabled', true);
    // loop takes (no hits): structure changes, undo/redo/remove/instrument while recording
    s('resetToStarter');
    const d = e.getActiveDrumTrackId()!;
    s('setLoopSettings', { bars: 1, countInBars: 0, metronome: false });
    s('toggleStep', d, 0, 0, 0.25);
    s('setLoopSettings', { bars: 4 });
    s('startLoopRecording');
    s('stopLoopRecording');
    s('startLoopRecording');
    s('stopLoopRecording');
    s('setLoopSettings', { bars: 1 });
    s('startLoopRecording');
    s('setLoopSettings', { bars: 8 });
    s('performUndo');
    s('startLoopRecording');
    s('setLoopSettings', { bars: 2 });
    s('removeTrack', d);
    s('performUndo');
    const d3 = e.getActiveDrumTrackId()!;
    s('setLoopSettings', { bars: 4 });
    s('startLoopRecording');
    s('setTrackInstrument', d3, { type: 'drums', presetIndex: 1 });
    s('setLoopSettings', { bars: 8 });
    s('startLoopRecording');
    s('restoreFromSnapshot', structuredClone(e.getArrangement()));
    s('performRedo');
    s('resetToStarter');
  }

  writeFixture('doc', { presetNames: SYNTH_PRESETS.map((p) => p.name), sessions });
});
