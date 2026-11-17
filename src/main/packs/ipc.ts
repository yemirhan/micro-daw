/**
 * Pack store IPC (main process). The renderer only ever passes a pack id or a
 * preview URL: manifests (and so every download URL) come from the validated
 * catalog held here, never from the renderer.
 */
import { app, BrowserWindow, ipcMain } from 'electron';
import path from 'node:path';
import type { InstalledPack, PackCatalog, PackInstallState } from '../../types/packs';
import { fetchCatalog } from './catalog';
import { PackInstaller } from './installer';
import { PreviewFetcher } from './preview';
import { isAllowedUrl, isValidPackId } from './validate';

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

/** Registers the `packs:*` handlers and cleans up leftovers of interrupted installs. */
export function registerPackIpc(): { packsDir: string; installer: PackInstaller } {
  const packsDir = path.join(app.getPath('userData'), 'packs');

  const installer = new PackInstaller({
    packsDir,
    onState: (id, state) => {
      for (const win of BrowserWindow.getAllWindows()) {
        if (!win.isDestroyed()) win.webContents.send(PACK_CHANNELS.progress, { id, state });
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

  void installer.cleanupStale();
  app.on('will-quit', () => {
    void installer.cancelAll();
  });

  ipcMain.handle(PACK_CHANNELS.fetchCatalog, () => loadCatalog());

  ipcMain.handle(PACK_CHANNELS.listInstalled, (): Promise<InstalledPack[]> => installer.listInstalled());

  ipcMain.handle(PACK_CHANNELS.install, async (_e, rawId: unknown): Promise<InstalledPack> => {
    const id = assertId(rawId);
    const cat = catalog ?? (await loadCatalog());
    const manifest = cat.packs.find((p) => p.id === id);
    if (!manifest) throw new Error(`Unknown pack: ${id}`);
    return installer.install(manifest);
  });

  ipcMain.handle(PACK_CHANNELS.cancel, (_e, rawId: unknown): boolean => installer.cancel(assertId(rawId)));

  ipcMain.handle(PACK_CHANNELS.uninstall, (_e, rawId: unknown): Promise<void> => installer.uninstall(assertId(rawId)));

  ipcMain.handle(PACK_CHANNELS.fetchPreview, (_e, url: unknown): Promise<ArrayBuffer> => {
    if (typeof url !== 'string' || !isAllowedUrl(url)) throw new Error('Preview URL not allowed');
    return previews.fetch(url);
  });

  ipcMain.handle(PACK_CHANNELS.dir, () => packsDir);

  ipcMain.handle(PACK_CHANNELS.states, (): Record<string, PackInstallState> => {
    const out: Record<string, PackInstallState> = {};
    for (const p of catalog?.packs ?? []) {
      if (installer.isInstalling(p.id)) out[p.id] = installer.getState(p.id);
    }
    return out;
  });

  return { packsDir, installer };
}
