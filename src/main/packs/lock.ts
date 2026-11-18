/**
 * Cross-process lock on the packs folder: `<packs>/.lock/` (a directory, so
 * taking it is one atomic mkdir), shared with the Rust port. The protocol is
 * `docs/rune-port/packs-dir-protocol.md` §1; the Rust side is
 * `rune/crates/mdaw-packs/src/lock.rs`.
 *
 * - `owner.json` = `{ pid, app: 'electron', token, acquiredAt }`.
 * - Held for every mutating operation (install / resume, uninstall, startup
 *   cleanup), refcounted per lock object: concurrent operations of one
 *   installer share the hold.
 * - Heartbeat: the `.lock` directory's mtime is refreshed every 5 s.
 * - Stale when the owner's pid is dead (`process.kill(pid, 0)` → ESRCH; not on
 *   Windows) or the mtime is older than 30 s. A stale lock is stolen by
 *   renaming it to `.lock-stale-<8hex>` and checking the token it carried.
 * - Release removes the lock only while it still carries our token.
 */
import { randomBytes } from 'node:crypto';
import fs from 'node:fs/promises';
import path from 'node:path';

export const LOCK_DIR = '.lock';
export const LOCK_OWNER_FILE = 'owner.json';
export const LOCK_STALE_PREFIX = '.lock-stale-';
export const LOCK_HEARTBEAT_MS = 5_000;
export const LOCK_STALE_MS = 30_000;
export const LOCK_RETRY_MS = 50;
/** Uninstall gives up waiting for another owner after this long. */
export const UNINSTALL_LOCK_WAIT_MS = 10_000;
export const LOCK_BUSY_MESSAGE = 'The packs folder is busy (another micro-daw window is changing packs)';

export interface LockOwner {
  pid: number;
  app: string;
  token: string;
  acquiredAt: string;
}

export interface PacksDirLockOptions {
  heartbeatMs?: number;
  staleMs?: number;
  retryMs?: number;
  /** Tests: the pid written to owner.json and treated as "this process". */
  pid?: number;
  /** Tests: liveness check of a foreign pid. */
  isAlive?: (pid: number) => boolean;
}

export interface AcquireOptions {
  /** Aborting it rejects the wait with `signal.reason`. */
  signal?: AbortSignal;
  /** Give up after this long (0: one try). Undefined: wait until acquired or aborted. */
  timeoutMs?: number;
}

/** Releases one hold. */
export type LockRelease = () => Promise<void>;

/** `kill(pid, 0)`: ESRCH = dead, EPERM = alive (someone else's). Windows: always "alive" (mtime rule only). */
export function isPidAlive(pid: number): boolean {
  if (process.platform === 'win32') return true;
  try {
    process.kill(pid, 0);
    return true;
  } catch (err) {
    return (err as NodeJS.ErrnoException).code !== 'ESRCH';
  }
}

async function readOwner(dir: string): Promise<LockOwner | null> {
  try {
    const raw = JSON.parse(await fs.readFile(path.join(dir, LOCK_OWNER_FILE), 'utf-8')) as Partial<LockOwner>;
    if (!Number.isSafeInteger(raw.pid) || (raw.pid as number) <= 0 || typeof raw.token !== 'string') return null;
    return raw as LockOwner;
  } catch {
    return null;
  }
}

function sleep(ms: number, signal?: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    if (signal?.aborted) return reject(signal.reason);
    const t = setTimeout(() => {
      signal?.removeEventListener('abort', onAbort);
      resolve();
    }, ms);
    const onAbort = () => {
      clearTimeout(t);
      reject(signal?.reason);
    };
    signal?.addEventListener('abort', onAbort, { once: true });
  });
}

type TryResult = 'acquired' | 'busy' | 'retry';

export class PacksDirLock {
  readonly packsDir: string;
  readonly lockDir: string;
  private readonly heartbeatMs: number;
  private readonly staleMs: number;
  private readonly retryMs: number;
  private readonly pid: number;
  private readonly isAlive: (pid: number) => boolean;
  private holds = 0;
  private token: string | null = null;
  private timer: ReturnType<typeof setInterval> | null = null;
  private attempt: Promise<TryResult> | null = null;
  private releasing: Promise<void> | null = null;

  constructor(packsDir: string, opts: PacksDirLockOptions = {}) {
    this.packsDir = packsDir;
    this.lockDir = path.join(packsDir, LOCK_DIR);
    this.heartbeatMs = opts.heartbeatMs ?? LOCK_HEARTBEAT_MS;
    this.staleMs = opts.staleMs ?? LOCK_STALE_MS;
    this.retryMs = opts.retryMs ?? LOCK_RETRY_MS;
    this.pid = opts.pid ?? process.pid;
    this.isAlive = opts.isAlive ?? isPidAlive;
  }

