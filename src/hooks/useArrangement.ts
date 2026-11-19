import { useEffect, useRef, useSyncExternalStore } from 'react';
import { arrangementEngine } from '@/services/ArrangementEngine';
import type { Track, Region, RegionNote, TrackInstrument, ArrangementTransportState, LoopMarkers, Marker, TrackGroup, AutomationParameter } from '@/types/arrangement';
import type { KitState, LoopRecordSettings, PadIndex, PadSample, PadSettings } from '@/types/sampler';

// ===========================================================================
// Fine-grained engine stores (review E4)
//
// The engine notifies on EVERY change (a recorded pad hit included) and on
// every playhead frame. Each slice below re-reads only what it shows and keeps
// its identity when that did not change, so a component re-renders only for
// the slices it subscribes to:
//
//   transport  state, tempo, loop, markers, groups, metronome   (rare changes)
//   tracks     the track list with regions/notes                (every edit, recorded hits)
//   beat       active drum track, kit, pattern, count-in...     (pad edits, recorded hits)
//   position   playhead beat + live recording region            (every frame while playing)
// ===========================================================================

type Unsubscribe = () => void;

export interface SliceStore<T> {
  subscribe: (listener: () => void) => Unsubscribe;
  getSnapshot: () => T;
}

/**
 * A cached, lazily re-read view of the engine. `source` notifies (any engine
 * change); the slice re-reads on the next getSnapshot and keeps the previous
 * object when `equal` says nothing it shows changed, so useSyncExternalStore
 * bails out. Attached to the engine once, for the app's lifetime (the engine is
 * a singleton), so unsubscribed readers still see fresh, stable snapshots.
 */
