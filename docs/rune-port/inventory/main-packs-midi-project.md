# Parity checklist: Micro DAW Electron app → Rust port (main process, packs, MIDI, project format)

Every path below is relative to ``. Citations are `file:line`.

**Main files read:**
- `src/main.ts`, `src/preload.ts`, `src/types/electron.d.ts`, `src/main/security.ts`
- `src/main/packs/{ipc,installer,download,catalog,preview,quit,pausedState,sender,validate}.ts`
- `src/shared/{packValidate,packSource,fsEntries,paths}.ts`
- `src/services/{PackService,MidiService,ControllerMapping,KnobRouter,KnobPickup,NoteEventBus,ProjectManager,SampleManager,KitStore,SampleProcessor}.ts`
- `src/hooks/{usePacks,useMidiNotes,useControllerMapping,useMidi,useProject,useArrangement}.ts`
- `src/components/packs/*`, `src/components/controller/ControllerSetupDialog.tsx`
- Also needed for migrations: `src/audio/sampler/factoryKit.ts`, `src/utils/arrangementHelpers.ts`, `src/services/ArrangementEngine.ts`
- `packs/catalog.json`, `packs/withdrawn.json`, `docs/packs.md`, `scripts/pack-check.mjs`, `index.html`

---

## 1. Main process

### 1.1 Startup and app lifecycle
- **Squirrel:** if `electron-squirrel-startup` reports an install/uninstall event, the app quits immediately (Windows shortcut handling) (`main.ts:14-16`).
- **Smoke-test mode:** `MICRO_DAW_SMOKE=1` keeps the window hidden and DevTools closed (`main.ts:22,108,111`).
- **Trusted renderer URL:** the Vite dev-server URL, or `file://…/../renderer/<name>/index.html` (`main.ts:29-31`).
- **On `ready`** (`main.ts:279-295`):
  - Permission check and request handlers allow only `midi` and `midiSysex`; everything else is denied (`:281-287`).
  - IPC handlers are registered (`:289`).
  - `registerPackIpc` runs and returns `packsDir` (`:292`).
  - The window is created with `packsDir` (`:294`).
- **`window-all-closed`:** quit unless macOS (`:297-301`).
- **`activate`:** recreate the window if none exist and `packsDir` is known (`:303-307`).
- **Menus:** there are none. There is no `Menu`/`setApplicationMenu` anywhere, so the app uses Electron's default menu. Decide what the Rust app's equivalent should be.
- **File associations:** none. There is no open-file handling, no `.mdaw` registration in main, and no second-instance handling.

### 1.2 Window options (`main.ts:77-97`)
- Size: width 1360, height 800, minWidth 1360, minHeight 768.
- Shown only after `ready-to-show` (`show:false`; `:107-109`).
- Initial title: `'Untitled - Micro DAW'`.
- macOS chrome: `titleBarStyle:'hiddenInset'`, `trafficLightPosition {x:12,y:17}`.
- `backgroundColor '#100e0c'` (matches the splash background, so no flash).
- `webPreferences`: `contextIsolation`, `sandbox`, `nodeIntegration:false`, `webviewTag:false`, preload `preload.cjs`.
- `additionalArguments: ['--mdaw-packs-dir=<packsDir>']` (`:95`; constant at `shared/packSource.ts:14`). The preload reads it back synchronously (`packSource.ts:17-21`, `preload.ts:6`).
- DevTools open when the app is not packaged and not in smoke mode (`:111-113`).
- Entering/leaving full screen sends `fullscreen-changed` (true/false) to the renderer (`:115-120`).

### 1.3 Close guard and before-quit
- **Window close** (`main.ts:123-127`): unless `forceCloseAllowed`, the close is prevented and `project:before-close` is sent to the renderer.
- **Renderer response** (`ProjectManager.handleBeforeClose`, `ProjectManager.ts:160-179`, wired in `useProject.ts:39-45`):
  - Not dirty: `forceClose`.
  - Dirty: show the unsaved dialog.
    - `save`: save; `forceClose` only if the save succeeded.
    - `dont-save`: `forceClose`.
    - `cancel`: nothing happens.
- **`project:force-close`** (`ipc.on`): sets `forceCloseAllowed = true`, then `mainWindow.close()` (`main.ts:189-192`).
- **`will-quit` with installs running** (`main/packs/quit.ts:42-53`):
  - Applies only if installs are busy and the quit hasn't already been paused.
  - Calls `preventDefault`, then `installer.pauseAll()`.
  - Waits at most `QUIT_PAUSE_TIMEOUT_MS = 3000` (`:12`), then calls `app.quit()`.
  - On timeout it logs `installs did not settle within 3000 ms; quitting anyway`.
  - Because this hooks `will-quit`, it runs after the unsaved prompt, so a cancelled quit never pauses installs.

### 1.4 Security
- **CSP** (`index.html:14-15`, a meta tag, not a header):
  `default-src 'self'; script-src 'self' blob:; worker-src 'self' blob:; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self' data:; media-src 'self' data: blob:; connect-src 'self' data: blob:; object-src 'none'; frame-src 'none'; base-uri 'none'; form-action 'none'`
- **Guards on every webContents** (`security.ts:47-70`, installed via `web-contents-created` at `main.ts:36-38`):
  - `setWindowOpenHandler` always returns deny. If the URL is plain `https:` with no username or password, it opens in the system browser (`externalUrlFor`, `:28-37`). Anything else is logged as `[security] blocked window.open(<url truncated to 200>)`.
  - `will-navigate`, `will-frame-navigate` and `will-redirect` are blocked unless the target is the trusted renderer URL (`:58-65`). This also covers a file dropped outside a drop zone.
  - `will-attach-webview` is always prevented (`:66-69`).
- **Trusted-sender rule** (`main/packs/sender.ts:12-39`):
  - The sender frame must exist, must be the top frame (`frame.parent` null), and its URL must match the renderer URL.
  - Matching for `http(s)`: same origin.
  - Matching for `file:`: same host and same pathname (query and hash ignored).
  - Any other protocol is rejected.
  - Rejection error text: `Untrusted IPC sender`.
- **Wrappers** (`security.ts:87-111`):
  - `handle` throws for untrusted senders.
  - `on` silently drops the message and logs `[security] ignored <channel> from an untrusted sender`.
- **Window drop guard (renderer)** (`components/windowDropGuard.ts:11-27`):
  - Bubble-phase listeners. `dragover` that no target accepted gets `preventDefault` and `dropEffect='none'`.
  - Every `drop` gets `preventDefault`.
- **Pad drop paths** come from `webUtils.getPathForFile`, which returns `''` on error (`preload.ts:42-48`).

### 1.5 Auto-updater (`main.ts:41-74, 255-276`)
- `autoDownload=false`, `autoInstallOnAppQuit=true`.
- Feed (packaged only): GitHub `owner:'yemirhan'`, `repo:'micro-daw'`, `releaseType:'release'`.
- Events are sent on `updater:status` (types in `types/settings.ts:7-14`):

| Event | Payload |
|---|---|
| checking | `{state:'checking'}` |
| update available | `{state:'available',version}` |
| no update | `{state:'not-available'}` |
| download progress | `{state:'downloading',percent}` |
| downloaded | `{state:'downloaded',version}` |
| error | `{state:'error',message ?? 'Unknown error'}` |

- The renderer auto-checks on mount when `settings.general.autoCheckUpdates` is true (`hooks/useAutoUpdater.ts:25-27`).

### 1.6 IPC channels
All go through the trusted-sender check. Preload names are in `preload.ts`; types are in `electron.d.ts`.

