import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from 'react';
import { arrangementEngine } from '@/services/ArrangementEngine';
import { kitStore } from '@/services/KitStore';
import { cloneKit, createFactoryKit } from '@/audio/sampler/factoryKit';
import type { ArrangementTransportState, Region, Track } from '@/types/arrangement';
import type { KitState, LoopRecordSettings, PadIndex, PadSample, PadSettings } from '@/types/sampler';
import { fileSample, planChop, type DroppedSample } from '@/components/beat/beatHelpers';

/**
 * Beat view state + actions, read straight from `arrangementEngine` (WP4's
 * beat API) with subscriptions. Kept independent of `useArrangement()` so
 * mounting it never re-runs that hook's project loading.
 *
 * The playhead is NOT part of this state (it would re-render the whole view
 * every frame); components that need it subscribe via `subscribePosition`.
 *
 * Re-render scope (review E4): `useBeat()` re-renders on beat state changes
 * (pattern notes, recorded hits, count-in...). Hosts that only need the
 * actions (App's shortcuts) use `useBeatActions()`, which holds no engine
 * state, so pad hits never re-render them.
 */

export interface BeatSnapshot {
  /** Active drum track, or null before one exists (actions create it on demand). */
  trackId: string | null;
  /** The active track's kit (factory kit for display when there is no track yet). */
  kit: KitState;
  selectedPad: PadIndex;
  loop: LoopRecordSettings;
  transportState: ArrangementTransportState;
  bpm: number;
  /** Pattern region on the active track covering the loop start. */
  patternRegion: Region | null;
  /** Changes whenever the pattern's notes change (even if the engine mutates the region in place). */
  patternVersion: number;
  /** Beat where the pattern (loop) starts. */
  patternStartBeat: number;
  /**
   * The transport loops: while Beat Play / loop recording runs this is the
   * pattern's transient loop (always on), otherwise the Song loop toggle.
   */
  loopEnabled: boolean;
  /**
   * The loop range the transport uses: while Beat Play / loop recording runs,
   * the pattern range (the stored markers are not rewritten: review U6),
   * otherwise the arrangement loop markers (null when unset). Compared with the
   * pattern range for the step-grid hint.
   */
  loopStartBeat: number | null;
  loopEndBeat: number | null;
  /** Beat Play is looping the pattern (transiently, without touching the loop markers). */
  patternPlayback: boolean;
  countInRemaining: number;
  isLoopRecording: boolean;
  /** Every drum track (for the beat view's track selector), in track order. */
  drumTracks: DrumTrackInfo[];
}

/** A drum track as the beat view's track selector shows it. */
export interface DrumTrackInfo {
  id: string;
  /** Track name, numbered when several drum tracks share it ("Drums", "Drums 2"). */
  label: string;
  color: string;
}

/** Drum tracks with display labels (duplicate names get a running number). */
export function listDrumTracks(tracks: readonly Pick<Track, 'id' | 'name' | 'color' | 'instrument'>[]): DrumTrackInfo[] {
  const drums = tracks.filter((t) => t.instrument.type === 'drums');
  const seen = new Map<string, number>();
  return drums.map((t) => {
    const n = (seen.get(t.name) ?? 0) + 1;
    seen.set(t.name, n);
    return { id: t.id, label: n > 1 ? `${t.name} ${n}` : t.name, color: t.color };
  });
}

const sameDrumTracks = (a: DrumTrackInfo[], b: DrumTrackInfo[]) =>
  a.length === b.length && a.every((t, i) => t.id === b[i].id && t.label === b[i].label && t.color === b[i].color);

/** Quiet time that closes a wheel / keyboard velocity burst's undo step. */
export const STEP_BURST_IDLE_MS = 400;

let displayKit: KitState | null = null;
const fallbackKit = () => (displayKit ??= createFactoryKit());

/** Cheap content hash of a region's notes. */
function notesVersion(region: Region | null): number {
  if (!region) return 0;
  let h = (region.notes.length * 31 + Math.round(region.startBeat * 960)) | 0;
  for (const n of region.notes) {
    h = (h * 31 + n.note) | 0;
    h = (h * 31 + Math.round(n.startBeat * 960)) | 0;
    h = (h * 31 + Math.round(n.velocity * 1000)) | 0;
  }
  return h;
}

