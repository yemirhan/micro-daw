/**
 * Installs packs into `<packsDir>/<id>` (main process).
 *
 * Files download into a fresh `<packsDir>/.tmp-<id>-XXXXXX` folder; once every
 * file is in and `pack.json` is written, the folder is renamed into place in
 * one step (an existing install is moved aside first and restored if the swap
 * fails). Temp folders are removed on failure, on cancel and at startup
 * (`cleanupStale`). Dot-folders are hidden from the sample browser listing.
 */
import { randomBytes } from 'node:crypto';
import type { Dirent } from 'node:fs';
import fs from 'node:fs/promises';
import path from 'node:path';
import type { InstalledPack, PackInstallState, PackManifest } from '../../types/packs';
import { downloadToFile, isAbortError, type FetchLike } from './download';
import {
  declaredPackBytes,
  isValidPackId,
  MAX_FILE_BYTES,
  MAX_PACK_BYTES,
  PACK_META_FILE,
  safePackPathSegments,
  validateInstalled,
  validateManifest,
} from './validate';

export const TMP_PREFIX = '.tmp-';
export const OLD_PREFIX = '.old-';
const MAX_META_BYTES = 10 * 1024 * 1024;

export interface PackInstallerOptions {
  packsDir: string;
  /** Receives every install state change (and throttled download progress). */
  onState?: (id: string, state: PackInstallState) => void;
  fetchImpl?: FetchLike;
  /** Parallel file downloads per pack (default 4). */
  concurrency?: number;
  /** Min ms between progress events (default 100). */
  progressIntervalMs?: number;
  idleTimeoutMs?: number;
}

interface ActiveInstall {
  ctrl: AbortController;
  promise: Promise<InstalledPack>;
  tmpDir: string | null;
}

/** Absolute destination of a pack file inside `root`; throws when it would escape `root`. */
export function resolveInside(root: string, rel: string): string {
  const segments = safePackPathSegments(rel);
  const base = path.resolve(root);
  const dest = path.resolve(base, ...segments);
  if (!dest.startsWith(base + path.sep)) throw new Error(`Unsafe file path: ${rel}`);
  return dest;
}

export class PackInstaller {
  readonly packsDir: string;
  private readonly opts: PackInstallerOptions;
  private readonly active = new Map<string, ActiveInstall>();
  private readonly states = new Map<string, PackInstallState>();

  constructor(opts: PackInstallerOptions) {
    this.opts = opts;
    this.packsDir = path.resolve(opts.packsDir);
  }

  getState(id: string): PackInstallState {
    return this.states.get(id) ?? { state: 'idle' };
  }

  isInstalling(id: string): boolean {
    return this.active.has(id);
  }

  private emit(id: string, state: PackInstallState): void {
    this.states.set(id, state);
    try {
      this.opts.onState?.(id, state);
    } catch (err) {
      console.warn('[packs] state listener failed', err);
    }
  }

  private packDir(id: string): string {
    if (!isValidPackId(id)) throw new Error('Invalid pack id');
    return path.join(this.packsDir, id);
  }

  /** Installs (or re-installs/updates) a pack. Concurrent calls for the same id share one install. */
  install(manifestRaw: PackManifest): Promise<InstalledPack> {
    const manifest = validateManifest(manifestRaw);
    const existing = this.active.get(manifest.id);
    if (existing) return existing.promise;
    const ctrl = new AbortController();
    const entry: ActiveInstall = { ctrl, tmpDir: null, promise: Promise.resolve(null as unknown as InstalledPack) };
    entry.promise = this.run(manifest, entry).finally(() => {
      if (this.active.get(manifest.id) === entry) this.active.delete(manifest.id);
    });
    this.active.set(manifest.id, entry);
    return entry.promise;
  }

  /** Cancels a running install (its temp folder is removed). Returns false if none was running. */
  cancel(id: string): boolean {
    const entry = this.active.get(id);
    if (!entry) return false;
    entry.ctrl.abort();
    return true;
  }