| Channel | Kind | Args | Returns | Behaviour / guards | Cite |
|---|---|---|---|---|---|
| `toggle-fullscreen` | invoke | – | bool | flips full screen, returns the new state | main.ts:135-138 |
| `is-fullscreen` | invoke | – | bool | | :140-142 |
| `fullscreen-changed` | main→r | bool | | | :115-120 |
| `project:save` | invoke | path, data(str) | void | writes UTF-8. **No path validation** | :145-147 |
| `project:load` | invoke | path | string | reads UTF-8. No validation | :149-151 |
| `project:show-save-dialog` | invoke | defaultName? | path\|null | title "Save Project", default `defaultName \|\| 'Untitled.mdaw'`, filter "Micro DAW Project" [`mdaw`] | :153-160 |
| `project:show-open-dialog` | invoke | – | path\|null | title "Open Project", filter `mdaw`, `openFile` | :162-169 |
| `project:set-title` | invoke | title | void | `setTitle` | :171-173 |
| `project:show-unsaved-dialog` | invoke | projectName | `'save'\|'dont-save'\|'cancel'` | warning box, title "Unsaved Changes", message `"<name>" has unsaved changes.`, detail "Do you want to save before continuing?", buttons Save / Don't Save / Cancel, defaultId 0, cancelId 2 | :175-187 |
| `project:force-close` | send | – | – | see §1.3 | :189-192 |
| `project:before-close` | main→r | – | | | :126 |
| `sample:show-import-dialog` | invoke | – | string[]\|null | title "Import Audio File", filter "Audio Files" [wav mp3 ogg flac aiff aif m4a], `openFile`+`multiSelections` | :195-204 |
| `sample:read-file` | invoke | path | ArrayBuffer | arbitrary read (exact-length slice). No validation | :206-209 |
| `fs:list-dir` | invoke | dirPath | FsDirEntry[] | rejects if not a string, empty, >4096 chars or containing NUL (`Invalid directory path`), or not absolute (`Directory path must be absolute`). See rules below | :214-242 |
| `fs:pick-folder` | invoke | – | path\|null | title "Add Sample Folder", `openDirectory` | :244-250 |
| `fs:home-dir` | invoke | – | string | `app.getPath('home')` | :252 |
| `updater:check` | invoke | – | void | null result sends `not-available`; an exception sends error (`'Check failed'` fallback) | :255-265 |
| `updater:download` | invoke | – | void | fire-and-forget; error sends `'Download failed'` | :266-270 |
| `updater:install` | invoke | – | – | `quitAndInstall(false, true)` | :271-273 |
| `updater:get-version` | invoke | – | string | | :274-276 |
| `updater:status` | main→r | UpdateStatus | | | :53-55 |
| `packs:fetch-catalog` | invoke | – | PackCatalog | shared in-flight promise. Never rejects for network errors (falls back to bundled) | packs/ipc.ts:64-82 |
| `packs:list-installed` | invoke | – | InstalledPack[] | awaits startup cleanup | :84-87 |
| `packs:install` | invoke | id | InstalledPack | `assertId` (`Invalid pack id`); awaits cleanup; catalog (loaded if needed); unknown id gives `Unknown pack: <id>` | :89-96 |
| `packs:cancel` | invoke | id | bool | false if nothing was running | :98 |
| `packs:uninstall` | invoke | id | void | `assertId`; awaits cleanup | :100-104 |
| `packs:fetch-preview` | invoke | url | ArrayBuffer | must be a string of at most 4096 chars and **exactly** the URL of a file in the loaded catalog, else `Preview URL not allowed` | :106-111 |
| `packs:dir` | invoke | – | string | `<userData>/packs` | :113 |
| `packs:states` | invoke | – | Record<id,state> | paused ids reported as `{state:'error',message:'Download paused'}`, overlaid by running installs (catalog packs only) | :115-124 |
| `packs:progress` | main→r | `{id,state}` | | sent only to windows whose URL is the trusted renderer | :53-58 |

**`fs:list-dir` listing rules:**
- Lists one level only; no recursion.
- Hidden: names starting with `.`.
- Symlinks are resolved with `stat`; dangling links are skipped.
- Includes folders and files with extensions `wav aif aiff mp3 flac ogg m4a` (`shared/fsEntries.ts:9`). Note the dialog filter also lists `ogg` and `m4a`, but the browser list does not include `wave`.
- Entry shape `{name, path, isDir, ext}`, where `ext` is lower-case with no dot and `''` for directories.
- Sort: folders first, then a case-insensitive natural collator (`Intl.Collator(undefined,{sensitivity:'base',numeric:true})`), with a raw-compare tie-break (`fsEntries.ts:27-44`).

**userData paths:**
- `packsDir = <userData>/packs` (`ipc.ts:49`).
- Every other piece of persistent state lives in renderer localStorage (see §4.6). There are no other userData files.

---

## 2. Pack store

### 2.1 Limits and constants
| Item | Value | Cite |
|---|---|---|
| File max | 50 MiB | packValidate.ts:19 |
| Pack max | 500 MiB | :21 |
| Preview max | 10 MiB | :23 |
| Catalog max | 5 MiB | :25 |
| Files per pack | 5000 | :27 |
| Redirect hops | 5 | :29 |
| Meta file | `pack.json` | :32 |
| Meta file read cap | 10 MiB | installer.ts:50 |
| Catalog timeout | 8000 ms total and 8000 ms idle | catalog.ts:12,52-53 |
| Preview timeout | 20 000 ms total, default 30 s idle | preview.ts:51 |
| Download idle timeout | 30 000 ms | download.ts:59 |
| User-Agent | `micro-daw-packs` | download.ts:60 |
| Concurrency | 4 workers per pack | installer.ts:240 |
| Progress throttle | 100 ms | installer.ts:161 |
| Retries | 3 attempts, delay = `500*3^(n-1)` (500 ms, then 1500 ms) | download.ts:261-271 |
| Staging max age | 7 days (by mtime) | installer.ts:49 |
| Preview cache | LRU, 24 entries / 48 MiB, at least 1 entry always kept | preview.ts:27-28,71 |

- Remote catalog URL: `https://raw.githubusercontent.com/yemirhan/micro-daw/main/packs/catalog.json` (`types/packs.ts:93`).
- Allowed hosts: `['raw.githubusercontent.com']` only (`types/packs.ts:90`). `pack-check.mjs:66-70` enforces that this list equals the one in `pack-lib.mjs`.

### 2.2 Validation rules (`shared/packValidate.ts`)

**Pack id** (`:60-62`): kebab-case `^[a-z0-9]+(?:-[a-z0-9]+)*$`, length 1–64.

**`isAllowedUrl`** (`:65-77`):
- A string of at most 4096 chars that parses as a URL.
- `https:` only, no username or password, port empty or 443.
- Hostname (lower-cased) is in the allowed-host list.

**`isPinnedPackUrl`** (`:85-94`):
- Must pass `isAllowedUrl`.
- `u.href === url` exactly (canonical form).
- Host is exactly `raw.githubusercontent.com`; no query and no hash.
- Path is `/owner/repo/<40-hex>/…` with at least 5 segments:
  - owner matches `^[A-Za-z0-9](?:[A-Za-z0-9-]{0,38})$`
  - repo matches `^[A-Za-z0-9_.-]{1,100}$`
  - commit matches `^[0-9a-f]{40}$`
- Remaining segments are non-empty, not `.` or `..`, and contain no `%2f`, `%5c` or `%2e%2e` (case-insensitive).

**`isGithubSourceUrl`** (`:97-106`): at most 2000 chars, `https`, host `github.com`, no credentials, port empty or 443.

**Colour** (`:108-119`): at most 64 chars, either `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`, or `oklch(L[%] C[%] H[deg] [/ A[%]])` with plain numbers only.

**`safePackPathSegments`** (`:127-146`):
- A string of 1–512 chars.
- No control characters (`\u0000-\u001f`, `\u007f`) and none of `\ : * ? " < > |`.
- No leading `/`.
- Each segment is non-empty, not `.` or `..`, at most 128 chars, does not start with `.`, does not end with `.` or a space, and is not a Windows reserved name (`con|prn|aux|nul|com0-9|lpt0-9` with an optional extension, case-insensitive).
- At most 8 segments.
- A root-level `pack.json` (case-insensitive) is rejected.
- Extension (lower-cased) must be one of `wav wave aif aiff flac mp3 ogg m4a txt md`.

