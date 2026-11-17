import { useState, useCallback, useEffect, useRef } from 'react';
import { arrangementEngine } from '@/services/ArrangementEngine';
import type { Track, Region, RegionNote, TrackInstrument, ArrangementTransportState, LoopMarkers, Marker, TrackGroup, AutomationParameter } from '@/types/arrangement';
import type { KitState, LoopRecordSettings, PadIndex, PadSample, PadSettings } from '@/types/sampler';
import { DEFAULT_BPM } from '@/utils/constants';

/** Everything the UI reads from the engine, refreshed as ONE state update per engine change. */
interface ArrangementSnapshot {
  tracks: Track[];
  transportState: ArrangementTransportState;
  bpm: number;
  lengthBeats: number;
  armedTrackId: string | null;
  recordingTrackId: string | null;
  loopEnabled: boolean;
  loopMarkers: LoopMarkers | undefined;
  markers: Marker[];
  groups: TrackGroup[];
  activeDrumTrackId: string | null;
  kit: KitState | null;
  loopSettings: LoopRecordSettings;
  selectedPad: PadIndex;
  patternRegion: Region | null;
  countInRemaining: number;
  isLoopRecording: boolean;
}

function readSnapshot(): ArrangementSnapshot {
  const arr = arrangementEngine.getArrangement();
  const drumId = arrangementEngine.getActiveDrumTrackId();
  // Copy: the engine edits regions in place, consumers need a new identity to see it.
  const pattern = drumId ? arrangementEngine.getPatternRegion(drumId) : null;
  return {
    tracks: [...arr.tracks],
    transportState: arrangementEngine.getState(),
    bpm: arr.bpm,
    lengthBeats: arr.lengthBeats,
    armedTrackId: arrangementEngine.getArmedTrackId(),
    recordingTrackId: arrangementEngine.getRecordingTrackId(),
    loopEnabled: arrangementEngine.isLoopEnabled(),
    loopMarkers: arrangementEngine.getLoopMarkers(),
    markers: arrangementEngine.getMarkers(),
    groups: arrangementEngine.getGroups(),
    activeDrumTrackId: drumId,
    kit: drumId ? arrangementEngine.getKit(drumId) : null,
    loopSettings: arrangementEngine.getLoopSettings(),
    selectedPad: arrangementEngine.getSelectedPad(),
    patternRegion: pattern ? { ...pattern } : null,
    countInRemaining: arrangementEngine.getCountInRemaining(),
    isLoopRecording: arrangementEngine.isLoopRecording(),
  };
}

/** Autosave delay: bursts of edits (knob turns, pad hits while recording) are written once. */
export const AUTOSAVE_DEBOUNCE_MS = 300;

