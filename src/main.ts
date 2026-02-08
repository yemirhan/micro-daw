import { app, BrowserWindow, dialog, ipcMain, session } from 'electron';
import path from 'node:path';
import fs from 'node:fs/promises';
import started from 'electron-squirrel-startup';
import { autoUpdater } from 'electron-updater';

// Handle creating/removing shortcuts on Windows when installing/uninstalling.
if (started) {
  app.quit();
}

let mainWindow: BrowserWindow | null = null;
let forceCloseAllowed = false;

// --- Auto-updater ---
autoUpdater.autoDownload = false;
autoUpdater.autoInstallOnAppQuit = true;

function sendUpdaterStatus(status: unknown) {
  mainWindow?.webContents.send('updater:status', status);
}

autoUpdater.on('checking-for-update', () => {
  sendUpdaterStatus({ state: 'checking' });
});
autoUpdater.on('update-available', (info) => {
  sendUpdaterStatus({ state: 'available', version: info.version });
});
autoUpdater.on('update-not-available', () => {
  sendUpdaterStatus({ state: 'not-available' });
});
autoUpdater.on('download-progress', (progress) => {
  sendUpdaterStatus({ state: 'downloading', percent: progress.percent });
});
autoUpdater.on('update-downloaded', (info) => {
  sendUpdaterStatus({ state: 'downloaded', version: info.version });
});
autoUpdater.on('error', (err) => {
  sendUpdaterStatus({ state: 'error', message: err?.message ?? 'Unknown error' });
});

const createWindow = () => {
  mainWindow = new BrowserWindow({
    width: 1360,
    height: 800,
    minWidth: 1360,
    minHeight: 768,
    show: false,
    title: 'Untitled - Micro DAW',
    titleBarStyle: 'hiddenInset',
    trafficLightPosition: { x: 12, y: 17 },
    backgroundColor: '#1a1a2e',
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
    },
  });

  if (MAIN_WINDOW_VITE_DEV_SERVER_URL) {
    mainWindow.loadURL(MAIN_WINDOW_VITE_DEV_SERVER_URL);
  } else {
    mainWindow.loadFile(
      path.join(__dirname, `../renderer/${MAIN_WINDOW_VITE_NAME}/index.html`),
    );
  }

  mainWindow.once('ready-to-show', () => {
    mainWindow!.show();
  });

  if (!app.isPackaged) {
    mainWindow.webContents.openDevTools();
  }

  mainWindow.on('enter-full-screen', () => {
    mainWindow!.webContents.send('fullscreen-changed', true);
  });
  mainWindow.on('leave-full-screen', () => {
    mainWindow!.webContents.send('fullscreen-changed', false);
  });

  // --- Close guard ---
  mainWindow.on('close', (e) => {
    if (forceCloseAllowed) return;
    e.preventDefault();
    mainWindow!.webContents.send('project:before-close');
  });
};

const registerIpcHandlers = () => {
  // --- Fullscreen ---
  ipcMain.handle('toggle-fullscreen', () => {
    mainWindow!.setFullScreen(!mainWindow!.isFullScreen());
    return mainWindow!.isFullScreen();
  });

  ipcMain.handle('is-fullscreen', () => {
    return mainWindow!.isFullScreen();
  });

  // --- Project I/O ---
  ipcMain.handle('project:save', async (_event, filePath: string, data: string) => {
    await fs.writeFile(filePath, data, 'utf-8');
  });

  ipcMain.handle('project:load', async (_event, filePath: string) => {
    return await fs.readFile(filePath, 'utf-8');
  });

  ipcMain.handle('project:show-save-dialog', async (_event, defaultName?: string) => {
    const result = await dialog.showSaveDialog(mainWindow!, {
      title: 'Save Project',
      defaultPath: defaultName || 'Untitled.mdaw',
      filters: [{ name: 'Micro DAW Project', extensions: ['mdaw'] }],
    });
    return result.canceled ? null : result.filePath;
  });

  ipcMain.handle('project:show-open-dialog', async () => {
    const result = await dialog.showOpenDialog(mainWindow!, {
      title: 'Open Project',
      filters: [{ name: 'Micro DAW Project', extensions: ['mdaw'] }],
      properties: ['openFile'],
    });
    return result.canceled ? null : result.filePaths[0];
  });

  ipcMain.handle('project:set-title', async (_event, title: string) => {
    mainWindow!.setTitle(title);
  });

  ipcMain.handle('project:show-unsaved-dialog', async (_event, projectName: string) => {
    const result = await dialog.showMessageBox(mainWindow!, {
      type: 'warning',
      title: 'Unsaved Changes',
      message: `"${projectName}" has unsaved changes.`,
      detail: 'Do you want to save before continuing?',
      buttons: ['Save', "Don't Save", 'Cancel'],
      defaultId: 0,
      cancelId: 2,
    });
    const map = ['save', 'dont-save', 'cancel'] as const;
    return map[result.response];
  });

  ipcMain.on('project:force-close', () => {
    forceCloseAllowed = true;
    mainWindow!.close();
  });

  // --- Sample I/O ---
  ipcMain.handle('sample:show-import-dialog', async () => {
    const result = await dialog.showOpenDialog(mainWindow!, {
      title: 'Import Audio File',
      filters: [
        { name: 'Audio Files', extensions: ['wav', 'mp3', 'ogg', 'flac', 'aiff', 'aif', 'm4a'] },
      ],
      properties: ['openFile', 'multiSelections'],
    });
    return result.canceled ? null : result.filePaths;
  });

  ipcMain.handle('sample:read-file', async (_event, filePath: string) => {
    const data = await fs.readFile(filePath);
    return data.buffer.slice(data.byteOffset, data.byteOffset + data.byteLength);
  });

  // --- Auto-updater ---
  ipcMain.handle('updater:check', () => {
    autoUpdater.checkForUpdates().catch((err) => {
      sendUpdaterStatus({ state: 'error', message: err?.message ?? 'Check failed' });
    });
  });
  ipcMain.handle('updater:download', () => {
    autoUpdater.downloadUpdate().catch((err) => {
      sendUpdaterStatus({ state: 'error', message: err?.message ?? 'Download failed' });
    });
  });
  ipcMain.handle('updater:install', () => {
    autoUpdater.quitAndInstall(false, true);
  });
  ipcMain.handle('updater:get-version', () => {
    return app.getVersion();
  });
};

app.on('ready', () => {
  // Auto-grant media + MIDI permissions for the renderer
  const allowedPermissions = new Set(['midi', 'midiSysex']);
  session.defaultSession.setPermissionCheckHandler((_webContents, permission) => {
    return allowedPermissions.has(permission);
  });
  session.defaultSession.setPermissionRequestHandler((_webContents, permission, callback) => {
    callback(allowedPermissions.has(permission));
  });

  registerIpcHandlers();
  createWindow();
});

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') {
    app.quit();
  }
});

app.on('activate', () => {
  if (BrowserWindow.getAllWindows().length === 0) {
    createWindow();
  }
});