**File entry** (`:173-192`):
- `path`: required, at most 2000 chars, passes the path rules above.
- `url`: required, at most 4096 chars, allowed and pinned. Error texts: `File URL not allowed:` and `File URL not pinned to a commit:`.
- `size`: integer greater than 0 and at most 50 MiB.
- `sha256`: matches `^[0-9a-f]{64}$` (lower-case only).
- Optional `label`: non-blank, at most 80 chars, no control characters.

**Kit** (`:194-225`):
- `id`: at most 64 chars. `name`: at most 200 chars.
- `pads` must be an array. Each pad:
  - `pad` is an integer in 0..15 with no duplicates.
  - `file` must be one of the manifest's file paths (exact case).
  - Optional `name` (at most 200 chars).
  - `settings`, if present, must be an object. Only scalar values (number, string, bool, null) are kept, and the keys `sample` and `__proto__` are skipped. Values are clamped later by `normalizePad`.

**Manifest** (`:233-297`). Checks run in this order:
1. Valid id.
2. Licence is one of `CC0-1.0`, `public-domain`, `CC-BY-4.0`.
3. `source` is a GitHub URL.
4. `tags` is an array of strings, each at most 64 chars.
5. `files` is non-empty and has at most 5000 entries.
6. Each file is valid.
7. No duplicate paths, compared case-insensitively.
8. Total size is at most 500 MiB.
9. No file/folder name clash (case-insensitive).
10. `kits` is an array (may be empty) with unique kit ids.
11. `version` is an integer ≥ 0.
12. Field lengths: `name` ≤ 200, `tagline` ≤ 200, `description` ≤ 5000, `author` ≤ 500.
13. Colour is safe.
14. `attribution` is optional (≤ 2000) but required for `CC-BY-4.0`.

Required strings must be non-blank. Every error is prefixed `Pack <id>: `.

**Catalog envelope** (`:310-329`):
- Must be an object with `version === 1` and `updatedAt` a string of at most 64 chars.
- `packs` must be an array.
- An invalid pack or a duplicate id is dropped and its message collected in `errors`. An invalid envelope throws.

**Installed `pack.json`** (`:332-339`):
- `manifest` must be a valid manifest, and `raw.id` must equal `manifest.id`.
- `version` falls back to the manifest version.
- `installedAt` falls back to the epoch.
- `dir` is always replaced by the folder the file was read from.

### 2.3 Catalog fetch, fallback and merge
- **Bundled catalog:** compiled into the main build and validated. If invalid, an empty `{version:1, updatedAt:epoch, packs:[]}` is used (`catalog.ts:20-29`).
- **`fetchCatalog`** (`catalog.ts:45-62`): download (5 MiB cap, timeouts above), parse, validate, then merge. Any error falls back to `{bundled, source:'bundled'}`.
- **`mergeCatalogs`** (`packValidate.ts:373-393`):
  - The remote copy loses if its `updatedAt` doesn't parse, or isn't strictly newer than a parseable bundled `updatedAt`.
  - The remote copy loses if it has no packs while the bundled one has some.
  - Per pack, the bundled entry wins if its `version` is greater.
  - A bundled pack missing from the remote is kept only if the remote errors mention it (`Pack <id>:` or `Duplicate pack id: <id>`). Otherwise it is withdrawn.
  - Remote-only packs are added.
  - The merged catalog uses the remote `updatedAt`.
- **Main-side caching:** the catalog is cached after the first load. In-flight fetches are de-duplicated and the shared promise is cleared in `finally` (`ipc.ts:61-74`).
- **Renderer** (`PackService.loadCatalog`, `PackService.ts:317-339`):
  - Fetches once; `force` refetches.
  - On success it remembers the pack names and sets `catalogStatus:'ready'`.
  - On error it sets `'error'` with `ipcErrorMessage`, which strips the `Error invoking remote method '<x>': <Name>Error: ` prefix (`:83-86`).

### 2.4 Download rules (`main/packs/download.ts`)
- **Redirects** are followed manually (`redirect:'manual'`, `credentials:'omit'`, headers User-Agent and `Accept: */*`). Every hop must pass `isAllowedUrl`, otherwise `Host not allowed: <host>`.
- **3xx handling:** a 3xx with no `location` gives `Redirect without location (<status>)`. The location is resolved relative to the current URL. An opaque redirect gives `Unverifiable redirect`. More than 5 hops gives `Too many redirects` (`:67-93`).
- **HTTP errors:** non-2xx gives `HTTP <status> for <host>`. It is retryable when the status is ≥ 500, 429 or 408 (`:86-89`).
- **Size cap:** a declared `Content-Length` over the cap gives `File too large (<n> bytes)`. Exceeding the cap while streaming gives `File too large (over <max> bytes)`. A missing body gives `Empty response` (`:134-152`).
- **Idle timer:** re-armed before the request and before each read. On expiry it aborts the request and cancels the reader. A timeout throws `DownloadTimeoutError`: `Download timed out: the server stopped responding` (retryable) (`:31-36,124-130,158,172`).
- **Outer abort:** if the outer signal's reason is a `TimeoutError` (`AbortSignal.timeout`), it becomes a timeout error. Otherwise it becomes `DownloadAbortedError` (`'Download cancelled'`, name `AbortError`) (`:38-43,167-171`).
- **Post-stream checks:**
  - A byte count mismatch gives `Size mismatch (<n> bytes, expected <m>)` (retryable).
  - A sha256 mismatch gives `Checksum mismatch` (retryable). The expected hash is lower-cased before comparison.
- **Other errors:** a non-`DownloadError` (a network `TypeError`) becomes a retryable `DownloadError(message)` (`:175`).
- **File output:** `downloadToFile` opens the destination with `wx` (exclusive) and removes the partial file on any failure (`:186-199`).
- **Retry loop:** retries only retryable errors and only while not aborted. `onRetry` fires before the sleep. The sleep can be aborted (`:234-273`).

### 2.5 Installer state machine (`main/packs/installer.ts`)

**States** (`types/packs.ts:79-83`):
- `idle`
- `downloading{receivedBytes,totalBytes,fileIndex,fileCount}`
- `installed`
- `error{message}`

**Folders under packsDir:**
- `<id>`: the installed pack.
- `.partial-<id>`: staging.
- `.old-<id>-<8hex>`: backup made during an update swap.
- `.del-<id>-<8hex>`: uninstall, or a restore setting a folder aside.
- `.tmp-*`: legacy, removed at startup (`:41-51`).

**`install(manifest)`** (`:127-143`):
- Validates first; an invalid manifest gives a rejected promise.
- Concurrent calls for the same id share one promise.
- The active entry is removed in `finally`.

**`run`** (`:153-288`):
1. Emit `downloading` (forced): received 0, `totalBytes` = sum of declared sizes, `fileIndex` = done count, `fileCount` = number of files.
2. `mkdir` packsDir and the staging dir. Record the staging dir in `entry.tmpDir`.
3. Resolve every destination with `resolveInside`. This throws `Unsafe file path: <rel>` if a path escapes the staging dir (`:82-88`).
4. Delete any staged `pack.json`.
5. `pruneUnexpected`: remove every file or symlink not in the expected set. Folders stay (`:515-527`).
6. `mkdir` every parent folder.
7. **Resume:** for each file, check abort, then `isVerifiedFile`. The file must pass `lstat` as a regular file of exactly the declared size with a matching sha256 (`:502-512`). A verified file counts as received and done; anything else is removed and queued. Emit progress (forced).
8. **Workers:** `max(1, min(4, todo))` workers take the next index while not aborted. Each calls `downloadToFileWithRetry` with:
   - `maxBytes = min(50 MiB, file.size)`, `expectedBytes`, `sha256`, the shared signal, and the idle timeout.
   - `onProgress`: if the running total exceeds 500 MiB, abort with `Error('Pack too large')`; otherwise emit throttled progress.
   - `onRetry`: reset that file's received count to 0 and emit progress (forced).
   - After each file: increment done and emit progress (forced).