function readSnapshot(): BeatSnapshot {
  const trackId = arrangementEngine.getActiveDrumTrackId();
  const transportState = arrangementEngine.getState();
  const countInRemaining = arrangementEngine.getCountInRemaining();
  const loopSettings = arrangementEngine.getLoopSettings();
  const patternRegion = trackId ? arrangementEngine.getPatternRegion(trackId) : null;
  const markers = arrangementEngine.getLoopMarkers();
  const patternStartBeat = markers?.startBeat ?? patternRegion?.startBeat ?? 0;
  const isLoopRecording = arrangementEngine.isLoopRecording();
  const patternPlayback = arrangementEngine.isPatternPlayback();
  // Pattern play / loop recording loop the pattern range transiently: report what the transport loops.
  const loop = patternPlayback || isLoopRecording ? arrangementEngine.getActiveLoopRange() : markers ?? null;
  return {
    trackId,
    kit: trackId ? arrangementEngine.getKit(trackId) : fallbackKit(),
    selectedPad: arrangementEngine.getSelectedPad(),
    loop: loopSettings,
    transportState,
    bpm: arrangementEngine.getBpm(),
    patternRegion,
    patternVersion: notesVersion(patternRegion),
    patternStartBeat,
    loopEnabled: patternPlayback || isLoopRecording ? loop !== null : arrangementEngine.isLoopEnabled(),
    loopStartBeat: loop?.startBeat ?? null,
    loopEndBeat: loop?.endBeat ?? null,
    patternPlayback,
    countInRemaining,
    isLoopRecording,
    drumTracks: listDrumTracks(arrangementEngine.getTracks()),
  };
}

function sameSnapshot(a: BeatSnapshot, b: BeatSnapshot): boolean {
  return (
    a.trackId === b.trackId &&
    a.kit === b.kit &&
    a.selectedPad === b.selectedPad &&
    a.loop === b.loop &&
    a.transportState === b.transportState &&
    a.bpm === b.bpm &&
    a.patternRegion === b.patternRegion &&
    a.patternVersion === b.patternVersion &&
    a.patternStartBeat === b.patternStartBeat &&
    a.loopEnabled === b.loopEnabled &&
    a.loopStartBeat === b.loopStartBeat &&
    a.loopEndBeat === b.loopEndBeat &&
    a.patternPlayback === b.patternPlayback &&
    a.countInRemaining === b.countInRemaining &&
    a.isLoopRecording === b.isLoopRecording &&
    sameDrumTracks(a.drumTracks, b.drumTracks)
  );
}

/** Copied pad (settings incl. sample), shared across mounts. */
let padClipboard: PadSettings | null = null;
const clipboardListeners = new Set<() => void>();
function setPadClipboard(settings: PadSettings | null): void {
  padClipboard = settings;
  for (const l of [...clipboardListeners]) l();
}
const subscribeClipboard = (cb: () => void) => {
  clipboardListeners.add(cb);
  return () => {
    clipboardListeners.delete(cb);
  };
};
const hasPadClipboard = () => padClipboard !== null;

export const subscribePosition = (cb: (beat: number) => void): (() => void) => arrangementEngine.onPositionChange(cb);

export function useBeat() {
  const [snap, setSnap] = useState<BeatSnapshot>(readSnapshot);
  const [userKits, setUserKits] = useState<KitState[]>(() => kitStore.list());
  const hasClipboard = useSyncExternalStore(subscribeClipboard, hasPadClipboard);

  // --- Subscriptions -----------------------------------------------------
  useEffect(() => {
    const sync = () => {
      const next = readSnapshot();
      setSnap((prev) => {
        if (sameSnapshot(prev, next)) return prev;
        // Keep the drum-track list's identity when only something else changed.
        return sameDrumTracks(prev.drumTracks, next.drumTracks) ? { ...next, drumTracks: prev.drumTracks } : next;
      });
    };
    sync();
    // State changes only: count-in beats and recorded hits notify too, so the
    // view is not re-read on every playhead frame.
    const unsubs = [
      arrangementEngine.onStateChange(sync),
      arrangementEngine.onKitChange(sync),
      kitStore.subscribe(() => setUserKits(kitStore.list())),
    ];
    return () => unsubs.forEach((u) => u());
  }, []);

  const actions = useBeatActions();
  return { ...snap, userKits, hasClipboard, actions };
}