export function createSlice<T>(source: (cb: () => void) => Unsubscribe, read: () => T, equal: (a: T, b: T) => boolean): SliceStore<T> {
  let current: T | undefined;
  let dirty = true;
  let attached = false;
  const listeners = new Set<() => void>();
  function getSnapshot(): T {
    attach();
    if (dirty || current === undefined) {
      dirty = false;
      const next = read();
      if (current === undefined || !equal(current, next)) current = next;
    }
    return current;
  }
  function attach(): void {
    if (attached) return;
    attached = true;
    source(() => {
      dirty = true;
      if (listeners.size === 0) return;
      const prev = current;
      if (getSnapshot() !== prev) for (const l of [...listeners]) l();
    });
  }
  return {
    getSnapshot,
    subscribe(listener) {
      attach();
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}

/** Any engine state or kit change (not playhead frames). */
export const onEngineChange = (cb: () => void): Unsubscribe => {
  const a = arrangementEngine.onStateChange(cb);
  const b = arrangementEngine.onKitChange(cb);
  return () => {
    a();
    b();
  };
};
const onPosition = (cb: () => void): Unsubscribe => arrangementEngine.onPositionChange(cb);
const noSubscription = (): Unsubscribe => () => undefined;

/** Song transport + structure the transport bar / timeline show. Rarely changes (never on a pad hit). */
export interface ArrangementTransportSnapshot {
  transportState: ArrangementTransportState;
  bpm: number;
  lengthBeats: number;
  armedTrackId: string | null;
  recordingTrackId: string | null;
  loopEnabled: boolean;
  loopMarkers: LoopMarkers | undefined;
  markers: Marker[];
  groups: TrackGroup[];
  metronomeOn: boolean;
}

/** Beat-mode state (changes on pad edits and recorded hits). */
export interface ArrangementBeatSnapshot {
  activeDrumTrackId: string | null;
  /** Kit of the active drum track (null when there is no drum track). */
  kit: KitState | null;
  loopSettings: LoopRecordSettings;
  selectedPad: PadIndex;
  /** Pattern region of the active drum track (covering the loop start). */
  patternRegion: Region | null;
  countInRemaining: number;
  isLoopRecording: boolean;
}

export interface ArrangementPositionSnapshot {
  position: number;
  liveRegion: Region | null;
}

// The engine mutates markers / groups / regions in place: slices hold copies
// (new identity when the content changed) and compare by content.
const sameJson = (a: unknown, b: unknown) => a === b || JSON.stringify(a) === JSON.stringify(b);

const transportStore = createSlice<ArrangementTransportSnapshot>(
  onEngineChange,
  () => {
    const arr = arrangementEngine.getArrangement();
    const m = arrangementEngine.getLoopMarkers();
    return {
      transportState: arrangementEngine.getState(),
      bpm: arr.bpm,
      lengthBeats: arr.lengthBeats,
      armedTrackId: arrangementEngine.getArmedTrackId(),
      recordingTrackId: arrangementEngine.getRecordingTrackId(),
      loopEnabled: arrangementEngine.isLoopEnabled(),
      loopMarkers: m ? { ...m } : undefined,
      markers: arrangementEngine.getMarkers().map((x) => ({ ...x })),
      groups: arrangementEngine.getGroups().map((g) => ({ ...g, trackIds: [...g.trackIds] })),
      metronomeOn: arrangementEngine.isMetronomeEnabled(),
    };
  },
  (a, b) =>
    a.transportState === b.transportState &&
    a.bpm === b.bpm &&
    a.lengthBeats === b.lengthBeats &&
    a.armedTrackId === b.armedTrackId &&
    a.recordingTrackId === b.recordingTrackId &&
    a.loopEnabled === b.loopEnabled &&
    a.metronomeOn === b.metronomeOn &&
    sameJson(a.loopMarkers, b.loopMarkers) &&
    sameJson(a.markers, b.markers) &&
    sameJson(a.groups, b.groups),
);

// Tracks are edited in place (volume, mute, notes...): a new array on every engine change, as before.
const tracksStore = createSlice<Track[]>(onEngineChange, () => [...arrangementEngine.getTracks()], () => false);

/** Cheap content hash of a region's notes (the engine edits regions in place). */
function regionVersion(region: Region | null): string {
  if (!region) return '';
  let h = (region.notes.length * 31 + Math.round(region.startBeat * 960)) | 0;
  for (const n of region.notes) {
    h = (h * 31 + n.note) | 0;
    h = (h * 31 + Math.round(n.startBeat * 960)) | 0;
    h = (h * 31 + Math.round(n.durationBeats * 960)) | 0;
    h = (h * 31 + Math.round(n.velocity * 1000)) | 0;
  }
  return `${region.id}:${region.lengthBeats}:${h}`;
}

const beatStore = createSlice<ArrangementBeatSnapshot & { patternVersion: string }>(
  onEngineChange,
  () => {
    const drumId = arrangementEngine.getActiveDrumTrackId();
    const pattern = drumId ? arrangementEngine.getPatternRegion(drumId) : null;
    return {
      activeDrumTrackId: drumId,
      kit: drumId ? arrangementEngine.getKit(drumId) : null,
      loopSettings: arrangementEngine.getLoopSettings(),
      selectedPad: arrangementEngine.getSelectedPad(),
      // Copy: the engine edits regions in place, consumers need a new identity to see it.
      patternRegion: pattern ? { ...pattern } : null,
      patternVersion: regionVersion(pattern),
      countInRemaining: arrangementEngine.getCountInRemaining(),
      isLoopRecording: arrangementEngine.isLoopRecording(),
    };
  },
  (a, b) =>
    a.activeDrumTrackId === b.activeDrumTrackId &&
    a.kit === b.kit &&
    a.loopSettings === b.loopSettings &&
    a.selectedPad === b.selectedPad &&
    a.patternVersion === b.patternVersion &&
    a.countInRemaining === b.countInRemaining &&
    a.isLoopRecording === b.isLoopRecording,
);

const positionStore = createSlice<ArrangementPositionSnapshot>(
  (cb) => {
    // Position frames + state changes (stop resets the playhead and ends the live region).
    const a = onPosition(cb);
    const b = onEngineChange(cb);
    return () => {
      a();
      b();
    };
  },
  () => ({ position: arrangementEngine.getPosition(), liveRegion: arrangementEngine.getLiveRegion() }),
  (a, b) => a.position === b.position && a.liveRegion === null && b.liveRegion === null,
);

/** Song transport state (no re-render on pad hits, recorded notes or playhead frames). */
export function useArrangementTransport(): ArrangementTransportSnapshot {
  return useSyncExternalStore(transportStore.subscribe, transportStore.getSnapshot);
}

/** The track list (re-renders on every arrangement edit, recorded hits included). */
export function useArrangementTracks(): Track[] {
  return useSyncExternalStore(tracksStore.subscribe, tracksStore.getSnapshot);
}

/** Beat-mode state of useArrangement (active drum track, kit, pattern, count-in). */
export function useArrangementBeat(): ArrangementBeatSnapshot {
  return useSyncExternalStore(beatStore.subscribe, beatStore.getSnapshot);
}


/** Playhead beat: re-renders every frame while the transport runs. Use it in the leaf that draws the playhead. */
export function usePlaybackPosition(): number {
  return useSyncExternalStore(positionStore.subscribe, positionStore.getSnapshot).position;
}

/** The region being recorded into a track (Song record), null otherwise. Updates every frame while recording. */
export function useLiveRegion(): Region | null {
  return useSyncExternalStore(positionStore.subscribe, positionStore.getSnapshot).liveRegion;
}

// ===========================================================================
// Actions: plain functions over the engine singleton (stable identities)
// ===========================================================================

export const arrangementActions = {
  addTrack: (type: 'synth' | 'drums' | 'audio' = 'synth', presetIndex = 0) => {
    arrangementEngine.addTrack(type, presetIndex);
  },
  removeTrack: (trackId: string) => arrangementEngine.removeTrack(trackId),
  setTrackInstrument: (trackId: string, instrument: TrackInstrument) => arrangementEngine.setTrackInstrument(trackId, instrument),
  setTrackVolume: (trackId: string, db: number) => arrangementEngine.setTrackVolume(trackId, db),
  setTrackPan: (trackId: string, pan: number) => arrangementEngine.setTrackPan(trackId, pan),
  setTrackMute: (trackId: string, muted: boolean) => arrangementEngine.setTrackMute(trackId, muted),
  setTrackSolo: (trackId: string, solo: boolean) => arrangementEngine.setTrackSolo(trackId, solo),
  addRegion: (trackId: string, region: Region) => arrangementEngine.addRegion(trackId, region),
  removeRegion: (trackId: string, regionId: string) => arrangementEngine.removeRegion(trackId, regionId),
  moveRegion: (trackId: string, regionId: string, newStartBeat: number) => arrangementEngine.moveRegion(trackId, regionId, newStartBeat),
  moveRegionToTrack: (fromTrackId: string, toTrackId: string, regionId: string) =>
    arrangementEngine.moveRegionToTrack(fromTrackId, toTrackId, regionId),
  splitRegion: (trackId: string, regionId: string, splitBeat: number) => arrangementEngine.splitRegion(trackId, regionId, splitBeat),
  duplicateRegion: (trackId: string, regionId: string) => arrangementEngine.duplicateRegion(trackId, regionId),
  play: () => arrangementEngine.play(),
  stop: () => arrangementEngine.stop(),
  setBpm: (newBpm: number) => arrangementEngine.setBpm(newBpm),
  toggleMetronome: () => arrangementEngine.setMetronomeEnabled(!arrangementEngine.isMetronomeEnabled()),
  armTrack: (trackId: string | null) => arrangementEngine.armTrackForRecording(trackId),
  startRecording: () => arrangementEngine.startRecordingToTrack(),
  stopRecording: () => {
    arrangementEngine.stopRecordingToTrack();
  },
  toggleLoop: () => arrangementEngine.setLoopEnabled(!arrangementEngine.isLoopEnabled()),
  setLoopMarkers: (startBeat: number, endBeat: number) => arrangementEngine.setLoopMarkers(startBeat, endBeat),
  addAutomationLane: (trackId: string, parameter: AutomationParameter) => arrangementEngine.addAutomationLane(trackId, parameter),
  removeAutomationLane: (trackId: string, parameter: AutomationParameter) => arrangementEngine.removeAutomationLane(trackId, parameter),
  setAutomationPoint: (trackId: string, parameter: AutomationParameter, beat: number, value: number, snapValue: number) =>
    arrangementEngine.setAutomationPoint(trackId, parameter, beat, value, snapValue),
  deleteAutomationPoint: (trackId: string, parameter: AutomationParameter, pointIndex: number) =>
    arrangementEngine.deleteAutomationPoint(trackId, parameter, pointIndex),
  toggleAutomationLaneVisibility: (trackId: string, parameter: AutomationParameter) =>
    arrangementEngine.toggleAutomationLaneVisibility(trackId, parameter),
  quantizeRegion: (trackId: string, regionId: string, snapValue: number) => arrangementEngine.quantizeRegionNotes(trackId, regionId, snapValue),
  updateRegionNotes: (trackId: string, regionId: string, notes: RegionNote[]) => arrangementEngine.updateRegionNotes(trackId, regionId, notes),
  resizeRegion: (trackId: string, regionId: string, newStartBeat?: number, newLengthBeats?: number) =>
    arrangementEngine.resizeRegion(trackId, regionId, newStartBeat, newLengthBeats),
  importAudioFile: async () => {
    await arrangementEngine.importAudioFile();
  },
  addAudioRegion: (trackId: string, sampleId: string, startBeat: number) => {
    arrangementEngine.addAudioRegion(trackId, sampleId, startBeat);
  },
  copyRegion: (trackId: string, regionId: string) => arrangementEngine.copyRegion(trackId, regionId),
  pasteRegion: (trackId: string, atBeat: number) => arrangementEngine.pasteRegion(trackId, atBeat),
  // --- Piano roll editor ---
  editRegionNotes: (trackId: string, regionId: string, notes: RegionNote[], label?: string, newLengthBeats?: number) =>
    arrangementEngine.editRegionNotes(trackId, regionId, notes, label, newLengthBeats),
  previewNote: (trackId: string, midi: number, velocity?: number, durationSec?: number) =>
    arrangementEngine.previewNote(trackId, midi, velocity, durationSec),
  playFrom: (beat: number) => arrangementEngine.playFrom(beat),
  seek: (beat: number) => arrangementEngine.seek(beat),
  /** The Song cue (stopped playhead), shared by the arrangement and the piano roll. */
  setCue: (beat: number) => arrangementEngine.setCue(beat),
  locate: (beat: number) => arrangementEngine.locate(beat),
  // --- Markers ---
  addMarker: (name: string, beat: number, color?: string) => {
    arrangementEngine.addMarker(name, beat, color);
  },
  removeMarker: (id: string) => arrangementEngine.removeMarker(id),
  updateMarker: (id: string, updates: Partial<Pick<Marker, 'name' | 'beat' | 'color'>>) => arrangementEngine.updateMarker(id, updates),
  seekToMarker: (id: string) => arrangementEngine.seekToMarker(id),
  seekToNextMarker: () => arrangementEngine.seekToNextMarker(),
  seekToPreviousMarker: () => arrangementEngine.seekToPreviousMarker(),
  // --- Groups ---
  createGroup: (name: string, trackIds: string[]) => {
    arrangementEngine.createGroup(name, trackIds);
  },
  removeGroup: (id: string) => arrangementEngine.removeGroup(id),
  renameGroup: (id: string, name: string) => arrangementEngine.renameGroup(id, name),
  toggleGroupCollapsed: (id: string) => arrangementEngine.toggleGroupCollapsed(id),
  addTrackToGroup: (groupId: string, trackId: string) => arrangementEngine.addTrackToGroup(groupId, trackId),
  removeTrackFromGroup: (groupId: string, trackId: string) => arrangementEngine.removeTrackFromGroup(groupId, trackId),
  setGroupMute: (groupId: string, muted: boolean) => arrangementEngine.setGroupMute(groupId, muted),
  setGroupSolo: (groupId: string, solo: boolean) => arrangementEngine.setGroupSolo(groupId, solo),
  // --- Beat mode ---
  setActiveDrumTrack: (trackId: string) => arrangementEngine.setActiveDrumTrack(trackId),
  ensureDrumTrack: () => arrangementEngine.ensureDrumTrack(),
  setKit: (trackId: string, nextKit: KitState) => arrangementEngine.setKit(trackId, nextKit),
  updatePad: (trackId: string, pad: PadIndex, patch: Partial<PadSettings>) => arrangementEngine.updatePad(trackId, pad, patch),
  setPadSample: (trackId: string, pad: PadIndex, sample: PadSample | null) => arrangementEngine.setPadSample(trackId, pad, sample),
  setSelectedPad: (pad: PadIndex) => arrangementEngine.setSelectedPad(pad),
  setLoopSettings: (patch: Partial<LoopRecordSettings>) => arrangementEngine.setLoopSettings(patch),
  ensurePatternRegion: (trackId: string) => arrangementEngine.ensurePatternRegion(trackId),
  toggleStep: (trackId: string, pad: PadIndex, step: number, stepBeats: number, velocity?: number) =>
    arrangementEngine.toggleStep(trackId, pad, step, stepBeats, velocity),
  setStepVelocity: (trackId: string, pad: PadIndex, step: number, stepBeats: number, velocity: number) =>
    arrangementEngine.setStepVelocity(trackId, pad, step, stepBeats, velocity),
  clearPattern: (trackId: string, pad?: PadIndex) => arrangementEngine.clearPattern(trackId, pad),
  startLoopRecording: () => arrangementEngine.startLoopRecording(),
  stopLoopRecording: () => arrangementEngine.stopLoopRecording(),
  /** Group continuous edits (knob/velocity drags) into one undo step. */
  beginUndoGroup: (label: string) => arrangementEngine.beginUndoGroup(label),
  endUndoGroup: () => arrangementEngine.endUndoGroup(),
} as const;

export type ArrangementActions = typeof arrangementActions;

// ===========================================================================
// Project lifetime: first load + debounced autosave
// ===========================================================================

/** Autosave delay: bursts of edits (knob turns, pad hits while recording) are written once. */
export const AUTOSAVE_DEBOUNCE_MS = 300;

/**
 * Load the saved project on first mount (or the starter project) and autosave
 * every change, debounced; a pending save is flushed on unload/unmount. Owns no
 * React state: it never re-renders its host.
 */
function useArrangementPersistence(): void {
  const saveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => {
    if (!initialized) {
      // First run (nothing saved): start with a drum track, ready to play.
      if (!arrangementEngine.loadFromLocalStorage()) arrangementEngine.resetToStarter();
      initialized = true;
    }
    const flush = () => {
      if (saveTimerRef.current === null) return;
      clearTimeout(saveTimerRef.current);
      saveTimerRef.current = null;
      arrangementEngine.saveToLocalStorage();
    };
    const schedule = () => {
      if (saveTimerRef.current !== null) clearTimeout(saveTimerRef.current);
      saveTimerRef.current = setTimeout(() => {
        saveTimerRef.current = null;
        arrangementEngine.saveToLocalStorage();
      }, AUTOSAVE_DEBOUNCE_MS);
    };
    const unsub = onEngineChange(schedule);
    window.addEventListener('beforeunload', flush);
    window.addEventListener('pagehide', flush);
    return () => {
      unsub();
      window.removeEventListener('beforeunload', flush);
      window.removeEventListener('pagehide', flush);
      flush();
    };
  }, []);
}

/** The project is loaded once per app run (StrictMode / remounts don't reload it). */
let initialized = false;

// ===========================================================================
// useArrangement
// ===========================================================================

/** Which engine slices re-render the caller. Unsubscribed slices still read fresh values whenever it renders. */
export interface UseArrangementOptions {
  /**
   * Re-render on every playhead frame while playing/recording (`position`,
   * `liveRegion`). Default true. Hosts that don't draw the playhead themselves
   * (App) should pass false and let the leaves use usePlaybackPosition() /
   * useLiveRegion().
   */
  position?: boolean;
  /** Re-render on track/region/note edits, recorded hits included (`tracks`). Default true. */
  tracks?: boolean;
  /** Re-render on beat-mode state: kit, pattern, count-in, selected pad, loop settings. Default true. */
  beat?: boolean;
}

/**
 * Arrangement state + actions for the Song view (and legacy callers). With no
 * options it re-renders on everything (as before); pass options to opt out of
 * the hot slices: e.g. App in the Beat view only needs `{ position: false,
 * tracks: false, beat: false }`, so pad hits and recorded notes never re-render
 * it (review E4).
 */
export function useArrangement(options: UseArrangementOptions = {}) {
  const { position: withPosition = true, tracks: withTracks = true, beat: withBeat = true } = options;
  useArrangementPersistence();
  const transport = useSyncExternalStore(transportStore.subscribe, transportStore.getSnapshot);
  const tracks = useSyncExternalStore(withTracks ? tracksStore.subscribe : noSubscription, tracksStore.getSnapshot);
  const beat = useSyncExternalStore(withBeat ? beatStore.subscribe : noSubscription, beatStore.getSnapshot);
  const pos = useSyncExternalStore(withPosition ? positionStore.subscribe : noSubscription, positionStore.getSnapshot);
  return {
    ...transport,
    tracks,
    activeDrumTrackId: beat.activeDrumTrackId,
    /** Kit of the active drum track (null when there is no drum track). */
    kit: beat.kit,
    loopSettings: beat.loopSettings,
    selectedPad: beat.selectedPad,
    /** Pattern region of the active drum track (covering the loop start). */
    patternRegion: beat.patternRegion,
    countInRemaining: beat.countInRemaining,
    isLoopRecording: beat.isLoopRecording,
    position: pos.position,
    liveRegion: pos.liveRegion,
    ...arrangementActions,
  };
}

/** Actions only (stable functions; never re-renders the caller). */
export function useArrangementActions(): ArrangementActions {
  return arrangementActions;
}

export type ArrangementController = ReturnType<typeof useArrangement>;