  private async run(manifest: PackManifest, entry: ActiveInstall): Promise<InstalledPack> {
    const { id } = manifest;
    const signal = entry.ctrl.signal;
    const files = manifest.files;
    const totalBytes = declaredPackBytes(manifest);
    const received = new Array<number>(files.length).fill(0);
    let done = 0;
    let lastEmit = 0;
    const interval = this.opts.progressIntervalMs ?? 100;
    const progress = (force: boolean) => {
      const now = Date.now();
      if (!force && now - lastEmit < interval) return;
      lastEmit = now;
      this.emit(id, {
        state: 'downloading',
        receivedBytes: received.reduce((a, b) => a + b, 0),
        totalBytes,
        fileIndex: done,
        fileCount: files.length,
      });
    };

    try {
      progress(true);
      await fs.mkdir(this.packsDir, { recursive: true });
      const tmpDir = await fs.mkdtemp(path.join(this.packsDir, `${TMP_PREFIX}${id}-`));
      entry.tmpDir = tmpDir;

      // Resolve every destination before downloading anything.
      const dests = files.map((f) => resolveInside(tmpDir, f.path));
      const dirs = new Set(dests.map((d) => path.dirname(d)));
      for (const d of dirs) await fs.mkdir(d, { recursive: true });

      let next = 0;
      const worker = async () => {
        while (!signal.aborted) {
          const i = next++;
          if (i >= files.length) return;
          const file = files[i];
          await downloadToFile(file.url, dests[i], {
            maxBytes: MAX_FILE_BYTES,
            sha256: file.sha256,
            signal,
            fetchImpl: this.opts.fetchImpl,
            idleTimeoutMs: this.opts.idleTimeoutMs,
            onProgress: (bytes) => {
              received[i] = bytes;
              if (received.reduce((a, b) => a + b, 0) > MAX_PACK_BYTES) {
                entry.ctrl.abort(new Error('Pack too large'));
                return;
              }
              progress(false);
            },
          });
          done++;
          progress(true);
        }
      };
      const workers = Array.from({ length: Math.max(1, Math.min(this.opts.concurrency ?? 4, files.length)) }, worker);
      // Abort the rest as soon as one file fails.
      await Promise.all(
        workers.map((w) =>
          w.catch((err) => {
            if (!signal.aborted) entry.ctrl.abort(err);
            throw err;
          }),
        ),
      ).catch(() => {
        throw signal.reason instanceof Error && !isAbortError(signal.reason) ? signal.reason : new Error('aborted');
      });
      if (signal.aborted) throw signal.reason ?? new Error('aborted');

      const finalDir = this.packDir(id);
      const installed: InstalledPack = {
        id,
        version: manifest.version,
        dir: finalDir,
        installedAt: new Date().toISOString(),
        manifest,
      };
      await fs.writeFile(path.join(tmpDir, PACK_META_FILE), JSON.stringify(installed, null, 2), 'utf-8');
      if (signal.aborted) throw signal.reason ?? new Error('aborted');
      await this.swapIntoPlace(tmpDir, finalDir, id);
      entry.tmpDir = null;
      this.emit(id, { state: 'installed' });
      return installed;
    } catch (rawErr) {
      if (entry.tmpDir) await fs.rm(entry.tmpDir, { recursive: true, force: true }).catch(() => undefined);
      entry.tmpDir = null;
      const cancelled = signal.aborted && (signal.reason === undefined || isAbortError(signal.reason));
      if (cancelled) {
        this.emit(id, (await this.isInstalled(id)) ? { state: 'installed' } : { state: 'idle' });
        throw new Error('Install cancelled', { cause: rawErr });
      }
      const message = rawErr instanceof Error ? rawErr.message : String(rawErr);
      this.emit(id, { state: 'error', message });
      throw rawErr instanceof Error ? rawErr : new Error(message);
    }
  }

  /** Atomically replaces `<packsDir>/<id>` with `tmpDir`. */
  private async swapIntoPlace(tmpDir: string, finalDir: string, id: string): Promise<void> {
    let oldDir: string | null = null;
    if (await exists(finalDir)) {
      oldDir = path.join(this.packsDir, `${OLD_PREFIX}${id}-${randomBytes(4).toString('hex')}`);
      await fs.rename(finalDir, oldDir);
    }
    try {
      await fs.rename(tmpDir, finalDir);
    } catch (err) {
      if (oldDir) await fs.rename(oldDir, finalDir).catch(() => undefined);
      throw err;
    }
    if (oldDir) await fs.rm(oldDir, { recursive: true, force: true }).catch(() => undefined);
  }

  async isInstalled(id: string): Promise<boolean> {
    return exists(path.join(this.packDir(id), PACK_META_FILE));
  }

  /** Removes an installed pack (cancelling a running install first). */
  async uninstall(id: string): Promise<void> {
    const dir = this.packDir(id);
    const running = this.active.get(id);
    if (running) {
      running.ctrl.abort();
      await running.promise.catch(() => undefined);
    }
    if (await exists(dir)) {
      // Move aside first so a half-deleted folder never looks installed.
      const doomed = path.join(this.packsDir, `${OLD_PREFIX}${id}-${randomBytes(4).toString('hex')}`);
      await fs.rename(dir, doomed);
      await fs.rm(doomed, { recursive: true, force: true });
    }
    this.emit(id, { state: 'idle' });
  }

  /** Installed packs with a valid pack.json (folder name must equal the pack id). */
  async listInstalled(): Promise<InstalledPack[]> {
    let dirents: Dirent[];
    try {
      dirents = await fs.readdir(this.packsDir, { withFileTypes: true });
    } catch {
      return [];
    }
    const out: InstalledPack[] = [];
    for (const d of dirents) {
      if (!d.isDirectory() || !isValidPackId(d.name)) continue;
      const dir = path.join(this.packsDir, d.name);
      try {
        const metaPath = path.join(dir, PACK_META_FILE);
        const st = await fs.stat(metaPath);
        if (!st.isFile() || st.size > MAX_META_BYTES) continue;
        const pack = validateInstalled(JSON.parse(await fs.readFile(metaPath, 'utf-8')), dir);
        if (pack.id !== d.name) continue;
        out.push(pack);
      } catch (err) {
        console.warn('[packs] skipping invalid installed pack', d.name, err instanceof Error ? err.message : err);
      }
    }
    return out.sort((a, b) => a.manifest.name.localeCompare(b.manifest.name));
  }

  /** Removes leftover temp/old folders (crash or quit mid-install). Skips folders of running installs. */
  async cleanupStale(): Promise<void> {
    let names: string[];
    try {
      names = await fs.readdir(this.packsDir);
    } catch {
      return;
    }
    const busy = new Set([...this.active.values()].map((a) => a.tmpDir).filter((d): d is string => !!d));
    await Promise.all(
      names
        .filter((n) => n.startsWith(TMP_PREFIX) || n.startsWith(OLD_PREFIX))
        .map((n) => path.join(this.packsDir, n))
        .filter((p) => !busy.has(p))
        .map((p) => fs.rm(p, { recursive: true, force: true }).catch(() => undefined)),
    );
  }

  /** Cancels everything (app quit); temp folders are removed by the failing installs. */
  async cancelAll(): Promise<void> {
    const running = [...this.active.values()];
    for (const r of running) r.ctrl.abort();
    await Promise.all(running.map((r) => r.promise.catch(() => undefined)));
  }
}

async function exists(p: string): Promise<boolean> {
  try {
    await fs.lstat(p);
    return true;
  } catch {
    return false;
  }
}