  /** True while this object holds the folder. */
  get held(): boolean {
    return this.token !== null;
  }

  /**
   * Takes one hold (waiting for other owners). Resolves with its release, or
   * null when `timeoutMs` passed; rejects with `signal.reason` when aborted.
   */
  async acquire({ signal, timeoutMs }: AcquireOptions = {}): Promise<LockRelease | null> {
    const deadline = timeoutMs === undefined ? Infinity : Date.now() + timeoutMs;
    let quickRetries = 0;
    for (;;) {
      if (signal?.aborted) throw signal.reason;
      if (this.releasing) await this.releasing;
      if (this.token !== null) {
        this.holds++;
        return this.releaser();
      }
      let result: TryResult;
      if (this.attempt) {
        await this.attempt.catch(() => undefined);
        continue;
      } else {
        this.attempt = this.tryOnce().finally(() => {
          this.attempt = null;
        });
        result = await this.attempt;
      }
      if (result === 'acquired') continue; // counted by the branch above
      // A steal (or a lock that vanished) is retried at once, a few times.
      if (result === 'retry' && quickRetries++ < 5) continue;
      quickRetries = 0;
      if (Date.now() >= deadline) return null;
      await sleep(Math.min(this.retryMs, Math.max(0, deadline - Date.now())), signal);
    }
  }

  private releaser(): LockRelease {
    let done = false;
    return async () => {
      if (done) return;
      done = true;
      if (--this.holds > 0) return;
      const token = this.token;
      this.token = null;
      if (this.timer) clearInterval(this.timer);
      this.timer = null;
      this.releasing = (async () => {
        const owner = await readOwner(this.lockDir);
        if (token && owner?.token === token) {
          await fs.rm(path.join(this.lockDir, LOCK_OWNER_FILE), { force: true }).catch(() => undefined);
          await fs.rmdir(this.lockDir).catch(() => undefined);
        }
      })().finally(() => {
        this.releasing = null;
      });
      await this.releasing;
    };
  }

  private async tryOnce(): Promise<TryResult> {
    await fs.mkdir(this.packsDir, { recursive: true });
    try {
      await fs.mkdir(this.lockDir);
    } catch (err) {
      if ((err as NodeJS.ErrnoException).code !== 'EEXIST') throw err;
      return this.stealIfStale();
    }
    const owner: LockOwner = {
      pid: this.pid,
      app: 'electron',
      token: randomBytes(16).toString('hex'),
      acquiredAt: new Date().toISOString(),
    };
    try {
      await fs.writeFile(path.join(this.lockDir, LOCK_OWNER_FILE), JSON.stringify(owner), 'utf-8');
    } catch (err) {
      await fs.rm(this.lockDir, { recursive: true, force: true }).catch(() => undefined);
      throw err;
    }
    this.token = owner.token;
    this.timer = setInterval(() => void this.heartbeat(), this.heartbeatMs);
    this.timer.unref?.();
    return 'acquired';
  }

  private async heartbeat(): Promise<void> {
    const token = this.token;
    if (!token) return;
    const owner = await readOwner(this.lockDir);
    if (owner?.token !== token) return; // stolen: never keep someone else's lock alive
    const now = new Date();
    await fs.utimes(this.lockDir, now, now).catch(() => undefined);
  }

  private async stealIfStale(): Promise<TryResult> {
    const st = await fs.stat(this.lockDir).catch(() => null);
    if (!st) return 'retry'; // released meanwhile
    const owner = await readOwner(this.lockDir);
    const deadPid = !!owner && owner.pid !== this.pid && !this.isAlive(owner.pid);
    const expired = Date.now() - st.mtimeMs > this.staleMs;
    if (!deadPid && !expired) return 'busy';
    const moved = path.join(this.packsDir, `${LOCK_STALE_PREFIX}${randomBytes(4).toString('hex')}`);
    try {
      await fs.rename(this.lockDir, moved);
    } catch {
      return 'retry'; // someone else stole or released it
    }
    const movedOwner = await readOwner(moved);
    if ((movedOwner?.token ?? null) === (owner?.token ?? null)) {
      console.warn(`[packs] removed a stale packs-folder lock (pid ${owner?.pid ?? '?'})`);
      await fs.rm(moved, { recursive: true, force: true }).catch(() => undefined);
    } else {
      // A fresh lock was taken between the check and the rename: put it back
      // (fails, harmlessly, if yet another lock exists by now).
      await fs.rename(moved, this.lockDir).catch(() => undefined);
    }
    return 'retry';
  }
}
