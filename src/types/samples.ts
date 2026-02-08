export interface SampleRef {
  id: string;
  name: string;
  path: string;
  durationSeconds: number;
  sampleRate: number;
  channels: number;
}

export interface WaveformPeaks {
  sampleId: string;
  peaks: Float32Array;
  length: number;
}

export interface SampleTrim {
  startSeconds: number;
  endSeconds: number;
}

export interface SampleLibraryEntry {
  sample: SampleRef;
  trim?: SampleTrim;
  addedAt: number;
}
