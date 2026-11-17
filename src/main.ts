import { app, BrowserWindow, dialog, ipcMain, session, shell } from 'electron';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import fs from 'node:fs/promises';
import started from 'electron-squirrel-startup';
import { autoUpdater } from 'electron-updater';
import type { FsDirEntry } from './types/electron';
import { extOf, isAudioFileName, isHiddenName, sortEntries } from './shared/fsEntries';
import { PACKS_DIR_ARG } from './shared/packSource';
import { registerPackIpc } from './main/packs/ipc';
import { createTrustedIpc, installWebContentsGuards } from './main/security';

// Handle creating/removing shortcuts on Windows when installing/uninstalling.
if (started) {
  app.quit();
}

/**
 * `npm run test:electron` (scripts/electron-smoke.mjs) runs the real app with
 * MICRO_DAW_SMOKE=1: the window stays hidden and DevTools stay closed.
 */
const SMOKE_TEST = process.env.MICRO_DAW_SMOKE === '1';

let mainWindow: BrowserWindow | null = null;
let forceCloseAllowed = false;
let packsDirPath: string | null = null;

/** URL the main window loads (Vite dev server, or the packaged index.html). The only trusted renderer. */
const RENDERER_URL =
  MAIN_WINDOW_VITE_DEV_SERVER_URL ||
  pathToFileURL(path.join(__dirname, `../renderer/${MAIN_WINDOW_VITE_NAME}/index.html`)).href;

// Guards for every webContents (main window and anything created later):
// no new windows (https links open in the browser), no navigation away from
// the app (incl. a file dropped outside a drop zone), no <webview>.
app.on('web-contents-created', (_event, contents) => {
  installWebContentsGuards(contents, { rendererUrl: RENDERER_URL, openExternal: (url) => shell.openExternal(url) });
});

// --- Auto-updater ---
autoUpdater.autoDownload = false;
autoUpdater.autoInstallOnAppQuit = true;

if (app.isPackaged) {
  autoUpdater.setFeedURL({
    provider: 'github',
    owner: 'yemirhan',
    repo: 'micro-daw',
    releaseType: 'release',
  });
}

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