export function useArrangement() {
  const [snap, setSnap] = useState<ArrangementSnapshot>(() => ({
    ...readSnapshot(),
    tracks: [],
    transportState: 'stopped',
    bpm: DEFAULT_BPM,
    lengthBeats: 64,
    patternRegion: null,
    kit: null,
  }));
  const [position, setPosition] = useState(0);
  const [metronomeOn, setMetronomeOn] = useState(false);
  const [liveRegion, setLiveRegion] = useState<Region | null>(null);
  const initializedRef = useRef(false);

  // Sync state from engine: one setState per engine change (was ~20).
  const syncState = useCallback(() => {
    setSnap(readSnapshot());
  }, []);

  useEffect(() => {
    if (!initializedRef.current) {
      // First run (nothing saved): start with a drum track, ready to play.
      if (!arrangementEngine.loadFromLocalStorage()) arrangementEngine.resetToStarter();
      syncState();
      initializedRef.current = true;
    }

    const unsubState = arrangementEngine.onStateChange(syncState);
    const unsubKit = arrangementEngine.onKitChange(syncState);
    const unsubPosition = arrangementEngine.onPositionChange((beat) => {
      setPosition(beat);
      setLiveRegion(arrangementEngine.getLiveRegion());
    });

    return () => {
      unsubState();
      unsubKit();
      unsubPosition();
    };
  }, [syncState]);

  // Auto-save on changes, debounced; pending saves are flushed on unload/unmount.
  const saveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const flushSave = useCallback(() => {
    if (saveTimerRef.current === null) return;
    clearTimeout(saveTimerRef.current);
    saveTimerRef.current = null;
    arrangementEngine.saveToLocalStorage();
  }, []);

  useEffect(() => {
    if (!initializedRef.current) return;
    if (saveTimerRef.current !== null) clearTimeout(saveTimerRef.current);
    saveTimerRef.current = setTimeout(() => {
      saveTimerRef.current = null;
      arrangementEngine.saveToLocalStorage();
    }, AUTOSAVE_DEBOUNCE_MS);
  }, [snap]);

  useEffect(() => {
    window.addEventListener('beforeunload', flushSave);
    window.addEventListener('pagehide', flushSave);
    return () => {
      window.removeEventListener('beforeunload', flushSave);
      window.removeEventListener('pagehide', flushSave);
      flushSave();
    };
  }, [flushSave]);

  const {
    tracks,
    transportState,
    bpm,
    lengthBeats,
    armedTrackId,
    recordingTrackId,
    loopEnabled,
    loopMarkers,
    markers,
    groups,
    activeDrumTrackId,
    kit,
    loopSettings,
    selectedPad,
    patternRegion,
    countInRemaining,
    isLoopRecording,
  } = snap;

  const addTrack = useCallback((type: 'synth' | 'drums' | 'audio' = 'synth', presetIndex = 0) => {
    arrangementEngine.addTrack(type, presetIndex);
  }, []);

  const removeTrack = useCallback((trackId: string) => {
    arrangementEngine.removeTrack(trackId);
  }, []);

  const setTrackInstrument = useCallback((trackId: string, instrument: TrackInstrument) => {
    arrangementEngine.setTrackInstrument(trackId, instrument);
  }, []);

  const setTrackVolume = useCallback((trackId: string, db: number) => {
    arrangementEngine.setTrackVolume(trackId, db);
  }, []);

  const setTrackPan = useCallback((trackId: string, pan: number) => {
    arrangementEngine.setTrackPan(trackId, pan);
  }, []);

  const setTrackMute = useCallback((trackId: string, muted: boolean) => {
    arrangementEngine.setTrackMute(trackId, muted);
  }, []);

  const setTrackSolo = useCallback((trackId: string, solo: boolean) => {
    arrangementEngine.setTrackSolo(trackId, solo);
  }, []);

  const addRegion = useCallback((trackId: string, region: Region) => {
    arrangementEngine.addRegion(trackId, region);
  }, []);

  const removeRegion = useCallback((trackId: string, regionId: string) => {
    arrangementEngine.removeRegion(trackId, regionId);
  }, []);

  const moveRegion = useCallback((trackId: string, regionId: string, newStartBeat: number) => {
    arrangementEngine.moveRegion(trackId, regionId, newStartBeat);
  }, []);

  const moveRegionToTrack = useCallback((fromTrackId: string, toTrackId: string, regionId: string) => {
    arrangementEngine.moveRegionToTrack(fromTrackId, toTrackId, regionId);
  }, []);

  const splitRegion = useCallback((trackId: string, regionId: string, splitBeat: number) => {
    arrangementEngine.splitRegion(trackId, regionId, splitBeat);
  }, []);

  const duplicateRegion = useCallback((trackId: string, regionId: string) => {
    arrangementEngine.duplicateRegion(trackId, regionId);
  }, []);

  const play = useCallback(() => {
    arrangementEngine.play();
  }, []);

  const stop = useCallback(() => {
    arrangementEngine.stop();
  }, []);

  const setBpm = useCallback((newBpm: number) => {
    arrangementEngine.setBpm(newBpm);
  }, []);

  const toggleMetronome = useCallback(() => {
    const next = !arrangementEngine.isMetronomeEnabled();
    arrangementEngine.setMetronomeEnabled(next);
    setMetronomeOn(next);
  }, []);

  const armTrack = useCallback((trackId: string | null) => {
    arrangementEngine.armTrackForRecording(trackId);
  }, []);

  const startRecording = useCallback(() => {
    arrangementEngine.startRecordingToTrack();
  }, []);

  const stopRecording = useCallback(() => {
    arrangementEngine.stopRecordingToTrack();
  }, []);

  const toggleLoop = useCallback(() => {
    const next = !arrangementEngine.isLoopEnabled();
    arrangementEngine.setLoopEnabled(next);
  }, []);

  const setLoopMarkers = useCallback((startBeat: number, endBeat: number) => {
    arrangementEngine.setLoopMarkers(startBeat, endBeat);
  }, []);

  const addAutomationLane = useCallback((trackId: string, parameter: AutomationParameter) => {
    arrangementEngine.addAutomationLane(trackId, parameter);
  }, []);

  const removeAutomationLane = useCallback((trackId: string, parameter: AutomationParameter) => {
    arrangementEngine.removeAutomationLane(trackId, parameter);
  }, []);

  const setAutomationPoint = useCallback((trackId: string, parameter: AutomationParameter, beat: number, value: number, snapValue: number) => {
    arrangementEngine.setAutomationPoint(trackId, parameter, beat, value, snapValue);
  }, []);

  const deleteAutomationPoint = useCallback((trackId: string, parameter: AutomationParameter, pointIndex: number) => {
    arrangementEngine.deleteAutomationPoint(trackId, parameter, pointIndex);
  }, []);

  const toggleAutomationLaneVisibility = useCallback((trackId: string, parameter: AutomationParameter) => {
    arrangementEngine.toggleAutomationLaneVisibility(trackId, parameter);
  }, []);

  const quantizeRegion = useCallback((trackId: string, regionId: string, snapValue: number) => {
    arrangementEngine.quantizeRegionNotes(trackId, regionId, snapValue);
  }, []);

  const updateRegionNotes = useCallback((trackId: string, regionId: string, notes: import('@/types/arrangement').RegionNote[]) => {
    arrangementEngine.updateRegionNotes(trackId, regionId, notes);
  }, []);

  const resizeRegion = useCallback((trackId: string, regionId: string, newStartBeat?: number, newLengthBeats?: number) => {
    arrangementEngine.resizeRegion(trackId, regionId, newStartBeat, newLengthBeats);
  }, []);

  const importAudioFile = useCallback(async () => {
    await arrangementEngine.importAudioFile();
  }, []);

  const addAudioRegion = useCallback((trackId: string, sampleId: string, startBeat: number) => {
    arrangementEngine.addAudioRegion(trackId, sampleId, startBeat);
  }, []);

  const copyRegion = useCallback((trackId: string, regionId: string) => {
    arrangementEngine.copyRegion(trackId, regionId);
  }, []);

  const pasteRegion = useCallback((trackId: string, atBeat: number) => {
    arrangementEngine.pasteRegion(trackId, atBeat);
  }, []);

  // --- Piano roll editor ---

  const editRegionNotes = useCallback((trackId: string, regionId: string, notes: RegionNote[], label?: string, newLengthBeats?: number) => {
    arrangementEngine.editRegionNotes(trackId, regionId, notes, label, newLengthBeats);
  }, []);

  const previewNote = useCallback((trackId: string, midi: number, velocity?: number, durationSec?: number) => {
    arrangementEngine.previewNote(trackId, midi, velocity, durationSec);
  }, []);

  const playFrom = useCallback((beat: number) => {
    arrangementEngine.playFrom(beat);
  }, []);

  const seek = useCallback((beat: number) => {
    arrangementEngine.seek(beat);
  }, []);

  // --- Markers ---

  const addMarker = useCallback((name: string, beat: number, color?: string) => {
    arrangementEngine.addMarker(name, beat, color);
  }, []);

  const removeMarker = useCallback((id: string) => {
    arrangementEngine.removeMarker(id);
  }, []);

  const updateMarker = useCallback((id: string, updates: Partial<Pick<Marker, 'name' | 'beat' | 'color'>>) => {
    arrangementEngine.updateMarker(id, updates);
  }, []);

  const seekToMarker = useCallback((id: string) => {
    arrangementEngine.seekToMarker(id);
  }, []);

  const seekToNextMarker = useCallback(() => {
    arrangementEngine.seekToNextMarker();
  }, []);

  const seekToPreviousMarker = useCallback(() => {
    arrangementEngine.seekToPreviousMarker();
  }, []);

  // --- Groups ---

  const createGroup = useCallback((name: string, trackIds: string[]) => {
    arrangementEngine.createGroup(name, trackIds);
  }, []);

  const removeGroup = useCallback((id: string) => {
    arrangementEngine.removeGroup(id);
  }, []);

  const renameGroup = useCallback((id: string, name: string) => {
    arrangementEngine.renameGroup(id, name);
  }, []);

  const toggleGroupCollapsed = useCallback((id: string) => {
    arrangementEngine.toggleGroupCollapsed(id);
  }, []);

  const addTrackToGroup = useCallback((groupId: string, trackId: string) => {
    arrangementEngine.addTrackToGroup(groupId, trackId);
  }, []);

  const removeTrackFromGroup = useCallback((groupId: string, trackId: string) => {
    arrangementEngine.removeTrackFromGroup(groupId, trackId);
  }, []);

  const setGroupMute = useCallback((groupId: string, muted: boolean) => {
    arrangementEngine.setGroupMute(groupId, muted);
  }, []);

  const setGroupSolo = useCallback((groupId: string, solo: boolean) => {
    arrangementEngine.setGroupSolo(groupId, solo);
  }, []);

  // --- Beat mode ---

  const setActiveDrumTrack = useCallback((trackId: string) => {
    arrangementEngine.setActiveDrumTrack(trackId);
  }, []);

  const ensureDrumTrack = useCallback(() => arrangementEngine.ensureDrumTrack(), []);

  const setKit = useCallback((trackId: string, nextKit: KitState) => {
    arrangementEngine.setKit(trackId, nextKit);
  }, []);

  const updatePad = useCallback((trackId: string, pad: PadIndex, patch: Partial<PadSettings>) => {
    arrangementEngine.updatePad(trackId, pad, patch);
  }, []);

  const setPadSample = useCallback((trackId: string, pad: PadIndex, sample: PadSample | null) => {
    arrangementEngine.setPadSample(trackId, pad, sample);
  }, []);

  const setSelectedPad = useCallback((pad: PadIndex) => {
    arrangementEngine.setSelectedPad(pad);
  }, []);

  const setLoopSettings = useCallback((patch: Partial<LoopRecordSettings>) => {
    arrangementEngine.setLoopSettings(patch);
  }, []);

  const ensurePatternRegion = useCallback((trackId: string) => arrangementEngine.ensurePatternRegion(trackId), []);

  const toggleStep = useCallback((trackId: string, pad: PadIndex, step: number, stepBeats: number, velocity?: number) => {
    arrangementEngine.toggleStep(trackId, pad, step, stepBeats, velocity);
  }, []);

  const setStepVelocity = useCallback((trackId: string, pad: PadIndex, step: number, stepBeats: number, velocity: number) => {
    arrangementEngine.setStepVelocity(trackId, pad, step, stepBeats, velocity);
  }, []);

  const clearPattern = useCallback((trackId: string, pad?: PadIndex) => {
    arrangementEngine.clearPattern(trackId, pad);
  }, []);

  const startLoopRecording = useCallback(() => {
    arrangementEngine.startLoopRecording();
  }, []);

  const stopLoopRecording = useCallback(() => {
    arrangementEngine.stopLoopRecording();
  }, []);

  /** Group continuous edits (knob/velocity drags) into one undo step. */
  const beginUndoGroup = useCallback((label: string) => {
    arrangementEngine.beginUndoGroup(label);
  }, []);

  const endUndoGroup = useCallback(() => {
    arrangementEngine.endUndoGroup();
  }, []);

  return {
    tracks,
    transportState,
    position,
    bpm,
    metronomeOn,
    armedTrackId,
    recordingTrackId,
    liveRegion,
    lengthBeats,
    addTrack,
    removeTrack,
    setTrackInstrument,
    setTrackVolume,
    setTrackPan,
    setTrackMute,
    setTrackSolo,
    addRegion,
    removeRegion,
    moveRegion,
    moveRegionToTrack,
    splitRegion,
    duplicateRegion,
    play,
    stop,
    setBpm,
    toggleMetronome,
    armTrack,
    startRecording,
    stopRecording,
    updateRegionNotes,
    resizeRegion,
    copyRegion,
    pasteRegion,
    loopEnabled,
    loopMarkers,
    toggleLoop,
    setLoopMarkers,
    quantizeRegion,
    importAudioFile,
    addAudioRegion,
    addAutomationLane,
    removeAutomationLane,
    setAutomationPoint,
    deleteAutomationPoint,
    toggleAutomationLaneVisibility,
    // Piano roll editor
    editRegionNotes,
    previewNote,
    playFrom,
    seek,
    // Markers
    markers,
    addMarker,
    removeMarker,
    updateMarker,
    seekToMarker,
    seekToNextMarker,
    seekToPreviousMarker,
    // Groups
    groups,
    createGroup,
    removeGroup,
    renameGroup,
    toggleGroupCollapsed,
    addTrackToGroup,
    removeTrackFromGroup,
    setGroupMute,
    setGroupSolo,
    // Beat mode
    activeDrumTrackId,
    /** Kit of the active drum track (null when there is no drum track). */
    kit,
    loopSettings,
    selectedPad,
    /** Pattern region of the active drum track (covering the loop start). */
    patternRegion,
    countInRemaining,
    isLoopRecording,
    setActiveDrumTrack,
    ensureDrumTrack,
    setKit,
    updatePad,
    setPadSample,
    setSelectedPad,
    setLoopSettings,
    ensurePatternRegion,
    toggleStep,
    setStepVelocity,
    clearPattern,
    startLoopRecording,
    stopLoopRecording,
    beginUndoGroup,
    endUndoGroup,
  };
}

export type ArrangementController = ReturnType<typeof useArrangement>;
