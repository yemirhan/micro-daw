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

  // Close guard
  onBeforeClose: (callback: () => void) => () => void;
  forceClose: () => void;
}

declare global {
  interface Window {
    electronAPI: ElectronAPI;
  }
}