9. **First failure:** the first worker to fail aborts the controller with its error. The run then waits for **all** workers with `allSettled`, so partial-file cleanup finishes before the install settles.
10. **Error selection:** if any worker failed, throw the abort reason if it is a non-abort `Error`, otherwise `Error('aborted')`. Also re-check abort after the workers.
11. Write staging `pack.json` = `{id, version, dir:<final>, installedAt:ISO now, manifest}` (pretty-printed, 2 spaces). Check abort.
12. **`swapIntoPlace`** (`:315-328`):
    - If `<id>` exists, rename it to `.old-<id>-<hex>`.
    - Rename staging to `<id>`. If that fails, rename the backup back and rethrow.
    - On success, remove the backup (errors ignored).
13. Emit `installed`; return the `InstalledPack`.

**Catch path** (`:272-287`):
- `paused`: aborted with reason `InstallPausedError` (message `'Install paused'`).
- `cancelled`: aborted, not paused, and the reason is undefined or an AbortError.
- **Cancel:** remove the whole staging folder.
- **Failure or pause:** keep staging but delete its `pack.json`.
- **Cancel or pause:** emit `installed` if `<id>/pack.json` exists, else `idle`.
  - Pause then throws `InstallPausedError`.
  - Cancel throws `Error('Install cancelled', {cause})`.
- **Otherwise:** emit `error{message}` and rethrow.

**Other operations:**
- **`cancel(id)`:** abort with no reason; returns false if nothing was running (`:146-151`).
- **`pauseAll`:** abort every running install with `InstallPausedError` and await all of them. **`cancelAll`:** abort with no reason. **`busy`:** true while any install is active (`:437-457`).
- **`pausedIds`** (`:295-306`): every `.partial-<validId>` that has no active install and contains at least one file (recursive). Sorted.
- **`uninstall`** (`:335-352`):
  1. If running: abort and await.
  2. If `<id>` exists: rename it to `.del-<id>-<hex>`, then remove it recursively.
  3. Remove staging (errors ignored).
  4. Emit `idle`.
- **`listInstalled`** (`:355-375`):
  - Only directories whose name is a valid id and whose valid `pack.json` has a matching `id`.
  - Invalid packs are skipped with a warning.
  - Sorted by `manifest.name.localeCompare`.
  - A missing packsDir returns `[]`.
- **`isInstalled`:** `<id>/pack.json` exists (`lstat`) (`:330-332`).
- **`cleanupStale`** (startup; `:384-434`; `ipc.ts:77-79` awaits it before list, install, uninstall and states):
  - **Step 1, recovery:** every `.old-<id>-<8hex>` (regex at `:51`) with no active install and a valid `pack.json` whose id matches is a candidate.
    - For each id: if `<id>` is present **and** valid, skip it (its backups are deleted in step 2).
    - Otherwise sort candidates by `installedAt` descending and `restoreBackup` the newest (`:469-490`):
      - If `<id>` is present but invalid, move it aside to `.del-*` first.
      - Rename the backup to `<id>`.
      - If that rename fails: put the aside folder back and keep every backup.
      - If the restored folder is not valid: rename it back to the backup, restore the aside folder, and keep every backup.
      - If it is valid: delete the aside folder; the restored folder is kept.
  - **Steps 2 and 3, removal:**
    - Remove every `.tmp-*`, `.old-*` and `.del-*` not in the keep set and not busy.
    - Remove `.partial-*` folders whose `mtime` is older than 7 days.
    - All removals ignore errors.

### 2.6 Progress events and renderer state (`PackService.ts`)
- **Fan-out:** every state change is sent on `packs:progress` to trusted windows only (`ipc.ts:53-58`). Downloading progress is throttled to 100 ms; forced emits happen at start, after resume, per file, and on retry.
- **`init`** (`:283-299`, idempotent):
  - Subscribes to progress and fetches `packsDir` and `packsActiveStates` (errors give `{}`).
  - Merges them as `{...active, ...current}`.
  - Calls `refreshInstalled`.
  - If the API is missing, `available` stays false; the UI then shows `'Downloading packs needs the desktop app.'` (`usePacksController.ts:209`).
- **`onProgress`** (`:301-314`):
  - `downloading` or `error`: store the state.
  - `installed` or `idle`: clear the state and refresh the installed list.
- **`refreshInstalled`** → **`invalidateChanged`** (`:361-370`):
  - The stamp is `${version}@${installedAt}`.
  - A pack whose stamp changed or that disappeared gets `invalidatePackBuffers` (`audio/sampler/bufferCache.ts:95-107`):
    - Bumps the pack's generation.
    - Drops pad, pending and preview cache keys for that pack.
    - Calls `sampleManager.reloadByPathPrefix(<packsDir>/<id>/)`, an in-place reload that keeps `sampleId`s.
  - The first refresh sets the baseline only.
- **`install(id)`** (`:477-495`):
  - Optimistically sets `downloading{0,null,0,0}`.
  - On success: clear the state and refresh.
  - On an error whose message matches `/cancelled/i`: clear the state if it is still `downloading`.
  - On any other error: set `error{message}` unless main already set an error.
  - Always resolves; returns null on failure.
- **`cancel(id)`:** also cancels a pending Get & load whose key starts with `<id>/` (`:497-500`).
- **`uninstall`** (`:654-666`):
  - Clears any previous uninstall error.
  - On failure, stores `uninstallErrors[id]` and rethrows.
  - Otherwise clears the install state and refreshes.
  - `dismissError` clears both the install error and the uninstall error (`:669-672`).
- **Status helpers:**
  - `hasUpdate`: installed version < catalog version (`:471-474`).
  - `packStatus`: `installing` (downloading), `installed`, `unknown` (installed list not loaded yet), `unavailable` (catalog loaded and the pack is neither in it nor installed), else `missing` (`:427-442`).
  - `missingPackOf` and `missingPacks` work over the project kits and also see legacy absolute paths through `packRefFromPath` (`:445-455`).
- **Pack names** (`:379-408`). Lookup order:
  1. Catalog.
  2. Installed copy.
  3. `packs/withdrawn.json`, a map of `{packs:{id:name}}`. It currently contains `tr8-mckaudio`. Never remove entries.
  4. localStorage `micro-daw:pack-names`.
  5. The id itself.
- **Remembered names:** keeps the most recent last and trims to 200 entries.
- **UI button states** (`packsModel.ts:233-275`):
  - `downloading` shows a label:
    - When progress is known: `NN%`. Progress is `received/total` when the total is greater than 0, else `fileIndex/fileCount`.
    - Otherwise, if `fileCount > 0`: `min(fileIndex+1,fileCount)/fileCount`.
    - Otherwise: `…`.
  - `resume`: an error whose message is exactly `'Download paused'` (`pausedState.ts:8`).
  - `error`, `installed`, `get` for the remaining cases.
- **Uninstall confirm:** a two-step button. It reverts after 4 s, or after 9 s when the project uses the pack. The warning reads "Your open project plays sounds from this pack. Its pads go silent until you get the pack again." (`PackDetail.tsx:123-129,214-218`).
- **Used-by-project:** `isUsedByProject`, or an absolute path under the installed `dir` (`usePacksController.ts:193-203`; `packsModel.ts:327-341`).
- **Offline list:** downloadable items are catalog packs plus installed packs the catalog no longer lists (`packsModel.ts:121-135`). If the catalog errors and installed packs exist, a banner says "Offline: showing installed packs only." (`PacksView.tsx:190-198`).

### 2.7 Previews
- **Main side** (`preview.ts:32-61`):
  - The URL must be allowed **and** exact-match a file in the catalog (`ipc.ts:106-111`).
  - Size and sha256 are verified.
  - In-flight fetches are de-duplicated and cached in an LRU (a hit moves to the most-recent position).
  - Each call returns a fresh `ArrayBuffer` copy.
