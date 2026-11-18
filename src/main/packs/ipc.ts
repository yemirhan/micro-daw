/**
 * Pack store IPC (main process). The renderer only ever passes a pack id or a
 * preview URL: manifests (and so every download URL) come from the validated
 * catalog held here, never from the renderer, and a preview URL must be a
 * file URL of that catalog.
 */
import { app, BrowserWindow, ipcMain } from 'electron';
import path from 'node:path';
import type { InstalledPack, PackCatalog, PackInstallState } from '../../types/packs';
import { fetchCatalog } from './catalog';
import { PackInstaller } from './installer';
import { PreviewFetcher } from './preview';
import { createTrustedIpc } from '../security';
import { isTrustedRendererUrl } from './sender';
import { findCatalogFile, isValidPackId } from './validate';
import { pauseInstallsOnQuit } from './quit';
import { INSTALL_PAUSED_MESSAGE } from './pausedState';

export const PACK_CHANNELS = {
  fetchCatalog: 'packs:fetch-catalog',
  listInstalled: 'packs:list-installed',
  install: 'packs:install',
  cancel: 'packs:cancel',
  uninstall: 'packs:uninstall',
  fetchPreview: 'packs:fetch-preview',
  dir: 'packs:dir',
  states: 'packs:states',
  progress: 'packs:progress',
} as const;

function assertId(id: unknown): string {
  if (!isValidPackId(id)) throw new Error('Invalid pack id');
  return id;
}

export interface PackIpcOptions {
  /**
   * URL the main window loads (Vite dev server, or the packaged index.html as a
   * file: URL). Only that renderer's top frame may call `packs:*`.
   */
  rendererUrl: string;
}

/** Registers the `packs:*` handlers and cleans up leftovers of interrupted installs. */
export function registerPackIpc({ rendererUrl }: PackIpcOptions): { packsDir: string; installer: PackInstaller } {
  /** ipcMain.handle that first checks the sender is the app's own renderer. */
  const { handle } = createTrustedIpc(ipcMain, rendererUrl);

  const packsDir = path.join(app.getPath('userData'), 'packs');

  const installer = new PackInstaller({
    packsDir,
    onState: (id, state) => {
      for (const win of BrowserWindow.getAllWindows()) {
        if (win.isDestroyed() || !isTrustedRendererUrl(win.webContents.getURL(), rendererUrl)) continue;
        win.webContents.send(PACK_CHANNELS.progress, { id, state });
      }
    },
  });
  const previews = new PreviewFetcher();
  let catalog: PackCatalog | null = null;
  let catalogLoading: Promise<PackCatalog> | null = null;

  const loadCatalog = (): Promise<PackCatalog> => {
    catalogLoading ??= fetchCatalog()
      .then((r) => {
        catalog = r.catalog;
        return r.catalog;
      })
      .finally(() => {
        catalogLoading = null;
      });
    return catalogLoading;
  };

  // Startup housekeeping (incl. restoring an interrupted update) before anything lists or installs.
  const ready = installer.cleanupStale().catch((err: unknown) => {
    console.warn('[packs] cleanup failed', err);
  });
  pauseInstallsOnQuit(app, installer);

  handle(PACK_CHANNELS.fetchCatalog, () => loadCatalog());

  handle(PACK_CHANNELS.listInstalled, async (): Promise<InstalledPack[]> => {
    await ready;
    return installer.listInstalled();
  });

  handle(PACK_CHANNELS.install, async (_e, rawId: unknown): Promise<InstalledPack> => {
    const id = assertId(rawId);
    await ready;
    const cat = catalog ?? (await loadCatalog());
    const manifest = cat.packs.find((p) => p.id === id);
    if (!manifest) throw new Error(`Unknown pack: ${id}`);
    return installer.install(manifest);
  });

  handle(PACK_CHANNELS.cancel, (_e, rawId: unknown): boolean => installer.cancel(assertId(rawId)));

  handle(PACK_CHANNELS.uninstall, async (_e, rawId: unknown): Promise<void> => {
    const id = assertId(rawId);
    await ready;
    return installer.uninstall(id);
  });

  handle(PACK_CHANNELS.fetchPreview, async (_e, url: unknown): Promise<ArrayBuffer> => {
    if (typeof url !== 'string' || url.length > 4096) throw new Error('Preview URL not allowed');
    const file = findCatalogFile(catalog ?? (await loadCatalog()), url);
    if (!file) throw new Error('Preview URL not allowed');
    return previews.fetch(file.url, { size: file.size, sha256: file.sha256 });
  });

  handle(PACK_CHANNELS.dir, () => packsDir);

  handle(PACK_CHANNELS.states, async (): Promise<Record<string, PackInstallState>> => {
    await ready;
    const out: Record<string, PackInstallState> = {};
    // N7: interrupted installs (staging kept) show "Resume" on their card.
    for (const id of await installer.pausedIds()) out[id] = { state: 'error', message: INSTALL_PAUSED_MESSAGE };
    for (const p of catalog?.packs ?? []) {
      if (installer.isInstalling(p.id)) out[p.id] = installer.getState(p.id);
    }
    return out;
  });

  return { packsDir, installer };
}
