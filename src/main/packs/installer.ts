/**
 * Installs packs into `<packsDir>/<id>` (main process).
 *
 * Files download into a staging folder `<packsDir>/.partial-<id>`. Each file
 * is retried with backoff on transient errors, and its exact size + sha256
 * are verified. Once every file is in and `pack.json` is written, the folder
 * is renamed into place in one step (an existing install is moved aside to
 * `.old-<id>-<hex>` first and restored if the swap fails).
 *
 * Resume: when an install fails (or is paused because the app quits, see
 * `pauseAll`), the staging folder is kept, so Retry only downloads the files
 * that are not already there with the right size and sha256. A cancel
 * removes it. An install settles only after every download worker has
 * settled (incl. the partial-file cleanup of aborted ones), so a Retry never
 * races the previous attempt's cleanup. Leftovers are handled at startup
 * (`cleanupStale`): an install interrupted between the two renames (pack
 * folder missing, valid `.old-<id>-*` present) is restored; `.del-*` (from
 * uninstall), legacy `.tmp-*` and other `.old-*` folders are removed (but a
 * backup is only removed once a valid install of that pack is confirmed);
 * staging folders older than a week are removed. Dot-folders are hidden from
 * the sample browser listing.
 *
 * Packs folder protocol (`docs/rune-port/packs-dir-protocol.md`, shared with
 * the Rust port): every mutating operation holds the cross-process lock
 * `<packs>/.lock/` (lock.ts); an install of a pack waits for an uninstall of
 * the same pack; downloaded files, pack.json, the staging folders and the
 * packs folder are fsynced before `.old-*` is removed (an "installed" pack
 * survives a power cut); symlinks planted in the packs folder are never
 * followed (a symlinked staging folder is unlinked and recreated).
 */
import { createHash, randomBytes } from 'node:crypto';
import { createReadStream, type Dirent } from 'node:fs';
import fs, { type FileHandle } from 'node:fs/promises';
import path from 'node:path';
import type { InstalledPack, PackInstallState, PackManifest } from '../../types/packs';
import { downloadToFileWithRetry, isAbortError, type FetchLike, type RetryOptions } from './download';
import { LOCK_BUSY_MESSAGE, LOCK_STALE_PREFIX, PacksDirLock, UNINSTALL_LOCK_WAIT_MS, type LockRelease, type PacksDirLockOptions } from './lock';
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

/** Legacy temp folders (older app versions); removed at startup. */
export const TMP_PREFIX = '.tmp-';
/** Previous install moved aside during an update swap. */
export const OLD_PREFIX = '.old-';
/** Folder being deleted by uninstall. */
export const DEL_PREFIX = '.del-';
/** Staging folder of an install (kept after a failure so Retry resumes). */
export const PARTIAL_PREFIX = '.partial-';
/** Staging folders untouched for this long are removed at startup. */
export const PARTIAL_MAX_AGE_MS = 7 * 24 * 60 * 60 * 1000;
const MAX_META_BYTES = 10 * 1024 * 1024;
const OLD_NAME_RE = /^\.old-([a-z0-9]+(?:-[a-z0-9]+)*)-[0-9a-f]{8}$/;

/** Abort reason of `pauseAll` (app quit): the staging folder is kept so the next start resumes. */
export class InstallPausedError extends Error {
  constructor() {
    super('Install paused');
    this.name = 'InstallPausedError';
  }
}

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
  /** Per-file retry policy (default 3 attempts, 500 ms / 1.5 s backoff). */
  retry?: Omit<RetryOptions, 'onRetry'>;
  /** Packs-folder lock tuning (tests). */
  lock?: PacksDirLockOptions;
  /** How long an uninstall waits for another owner of the packs folder (default 10 s). */
  uninstallLockWaitMs?: number;
}

interface ActiveInstall {
  ctrl: AbortController;
  promise: Promise<InstalledPack>;
  tmpDir: string | null;
  /** The packs-folder lock hold of this install (released once it settled). */
  release: LockRelease | null;
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
  /** Uninstalls in progress, by pack id: an install of that pack waits for it (§2). */
  private readonly uninstalling = new Map<string, Promise<void>>();
  /** Cross-process lock on the packs folder (§1). */
  readonly lock: PacksDirLock;

