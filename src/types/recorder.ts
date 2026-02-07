export interface RecordedEvent {
  type: 'noteOn' | 'noteOff';
  time: number;        // ms offset from recording start
  note: number;        // MIDI note number
  velocity: number;    // 0-1 (only for noteOn)
  isDrum: boolean;
}

export interface Recording {
  id: string;
  name: string;
  duration: number;    // ms
  events: RecordedEvent[];
  bpm: number;
  createdAt: number;   // timestamp
}

export type TransportState = 'stopped' | 'recording' | 'playing';