/**
 * The beat view's actions without any engine state: the caller never
 * re-renders because of pad hits, recorded notes or the playhead (review E4).
 */
export function useBeatActions() {
  // --- Helpers -------------------------------------------------------------
  const track = useCallback((): string => arrangementEngine.getActiveDrumTrackId() ?? arrangementEngine.ensureDrumTrack(), []);

  /** Continuous edits (knob drags, trim handles) become one undo step. */
  const editGroupOpen = useRef(false);
  const editGroupTimer = useRef<number | undefined>(undefined);
  const endPadEdit = useCallback(() => {
    window.clearTimeout(editGroupTimer.current);
    if (!editGroupOpen.current) return;
    editGroupOpen.current = false;
    arrangementEngine.endUndoGroup();
  }, []);
  const beginPadEdit = useCallback(
    (label = 'Edit pad') => {
      window.clearTimeout(editGroupTimer.current);
      if (!editGroupOpen.current) {
        editGroupOpen.current = true;
        arrangementEngine.beginUndoGroup(label);
      }
      // Wheel / keyboard edits have no explicit end: close the group when idle.
      editGroupTimer.current = window.setTimeout(endPadEdit, 700);
    },
    [endPadEdit],
  );
  useEffect(() => endPadEdit, [endPadEdit]);

  // --- Pads -----------------------------------------------------------------
  const selectPad = useCallback((pad: PadIndex) => arrangementEngine.setSelectedPad(pad), []);

  const updatePad = useCallback((pad: PadIndex, patch: Partial<PadSettings>) => {
    arrangementEngine.updatePad(track(), pad, patch);
  }, [track]);

  /** Like updatePad, but merged into one undo step with neighbouring calls (drags). */
  const editPad = useCallback(
    (pad: PadIndex, patch: Partial<PadSettings>, label?: string) => {
      beginPadEdit(label);
      arrangementEngine.updatePad(track(), pad, patch);
    },
    [beginPadEdit, track],
  );

  const setPadSample = useCallback((pad: PadIndex, sample: PadSample | null) => {
    arrangementEngine.setPadSample(track(), pad, sample);
  }, [track]);

  const assignFile = useCallback((pad: PadIndex, file: DroppedSample) => {
    arrangementEngine.setPadSample(track(), pad, fileSample(file));
  }, [track]);

  const clearPad = useCallback((pad: PadIndex) => {
    arrangementEngine.setPadSample(track(), pad, null);
  }, [track]);

  const renamePad = useCallback((pad: PadIndex, name: string) => {
    const id = track();
    const sample = arrangementEngine.getKit(id).pads[pad]?.sample;
    const trimmed = name.trim();
    if (!sample || !trimmed || trimmed === sample.name) return;
    arrangementEngine.setPadSample(id, pad, { ...sample, name: trimmed });
  }, [track]);

  const copyPad = useCallback((pad: PadIndex) => {
    const settings = arrangementEngine.getKit(track()).pads[pad];
    if (!settings) return;
    setPadClipboard(structuredClone(settings));
  }, [track]);

  const pastePad = useCallback((pad: PadIndex) => {
    if (!padClipboard) return;
    const id = track();
    const { sample, ...rest } = structuredClone(padClipboard);
    arrangementEngine.beginUndoGroup('Paste pad');
    try {
      arrangementEngine.setPadSample(id, pad, sample);
      arrangementEngine.updatePad(id, pad, rest);
    } finally {
      arrangementEngine.endUndoGroup();
    }
  }, [track]);

  /** Assign `ranges` of `sample` to consecutive pads from `fromPad` (same file, different trim). */
  const chopToPads = useCallback((fromPad: PadIndex, sample: PadSample, ranges: { startSec: number; endSec: number }[]) => {
    const plan = planChop(fromPad, ranges);
    if (plan.length === 0) return;
    const id = track();
    arrangementEngine.beginUndoGroup('Chop to pads');
    try {
      for (const { pad, startSec, endSec } of plan) {
        if (pad !== fromPad) arrangementEngine.setPadSample(id, pad, { source: { ...sample.source }, name: `${sample.name} ${pad - fromPad + 1}` });
        arrangementEngine.updatePad(id, pad, { startSec, endSec, reverse: false });
      }
    } finally {
      arrangementEngine.endUndoGroup();
    }
  }, [track]);

  // --- Step gestures (one undo step each) -----------------------------------
  /**
   * A step-grid gesture: a pointer paint / velocity drag (explicit begin/end) or
   * a burst of wheel / keyboard velocity nudges (closed after STEP_BURST_IDLE_MS
   * of quiet). Whatever it changes undoes in one step.
   */
  const stepGesture = useRef<'drag' | 'burst' | null>(null);
  const burstTimer = useRef<number | undefined>(undefined);
  const endStepGesture = useCallback(() => {
    window.clearTimeout(burstTimer.current);
    if (!stepGesture.current) return;
    stepGesture.current = null;
    arrangementEngine.endUndoGroup();
  }, []);
  const beginStepGesture = useCallback(
    (label = 'Edit steps') => {
      endStepGesture();
      stepGesture.current = 'drag';
      arrangementEngine.beginUndoGroup(label);
    },
    [endStepGesture],
  );
  useEffect(() => endStepGesture, [endStepGesture]);

  // --- Pattern --------------------------------------------------------------
  const toggleStep = useCallback((pad: PadIndex, step: number, stepBeats: number, velocity?: number) => {
    arrangementEngine.toggleStep(track(), pad, step, stepBeats, velocity);
  }, [track]);

  const setStepVelocity = useCallback((pad: PadIndex, step: number, stepBeats: number, velocity: number) => {
    arrangementEngine.setStepVelocity(track(), pad, step, stepBeats, velocity);
  }, [track]);

  /**
   * setStepVelocity for wheel / keyboard nudges: consecutive calls merge into one
   * undo step until STEP_BURST_IDLE_MS pass without one (inside a drag they just
   * join the drag's step).
   */
  const nudgeStepVelocity = useCallback(
    (pad: PadIndex, step: number, stepBeats: number, velocity: number) => {
      if (!stepGesture.current) {
        stepGesture.current = 'burst';
        arrangementEngine.beginUndoGroup('Step velocity');
      }
      if (stepGesture.current === 'burst') {
        window.clearTimeout(burstTimer.current);
        burstTimer.current = window.setTimeout(endStepGesture, STEP_BURST_IDLE_MS);
      }
      arrangementEngine.setStepVelocity(track(), pad, step, stepBeats, velocity);
    },
    [endStepGesture, track],
  );

  const clearPattern = useCallback((pad?: PadIndex) => {
    const id = arrangementEngine.getActiveDrumTrackId();
    if (id) arrangementEngine.clearPattern(id, pad);
  }, []);

  // --- Transport / loop -------------------------------------------------------
  /**
   * Make the transport loop exactly the pattern the grid shows (the step-grid
   * "Loop pattern" hint button). Undoable, works while playing.
   */
  const loopPattern = useCallback(() => {
    const start = readSnapshot().patternStartBeat;
    const length = arrangementEngine.getLoopSettings().bars * 4;
    arrangementEngine.beginUndoGroup('Loop pattern');
    try {
      if (!arrangementEngine.isLoopEnabled()) arrangementEngine.setLoopEnabled(true);
      arrangementEngine.setLoopMarkers(start, start + length);
    } finally {
      arrangementEngine.endUndoGroup();
    }
  }, []);
  /**
   * Play the pattern on a loop from its start (beat mode always loops the
   * pattern): the engine's playPattern() loops [pattern start, + bars × 4)
   * transiently and starts there; the project's loop markers are kept.
   */
  const play = useCallback(() => {
    if (arrangementEngine.getState() !== 'stopped') return;
    arrangementEngine.playPattern();
  }, []);
  const stop = useCallback(() => {
    if (arrangementEngine.isLoopRecording()) arrangementEngine.stopLoopRecording();
    arrangementEngine.stop();
  }, []);
  const togglePlay = useCallback(() => {
    if (arrangementEngine.getState() === 'stopped' && !arrangementEngine.isLoopRecording()) play();
    else stop();
  }, [play, stop]);
  const toggleRecord = useCallback(() => {
    if (arrangementEngine.isLoopRecording()) {
      arrangementEngine.stopLoopRecording();
    } else {
      track();
      arrangementEngine.startLoopRecording();
    }
  }, [track]);
  const setBpm = useCallback((bpm: number) => arrangementEngine.setBpm(bpm), []);
  const setLoopSettings = useCallback((patch: Partial<LoopRecordSettings>) => arrangementEngine.setLoopSettings(patch), []);

  // --- Kits -----------------------------------------------------------------
  const loadKit = useCallback((kit: KitState) => {
    arrangementEngine.setKit(track(), cloneKit(kit));
  }, [track]);
  const loadFactoryKit = useCallback(() => {
    arrangementEngine.setKit(track(), createFactoryKit());
  }, [track]);
  const saveKit = useCallback((name: string) => {
    const kit = cloneKit(arrangementEngine.getKit(track()));
    const trimmed = name.trim() || 'My kit';
    const existing = kitStore.list().find((k) => k.name === trimmed);
    kitStore.save({ ...kit, id: existing?.id ?? crypto.randomUUID(), name: trimmed });
  }, [track]);
  const deleteKit = useCallback((id: string) => kitStore.remove(id), []);

  // --- Drum tracks ------------------------------------------------------------
  /** Point the beat view (pads, steps, live hits) at another drum track. */
  const setActiveDrumTrack = useCallback((trackId: string) => arrangementEngine.setActiveDrumTrack(trackId), []);
  /** Add a drum track and make it the active one; returns its id. */
  const newDrumTrack = useCallback((): string => {
    const id = arrangementEngine.addTrack('drums').id;
    arrangementEngine.setActiveDrumTrack(id);
    return id;
  }, []);

  return useMemo(
    () => ({
      selectPad,
      updatePad,
      editPad,
      beginPadEdit,
      endPadEdit,
      setPadSample,
      assignFile,
      clearPad,
      renamePad,
      copyPad,
      pastePad,
      chopToPads,
      toggleStep,
      setStepVelocity,
      nudgeStepVelocity,
      beginStepGesture,
      endStepGesture,
      clearPattern,
      play,
      loopPattern,
      stop,
      togglePlay,
      toggleRecord,
      setBpm,
      setLoopSettings,
      loadKit,
      loadFactoryKit,
      saveKit,
      deleteKit,
      setActiveDrumTrack,
      newDrumTrack,
      subscribePosition,
    }),
    [
      selectPad, updatePad, editPad, beginPadEdit, endPadEdit, setPadSample, assignFile, clearPad, renamePad,
      copyPad, pastePad, chopToPads, toggleStep, setStepVelocity, nudgeStepVelocity, beginStepGesture,
      endStepGesture, clearPattern, play, loopPattern, stop, togglePlay, toggleRecord, setBpm, setLoopSettings, loadKit,
      loadFactoryKit, saveKit, deleteKit, setActiveDrumTrack, newDrumTrack,
    ],
  );
}