  constructor(opts: PackInstallerOptions) {
    this.opts = opts;
    this.packsDir = path.resolve(opts.packsDir);
    this.lock = new PacksDirLock(this.packsDir, opts.lock);
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

  /**
   * Installs (or re-installs/updates) a pack. Concurrent calls for the same id
   * share one install. An invalid manifest rejects (never throws synchronously).
   */
  install(manifestRaw: PackManifest): Promise<InstalledPack> {
    let manifest: PackManifest;
    try {
      manifest = validateManifest(manifestRaw);
    } catch (err) {
      return Promise.reject(err instanceof Error ? err : new Error(String(err)));
    }
    const existing = this.active.get(manifest.id);
    if (existing) return existing.promise;
    const ctrl = new AbortController();
    const entry: ActiveInstall = { ctrl, tmpDir: null, release: null, promise: Promise.resolve(null as unknown as InstalledPack) };
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

  /**
   * An install request that was cancelled before it reached the installer
   * (ipc.ts: still waiting for the startup cleanup or the catalog): reports
   * the pack's state the way a cancelled install does.
   */
  async reportCancelled(id: string): Promise<void> {
    if (this.active.has(id)) return;
    this.emit(id, (await this.isInstalled(id)) ? { state: 'installed' } : { state: 'idle' });
  }

  private async run(manifest: PackManifest, entry: ActiveInstall): Promise<InstalledPack> {
    try {
      return await this.runLocked(manifest, entry);
    } finally {
      const release = entry.release;
      entry.release = null;
      await release?.().catch(() => undefined);
    }
  }

  /** Waits (honouring `signal`) for an uninstall of `id` in progress (§2). */
  private async afterUninstall(id: string, signal: AbortSignal): Promise<void> {
    for (let u = this.uninstalling.get(id); u; u = this.uninstalling.get(id)) {
      if (signal.aborted) throw signal.reason;
      await new Promise<void>((resolve, reject) => {
        const onAbort = () => reject(signal.reason);
        signal.addEventListener('abort', onAbort, { once: true });
        void u!.catch(() => undefined).then(() => {
          signal.removeEventListener('abort', onAbort);
          resolve();
        });
      });
    }
  }

  private async runLocked(manifest: PackManifest, entry: ActiveInstall): Promise<InstalledPack> {
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
      // State stays "downloading 0 B" while another uninstall / owner holds the pack or folder.
      await this.afterUninstall(id, signal);
      entry.release = await this.lock.acquire({ signal });
      await fs.mkdir(this.packsDir, { recursive: true });
      const stageDir = this.stagingDir(id);
      entry.tmpDir = stageDir;
      await ensureRealDir(stageDir);

      // Resolve every destination before downloading anything.
      const dests = files.map((f) => resolveInside(stageDir, f.path));
      await fs.rm(path.join(stageDir, PACK_META_FILE), { force: true });
      await pruneUnexpected(stageDir, new Set(dests));
      const dirs = new Set(dests.map((d) => path.dirname(d)));
      for (const d of dirs) await fs.mkdir(d, { recursive: true });

      // Resume: keep files of an earlier attempt that are already complete and verified.
      const todo: number[] = [];
      for (let i = 0; i < files.length; i++) {
        if (signal.aborted) throw signal.reason ?? new Error('aborted');
        if (await isVerifiedFile(dests[i], files[i].size, files[i].sha256)) {
          received[i] = files[i].size;
          done++;
        } else {
          await fs.rm(dests[i], { recursive: true, force: true });
          todo.push(i);
        }
      }
      progress(true);

      let next = 0;
      const worker = async () => {
        while (!signal.aborted) {
          const i = todo[next++];
          if (i === undefined) return;
          const file = files[i];
          await downloadToFileWithRetry(
            file.url,
            dests[i],
            {
              maxBytes: Math.min(MAX_FILE_BYTES, file.size),
              expectedBytes: file.size,
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
            },
            {
              ...this.opts.retry,
              onRetry: () => {
                received[i] = 0;
                progress(true);
              },
            },
          );
          done++;
          progress(true);
        }
      };
      const workers = Array.from({ length: Math.max(1, Math.min(this.opts.concurrency ?? 4, todo.length)) }, worker);
      // Abort the rest as soon as one file fails, then wait for EVERY worker to
      // settle (B2): an aborted worker still removes its partial file, and that
      // must happen before this install settles and a Retry may start writing
      // the same staging paths.
      const results = await Promise.allSettled(
        workers.map((w) =>
          w.catch((err: unknown) => {
            if (!signal.aborted) entry.ctrl.abort(err);
            throw err;
          }),
        ),
      );
      if (results.some((r) => r.status === 'rejected')) {
        throw signal.reason instanceof Error && !isAbortError(signal.reason) ? signal.reason : new Error('aborted');
      }
      if (signal.aborted) throw signal.reason ?? new Error('aborted');

      const finalDir = this.packDir(id);
      const installed: InstalledPack = {
        id,
        version: manifest.version,
        dir: finalDir,
        installedAt: new Date().toISOString(),
        manifest,
      };
      await writeFileDurable(path.join(stageDir, PACK_META_FILE), JSON.stringify(installed, null, 2));
      // Every folder of the staging tree, deepest first, before the renames (§3).
      const stageDirs = new Set<string>([stageDir]);
      for (const d of dests) for (let p = path.dirname(d); p.startsWith(stageDir + path.sep); p = path.dirname(p)) stageDirs.add(p);
      for (const d of [...stageDirs].sort((a, b) => b.length - a.length)) await fsyncDir(d);
      if (signal.aborted) throw signal.reason ?? new Error('aborted');
      await this.swapIntoPlace(stageDir, finalDir, id);
      entry.tmpDir = null;
      this.emit(id, { state: 'installed' });
      return installed;
    } catch (rawErr) {
      const paused = signal.aborted && signal.reason instanceof InstallPausedError;
      const cancelled = !paused && signal.aborted && (signal.reason === undefined || isAbortError(signal.reason));
      // A cancel discards the staging folder; a failure or pause keeps it so Retry resumes.
      if (entry.tmpDir && cancelled) await fs.rm(entry.tmpDir, { recursive: true, force: true }).catch(() => undefined);
      else if (entry.tmpDir) await fs.rm(path.join(entry.tmpDir, PACK_META_FILE), { force: true }).catch(() => undefined);
      entry.tmpDir = null;
      if (cancelled || paused) {
        this.emit(id, (await this.isInstalled(id)) ? { state: 'installed' } : { state: 'idle' });
        if (paused) throw new InstallPausedError();
        throw new Error('Install cancelled', { cause: rawErr });
      }
      const message = rawErr instanceof Error ? rawErr.message : String(rawErr);
      this.emit(id, { state: 'error', message });
      throw rawErr instanceof Error ? rawErr : new Error(message);
    }
  }

  /**
   * Ids of packs with a resumable staging folder (`.partial-<id>` holding at
   * least one downloaded file) and no install running: an install paused by
   * an app quit or failed in an earlier session (N7). Get resumes them.
   */
  async pausedIds(): Promise<string[]> {
    const names = await fs.readdir(this.packsDir).catch(() => [] as string[]);
    const out: string[] = [];
    for (const n of names) {
      if (!n.startsWith(PARTIAL_PREFIX)) continue;
      const id = n.slice(PARTIAL_PREFIX.length);
      if (!isValidPackId(id) || this.active.has(id)) continue;
      const entries = await fs.readdir(path.join(this.packsDir, n), { recursive: true, withFileTypes: true }).catch(() => [] as Dirent[]);
      if (entries.some((e) => e.isFile())) out.push(id);
    }
    return out.sort();
  }

  /** Staging folder of an install of `id`. */
  stagingDir(id: string): string {
    if (!isValidPackId(id)) throw new Error('Invalid pack id');
    return path.join(this.packsDir, `${PARTIAL_PREFIX}${id}`);
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
    // Both renames durable before the backup goes (§3). Should the fsync fail,
    // the backup stays: the next start drops it once the install checks out.
    try {
      await fsyncDir(this.packsDir);
    } catch (err) {
      console.warn('[packs] could not sync the packs folder; keeping the backup', err instanceof Error ? err.message : err);
      return;
    }
    if (oldDir) await fs.rm(oldDir, { recursive: true, force: true }).catch(() => undefined);
  }

  async isInstalled(id: string): Promise<boolean> {
    return exists(path.join(this.packDir(id), PACK_META_FILE));
  }

  /** Removes an installed pack (cancelling a running install first). */
  async uninstall(id: string): Promise<void> {
    const dir = this.packDir(id);
    const previous = this.uninstalling.get(id);
    // The install running now is cancelled; one started after this call waits for it (§2).
    const running = this.active.get(id);
    const op = (async () => {
      await previous?.catch(() => undefined);
      if (running) {
        running.ctrl.abort();
        await running.promise.catch(() => undefined);
      }
      const release = await this.lock.acquire({ timeoutMs: this.opts.uninstallLockWaitMs ?? UNINSTALL_LOCK_WAIT_MS });
      if (!release) throw new Error(LOCK_BUSY_MESSAGE);
      try {
        if (await exists(dir)) {
          // Move aside first so a half-deleted folder never looks installed. A
          // different prefix from the update swap, so startup recovery never
          // brings an uninstalled pack back. A symlinked <id> moves as a link
          // and fs.rm unlinks it without following it.
          const doomed = path.join(this.packsDir, `${DEL_PREFIX}${id}-${randomBytes(4).toString('hex')}`);
          await fs.rename(dir, doomed);
          await fs.rm(doomed, { recursive: true, force: true });
        }
        await fs.rm(this.stagingDir(id), { recursive: true, force: true }).catch(() => undefined);
      } finally {
        await release();
      }
      this.emit(id, { state: 'idle' });
    })();
    // Registered before the first await: an install of this pack starting now waits for it.
    this.uninstalling.set(id, op);
    try {
      await op;
    } finally {
      if (this.uninstalling.get(id) === op) this.uninstalling.delete(id);
    }
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
        const pack = await readInstalledMeta(dir);
        if (!pack || pack.id !== d.name) continue;
        out.push(pack);
      } catch (err) {
        console.warn('[packs] skipping invalid installed pack', d.name, err instanceof Error ? err.message : err);
      }
    }
    return out.sort((a, b) => a.manifest.name.localeCompare(b.manifest.name));
  }

  /**
   * Startup housekeeping (skips folders of running installs):
   * 1. restores a pack whose update swap was interrupted (`<id>` missing, a
   *    `.old-<id>-*` with a valid pack.json present; the newest one wins);
   * 2. removes `.tmp-*`, `.del-*` and the remaining `.old-*` folders;
   * 3. removes staging folders (`.partial-*`) older than PARTIAL_MAX_AGE_MS.
   */
  async cleanupStale(now = Date.now()): Promise<void> {
    if (!(await exists(this.packsDir))) return;
    // One try: another owner (window) working in the folder already cleaned up at its start.
    const release = await this.lock.acquire({ timeoutMs: 0 });
    if (!release) {
      console.warn('[packs] packs folder in use by another micro-daw window; startup cleanup skipped');
      return;
    }
    try {
      await this.cleanupStaleLocked(now);
    } finally {
      await release();
    }
  }

  private async cleanupStaleLocked(now: number): Promise<void> {
    let names: string[];
    try {
      names = await fs.readdir(this.packsDir);
    } catch {
      return;
    }
    const busy = new Set([...this.active.values()].map((a) => a.tmpDir).filter((d): d is string => !!d));
    const present = new Set(names);

    // 1. Recovery of interrupted swaps. A backup (`.old-<id>-*` with a valid
    // pack.json) is only deleted once a valid install of <id> is confirmed:
    // when <id> is missing or invalid, the newest backup is restored; if that
    // fails, every backup of <id> is kept for the next start (M1).
    const candidates = new Map<string, { dir: string; installedAt: number; complete: boolean }[]>();
    for (const n of names) {
      const m = OLD_NAME_RE.exec(n);
      if (!m || this.active.has(m[1])) continue;
      const dir = path.join(this.packsDir, n);
      if (!(await isRealDir(dir))) continue; // a symlink is never a backup
      const pack = await readInstalledMeta(dir).catch(() => null);
      if (!pack || pack.id !== m[1]) continue;
      const list = candidates.get(m[1]) ?? [];
      list.push({ dir, installedAt: Date.parse(pack.installedAt) || 0, complete: await isCompleteInstallDir(dir, m[1]) });
      candidates.set(m[1], list);
    }
    const keep = new Set<string>();
    for (const [id, list] of candidates) {
      const finalDir = this.packDir(id);
      if (present.has(id) && (await this.isValidInstall(id))) continue; // backups are obsolete
      // The newest backup whose files all check out (pack.json + every file's size, §3).
      const best = list.filter((c) => c.complete).sort((a, b) => b.installedAt - a.installedAt)[0];
      if (best && (await this.restoreBackup(id, finalDir, best.dir, present.has(id)))) {
        keep.add(best.dir); // it is the pack folder now
        console.warn(`[packs] restored ${id} after an interrupted update`);
      } else {
        for (const c of list) keep.add(c.dir);
      }
    }

    // 2 + 3. Removal.
    const doomed: string[] = [];
    for (const n of names) {
      const full = path.join(this.packsDir, n);
      if (busy.has(full) || keep.has(full)) continue;
      if (n.startsWith(TMP_PREFIX) || n.startsWith(OLD_PREFIX) || n.startsWith(DEL_PREFIX) || n.startsWith(LOCK_STALE_PREFIX)) doomed.push(full);
      else if (n.startsWith(PARTIAL_PREFIX)) {
        const st = await fs.stat(full).catch(() => null);
        if (st && now - st.mtimeMs > PARTIAL_MAX_AGE_MS) doomed.push(full);
      }
    }
    await Promise.all(doomed.map((p) => fs.rm(p, { recursive: true, force: true }).catch(() => undefined)));
  }

  /** Cancels everything; temp folders are removed by the cancelled installs. */
  async cancelAll(): Promise<void> {
    const running = [...this.active.values()];
    for (const r of running) r.ctrl.abort();
    await Promise.all(running.map((r) => r.promise.catch(() => undefined)));
  }

  /**
   * App quit: stops every running install but KEEPS its staging folder, so the
   * next start resumes with the verified files (unlike `cancelAll`). Resolves
   * once every install has settled (files closed, partial files removed).
   */
  async pauseAll(): Promise<void> {
    const running = [...this.active.values()];
    for (const r of running) if (!r.ctrl.signal.aborted) r.ctrl.abort(new InstallPausedError());
    await Promise.all(running.map((r) => r.promise.catch(() => undefined)));
  }

  /** True while any install runs. */
  get busy(): boolean {
    return this.active.size > 0;
  }

  /** A real folder whose pack.json validates and whose files all have their declared size (§3). */
  private isValidInstall(id: string): Promise<boolean> {
    return isCompleteInstallDir(this.packDir(id), id);
  }

  /**
   * Moves `backup` into `finalDir` (a present but invalid `finalDir` is moved
   * aside first and put back if the restore fails). True once the restored
   * folder is confirmed to be a valid install.
   */
  private async restoreBackup(id: string, finalDir: string, backup: string, present: boolean): Promise<boolean> {
    let aside: string | null = null;
    try {
      if (present) {
        aside = path.join(this.packsDir, `${DEL_PREFIX}${id}-${randomBytes(4).toString('hex')}`);
        await fs.rename(finalDir, aside);
      }
      await fs.rename(backup, finalDir);
    } catch (err) {
      console.warn('[packs] could not restore', id, err instanceof Error ? err.message : err);
      if (aside) await fs.rename(aside, finalDir).catch(() => undefined);
      return false;
    }
    if (await this.isValidInstall(id)) {
      if (aside) await fs.rm(aside, { recursive: true, force: true }).catch(() => undefined);
      return true;
    }
    // Should not happen (the backup's pack.json was valid): put it back as a backup.
    await fs.rename(finalDir, backup).catch(() => undefined);
    if (aside) await fs.rename(aside, finalDir).catch(() => undefined);
    return false;
  }
}