- **Renderer, single sound:** `previewRemote` keeps an 8-entry decoded LRU (`PackService.ts:240,688-710`). Installed files go through `previewFile` / `previewPackFile`.
  - A token cancels superseded previews.
  - Playback is at -6 dB through `Tone.Volume`, with gain 0.9 and a 10 ms linear fade-out (`:208-209,742-759`).
  - `previewing` is cleared when the sound ends.
- **Kit preview** (`kitPreview.ts`):
  - Plays a 2-bar demo groove at 96 BPM in 16ths (32 steps), scheduled on the AudioContext clock (`t0 = now + 0.08 s`, `:60-61,205`).
  - It stops automatically after `(32*stepDur + 1.2) s` (`:210-213`).
  - **Role detection** by sample name, in this order:
    - openHat: `/open/` or the tokens `oh|ohh`
    - closedHat: `hi-hat|closed|pedal` or the tokens `ch|chh|phh|hh|hat`
    - kick: `kick|bass drum` or `bd|kik|kk`
    - snare: `snare` or `sd|snr|sn`
    - clap: `clap` or `cp`
    - perc: a word list (cowbell, clave, shaker, maraca, conga, bongo, tom, rim, tamb, perc, cabasa, agogo) or the tokens `rs|cb|cl|ma|lt|mt|ht` (`:16-31`)
  - **Fallback pads:** kick 0, snare 1, closedHat 2, openHat 3, clap 4 (`:34-51`).
  - **Pattern** (`:64-85`), as steps and velocity:

    | Role | Steps | Velocity |
    |---|---|---|
    | kick | 0, 6, 10, 16, 22, 26, 27 | 0.95 |
    | snare (or clap if no snare) | 4, 12, 20, 28 | 0.9 |
    | clap, when a snare exists | 28 | 0.7 |
    | closedHat | even steps except 14 and 30 | 0.6 |
    | closedHat ghost notes | 7, 23 | 0.35 |
    | openHat | 14, 30 | 0.7 |
    | perc | 11, 25 | 0.55 |

    If no role matched: the first 8 pads with sound, every 4 steps, velocity 0.85.
- **Remote kit preview (not installed)** (`remoteKit.ts`):
  - Uses a 32-entry decoded LRU and in-flight de-duplication.
  - Kit id is `preview:<pack>:<kit>`; sources are `file` with path `remote:<url>`.
  - Error messages:
    - Nothing loaded: `Couldn't load this kit's sounds (<err>)`.
    - Partial: `<f> of <n> sounds couldn't load (<err>)`, and it still plays (`kitPreview.ts:196-202`).
  - Without the Electron API: `'Previews need the desktop app'`.

### 2.8 Get & load intent (`PackService.ts:518-648`, docs/packs.md:57-67)
- **Request** (`getAndLoad(packId,kitId)`):
  - Drops any previous pending request and bumps `loadToken`.
  - Captures the target: active drum track id (or null), that track's kit id, and `arrangementEngine.getSessionId()`.
  - Starts watching the target.
  - Sets `pendingLoad = "<pack>/<kit>"` and `pendingLoadInfo{key, packId, kitId, packName, waitingForRecording:false}`.
- **Install step:** installs when the pack isn't installed **or** the catalog version is newer.
  - Install failure (shown in the install state), a superseded token, or a missing kit (warning) returns null.
- **Target check:** the target must still hold (`:192-201`):
  - Same session.
  - Same active drum track, including null staying null. A drum track created since then breaks the target.
  - If the track is non-null, the same kit id.
- **Recording deferral:** while `isLoopRecording()` or the transport state is `'recording'`:
  - Set `waitingForRecording:true` and wait on a waiter.
  - On wake, re-check the token and the target.
  - Loop, so Stop followed immediately by Record waits again (`:541-545`).
- **Apply:**
  - Set `applyingKit = true` so the load's own kit change doesn't cancel it.
  - `ensureDrumTrack()` if the target was null.
  - `arrangementEngine.setKit(id, kit, 'pack-load')`.
  - If still current, notify `onKitLoaded` listeners with `{packId,kitId,kit,trackId}`.
  - The Packs page then navigates to Beat, but only while it is mounted (`usePacksController.ts:98`).
- **Cancellation triggers:**
  - A newer `getAndLoad`.
  - `cancelPendingLoad()`, which `loadKit` calls on every explicit kit choice (`usePacksController.ts:177`).
  - `onKitSelected` with source `'user'` (`:613-615`; `KitSelectionSource` at `ArrangementEngine.ts:49`).
  - A state or kit change that breaks the target (checked on every `onStateChange` / `onKitChange`; `:595-621`).
  - `cancel(packId)`.
- **The download keeps running** after a cancel; only the load is dropped.
- **Cleanup:** `dropPendingLoad` wakes all recording waiters and clears both pending fields.
- **Kit built from a pack** (`kitFromPack`, `:37-53`):
  - Id `pack:<packId>:<kitId>`.
  - Pads start as defaults. Pads with invalid indexes, unknown files or unsafe paths are skipped.
  - Each kept pad is `normalizePad({...defaults, ...settings, sample:{source:{kind:'pack',packId,file}, name: p.name || label || basename-without-ext}})`.
- **UI rule:** "Get & load" is disabled while that pack is downloading. A waiting row shows "Getting…" (`PackDetail.tsx:259-273`).

### 2.9 `pack-check.mjs` (curation CLI)
- **Default run:** schema, unique ids, kit files exist, pinned URLs, sizes, `packs/sources` sync, the host allowlist matching `PACK_ALLOWED_HOSTS`, and catalog order matching sources order (`:57-72`).
- **`--online`:** HEAD every URL with `redirect:'error'`. Requires HTTP 200 and `Content-Length == size`, unless the response has a `content-encoding` (gzip text).
- **`--deep`:** GET every URL and verify size and sha256. 3 attempts with a `500*attempt` ms backoff; concurrency 8 (`:26-48,79-94`). Exits with code 1 on any error.
- **Curation-only rules** (docs/packs.md:107-142):
  - Pack at most 60 MB.
  - Lower-case paths with no whitespace.
  - 1–4 kits, each filling all 16 pads; kit names at most 24 chars.
  - Manufacturer names banned in `name`/`tagline`.
  - Hats share choke group 1.
- **Current catalog:** version 1, updatedAt `2026-11-18`, 4 packs: `tr808-fischer`, `uzu-drumkit`, `sonic-pi-drums`, `freepats-synth-percussion`.

---

## 3. MIDI

### 3.1 Device handling (`MidiService.ts`)
- **Enable:** `WebMidi.enable()` raced against a 5000 ms timeout (`Timed out waiting for Web MIDI access`). Never throws; returns false on failure (`:9,34-61`). Called from the Start button: audio, then drums, then MIDI (`App.tsx:205-209`).
- **Device listing:** `getDevices` maps inputs to `{id, name, manufacturer, connected:true}` (`:111-119`).
- **Auto-select** (`:160-174`):
  1. The preferred device, if present.
  2. Else the first input whose name matches `/mpk\s*mini/i` (`constants.ts:23`).
  3. Else the first input.
  - The first automatic pick becomes the preferred device. Later fallbacks don't replace it.
- **`selectDevice(id)`:** ignored if the id is not present. Otherwise sets it as preferred and attaches. Re-attaching to the same input is a no-op (`:121-135`).
- **Hot-plug** (`:176-196`):
  - Active input removed: detach (releasing held notes), then auto-select.
  - No active input, or the preferred device came back while a different one is active: auto-select (re-attach to the preferred device).
  - Always notify device-change listeners.
