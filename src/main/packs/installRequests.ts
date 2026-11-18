/**
 * `packs:install` requests between the IPC call and the installer (phase-6
 * review finding 10, fixed in both apps). A request first waits for the
 * startup cleanup and the catalog; before this, a cancel in that window was
 * lost (`installer.cancel` returned false: nothing was installing yet) and the
 * install then started anyway, and a quit in that window did not pause it.
 *
 * Every request is registered before those waits: `cancel` marks queued
 * requests of the pack, `pauseAll` (app quit) marks every queued request, and
 * `busy` counts them. A marked request never reaches the installer: it
 * rejects with `Error('Install cancelled')` (the pack's state is reported the
 * way a cancelled install reports it) or `InstallPausedError`.
 */
import type { InstalledPack, PackManifest } from '../../types/packs';
import { InstallPausedError } from './installer';

export interface RequestInstaller {
  readonly busy: boolean;
  install(manifest: PackManifest): Promise<InstalledPack>;
  cancel(id: string): boolean;
  pauseAll(): Promise<void>;
  reportCancelled(id: string): Promise<void>;
}

interface Queued {
  id: string;
  mark: null | 'cancelled' | 'paused';
}

export class InstallRequests {
  private readonly queued = new Set<Queued>();

  constructor(private readonly installer: RequestInstaller) {}

  /** `packs:install`: waits for `prepare` (cleanup + catalog lookup), then installs unless cancelled / paused meanwhile. */
  async install(id: string, prepare: () => Promise<PackManifest>): Promise<InstalledPack> {
    const req: Queued = { id, mark: null };
    this.queued.add(req);
    let manifest: PackManifest;
    try {
      manifest = await prepare();
    } finally {
      this.queued.delete(req);
    }
    if (req.mark === 'paused') throw new InstallPausedError();
    if (req.mark === 'cancelled') {
      await this.installer.reportCancelled(id);
      throw new Error('Install cancelled');
    }
    return this.installer.install(manifest);
  }

  /** `packs:cancel`: true when an install or a queued request of `id` was cancelled. */
  cancel(id: string): boolean {
    let hit = false;
    for (const r of this.queued) {
      if (r.id === id && r.mark === null) {
        r.mark = 'cancelled';
        hit = true;
      }
    }
    return this.installer.cancel(id) || hit;
  }

  /** Installs running or requests still waiting to start. */
  get busy(): boolean {
    return this.installer.busy || this.queued.size > 0;
  }

  /** App quit: queued requests never start; running installs pause (staging kept). */
  async pauseAll(): Promise<void> {
    for (const r of this.queued) r.mark ??= 'paused';
    await this.installer.pauseAll();
  }
}