/**
 * Recovery check (§3): `dir` is a real directory (not a symlink), its
 * pack.json validates with id `id`, and every manifest file is a regular file
 * of its declared size (lstat only, no hashing).
 */
export async function isCompleteInstallDir(dir: string, id: string): Promise<boolean> {
  if (!(await isRealDir(dir))) return false;
  const pack = await readInstalledMeta(dir).catch(() => null);
  if (!pack || pack.id !== id) return false;
  for (const f of pack.manifest.files) {
    let file: string;
    try {
      file = resolveInside(dir, f.path);
    } catch {
      return false;
    }
    const st = await fs.lstat(file).catch(() => null);
    if (!st || !st.isFile() || st.size !== f.size) return false;
  }
  return true;
}

async function isRealDir(p: string): Promise<boolean> {
  const st = await fs.lstat(p).catch(() => null);
  return !!st && st.isDirectory();
}

/**
 * `mkdir -p` that never follows a planted link (§4): a symlink or a file at
 * `dir` is unlinked (never its target) and a real folder created instead.
 */
async function ensureRealDir(dir: string): Promise<void> {
  const st = await fs.lstat(dir).catch(() => null);
  if (st && !st.isDirectory()) {
    console.warn('[packs] replacing a non-folder staging entry', dir);
    await fs.unlink(dir);
  }
  await fs.mkdir(dir, { recursive: true });
}