const createWindow = (packsDir: string) => {
  mainWindow = new BrowserWindow({
    width: 1360,
    height: 800,
    minWidth: 1360,
    minHeight: 768,
    show: false,
    title: 'Untitled - Micro DAW',
    titleBarStyle: 'hiddenInset',
    trafficLightPosition: { x: 12, y: 17 },
    // Matches the StartScreen / splash background (--intro-bg) so there is no flash.
    backgroundColor: '#100e0c',
    webPreferences: {
      preload: path.join(__dirname, 'preload.cjs'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
      webviewTag: false,
      // Lets the preload expose the packs folder synchronously (pack sample sources resolve against it).
      additionalArguments: [`${PACKS_DIR_ARG}${packsDir}`],
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
    if (!SMOKE_TEST) mainWindow!.show();
  });

  if (!app.isPackaged && !SMOKE_TEST) {
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
  // Every handler checks the sender is the app renderer's top frame.
  const ipc = createTrustedIpc(ipcMain, RENDERER_URL);

  // --- Fullscreen ---
  ipc.handle('toggle-fullscreen', () => {
    mainWindow!.setFullScreen(!mainWindow!.isFullScreen());
    return mainWindow!.isFullScreen();
  });

  ipc.handle('is-fullscreen', () => {
    return mainWindow!.isFullScreen();
  });

  // --- Project I/O ---
  ipc.handle('project:save', async (_event, filePath: string, data: string) => {
    await fs.writeFile(filePath, data, 'utf-8');
  });

  ipc.handle('project:load', async (_event, filePath: string) => {
    return await fs.readFile(filePath, 'utf-8');
  });

  ipc.handle('project:show-save-dialog', async (_event, defaultName?: string) => {
    const result = await dialog.showSaveDialog(mainWindow!, {
      title: 'Save Project',
      defaultPath: defaultName || 'Untitled.mdaw',
      filters: [{ name: 'Micro DAW Project', extensions: ['mdaw'] }],
    });
    return result.canceled ? null : result.filePath;
  });

  ipc.handle('project:show-open-dialog', async () => {
    const result = await dialog.showOpenDialog(mainWindow!, {
      title: 'Open Project',
      filters: [{ name: 'Micro DAW Project', extensions: ['mdaw'] }],
      properties: ['openFile'],
    });
    return result.canceled ? null : result.filePaths[0];
  });

  ipc.handle('project:set-title', async (_event, title: string) => {
    mainWindow!.setTitle(title);
  });

  ipc.handle('project:show-unsaved-dialog', async (_event, projectName: string) => {
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

  ipc.on('project:force-close', () => {
    forceCloseAllowed = true;
    mainWindow!.close();
  });

  // --- Sample I/O ---
  ipc.handle('sample:show-import-dialog', async () => {
    const result = await dialog.showOpenDialog(mainWindow!, {
      title: 'Import Audio File',
      filters: [
        { name: 'Audio Files', extensions: ['wav', 'mp3', 'ogg', 'flac', 'aiff', 'aif', 'm4a'] },
      ],
      properties: ['openFile', 'multiSelections'],
    });
    return result.canceled ? null : result.filePaths;
  });

  ipc.handle('sample:read-file', async (_event, filePath: string) => {
    const data = await fs.readFile(filePath);
    return data.buffer.slice(data.byteOffset, data.byteOffset + data.byteLength);
  });

  // --- File system browsing (sample browser) ---
  // Lists a single directory level (no recursion): sub-folders + audio files,
  // dotfiles hidden, folders first then case-insensitive by name.
  ipc.handle('fs:list-dir', async (_event, dirPath: unknown): Promise<FsDirEntry[]> => {
    if (typeof dirPath !== 'string' || dirPath.length === 0 || dirPath.length > 4096 || dirPath.includes('\0')) {
      throw new Error('Invalid directory path');
    }
    if (!path.isAbsolute(dirPath)) throw new Error('Directory path must be absolute');
    const dir = path.resolve(dirPath);
    const dirents = await fs.readdir(dir, { withFileTypes: true });
    const entries: FsDirEntry[] = [];
    await Promise.all(
      dirents.map(async (d) => {
        if (isHiddenName(d.name)) return;
        const full = path.join(dir, d.name);
        let isDir = d.isDirectory();
        let isFile = d.isFile();
        if (d.isSymbolicLink()) {
          try {
            const st = await fs.stat(full);
            isDir = st.isDirectory();
            isFile = st.isFile();
          } catch {
            return; // dangling link
          }
        }
        if (isDir) entries.push({ name: d.name, path: full, isDir: true, ext: '' });
        else if (isFile && isAudioFileName(d.name)) entries.push({ name: d.name, path: full, isDir: false, ext: extOf(d.name) });
      }),
    );
    return sortEntries(entries);
  });

  ipc.handle('fs:pick-folder', async () => {
    const result = await dialog.showOpenDialog(mainWindow!, {
      title: 'Add Sample Folder',
      properties: ['openDirectory'],
    });
    return result.canceled || result.filePaths.length === 0 ? null : result.filePaths[0];
  });

  ipc.handle('fs:home-dir', () => app.getPath('home'));

  // --- Auto-updater ---
  ipc.handle('updater:check', async () => {
    try {
      const result = await autoUpdater.checkForUpdates();
      if (!result) {
        sendUpdaterStatus({ state: 'not-available' });
      }
    } catch (err: unknown) {
      const message = err instanceof Error ? err.message : 'Check failed';
      sendUpdaterStatus({ state: 'error', message });
    }
  });
  ipc.handle('updater:download', () => {
    autoUpdater.downloadUpdate().catch((err) => {
      sendUpdaterStatus({ state: 'error', message: err?.message ?? 'Download failed' });
    });
  });
  ipc.handle('updater:install', () => {
    autoUpdater.quitAndInstall(false, true);
  });
  ipc.handle('updater:get-version', () => {
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
  // --- Pack store (packs:*) ---
  // Only the main window's renderer (dev server or packaged index.html) may call packs:*.
  const { packsDir } = registerPackIpc({ rendererUrl: RENDERER_URL });
  packsDirPath = packsDir;
  createWindow(packsDir);
});

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') {
    app.quit();
  }
});

app.on('activate', () => {
  if (BrowserWindow.getAllWindows().length === 0 && packsDirPath) {
    createWindow(packsDirPath);
  }
});
