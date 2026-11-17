import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { arrangementEngine } from '@/services/ArrangementEngine';
import { kitStore } from '@/services/KitStore';
import { cloneKit, createFactoryKit } from '@/audio/sampler/factoryKit';
import type { ArrangementTransportState, Region } from '@/types/arrangement';
import type { KitState, LoopRecordSettings, PadIndex, PadSample, PadSettings } from '@/types/sampler';
import { fileSample, planChop, type DroppedSample } from '@/components/beat/beatHelpers';

/**
 * Beat view state + actions, read straight from `arrangementEngine` (WP4's
 * beat API) with subscriptions. Kept independent of `useArrangement()` so
 * mounting it never re-runs that hook's project loading.
 *
 * The playhead is NOT part of this state (it would re-render the whole view
 * every frame); components that need it subscribe via `subscribePosition`.
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
  countInRemaining: number;
  isLoopRecording: boolean;
}

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
  const loop = arrangementEngine.getLoopSettings();
  const patternRegion = trackId ? arrangementEngine.getPatternRegion(trackId) : null;
  const patternStartBeat = arrangementEngine.getLoopMarkers()?.startBeat ?? patternRegion?.startBeat ?? 0;
  return {
    trackId,
    kit: trackId ? arrangementEngine.getKit(trackId) : fallbackKit(),
    selectedPad: arrangementEngine.getSelectedPad(),
    loop,
    transportState,
    bpm: arrangementEngine.getBpm(),
    patternRegion,
    patternVersion: notesVersion(patternRegion),
    patternStartBeat,
    countInRemaining,
    isLoopRecording: arrangementEngine.isLoopRecording(),
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
    a.countInRemaining === b.countInRemaining &&
    a.isLoopRecording === b.isLoopRecording
  );
}

/** Copied pad (settings incl. sample), shared across mounts. */
let padClipboard: PadSettings | null = null;

export const subscribePosition = (cb: (beat: number) => void): (() => void) => arrangementEngine.onPositionChange(cb);

export function useBeat() {
  const [snap, setSnap] = useState<BeatSnapshot>(readSnapshot);
  const [userKits, setUserKits] = useState<KitState[]>(() => kitStore.list());
  const [hasClipboard, setHasClipboard] = useState(padClipboard !== null);

  // --- Subscriptions -----------------------------------------------------
  useEffect(() => {
    const sync = () => {
      const next = readSnapshot();
      setSnap((prev) => (sameSnapshot(prev, next) ? prev : next));
    };
    sync();
    const unsubs = [
      arrangementEngine.onStateChange(sync),
      arrangementEngine.onKitChange(sync),
      // Count-in and live-recorded hits change while playing; cheap to re-read.
      arrangementEngine.onPositionChange(sync),
      kitStore.subscribe(() => setUserKits(kitStore.list())),
    ];
    return () => unsubs.forEach((u) => u());
  }, []);

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
    padClipboard = structuredClone(settings);
    setHasClipboard(true);
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

  // --- Pattern --------------------------------------------------------------
  const toggleStep = useCallback((pad: PadIndex, step: number, stepBeats: number, velocity?: number) => {
    arrangementEngine.toggleStep(track(), pad, step, stepBeats, velocity);
  }, [track]);

  const setStepVelocity = useCallback((pad: PadIndex, step: number, stepBeats: number, velocity: number) => {
    arrangementEngine.setStepVelocity(track(), pad, step, stepBeats, velocity);
  }, [track]);

  const clearPattern = useCallback((pad?: PadIndex) => {
    const id = arrangementEngine.getActiveDrumTrackId();
    if (id) arrangementEngine.clearPattern(id, pad);
  }, []);

  // --- Transport / loop -------------------------------------------------------
  /** Play the pattern on a loop from its start (beat mode always loops the pattern). */
  const play = useCallback(() => {
    if (arrangementEngine.getState() !== 'stopped') return;
    if (!arrangementEngine.isLoopEnabled()) arrangementEngine.setLoopEnabled(true);
    // Re-apply the pattern range: enabling the loop may have picked other markers.
    arrangementEngine.setLoopSettings({});
    arrangementEngine.playFrom(arrangementEngine.getLoopMarkers()?.startBeat ?? 0);
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

  const actions = useMemo(
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
      clearPattern,
      play,
      stop,
      togglePlay,
      toggleRecord,
      setBpm,
      setLoopSettings,
      loadKit,
      loadFactoryKit,
      saveKit,
      deleteKit,
      subscribePosition,
    }),
    [
      selectPad, updatePad, editPad, beginPadEdit, endPadEdit, setPadSample, assignFile, clearPad, renamePad,
      copyPad, pastePad, chopToPads, toggleStep, setStepVelocity, clearPattern, play, stop, togglePlay,
      toggleRecord, setBpm, setLoopSettings, loadKit, loadFactoryKit, saveKit, deleteKit,
    ],
  );

  return { ...snap, userKits, hasClipboard, actions };
}

export type BeatController = ReturnType<typeof useBeat>;
export type BeatActions = BeatController['actions'];
