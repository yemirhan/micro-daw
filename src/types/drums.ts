export type DrumPadId = 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7;

export interface DrumSound {
  id: DrumPadId;
  name: string;
  shortName: string;
  midiNote: number;
  color: string;
}
