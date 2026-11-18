/**
 * ArrangementDoc parity: drives the REAL `arrangementEngine` (Tone mock, fake DrumEngine) through
 * long scripted edit sessions plus seeded random ones, and records, after every step, the
 * arrangement JSON, the undo/redo labels, canUndo/canRedo, the session id, a few engine flags,
 * the op's return value and the engine side effects (`fx`). The Rust test
 * (`rune/crates/mdaw-model/src/doc/golden_tests.rs`) replays the same ops on `ArrangementDoc`.
 *
 * Ids from `crypto.randomUUID` are a deterministic `uuid-<n>` counter reset at the start of each
 * session (the Rust side installs the same counter). One engine instance runs every session in
 * order, like the singleton (clipboard, colour index and session id carry over).
 *
 * Side effects (`fx`): spies on the engine's private methods that the Rust doc reports as
 * `DocEvent`s. Only top-level calls are logged (a spied method called from inside another spied
 * method is not), e.g. `create:<trackId>`, `dispose:<trackId>`, `reschedule:<trackId>`,
 * `syncDrumSink`, `applyLoop`, `solo`, `effect:<name>`, `resetEffects:<trackId>`, `metronome`,
 * `applyKit:<trackId>`, `stopTransport`, `scheduleAll`, `kitSelected:<trackId>:<kitId>:<source>`. Inside
 * `restoreFromSnapshot` / `loadFromLocalStorage` the per-track dispose/create calls are replaced
 * by ONE `rebuildAll` entry, written just before that call's `syncDrumSink` (the Rust doc emits a
 * single `DocEvent::RebuildAll`; the TS disposes in `trackAudio` Map insertion order, which the
 * doc does not model). The Rust side maps its events onto these names (see `event_log` there).
 *
 * To keep the fixture small (< 2 MB) the arrangement is delta-encoded: `arr` is a list of
 * JSON-pointer patches against the previous step: `[pointer, value]` sets (object key or array
 * element), `[pointer, value, 1]` inserts into an array, `[pointer]` deletes (object key, or
 * array element with the later ones shifting down). Object return values that equal a subtree
 * of the arrangement are stored as `{ "$at": pointer }`; big args as `{ "$ref": "prev" }` (the
 * previous arrangement), `"prevJson"` (its JSON text) or `"template:<id>"`.
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

  const isObj = (v: unknown): v is Record<string, Json> => typeof v === 'object' && v !== null && !Array.isArray(v);
  const esc = (k: string) => k.replace(/~/g, '~0').replace(/\//g, '~1');
  /** JSON-pointer patches turning `a` into `b`. */
  const diff = (a: Json, b: Json, at: string, out: Json[]): void => {
    if (JSON.stringify(a) === JSON.stringify(b)) return;
    if (isObj(a) && isObj(b)) {
      for (const k of new Set([...Object.keys(a), ...Object.keys(b)])) {
        if (!(k in b)) out.push([`${at}/${esc(k)}`]);
        else if (!(k in a)) out.push([`${at}/${esc(k)}`, b[k]]);
        else diff(a[k], b[k], `${at}/${esc(k)}`, out);
      }
    } else if (Array.isArray(a) && Array.isArray(b) && a.length === b.length) {
      b.forEach((x, i) => diff(a[i], x, `${at}/${i}`, out));
    } else if (Array.isArray(a) && Array.isArray(b)) {
      // Insertions / removals in one place (common prefix + suffix): splice patches.
      const same = (x: Json, y: Json) => JSON.stringify(x) === JSON.stringify(y);
      const min = Math.min(a.length, b.length);
      let p = 0;
      while (p < min && same(a[p], b[p])) p++;
      let sf = 0;
      while (sf < min - p && same(a[a.length - 1 - sf], b[b.length - 1 - sf])) sf++;
      if (a.length - p - sf === 0) {
        for (let i = p; i < b.length - sf; i++) out.push([`${at}/${i}`, b[i], 1]);
      } else if (b.length - p - sf === 0) {
        for (let i = p; i < a.length - sf; i++) out.push([`${at}/${p}`]);
      } else {
        out.push([at, b]);
      }
    } else {
      out.push([at, b]);
    }
  };
  /** Pointer of a subtree of `root` that is JSON-equal to `v` (objects only), else null. */
  const findPtr = (root: Json, v: Json): string | null => {
    if (!isObj(v)) return null;
    const want = JSON.stringify(v);
    const walk = (x: Json, at: string): string | null => {
      if (JSON.stringify(x) === want) return at;
      if (isObj(x)) {
        for (const [k, y] of Object.entries(x)) {
          const hit = walk(y, `${at}/${esc(k)}`);
          if (hit !== null) return hit;
        }
      } else if (Array.isArray(x)) {
        for (let i = 0; i < x.length; i++) {
          const hit = walk(x[i], `${at}/${i}`);
          if (hit !== null) return hit;
        }
      }
      return null;
    };
    return walk(root, '');
  };
  /** Large args by reference: the previous arrangement (`$prev`, `$prevJson`) or a template. */
  const encodeArgs = (op: string, args: unknown[]): Json[] => {
    const json = JSON.parse(JSON.stringify(args)) as Json[];
    const prevJson = JSON.stringify(prev);
    if (op === 'restoreFromSnapshot' && JSON.stringify(json[0]) === prevJson) return [{ $ref: 'prev' }];
    if (op === 'loadFromLocalStorage' && json[0] === prevJson) return [{ $ref: 'prevJson' }];
    if (op === 'loadFromTemplate' || op === 'loadProject') {
      const t = ALL_TEMPLATES.find((x) => JSON.stringify(x.arrangement) === JSON.stringify(json[0]));
      if (t) return [{ $ref: `template:${t.id}` }];
    }
    return json;
  };

  // --- side-effect spies ---
  let fx: string[] = [];
  let depth = 0;
  let rebuilding = 0;
  let rebuildLogged = false;
  const log = (entry: string) => {
    if (depth === 0) fx.push(entry);
  };
  const spy = (name: string, entry: (...args: never[]) => string | null) => {
    const orig = engine[name].bind(e);
    engine[name] = (...args: unknown[]) => {
      const text = entry(...(args as never[]));
      if (text !== null) {
        if (text === 'syncDrumSink' && rebuilding > 0 && !rebuildLogged && depth === 0) {
          rebuildLogged = true;
          fx.push('rebuildAll');
        }
        log(text);
      }
      depth++;
      try {
        return orig(...args);
      } finally {
        depth--;
      }
    };
  };
  spy('createTrackAudio', (t: { id: string }) => (rebuilding > 0 ? null : `create:${t.id}`));
  spy('disposeTrackAudio', (id: string) => (rebuilding > 0 ? null : `dispose:${id}`));
  spy('refreshPlaybackSchedule', (id: string) => `reschedule:${id}`);
  spy('syncDrumSink', () => 'syncDrumSink');
  spy('applyLoopToTransport', () => 'applyLoop');
  spy('updateSoloState', () => 'solo');
  spy('applyTrackEffect', (_a: unknown, _fx: unknown, name: string) => `effect:${name}`);
  spy('resetTrackEffects', (t: { id: string }) => `resetEffects:${t.id}`);
  spy('updateMetronome', () => 'metronome');
  spy('applyKit', (t: { id: string }) => `applyKit:${t.id}`);
  spy('stopTransport', () => 'stopTransport');
  spy('scheduleAllTracks', () => 'scheduleAll');
  for (const name of ['restoreFromSnapshot', 'loadFromLocalStorage']) {
    const orig = engine[name].bind(e);
    engine[name] = (...args: unknown[]) => {
      const outer = rebuilding === 0;
      if (outer) rebuildLogged = false;
      rebuilding++;
      try {
        return orig(...args);
      } finally {
        rebuilding--;
      }
    };
  }
  e.onKitSelected((ev) => log(`kitSelected:${ev.trackId}:${ev.kitId}:${ev.source}`));

  function session(name: string) {
    n = 0;
    prev = null;
    const steps: Json[] = [];
    sessions.push({ name, steps });
    return (op: string, ...args: unknown[]) => {
      const callArgs = structuredClone(args);
      if (op === 'quantizeSelectedNotes') callArgs[2] = new Set(callArgs[2] as number[]);
      let ret: unknown;
      fx = [];
      if (op === 'loadFromLocalStorage') {
        localStorage.setItem('micro-daw-arrangement', args[0] as string);
        ret = e.loadFromLocalStorage();
      } else {
        ret = engine[op](...callArgs);
      }
      const cur = JSON.parse(JSON.stringify(e.getArrangement())) as Arr;
      const arr: Json[] = [];
      diff(prev, cur, '', arr);
      const retJson = ret === undefined ? null : (JSON.parse(JSON.stringify(ret)) as Json);
      const retPtr = findPtr(cur, retJson);
      steps.push({
        op,
        args: encodeArgs(op, args),
        // Object return values found in the arrangement are stored by pointer.
        ret: retPtr !== null ? { $at: retPtr } : retJson,
        arr,
        fx,
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

  // --- 5: seeded random sessions over every doc op (review L6) ---
  const EFFECTS = ['reverb', 'delay', 'chorus', 'distortion', 'eq', 'compressor', 'filter'] as const;
  const PARAMS = ['volume', 'pan', 'reverbWet', 'delayWet', 'chorusDepth', 'distortionWet', 'filterCutoff', 'eqLow', 'eqMid', 'eqHigh'] as const;
  const QUANT = ['off', '1/4', '1/8', '1/8T', '1/16', '1/16T', '1/32'] as const;
  for (let seed = 1; seed <= 8; seed++) {
    // mulberry32
    let st = seed * 0x9e3779b9;
    const rnd = () => {
      st = (st + 0x6d2b79f5) | 0;
      let t = Math.imul(st ^ (st >>> 15), 1 | st);
      t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
    const int = (lo: number, hi: number) => lo + Math.floor(rnd() * (hi - lo + 1));
    const pick = <T,>(xs: readonly T[]): T => xs[Math.floor(rnd() * xs.length)];
    const q = (x: number, step = 0.25) => Math.round(x / step) * step;
    const s = session(`random-${seed}`);
    let groupDepth = 0;
    let made = 0;
    const trackId = () => (rnd() < 0.06 || tracks().length === 0 ? 'nope' : pick(tracks()).id);
    const drumId = () => {
      const ds = tracks().filter((t) => t.instrument.type === 'drums');
      return ds.length === 0 || rnd() < 0.05 ? trackId() : pick(ds).id;
    };
    const regionOf = (tid: string) => {
      const t = tracks().find((x) => x.id === tid);
      return !t || t.regions.length === 0 || rnd() < 0.05 ? 'nope' : pick(t.regions).id;
    };
    /** A track id and (mostly) one of its existing lanes' parameter. */
    const laneTarget = (): [string, string] => {
      const withLanes = tracks().filter((t) => (t.automation?.length ?? 0) > 0);
      if (withLanes.length && rnd() < 0.8) {
        const t = pick(withLanes);
        return [t.id, pick(t.automation!).parameter];
      }
      return [trackId(), pick(PARAMS)];
    };
    /** (pad, step) of an existing 1/16 pattern note of the drum track (or random). */
    const stepTarget = (tid: string): [number, number] => {
      const region = e.getPatternRegion(tid);
      const start = e.getLoopMarkers()?.startBeat ?? 0;
      const hits = (region?.notes ?? []).filter((x) => x.note >= 36 && x.note < 52);
      if (region && hits.length && rnd() < 0.8) {
        const h = pick(hits);
        return [h.note - 36, Math.round((region.startBeat + h.startBeat - start) / 0.25)];
      }
      return [int(0, 15), int(0, 16)];
    };
    const randNotes = (count: number, len: number) =>
      Array.from({ length: count }, () => note(int(30, 90), q(rnd() * len, 0.125), pick([0.125, 0.25, 0.5, 1, 2]), pick([0.3, 0.8, 1, 0.55, 100]), rnd() < 0.3));
    const newRegion = () => {
      const len = pick([1, 2, 4, 8, 3.5]);
      const r: Record<string, Json> = { id: `rr-${seed}-${++made}`, startBeat: q(rnd() * 40, 0.5), lengthBeats: len, notes: randNotes(int(0, 6), len) };
      if (rnd() < 0.3) r.name = pick(['A', 'B', '']);
      if (rnd() < 0.15) r.audio = { sampleId: `smp-${made}`, samplePath: `/s/${made}.wav`, offsetSeconds: q(rnd(), 0.1), gainDb: -3, originalDuration: 4 };
      return r;
    };
    if (rnd() < 0.5) s('resetToStarter');
    else s('loadFromTemplate', pick(ALL_TEMPLATES).arrangement);
    for (let step = 0; step < 300; step++) {
      const r = rnd();
      const op = pick([
        'addTrack', 'removeTrack', 'setTrackVolume', 'setTrackPan', 'setTrackMute', 'setTrackSolo', 'setTrackEffect',
        'setTrackSynthOverrides', 'setTrackInstrument', 'addRegion', 'addRegion', 'removeRegion', 'moveRegion',
        'moveRegionToTrack', 'splitRegion', 'duplicateRegion', 'resizeRegion', 'quantizeRegionNotes',
        'quantizeSelectedNotes', 'updateRegionNotes', 'editRegionNotes', 'copyRegion', 'pasteRegion', 'setBpm',
        'addMarker', 'updateMarker', 'removeMarker', 'createGroup', 'addTrackToGroup', 'removeTrackFromGroup',
        'renameGroup', 'toggleGroupCollapsed', 'setGroupMute', 'setGroupSolo', 'removeGroup', 'addAutomationLane',
        'addAutomationLane', 'setAutomationPoint', 'setAutomationPoint', 'setAutomationPoint', 'deleteAutomationPoint',
        'setStepVelocity',
        'removeAutomationLane', 'setAutomationPoint', 'deleteAutomationPoint', 'toggleAutomationLaneVisibility',
        'setLoopEnabled', 'setLoopMarkers', 'beginUndoGroup', 'endUndoGroup', 'performUndo', 'performUndo',
        'performRedo', 'getActiveDrumTrackId', 'ensureDrumTrack', 'setActiveDrumTrack', 'armTrackForRecording',
        'getKit', 'setKit', 'updatePad', 'setPadSample', 'setSelectedPad', 'getSelectedPad', 'getLoopSettings',
        'setLoopSettings', 'getPatternRegion', 'ensurePatternRegion', 'toggleStep', 'toggleStep', 'toggleStep',
        'setStepVelocity', 'clearPattern', 'startLoopRecording', 'stopLoopRecording', 'restoreFromSnapshot',
        'loadFromLocalStorage', 'loadProject', 'loadFromTemplate', 'resetToStarter',
      ] as const);
      const rare = ['loadProject', 'loadFromTemplate', 'resetToStarter', 'loadFromLocalStorage'].includes(op);
      if (rare && r < 0.7) continue;
      switch (op) {
        case 'addTrack': s(op, pick(['synth', 'drums', 'audio']), int(-1, 44)); break;
        case 'removeTrack': s(op, trackId()); break;
        case 'setTrackVolume': s(op, trackId(), int(-40, 6)); break;
        case 'setTrackPan': s(op, trackId(), q(rnd() * 2.4 - 1.2, 0.05)); break;
        case 'setTrackMute': case 'setTrackSolo': s(op, trackId(), rnd() < 0.5); break;
        case 'setTrackEffect': {
          const name = pick(EFFECTS);
          const params: Record<string, Json> = { enabled: rnd() < 0.6 };
          if (rnd() < 0.5) params.wet = q(rnd(), 0.05);
          if (name === 'filter' && rnd() < 0.5) Object.assign(params, { type: pick(['highpass', 'bandpass', 'lowpass']), cutoff: int(100, 9000), lfoDepth: q(rnd(), 0.1) });
          if (name === 'compressor' && rnd() < 0.5) params.ratio = int(1, 10);
          if (name === 'eq' && rnd() < 0.5) Object.assign(params, { low: int(-6, 6), high: int(-6, 6) });
          s(op, trackId(), name, params);
          break;
        }
        case 'setTrackSynthOverrides': s(op, trackId(), rnd() < 0.5 ? { a: int(0, 9) } : { env: { attack: q(rnd(), 0.1) }, b: 1 }); break;
        case 'setTrackInstrument': s(op, trackId(), { type: pick(['synth', 'drums', 'audio']), presetIndex: int(0, 44) }); break;
        case 'addRegion': s(op, trackId(), newRegion()); break;
        case 'removeRegion': case 'duplicateRegion': case 'copyRegion': { const t = trackId(); s(op, t, regionOf(t)); break; }
        case 'moveRegion': { const t = trackId(); s(op, t, regionOf(t), q(rnd() * 48 - 4, 0.5)); break; }
        case 'moveRegionToTrack': { const t = trackId(); s(op, t, trackId(), regionOf(t)); break; }
        case 'splitRegion': {
          const t = trackId();
          const rid = regionOf(t);
          const reg = tracks().find((x) => x.id === t)?.regions.find((x) => x.id === rid);
          s(op, t, rid, reg && rnd() < 0.8 ? q(reg.startBeat + rnd() * reg.lengthBeats, 0.25) : q(rnd() * 48, 0.25));
          break;
        }
        case 'resizeRegion': {
          const t = trackId();
          s(op, t, regionOf(t), rnd() < 0.5 ? q(rnd() * 40, 0.5) : undefined, rnd() < 0.6 ? q(rnd() * 8, 0.25) : undefined);
          break;
        }
        case 'quantizeRegionNotes': { const t = trackId(); s(op, t, regionOf(t), pick([0, 0.25, 0.5, 1 / 3])); break; }
        case 'quantizeSelectedNotes': { const t = trackId(); s(op, t, regionOf(t), [0, int(1, 4)], pick([0.25, 0.5])); break; }
        case 'updateRegionNotes': { const t = trackId(); s(op, t, regionOf(t), randNotes(int(0, 5), 4)); break; }
        case 'editRegionNotes': {
          const t = trackId();
          s(op, t, regionOf(t), randNotes(int(0, 5), 6), pick(['Draw Note', 'Delete Note', 'Move Notes']), rnd() < 0.5 ? int(1, 12) : undefined);
          break;
        }
        case 'pasteRegion': s(op, trackId(), q(rnd() * 40, 1)); break;
        case 'setBpm': s(op, int(60, 180)); break;
        case 'addMarker': s(op, pick(['Intro', 'Drop', '']), q(rnd() * 64 - 4, 1), ...(rnd() < 0.3 ? ['#abc'] : [])); break;
        case 'updateMarker': {
          const ms = e.getMarkers();
          const id = ms.length && rnd() > 0.1 ? pick(ms).id : 'nope';
          s(op, id, rnd() < 0.5 ? { beat: int(0, 60) } : { name: 'Renamed', color: '#123' });
          break;
        }
        case 'removeMarker': { const ms = e.getMarkers(); s(op, ms.length && rnd() > 0.1 ? pick(ms).id : 'nope'); break; }
        case 'createGroup': s(op, pick(['G', 'Drums']), tracks().filter(() => rnd() < 0.4).map((t) => t.id)); break;
        case 'addTrackToGroup': case 'removeTrackFromGroup': {
          const gs = e.getGroups();
          s(op, gs.length && rnd() > 0.1 ? pick(gs).id : 'nope', trackId());
          break;
        }
        case 'renameGroup': case 'toggleGroupCollapsed': case 'removeGroup': case 'setGroupMute': case 'setGroupSolo': {
          const gs = e.getGroups();
          const id = gs.length && rnd() > 0.1 ? pick(gs).id : 'nope';
          if (op === 'renameGroup') s(op, id, 'Renamed');
          else if (op === 'setGroupMute' || op === 'setGroupSolo') s(op, id, rnd() < 0.5);
          else s(op, id);
          break;
        }
        case 'addAutomationLane': s(op, trackId(), pick(PARAMS)); break;
        case 'removeAutomationLane': case 'toggleAutomationLaneVisibility': s(op, ...laneTarget()); break;
        case 'setAutomationPoint': s(op, ...laneTarget(), q(rnd() * 32, 0.125), q(rnd() * 1.4 - 0.2, 0.05), pick([0, 0.25, 1])); break;
        case 'deleteAutomationPoint': s(op, ...laneTarget(), int(0, 3)); break;
        case 'setLoopEnabled': s(op, rnd() < 0.5); break;
        case 'setLoopMarkers': s(op, q(rnd() * 40 - 4, 1), q(rnd() * 48, 1)); break;
        case 'beginUndoGroup': if (groupDepth < 3) { groupDepth++; s(op, pick(['Gesture', 'Pad Gain', 'Paste'])); } break;
        case 'endUndoGroup': if (groupDepth > 0) { groupDepth--; s(op); } break;
        case 'performUndo': case 'performRedo': case 'getActiveDrumTrackId': case 'ensureDrumTrack': case 'getSelectedPad':
        case 'getLoopSettings': case 'stopLoopRecording': case 'resetToStarter':
          s(op);
          break;
        case 'startLoopRecording': s(op); break;
        case 'setActiveDrumTrack': {
          const others = tracks().filter((t) => t.instrument.type === 'drums' && t.id !== e.getActiveDrumTrackId());
          s(op, others.length && rnd() < 0.8 ? pick(others).id : drumId());
          break;
        }
        case 'armTrackForRecording': s(op, rnd() < 0.3 ? null : trackId()); break;
        case 'getKit': case 'getPatternRegion': case 'ensurePatternRegion': s(op, drumId()); break;
        case 'setKit':
          s(op, drumId(), { id: rnd() < 0.5 ? `kit-${int(1, 3)}` : '', name: pick(['Mine', '']), pads: [{ gainDb: int(-12, 0) }, { pitch: int(-3, 3), mode: 'gate' }] }, ...(rnd() < 0.3 ? ['pack-load'] : []));
          break;
        case 'updatePad': s(op, drumId(), int(0, 17), pick([{ gainDb: int(-20, 20) }, { pitch: int(-30, 30) }, { decay: q(rnd(), 0.1), mode: pick(['gate', 'oneshot']) }, { startSec: 0.1, endSec: 0.2, reverse: true }, { filterCutoff: int(10, 30000), chokeGroup: int(0, 3) }])); break;
        case 'setPadSample':
          s(op, drumId(), int(0, 16), pick([null, { name: 'Clap', source: { kind: 'file', path: `/s/c${int(1, 2)}.wav` } }, { name: 'Snare', source: { kind: 'factory', voice: pick(['snare', 'kick', 'clap']) } }]));
          break;
        case 'setSelectedPad': s(op, int(0, 17)); break;
        case 'setLoopSettings': {
          const patch: Record<string, Json> = {};
          if (rnd() < 0.5) patch.bars = pick([1, 2, 4, 8, 3]);
          if (rnd() < 0.4) patch.swing = q(rnd() * 0.9, 0.05);
          if (rnd() < 0.3) patch.quantize = pick(QUANT);
          if (rnd() < 0.2) patch.swingGrid = pick(['1/8', '1/16']);
          if (rnd() < 0.2) patch.overdub = rnd() < 0.5;
          if (rnd() < 0.2) patch.countInBars = int(0, 3);
          s(op, patch);
          break;
        }
        case 'toggleStep': s(op, drumId(), int(0, 16), int(-1, 17), pick([0.25, 0.5, 1 / 3]), ...(rnd() < 0.3 ? [q(rnd() * 1.2, 0.05)] : [])); break;
        case 'setStepVelocity': { const d = drumId(); s(op, d, ...stepTarget(d), 0.25, q(rnd() * 1.4 - 0.2, 0.05)); break; }
        case 'clearPattern': s(op, drumId(), ...(rnd() < 0.5 ? [int(0, 16)] : [])); break;
        case 'restoreFromSnapshot': s(op, structuredClone(e.getArrangement())); break;
        case 'loadFromLocalStorage': s(op, JSON.stringify(e.getArrangement())); break;
        case 'loadProject': s(op, legacyArrangement()); break;
        case 'loadFromTemplate': s(op, pick(ALL_TEMPLATES).arrangement); break;
      }
    }
    while (groupDepth > 0) {
      groupDepth--;
      s('endUndoGroup');
    }
    if (e.isLoopRecording()) s('stopLoopRecording');
  }

  writeFixture('doc', { presetNames: SYNTH_PRESETS.map((p) => p.name), sessions });
});