- **Held notes:** tracked as `"channel:note"`. On detach (switch, unplug, dispose), synthetic note-offs are sent for every held note (`:20,92-105,235-248`).
- **Note-on with velocity 0** is treated as a note-off (`:81-90`; also `useMidiNotes.ts:279-283`).
- **Velocity:** `rawAttack/127`, clamped to 0..1 (`noteHelpers.ts:28-30`).
- **CC:** forwarded with `(controller.number, rawValue 0..127, channel)`.
- **Raw `midimessage`** goes to raw listeners (the MIDI monitor) (`:203-223`).
- **Settings gap:** `settings.midi.autoConnectLastDevice` is shown in the UI only. The preferred device is in memory only and is **not persisted** (`SettingsView.tsx:233`; there is no read site). **Electron gap fixed in Rust** (user decision 2026-11-18): see [phase 5](../phase-5-midi.md#electron-gap-fixed-in-rust-autoconnectlastdevice-user-decision-2026-11-18).

### 3.2 Routing (`hooks/useMidiNotes.ts`)
- **Modes:** `'auto' | 'keys' | 'split'` (`constants.ts:66`), default `'auto'`. The mode is held in React state only, not persisted (`:88-90`).
- **Actual behaviour:** only `'keys'` is special; it sends everything to the synth. `'auto'` and `'split'` behave **identically**: if the controller profile maps the note (with its channel filter) it plays that pad, otherwise the synth (`:272-275`).
  - The selector tooltips claim "low keys drums" and "left half drums" (`RoutingModeSelector.tsx:9-13`), but nothing implements that. Decide whether to port the behaviour or the description.
  - **Decided (user decision 2026-11-18, Electron bug fixed in both apps):** Split is a keyboard split. Notes below the split point (default C4, `micro-daw:midi-split-point`) trigger pads chromatically; the rest play the synth. The Auto tooltip now describes the profile pad map. See [`../review-phase-5.md`](../review-phase-5.md) #11.
- **Channel 10 is not special.** `DRUM_CHANNEL` and `DRUM_NOTE_TO_PAD` are unused (`:73`; `constants.ts:53-58`).
- **Note-on order** (`:278-298`):
  1. Velocity 0 → note-off.
  2. `controllerMapping.handleNoteOn`. If a learn consumes the note, remember the key so its note-off is swallowed too.
  3. If the note maps to a pad: a re-hit without a note-off releases the previous pad first, then `pressPad` (hit plus note-repeat press).
  4. Otherwise `keyDown` on the synth.
- **Note-off** uses the route chosen at note-on, never the current routing (`:301-311`).
- **On-screen keyboard** always goes to the synth on channel 0 (`:314-320`).
- **Synth voice model** (`:58-64,244-269`):
  - At most one voice per pitch. A re-press releases the old voice first; while recording, it also captures a note-off.
  - Muted: the voice is tracked with `attacked:false` and no sound.
  - `keyUp`: skipped if the note isn't held. Otherwise capture the note-off while recording, and release now unless sustain is on.
- **Pads:**
  - `hitPad` with a scheduled time (note repeat) defers the flash, the event-bus event and the recorder capture until the audio time. A delay of at most 1 ms runs immediately (`:166-192`).
  - `releasePad`: note-repeat release, settle pending hits (those already due run, the rest are dropped), `drumEngine.release`, and a note-off capture while recording (`:209-215`).
- **Sustain:** CC64, on when the value is ≥ 64. Pedal up releases every voice that is no longer held (`:29,324-334,399-402`).
- **Panic:**
  - CC120 (All Sound Off) and CC123 (All Notes Off) → `allNotesOff()`.
  - CC121 (Reset All Controllers) → sustain off only (`:30-32,403-410`).
  - **`allNotesOff`** (`:341-359`):
    1. While recording, capture note-offs for held voices.
    2. Release all voices and all pads, and clear learned notes.
    3. `noteRepeat.releaseAll`, then settle all pending hits.
    4. Sustain off; `audioEngine.releaseAll()`.
    5. Emit `{type:'allnotesoff'}`.
  - Also triggered on every app-mode switch (`App.tsx:196-203`) and on unmount (`:370`).
- **CC dispatch order** (`:398-452`):
  1. Sustain or panic CCs, as above.
  2. `controllerMapping.handleCC`: a knob learn binds the CC; a mapped CC dispatches knob handlers. Either consumes the CC.
  3. **Legacy fallbacks** for unmapped CCs:
     - **Volume CCs `[5, 74]`:** `round(-40 + v/127*40)` dB, sent to `onVolumeChange` (master) or `audioEngine.setVolume`. No pickup.
     - **Effect CCs** (`constants.ts:26-35`):

       | CCs | Effect | Range / curve |
       |---|---|---|
       | 1 & 70 | filter cutoff | log 60..18000 Hz: `60*(18000/60)^n` |
       | 2 & 71 | resonance | `n*20` |
       | 3 & 72 | reverb wet | `n` |
       | 4 & 73 | chorus depth | `n` |

       These use soft takeover keyed by the CC number, with the inverse curves at `:378-386`. With the default profile, CCs 70–77 are mapped, so the 70–74 fallbacks only apply after the user remaps.
- **Event bus** (`NoteEventBus.ts:12-21`): `noteon`, `noteoff`, `drum{padId, note, velocity, time, audioTime?}`, `allnotesoff`. Listener errors are caught. Time comes from `performance.now()`.

### 3.3 Controller mapping (`ControllerMapping.ts`)
- **Storage:** localStorage `micro-daw:controller` = `{"version":1,"profile":{…}}` (`:22-23,163-165`). Anything missing, corrupt or of another version gives the default profile (`:168-177`).
- **Default profile** `mpk-mini-default`, "MPK Mini (default)" (`:80-105`):
  - Pads: notes 36..51 for pads 0..15, any channel.
  - Knobs, all on any channel:

    | CC | Target |
    |---|---|
    | 70 | pad pitch |
    | 71 | pad decay |
    | 72 | pad filterCutoff |
    | 73 | pad gainDb |
    | 74 | swing |
    | 75 | bpm |
    | 76 | trackVolume |
    | 77 | masterVolume |

- **Knob targets in menu order** (`:44-54`):
  - Keys `pad:pitch`, `pad:decay`, `pad:filterCutoff`, `pad:gainDb`, `pad:pan`, `pad:startSec`, `swing`, `bpm`, `trackVolume`, `masterVolume`.
  - Labels: "Pad pitch", "Pad decay", "Pad filter", "Pad gain", "Pad pan", "Pad start", "Swing", "Tempo (BPM)", "Drum track volume", "Master volume".
- **`validateProfile`** (`:135-161`):
  - `id` and `name` must be strings.
  - `padMap.notes` must have exactly 16 entries, each an integer 0..127 or null.
  - `padMap.channel` must be null or 1..16.
  - CC mappings with invalid `cc`, `channel` or target are dropped individually.
- **Lookups:**
  - `findPadForNote`: the channel filter, then the first index of the note (`:182-187`).
  - `findCCMappings`: every row with the same cc whose channel is null or matches. Several rows can fire (`:190-192`).
- **Edits:**
  - `assignPadNote`: the note moves off any other pad (`:198-203`).
  - `assignKnobCC` (`:211-231`):
    - With an index: that row, or a new row if the index is out of range.
    - Without an index: the first row with the same target, else a new row.
    - The bound row gets `channel:null`, and every other row on the same CC is removed.
- **Learn flows:**
  - **Pad wizard** (`startPadLearn('sequence')`):
    - Starts at pad 0 and clears the set of seen channels.
    - Each note-on is consumed: the note is assigned to the current pad. If the pad map is filtered to a different channel, the filter widens to any.
    - The channel is recorded, then the wizard advances. After pad 15 it finishes.
    - On finish, if exactly one channel was seen, `padMap.channel` narrows to it (`:387-396,409-430`).
    - Skip advances; Back (Backspace) goes to `max(0, pad-1)`.
  - **Single pad:** learns once, then returns to idle.
  - **Knob learn** (`startKnobLearn(target, index?)`): the next CC is bound and learn returns to idle (`:437-444`). "Add knob" learns into a new row whose default target is the first one not mapped yet (`ControllerSetupDialog.tsx:371-375`).
  - **Cancel:**
    - Closing the dialog cancels a learn.
    - The first Esc cancels a learn; a second Esc closes the dialog (`ControllerSetupDialog.tsx:50-52,77-83`).
    - Reset to default cancels any learn and restores the default profile.
- **CC to knob value:** `value01 = clamp(raw/127, 0, 1)` (`:455`).
- **Activity record:** the last note or CC is recorded for the UI, as `{kind:'note', note, velocity, channel, pad, at}` or `{kind:'cc', cc, value, channel, targets, at}` (`:242-244`).

### 3.4 KnobRouter (`KnobRouter.ts`) and soft takeover (`KnobPickup.ts`)
**Absolute knobs only;** there is no relative or endless-encoder mode.

**Value curves** (`:25-68`):

| Target | Curve | Range | Rounding |
|---|---|---|---|
| pitch | linear | -24..24 | integer |
| decay | log | 0.005..4 s | 0.001 |
| filterCutoff | log | 20..20000 Hz | integer |
| gainDb | linear | -40..+6 dB (the pad itself allows -60..12) | 0.1 |
| pan | linear | -1..1 | 0.01 |
| startSec | see below | 0..(trim end − 0.01) | 0.001 |
| swing | linear | 0..0.75 | 0.01 |
| bpm | linear | 60..180 | integer |
| track / master volume | linear | -40..0 dB | 0.5 |

- `startSec`: the trim end is `min(bufferDur, endSec ?? bufferDur)`. Without a known buffer length it does nothing.
- Inverse curves are at `:70-97`.

**Undo grouping:**
- One undo group per gesture key (`trackId:pad:param`, `swing`, `bpm`, `trackVolume:<id>`).
- The group closes after 400 ms idle, or when another key starts (`:145-168`).
- Labels: "Pad Gain/Pan/Pitch/Decay/Filter/Start", "Swing", "Tempo", "Track Volume".
- Master volume is not undoable.

**Pad targets** act on `getSelectedPad()` of the active drum track, creating a drum track if there is none. Track volume does nothing without an active drum track. Changes are skipped when the value is unchanged (`:185-241`).

**Soft takeover** (`KnobPickup.ts`):
- The knob takes control when it is within 0.05 of the current value, or when it crosses the value between consecutive messages.
- It is un-picked when the parameter moved more than 0.02 from what the knob last set (`:15-50`).
- Pickup state is per key.
- All pickups reset when the active-track/selected-pad context changes (`KnobRouter.ts:172-179`).
- The legacy effect CCs use the same pickup logic, keyed by the CC number.

---

## 4. Project format, migrations, persistence

### 4.1 File
- **Extension and file type:** `.mdaw`, UTF-8 JSON, `JSON.stringify(…, null, 2)` (`ProjectManager.ts:228`).
- **Default name:** `Untitled.mdaw`. The save dialog defaults to `<projectName>.mdaw` (`:62`).
- **Window title:** `<name>[ *] - Micro DAW` (`:305-311`). The project name is the file name without `.mdaw`, **split on `/` only** (`:37-42,317`), so Windows paths come out wrong. Decide whether to keep or fix this.

**Top-level shape** (`types/project.ts:4-22`):
- `version`: `'1.0.0'`. On load, any other value only logs a warning (`ProjectManager.ts:243-245`).
- `arrangement`: an `Arrangement`.
- `audioConfig`: `{masterVolume (dB), effectParams: EffectParams}` (all 20 keys, `types/effects.ts:4-25`).
- `createdAt`, `modifiedAt`: ISO strings.
- `sampleManifest`: an optional array of `{sampleId, path, name}`. It is written only when non-empty, with one entry per unique `region.audio.sampleId`. The name is the sample name, else the region name, else `'Sample'` (`:200-226`).

**Arrangement** (`types/arrangement.ts`):
- `{id, name, bpm, lengthBeats, tracks[], loopMarkers?{startBeat,endBeat}, markers?[{id,name,beat,color}], groups?[{id,name,color,trackIds,collapsed}], beat?{activeDrumTrackId, loop:LoopRecordSettings, selectedPad}}`
- **Track:** `{id, name, instrument{type:'synth'|'drums'|'audio', presetIndex, synthOverrides?, kit?:KitState}, regions[], volume, pan, muted, solo, color, effects?:TrackEffectState, automation?[{parameter, points[{beat,value 0..1}], visible}]}`
- **Region:** `{id, startBeat, lengthBeats, notes[{note, velocity 0..1, startBeat (relative), durationBeats, isDrum}], color?, name?, audio?{sampleId, samplePath, offsetSeconds, gainDb, originalDuration}}`
- **Drum-region notes** are pad notes `36 + pad` (36..51) (`types/sampler.ts:16-25`).

**Kit** (`types/sampler.ts:57-86`): `KitState{id, name, pads[16]}`.
- `PadSettings{sample:{source,name}|null, gainDb, pan, pitch, startSec, endSec|null, decay, filterCutoff, reverse, mode:'oneshot'|'gate', chokeGroup, color?}`
- Source kinds:
  - `factory{voice}`
  - `classic{machine,voice}`
  - `file{path, sampleId?}`
  - `pack{packId, file}`

**LoopRecordSettings** (`types/sampler.ts:125-142`): `{bars 1|2|4|8, quantize off|1/4|1/8|1/8T|1/16|1/16T|1/32, swing 0..0.75, swingGrid 1/8|1/16, countInBars 0|1|2, metronome, overdub}`.

### 4.2 Load sequence (`ProjectManager.loadFromFile`, `:235-286`)
1. Read and `JSON.parse`. Any exception logs `Failed to load project:` and returns false; no UI error is shown.
2. For each `sampleManifest` entry, `sampleManager.loadFromPathWithId(sampleId, path)`:
   - Skipped if the id is already loaded.
   - On failure, warns `could not load sample "<name>" from <path>` and continues. The region stays, silent (`SampleManager.ts:63-96`).
3. `preloadProjectKits`: normalize each drum kit, preload its buffers, ignore errors. A missing file leaves the pad's name and path but it is silent (`:352-357`).
4. `arrangementEngine.loadProject`: start a new session, then `restoreFromSnapshot`, which runs `migrateArrangement` (below).
5. Clear undo history.
6. **Audio config:**
   - If `masterVolume` is finite, `setMasterVolumeDb` (clamped to -40..0).
   - Apply `reverbWet`, `chorusDepth`, `filterCutoff`, `filterResonance` only. The other 16 `effectParams` keys are saved but **not restored**.
7. Set the file path; clear dirty; `createdAt` from the file, or now; add to recent (using the file's `modifiedAt`); update the title.

### 4.3 Migrations on load
**`migrateArrangement`** (`ArrangementEngine.ts:2831-2843`):
1. **Per track** (`migrateTrack`, `:2816-2828`):
   - `pan ??= 0`; `effects ??= DEFAULT_TRACK_EFFECTS` (`types/effects.ts:73-81`).
   - Any note velocity > 1 becomes `min(1, v/127)`.
   - **Drum track with a kit:** `normalizeKit` (`arrangementHelpers.ts:259-264`).
   - **Kitless drum track (legacy GM):**
     - Every non-audio region's notes are mapped GM note → factory voice → factory pad note (`factoryKit.ts:114-124`).
     - Notes with no mapping are dropped, with the warning `"<track>": N legacy drum note(s) have no pad and were dropped`.
     - Hits on the same pad at the same time merge, with the louder one winning (`mergeDrumHit`).
     - The track gets the factory kit (`arrangementHelpers.ts:239-274`).
   - **GM → voice map:**
     - kick: 35, 36
     - rim: 37
     - snare: 38, 40
     - clap: 39
     - chh: 42; phh: 44; ohh: 46
     - tom-lo: 41, 43; tom-mid: 45, 47; tom-hi: 48, 50
     - crash: 49, 52, 55, 57
     - ride: 51, 53, 59
     - shaker: 54, 69, 70, 82
     - perc: 56, 76, 77
     - snap: 75
   - **Factory pad order:** kick, snare, chh, ohh, clap, tom-mid, crash, ride, kick-sub, rim, phh, shaker, snap, tom-lo, tom-hi, perc. Hats use choke group 1 (`factoryKit.ts:65-71`).
2. **`beat = normalizeBeatState`** (`arrangementHelpers.ts:215-228`):
   - The active id must be a drum track's id, else the first drum track, else null.
   - `selectedPad` is rounded; outside 0..15 it becomes 0.
   - `loop = normalizeLoopSettings` (`:191-212`):
     - Invalid enum values fall back to the defaults `{bars 2, quantize '1/16', swing 0, swingGrid '1/16', countIn 1, metronome true, overdub true}` (`:42-50`).
     - Swing is clamped to 0..0.75.
     - **`swingGrid` migration:** when missing but `quantize` is a string, it becomes `'1/8'` if quantize was `'1/8'`, else `'1/16'`.
3. **Missing or invalid loop markers** (non-finite, or end ≤ start): set to `{0, bars*4}`. If `lengthBeats < endBeat`, set `lengthBeats = ceil(end/4)*4`.

**`normalizeKit` / `normalizePad`** (`factoryKit.ts:225-259`):
- `id` defaults to a random UUID; `name` defaults to `'Kit'`.
- Always 16 pads; missing pads get defaults.
- **Pad defaults:** gain 0, pan 0, pitch 0, start 0, end null, decay 0.05, cutoff 20000, reverse false, oneshot, choke 0.
- **Clamps:** gain -60..12, pan ±1, pitch ±24, `start ≥ 0`, end kept only if it is finite and greater than start, decay 0.005..4, cutoff 20..20000, choke rounded and ≥ 0. `mode` is `gate` only when exactly `'gate'`. `color` is kept if it is a non-empty string.
- **Sources** (`:181-200`):
  - `factory`: the voice must be valid.
  - `classic`: machine and voice must be valid.
  - `pack`: the id must be valid and the path safe.
  - `file`: the path must be non-empty.
  - **Absolute-path migration:** a `file` path inside `<packsDir>/<id>/…` (either separator, safe id and path) becomes `pack{packId, file}` (`shared/packSource.ts:45-57`).
  - Anything invalid → `sample: null`.
  - **Name fallback:** factory label, classic name, or the file basename without extension.

**Other renderer path:** `restoreFromSnapshot` and `loadFromLocalStorage` also run `migrateArrangement` (`ArrangementEngine.ts:1535-1551,1590-1607`).

**Pack resolution at runtime:** `<packsDir>/<packId>/<file>`, with separators following `packsDir` (`packSource.ts:24-38`). If it cannot be resolved or read, the result is `MissingPackSampleError(packId, file)` (`bufferCache.ts:42-86,315`).

**New / template projects** (`ProjectManager.ts:91-151`):
- Prompt about unsaved changes, then ask for a save path (required). Cancelling aborts.
- `resetToStarter`: a fresh arrangement (`{uuid, 'Untitled', bpm 120, length 64}`) plus a drum track and the default loop. Or `loadFromTemplate`: deep clone with fresh UUIDs for the arrangement, tracks and regions.
- Clear undo history.
- Master volume = `MASTER_REFERENCE_DB`; reverb 0, chorus 0, cutoff 18000, resonance 1.
- Write the file immediately.

### 4.4 Recent projects
- localStorage `micro-daw-recent-projects`: a JSON array of `{filePath, name, modifiedAt}`.
- Most recent first, de-duplicated by path, at most 10 entries.
- Updated on every save and load (`ProjectManager.ts:11-12,313-344`).
- The loaded value is not validated (raw `JSON.parse`).

### 4.5 Autosave and dirty tracking
- **No autosave of the `.mdaw` file.**
- The **working arrangement** autosaves to localStorage `micro-daw-arrangement` (the raw `Arrangement` JSON). It is debounced 300 ms after every engine change and flushed on `beforeunload`, `pagehide` and unmount (`hooks/useArrangement.ts:339-375`; `ArrangementEngine.ts:1527-1551`).
- **First mount:** `loadFromLocalStorage()`, otherwise `resetToStarter()`.
- **Dirty flag:** any arrangement state change marks the project dirty, except the first change after open/new/template (`useProject.ts:26-36`).
- Master volume and effect edits do **not** mark the project dirty.

### 4.6 localStorage keys (complete list; all lost if the Rust app doesn't migrate them)
| Key | Format | Cite |
|---|---|---|
| `micro-daw-arrangement` | Arrangement JSON (autosave) | constants.ts:68; ArrangementEngine.ts:1529 |
| `micro-daw-recent-projects` | `[{filePath,name,modifiedAt}]`, max 10 | ProjectManager.ts:11 |
| `micro-daw-settings` | `{general:{autoCheckUpdates}, audio:{defaultMasterVolume, bufferSizeHint 128..2048}, midi:{autoConnectLastDevice}}`. Only known keys survive load. Defaults: true / -12 / 256 / true | constants.ts:102,105-109; useSettings.ts:5-22 |
| `micro-daw-sample-library` | `[{sampleId, samplePath, sampleName, trim?{startSeconds,endSeconds}, addedAt}]` (entries with an empty path are excluded) | constants.ts:103; useSampleLibrary.ts:28-60 |
| `micro-daw-piano-roll` | `{snap≥0, drawLength>0, follow:bool}`; default snap 0.25, follow true | PianoRollEditor.tsx:70-90,191 |
| `micro-daw-recordings` | defined (`MAX_RECORDINGS` 20) but **no read or write sites** | constants.ts:63-64 |
| `micro-daw:controller` | `{version:1, profile}` | ControllerMapping.ts:22 |
| `micro-daw:kits` | `KitState[]` (normalized; `file.sampleId` stripped) | KitStore.ts:4,66-73 |
| `micro-daw:pack-names` | `{[packId]: name}`, max 200, most recent last | PackService.ts:92-106,389-408 |
| `micro-daw:masterVolume` | number string (dB, clamped -40..0; default `DEFAULT_VOLUME` -12) | masterBus.ts:53,64-96 |
| `micro-daw:browser-open` | `'false'` = closed; anything else = open | App.tsx:50-55,103 |
| `micro-daw:browser-width` | number, clamped 200..520, default 248 | BrowserPanel.tsx:5-20 |
| `micro-daw:browser-roots` | JSON array of folder paths (`normalizeRoots`) | useSampleBrowser.ts:17,38-46 |
| `micro-daw:browser-last-path` | path string; used only if it lies under a root | useSampleBrowser.ts:18,48-51 |
| `micro-daw:browser-auto-preview` | flag string | useSampleBrowser.ts:19 |
| `micro-daw:dock-pad-bank` | `'A'` or `'B'` | InstrumentDock.tsx:37-41 |
| `micro-daw:beat-pad-layout`, `micro-daw:beat-step-grid`, `micro-daw:beat-key-hints` | enum strings read through `readPref` with an allowed list (see BeatView for the values) | BeatView.tsx:49-65 |
| `micro-daw:midi-split-point` | MIDI note number (JSON number), the Split routing's keyboard split; default 60 (added with the phase 5 decision; missing from this list until the [phase 10 review](../review-phase-10.md) #9) | midiSplit.ts:12 |

---

## 5. Gaps to decide on (current behaviour that is probably not intended)
1. Routing modes `auto` and `split` behave the same (`useMidiNotes.ts:273`).
2. `autoConnectLastDevice` is not wired, and the preferred MIDI device is not persisted. → **Electron gap fixed in Rust** (phase 5; `src/` unchanged).
3. Only 4 of the 20 `effectParams` are restored on project load (`ProjectManager.ts:268-272`).
4. Project names split on `/` only (`ProjectManager.ts:39,317`; `SampleManager.ts:75`).
5. `project:save`, `project:load` and `sample:read-file` accept any path; the only check is the trusted sender.
6. There is no application menu and no `.mdaw` file association or open-file handling.
7. `micro-daw-recordings` is defined but never used.