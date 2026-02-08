export interface AppSettings {
  general: { autoCheckUpdates: boolean; hasCompletedOnboarding: boolean };
  audio: { defaultMasterVolume: number; bufferSizeHint: 128 | 256 | 512 | 1024 | 2048 };
  midi: { autoConnectLastDevice: boolean };
}

export type UpdateStatus =
  | { state: 'idle' }
  | { state: 'checking' }
  | { state: 'available'; version: string }
  | { state: 'not-available' }
  | { state: 'downloading'; percent: number }
  | { state: 'downloaded'; version: string }
  | { state: 'error'; message: string };