/** Writes `text` and fsyncs it before closing (errors, ENOSPC included, propagate). */
async function writeFileDurable(file: string, text: string): Promise<void> {
  const fh = await fs.open(file, 'w');
  try {
    await fh.writeFile(text, 'utf-8');
    await fh.sync();
  } finally {
    await fh.close();
  }
}

/** Error codes of platforms that cannot fsync a directory (Windows): skipped. */
const DIR_SYNC_UNSUPPORTED = new Set(['EPERM', 'EISDIR', 'EINVAL', 'ENOTSUP', 'EBADF']);

/** fsync of a directory (its entries: renames, new files). Skipped where unsupported. */
export async function fsyncDir(dir: string): Promise<void> {
  let fh: FileHandle | undefined;
  try {
    fh = await fs.open(dir, 'r');
    await fh.sync();
  } catch (err) {
    if (DIR_SYNC_UNSUPPORTED.has((err as NodeJS.ErrnoException).code ?? '')) return;
    throw err;
  } finally {
    await fh?.close().catch(() => undefined);
  }
}

/** Validated pack.json of a folder, or null when there is none (throws when invalid). */
async function readInstalledMeta(dir: string): Promise<InstalledPack | null> {
  const metaPath = path.join(dir, PACK_META_FILE);
  const st = await fs.stat(metaPath).catch(() => null);
  if (!st || !st.isFile() || st.size > MAX_META_BYTES) return null;
  return validateInstalled(JSON.parse(await fs.readFile(metaPath, 'utf-8')), dir);
}

/** True when `file` is a regular file of exactly `size` bytes with the given sha256. */
export async function isVerifiedFile(file: string, size: number, sha256: string): Promise<boolean> {
  const st = await fs.lstat(file).catch(() => null);
  if (!st || !st.isFile() || st.size !== size) return false;
  const hash = createHash('sha256');
  try {
    for await (const chunk of createReadStream(file)) hash.update(chunk as Buffer);
  } catch {
    return false;
  }
  return hash.digest('hex') === sha256.toLowerCase();
}

/** Removes every file/symlink under `root` that is not in `keep` (absolute paths); empty folders stay. */
async function pruneUnexpected(root: string, keep: ReadonlySet<string>): Promise<void> {
  let dirents: Dirent[];
  try {
    dirents = await fs.readdir(root, { withFileTypes: true });
  } catch {
    return;
  }
  for (const d of dirents) {
    const full = path.join(root, d.name);
    if (d.isDirectory()) await pruneUnexpected(full, keep);
    else if (!keep.has(full)) await fs.rm(full, { force: true }).catch(() => undefined);
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
