import type { Arrangement } from './arrangement';
import type { EffectParams } from './effects';

export interface ProjectAudioConfig {
  masterVolume: number;        // dB
  effectParams: EffectParams;  // reverb, chorus, filter settings
}

export interface ProjectFile {
  version: '1.0.0';
  arrangement: Arrangement;
  audioConfig: ProjectAudioConfig;
  createdAt: string;           // ISO 8601
  modifiedAt: string;          // ISO 8601
}

export interface ProjectMeta {
  filePath: string;
  name: string;
  modifiedAt: string;
}
