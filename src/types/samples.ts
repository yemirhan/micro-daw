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

export type SampleFilterType = 'lowpass' | 'highpass' | 'bandpass';
export type SignalType = 'sine' | 'square' | 'sawtooth' | 'triangle' | 'white-noise';

export interface SampleProcessingParams {
  filter: { enabled: boolean; type: SampleFilterType; cutoff: number; resonance: number };
  pitchShift: { enabled: boolean; semitones: number };
  reverse: { enabled: boolean };
  normalize: { enabled: boolean };
  reverb: { enabled: boolean; decay: number; wet: number };
  delay: { enabled: boolean; time: number; feedback: number; wet: number };
}

export const DEFAULT_PROCESSING: SampleProcessingParams = {
  filter: { enabled: false, type: 'lowpass', cutoff: 1000, resonance: 1 },
  pitchShift: { enabled: false, semitones: 0 },
  reverse: { enabled: false },
  normalize: { enabled: false },
  reverb: { enabled: false, decay: 2.5, wet: 0.3 },
  delay: { enabled: false, time: 0.25, feedback: 0.3, wet: 0.3 },
};

export interface SignalGeneratorParams {
  type: SignalType;
  frequency: number;
  duration: number;
  amplitude: number;
}

export const DEFAULT_SIGNAL_PARAMS: SignalGeneratorParams = {
  type: 'sine',
  frequency: 440,
  duration: 1,
  amplitude: 0.8,
};
