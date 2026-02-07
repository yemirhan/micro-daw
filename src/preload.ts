import { contextBridge, ipcRenderer } from 'electron';

contextBridge.exposeInMainWorld('electronAPI', {
  toggleFullscreen: () => ipcRenderer.invoke('toggle-fullscreen'),
  isFullscreen: () => ipcRenderer.invoke('is-fullscreen'),
  onFullscreenChanged: (callback: (isFullscreen: boolean) => void) => {
    const listener = (_event: Electron.IpcRendererEvent, value: boolean) => callback(value);
    ipcRenderer.on('fullscreen-changed', listener);
    return () => ipcRenderer.removeListener('fullscreen-changed', listener);
  },
});
