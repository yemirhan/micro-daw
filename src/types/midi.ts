export interface MidiDeviceInfo {
  id: string;
  name: string;
  manufacturer: string;
  connected: boolean;
}

export interface NoteInfo {
  note: number;       // MIDI note number (0-127)
  name: string;       // e.g. "C4"
  velocity: number;   // 0-1 normalized
  channel: number;
}

export interface MidiCallbacks {
  onNoteOn: (note: number, velocity: number, channel: number) => void;
  onNoteOff: (note: number, channel: number) => void;
  onControlChange: (cc: number, value: number) => void;
}
