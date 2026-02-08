import { useState, useCallback, useEffect, useRef } from 'react';
import { arrangementEngine } from '@/services/ArrangementEngine';
import type { Track, Region, TrackInstrument, ArrangementTransportState, LoopMarkers, AutomationParameter } from '@/types/arrangement';
import { DEFAULT_BPM } from '@/utils/constants';

export function useArrangement() {
  const [tracks, setTracks] = useState<Track[]>([]);
  const [transportState, setTransportState] = useState<ArrangementTransportState>('stopped');
  const [position, setPosition] = useState(0);
  const [bpm, setBpmState] = useState(DEFAULT_BPM);
  const [metronomeOn, setMetronomeOn] = useState(false);
  const [armedTrackId, setArmedTrackId] = useState<string | null>(null);
  const [recordingTrackId, setRecordingTrackId] = useState<string | null>(null);
  const [liveRegion, setLiveRegion] = useState<Region | null>(null);
  const [lengthBeats, setLengthBeats] = useState(64);
  const [loopEnabled, setLoopEnabledState] = useState(false);
  const [loopMarkers, setLoopMarkersState] = useState<LoopMarkers | undefined>(undefined);
  const initializedRef = useRef(false);

  // Sync state from engine
  const syncState = useCallback(() => {
    const arr = arrangementEngine.getArrangement();
    setTracks([...arr.tracks]);
    setTransportState(arrangementEngine.getState());
    setBpmState(arr.bpm);
    setLengthBeats(arr.lengthBeats);
    setArmedTrackId(arrangementEngine.getArmedTrackId());
    setRecordingTrackId(arrangementEngine.getRecordingTrackId());
    setLoopEnabledState(arrangementEngine.isLoopEnabled());
    setLoopMarkersState(arrangementEngine.getLoopMarkers());
  }, []);

  useEffect(() => {
    if (!initializedRef.current) {
      arrangementEngine.loadFromLocalStorage();
      syncState();
      initializedRef.current = true;
    }

    const unsubState = arrangementEngine.onStateChange(syncState);
    const unsubPosition = arrangementEngine.onPositionChange((beat) => {
      setPosition(beat);
      setLiveRegion(arrangementEngine.getLiveRegion());
    });

    return () => {
      unsubState();
      unsubPosition();
    };
  }, [syncState]);

  // Auto-save on changes
  useEffect(() => {
    if (!initializedRef.current) return;
    arrangementEngine.saveToLocalStorage();
  }, [tracks, bpm]);

  const addTrack = useCallback((type: 'synth' | 'drums' = 'synth', presetIndex = 0) => {
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

  const copyRegion = useCallback((trackId: string, regionId: string) => {
    arrangementEngine.copyRegion(trackId, regionId);
  }, []);

  const pasteRegion = useCallback((trackId: string, atBeat: number) => {
    arrangementEngine.pasteRegion(trackId, atBeat);
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
    addAutomationLane,
    removeAutomationLane,
    setAutomationPoint,
    deleteAutomationPoint,
    toggleAutomationLaneVisibility,
  };
}
