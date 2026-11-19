# Packs folder protocol (shared by the Electron app and the Rust port)

Both apps read and write the same `<packs>` layout (`docs/rune-port/phase-6-packs.md`). This
file is the contract both implementations follow for **locking**, **durability** and
**symlinks** (phase-6 review findings 2, 3, 4, 13; user decision 2026-11-18: fixed in both apps).
Implementations: `src/main/packs/lock.ts` + `installer.ts` (Electron),
`rune/crates/mdaw-packs/src/lock.rs` + `installer.rs` (Rust).

## 1. Cross-process lock: `<packs>/.lock/`

A **directory** lock (mkdir is atomic on every platform and needs no native module in Node).

- **Layout.** `<packs>/.lock/owner.json`:
  `{"pid": <int>, "app": "electron" | "rune", "token": "<32 lower-case hex>", "acquiredAt": "<ISO 8601>"}`.
- **Held** for the whole of every mutating operation: an install (including a resume: from
  before the staging folder is touched until the swap is done or the failure cleanup ran),
  an uninstall, and the startup cleanup (`cleanupStale`). Read-only calls (`listInstalled`,
  `pausedIds`, `isInstalled`) do not take it.
- **Per process, refcounted.** One lock object per packs folder per installer. Concurrent
  operations of the same installer share the hold; the folder is released when the last one
  ends. A second installer object on the same folder (even in the same process) is another
  owner and waits.
- **Acquire.** `mkdir -p <packs>`; `mkdir <packs>/.lock` (not recursive).
  - Success: write `owner.json`, start the heartbeat.
  - `EEXIST`: if the holder is **stale** (below), steal; otherwise wait 50 ms and retry.
- **Heartbeat.** While held, set the `.lock` directory's mtime to now every **5 s**.
- **Stale** when either:
  - `owner.json` parses, its `pid` is not this process's pid, and that pid is **not alive**
    (`kill(pid, 0)` fails with `ESRCH`; Node: `process.kill(pid, 0)` throws `ESRCH`; `EPERM`
    means alive). Windows: no pid check, the mtime rule only.
  - or the `.lock` directory's mtime is older than **30 s** (a holder that stopped
    heartbeating; also covers a missing or unparseable `owner.json`, and pid reuse).
- **Steal.** Rename `.lock` → `.lock-stale-<8 hex>`. If the rename fails (someone else stole or
  released it), retry the acquire. Otherwise read the moved `owner.json`: if its token is the
  one judged stale (or both are missing) remove the moved folder and retry the acquire at once;
  if not (a fresh lock was taken between the check and the rename), rename it back to `.lock`
  (a directory rename onto an existing non-empty `.lock` fails, so it never clobbers a newer
  lock) and retry. Leftover `.lock-stale-*` folders are removed by the next steal or cleanup.
- **Release.** Stop the heartbeat; if `owner.json` still carries our token, remove it and
  `rmdir .lock`. Otherwise (stolen as stale) do nothing.
- **Waiting policy.**
  - Install: waits while honouring its cancel / pause signal (state stays `downloading` 0 B).
  - Uninstall: waits at most **10 s**, then fails with
    `The packs folder is busy (another micro-daw window is changing packs)`.
  - Startup cleanup: one try; when another owner holds the lock the cleanup is skipped (that
    owner's operations are in progress and its own start already cleaned up).
- **Residual risk (documented, accepted).** The steal has a tiny window (two stealers between a
  check and a rename) in which a fresh lock can be broken for a moment; the rename-back limits
  it. Network file systems are out of scope (`<packs>` lives under the local user-data folder).

## 2. Per-pack operations inside one process (finding 13)

An uninstall and an install of the **same pack** never overlap: an install started while an
uninstall of that pack runs waits for the uninstall to finish (honouring its cancel signal),
then starts from scratch; an uninstall cancels a running install and waits for it, as before.

## 3. Durability (finding 3)

Order of an install, after every file is downloaded:

1. Each downloaded file: written, **fsync**ed (`File::sync_all` / `FileHandle.sync()`), closed.
   An fsync or write error (ENOSPC, EIO) fails the file and is not retried.
2. `pack.json`: written with an explicit open / write / **fsync** / close (errors propagate).
3. **fsync every directory** of the staging tree (deepest first, the staging root last).
4. Rename `<id>` → `.old-<id>-<8hex>` (if present), rename staging → `<id>`.
5. **fsync `<packs>`** (makes both renames durable).
6. Only then remove `.old-<id>-*`. If the fsync of `<packs>` fails (the renames are done, the
   new version is in place), both apps log it, **keep** `.old-<id>-*` and still report the pack
   installed; the next start's cleanup removes the backup once `<id>` passes the completeness
   check below.

Directory fsync: open the directory read-only and fsync it. Where the platform refuses
(Windows: `EPERM` / `EISDIR` / `EINVAL`), it is skipped. On macOS both apps get `F_FULLFSYNC`
(Rust's `sync_all`, libuv's `fsync`).

**Recovery** (`cleanupStale`) treats `<id>` as a valid install only when its `pack.json`
validates **and every manifest file is a regular file of the declared size** (`lstat`, no
hashing). Only then are its `.old-<id>-*` backups obsolete; otherwise the newest backup that
passes the same check is restored (the current `<id>` is moved aside to `.del-*`).

**What is guaranteed.**
- Process kill / crash at any point: `<id>` is either the old version or the new complete one
  after the next start's cleanup; a pack reported `installed` stays installed.
- Power loss: a pack reported `installed` survives (its files, `pack.json` and both renames were
  fsynced before the report). Power loss during an install may leave the staging folder with
  some files missing or short; they fail the resume's size + sha256 check and are downloaded
  again. Not verified on real hardware (no power-cut test rig); the order above is checked by
  tests that record the fsync / rename sequence.

## 4. Symlinks (finding 2)

No operation follows a symlink it did not create.

- The staging folder `.partial-<id>`: `lstat` before use. A symlink (or a regular file) there is
  **unlinked** (the link itself, never its target) and a real folder created in its place.
  Inside it, every non-directory entry that is not an expected file is unlinked (symlinks
  included) before any parent folder is created, and destination files are created with
  exclusive create (`O_EXCL`), which fails on a symlink.
- `<id>`, `.old-*`, `.del-*`, `.tmp-*`: only real directories count as installs or backups;
  removal unlinks a symlink without following it; a symlink `<id>` is not a valid install.
- Not race-proof against a concurrent local attacker who can write to the packs folder (that
  attacker already owns the user's data); the checks stop planted links from earlier sessions.
