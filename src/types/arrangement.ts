export interface RegionNote {
  note: number;
  velocity: number;
  startBeat: number;     // relative to region start
  durationBeats: number;
  isDrum: boolean;
}

export interface Region {
  id: string;
  startBeat: number;
  lengthBeats: number;
  notes: RegionNote[];
  color?: string;
  name?: string;
}

export interface TrackInstrument {
  type: 'synth' | 'drums';
  presetIndex: number;
}

export interface Track {
  id: string;
  name: string;
  instrument: TrackInstrument;
  regions: Region[];
  volume: number;
  muted: boolean;
  solo: boolean;
  color: string;
}

export interface Arrangement {
  id: string;
  name: string;
  bpm: number;
  lengthBeats: number;
  tracks: Track[];
}

export type ArrangementTransportState = 'stopped' | 'playing' | 'recording';
