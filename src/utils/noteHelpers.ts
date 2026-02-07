const NOTE_NAMES = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'] as const;

export function midiToNoteName(midi: number): string {
  const octave = Math.floor(midi / 12) - 1;
  const note = NOTE_NAMES[midi % 12];
  return `${note}${octave}`;
}

export function noteNameToMidi(name: string): number {
  const match = name.match(/^([A-G]#?)(-?\d)$/);
  if (!match) return -1;
  const [, note, octaveStr] = match;
  const noteIndex = NOTE_NAMES.indexOf(note as typeof NOTE_NAMES[number]);
  if (noteIndex === -1) return -1;
  return (parseInt(octaveStr) + 1) * 12 + noteIndex;
}

export function isBlackKey(midi: number): boolean {
  const n = midi % 12;
  return n === 1 || n === 3 || n === 6 || n === 8 || n === 10;
}

export function midiToFrequency(midi: number): number {
  return 440 * Math.pow(2, (midi - 69) / 12);
}

export function velocityToNormalized(velocity: number): number {
  return Math.max(0, Math.min(1, velocity / 127));
}
