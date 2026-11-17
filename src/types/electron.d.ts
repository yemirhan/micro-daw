import type { UpdateStatus } from './settings';

/** One row of a directory listing from `fsListDir` (directories + audio files only). */
export interface FsDirEntry {
  /** Base name, e.g. "Kick 01.wav". */
  name: string;
  /** Absolute path. */
  path: string;
  isDir: boolean;
  /** Lower-case extension without the dot ("wav"); '' for directories. */
  ext: string;
}

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

  // File system browsing (sample browser)
  /** Lists one directory level: sub-folders first, then audio files; dotfiles hidden. Rejects if unreadable. */
  fsListDir: (dirPath: string) => Promise<FsDirEntry[]>;
  /** Native folder picker; null when cancelled. */
  fsPickFolder: () => Promise<string | null>;
  fsHomeDir: () => Promise<string>;
  /** Absolute path of a dropped/selected File (Electron `webUtils.getPathForFile`); '' if unknown. */
  getPathForFile: (file: File) => string;

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
    /** Undefined when the renderer runs in a plain browser (vite dev / ui-snap). */
    electronAPI?: ElectronAPI;
  }
}
