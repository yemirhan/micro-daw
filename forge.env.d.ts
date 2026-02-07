/// <reference types="@electron-forge/plugin-vite/forge-vite-env" />

interface ElectronAPI {
  toggleFullscreen: () => Promise<boolean>;
  isFullscreen: () => Promise<boolean>;
  onFullscreenChanged: (callback: (isFullscreen: boolean) => void) => () => void;
}

interface Window {
  electronAPI: ElectronAPI;
}
