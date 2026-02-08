import { contextBridge, ipcRenderer } from 'electron';

contextBridge.exposeInMainWorld('electronAPI', {
  // Fullscreen
  toggleFullscreen: () => ipcRenderer.invoke('toggle-fullscreen'),
  isFullscreen: () => ipcRenderer.invoke('is-fullscreen'),
  onFullscreenChanged: (callback: (isFullscreen: boolean) => void) => {
    const listener = (_event: Electron.IpcRendererEvent, value: boolean) => callback(value);
    ipcRenderer.on('fullscreen-changed', listener);
    return () => ipcRenderer.removeListener('fullscreen-changed', listener);
  },

  // Project I/O
  projectSave: (filePath: string, data: string) => ipcRenderer.invoke('project:save', filePath, data),
  projectLoad: (filePath: string) => ipcRenderer.invoke('project:load', filePath),
  projectShowSaveDialog: (defaultName?: string) => ipcRenderer.invoke('project:show-save-dialog', defaultName),
  projectShowOpenDialog: () => ipcRenderer.invoke('project:show-open-dialog'),
  projectSetTitle: (title: string) => ipcRenderer.invoke('project:set-title', title),
  projectShowUnsavedDialog: (projectName: string) => ipcRenderer.invoke('project:show-unsaved-dialog', projectName),

  // Close guard
  onBeforeClose: (callback: () => void) => {
    const listener = () => callback();
    ipcRenderer.on('project:before-close', listener);
    return () => ipcRenderer.removeListener('project:before-close', listener);
  },
  forceClose: () => ipcRenderer.send('project:force-close'),

  // Auto-updater
  updaterCheck: () => ipcRenderer.invoke('updater:check'),
  updaterDownload: () => ipcRenderer.invoke('updater:download'),
  updaterInstall: () => ipcRenderer.invoke('updater:install'),
  getAppVersion: () => ipcRenderer.invoke('updater:get-version'),
  onUpdaterStatus: (callback: (status: unknown) => void) => {
    const listener = (_event: Electron.IpcRendererEvent, status: unknown) => callback(status);
    ipcRenderer.on('updater:status', listener);
    return () => ipcRenderer.removeListener('updater:status', listener);
  },
});
