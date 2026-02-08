import type { UpdateStatus } from './settings';

export interface ElectronAPI {
  // Fullscreen
  toggleFullscreen: () => Promise<boolean>;
  isFullscreen: () => Promise<boolean>;
  onFullscreenChanged: (callback: (isFullscreen: boolean) => void) => () => void;

  // Project I/O
  projectSave: (filePath: string, data: string) => Promise<void>;
  projectLoad: (filePath: string) => Promise<string>;
  projectShowSaveDialog: (defaultName?: string) => Promise<string | null>;
  projectShowOpenDialog: () => Promise<string | null>;
  projectSetTitle: (title: string) => Promise<void>;
  projectShowUnsavedDialog: (projectName: string) => Promise<'save' | 'dont-save' | 'cancel'>;

  // Sample I/O
  sampleShowImportDialog: () => Promise<string[] | null>;
  sampleReadFile: (filePath: string) => Promise<ArrayBuffer>;

  // Close guard
  onBeforeClose: (callback: () => void) => () => void;
  forceClose: () => void;

  // Auto-updater
  updaterCheck: () => void;
  updaterDownload: () => void;
  updaterInstall: () => void;
  getAppVersion: () => Promise<string>;
  onUpdaterStatus: (callback: (status: UpdateStatus) => void) => () => void;
}

declare global {
  interface Window {
    electronAPI: ElectronAPI;
  }
}
