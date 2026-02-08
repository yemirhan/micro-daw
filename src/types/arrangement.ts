export interface RegionNote {
  note: number;
  velocity: number;
  startBeat: number;     // relative to region start
  durationBeats: number;
  isDrum: boolean;
}

export interface AudioRegionData {
  sampleId: string;
  samplePath: string;
  offsetSeconds: number;
  gainDb: number;
  originalDuration: number; // seconds
}

export interface Region {
  id: string;
  startBeat: number;
  lengthBeats: number;
  notes: RegionNote[];
  color?: string;
  name?: string;
  audio?: AudioRegionData;
}

export interface TrackInstrument {
  type: 'synth' | 'drums' | 'audio';
  presetIndex: number;
}

export interface Track {
  id: string;
  name: string;
  instrument: TrackInstrument;
  regions: Region[];
  volume: number;
  pan: number; // -1 to 1, 0 = center
  muted: boolean;
  solo: boolean;
  color: string;
  effects?: import('@/types/effects').TrackEffectState;
  automation?: AutomationLane[];
}

export interface LoopMarkers {
  startBeat: number;
  endBeat: number;
}

export interface Marker {
  id: string;
  name: string;
  beat: number;
  color: string;
}

export interface TrackGroup {
  id: string;
  name: string;
  color: string;
  trackIds: string[];
  collapsed: boolean;
}

export interface Arrangement {
  id: string;
  name: string;
  bpm: number;
  lengthBeats: number;
  tracks: Track[];
  loopMarkers?: LoopMarkers;
  markers?: Marker[];
  groups?: TrackGroup[];
}

export interface AutomationPoint {
  beat: number;
  value: number; // normalized 0-1
}

export type AutomationParameter = 'volume' | 'pan' | 'reverbWet' | 'delayWet' | 'chorusDepth' | 'distortionWet' | 'filterCutoff' | 'eqLow' | 'eqMid' | 'eqHigh';

export interface AutomationLane {
  parameter: AutomationParameter;
  points: AutomationPoint[];
  visible: boolean;
}

export type ArrangementTransportState = 'stopped' | 'playing' | 'recording';