export type BeatController = ReturnType<typeof useBeat>;
export type BeatActions = ReturnType<typeof useBeatActions>;

// ---------------------------------------------------------------------------
// Light-weight read-only hooks (no per-frame position subscription)
// ---------------------------------------------------------------------------

/** Id of the drum track the beat view edits (null when there is none). */
export function useActiveDrumTrackId(): string | null {
  const [id, setId] = useState(() => arrangementEngine.getActiveDrumTrackId());
  useEffect(() => {
    const sync = () => setId(arrangementEngine.getActiveDrumTrackId());
    sync();
    const unsubs = [arrangementEngine.onStateChange(sync), arrangementEngine.onKitChange(sync)];
    return () => unsubs.forEach((u) => u());
  }, []);
  return id;
}

/** The active drum track's pads (factory kit before one exists) and the selected pad. */
function readActiveKitPads(): { pads: PadSettings[]; selectedPad: PadIndex } {
  const id = arrangementEngine.getActiveDrumTrackId();
  return { pads: (id ? arrangementEngine.getKit(id) : fallbackKit()).pads, selectedPad: arrangementEngine.getSelectedPad() };
}

export function useActiveKitPads(): { pads: PadSettings[]; selectedPad: PadIndex } {
  const [state, setState] = useState(readActiveKitPads);
  useEffect(() => {
    const sync = () => {
      const next = readActiveKitPads();
      setState((prev) => (prev.pads === next.pads && prev.selectedPad === next.selectedPad ? prev : next));
    };
    sync();
    const unsubs = [arrangementEngine.onStateChange(sync), arrangementEngine.onKitChange(sync)];
    return () => unsubs.forEach((u) => u());
  }, []);
  return state;
}
