# Phase 10: app shell parity (`rune/crates/mdaw-app`) vs the Electron app

**Scope** (plan §4 row 10, §5 gaps; inventory [`main-packs-midi-project.md`](inventory/main-packs-midi-project.md)
§1 and §4): the project service (New / Open / Save / Save As, recent projects, title + dirty
marker, the unsaved-changes prompt, ⌘N / ⌘O / ⌘S / ⇧⌘S and the palette entries, native file
dialogs, `.mdaw` drops), the window (min size, full screen, title, close guard, quit), the one-time
import of the Electron app's settings, "Check for updates", and the macOS bundle. Built on
`rune-port` after the phase-9 merge (e184ece).

**Review:** [`review-phase-10.md`](review-phase-10.md) (19 findings fixed after the merge
4d24e26: the recovery checkpoint, autosave durability, single instance, the import's
robustness, atomic writes, IO off the UI thread, …). This doc describes the state after it.

## Modules

| Module | Port of | Notes |
|---|---|---|
| `project.rs` | `services/ProjectManager.ts`, `hooks/useProject.ts` | `ProjectUi` (path, `createdAt`, recent list, prompt, load report, title), every flow below; atomic file writes |
| `shell.rs` | `main.ts` close / `project:force-close` / `window-all-closed` / `will-quit`, `useAutoUpdater` | close guard on top of Rune's `keep_running`, quit, MIDI suspended while the window is closed, update check at start, phase-10 dialog keys, packs folder choice |
| `recovery.rs` | `useArrangementPersistence` (autosave) | the recovery checkpoint (arrangement + session record in one write), its acknowledgement and retry, recovery WAVs, the restore (review) |
| `io.rs` | — | the project IO worker (review) |
| `instance.rs` | `app.requestSingleInstanceLock` (not used by the Electron app) | one process per data folder (review) |
| `leveldb_check.rs` | — | verifies a copied LevelDB before reading it (review) |
| `updater.rs` | `electron-updater` (GitHub provider, `semver.gt`) | `/releases/latest`, SemVer 2.0 precedence, "Open release page" (no download / install) |
| `electron_import.rs` | — | Chromium localStorage LevelDB reader (via `rusty-leveldb` 4.0.2 on a verified temp copy, on its own thread), one-time import with an in-progress mark and bounded retries, shared packs folder |
| `ui/shell.rs` | `dialog.showMessageBox` (unsaved), — | the unsaved prompt, the load report / save error dialog, the import note, the app icon |
| `scenes_shell.rs`, `scripts/parity-phase-10/` | — | 8 screenshot states + the `app-icon` scenes |
| `rune/scripts/build-mac-app.sh`, `rune/scripts/icon-matte.py`, `rune/scripts/mac-entitlements.plist` | Electron Forge makers | `Micro DAW.app` |
| `scripts/electron-profile-fixture/` | — | writes a synthetic profile with the real Electron (the import test's fixture) |

Shared files changed are listed at the end.

## Flows (same order and texts as the TS)

- **New** (⌘N, sidebar New, palette): unsaved prompt → save dialog "Save Project", default
  `Untitled.mdaw`, filter "Micro DAW Project" [`mdaw`] → starter project (one Drums track), undo
  cleared, master volume `MASTER_REFERENCE_DB` (persisted like `setMasterVolumeDb`), reverb 0,
  chorus 0, cutoff 18000, resonance 1 → written at once. Cancelling either step changes nothing.
  The new project is Untitled and unsaved until the write succeeds; a failed write leaves it so,
  with the error (review #11; Electron took the path and marked it clean first).
- **New from template** (template picker): the same with the template (fresh ids). Phase 9 loaded the
  template directly; it now asks first, like `newProjectFromTemplate`.
- **Open** (⌘O, sidebar Open, palette), **open recent** (sidebar Recent, palette "Recent
  Projects"), a **dropped `.mdaw`**: unsaved prompt → open dialog "Open Project" (not for recent /
  drop) → `loadFromFile` through `mdaw_model::open::open_project` (the phase-1 migrations +
  coercion): manifest samples by id (missing files reported), kits normalized, `loadProject` (new
  session), undo cleared, master volume when finite, the four restored live FX params, path,
  clean, `createdAt` from the file, recent list (with the file's `modifiedAt`), title.
- **Save** (⌘S, the top bar's project button, palette): the current path, else **Save As**
  (⇧⌘S, palette): save dialog with `<name>.mdaw`, `createdAt` = now. The file is the
  `writeToFile` JSON (`sampleManifest` only when non-empty) written by the IO worker through
  `mdaw_model::fsio::write_atomic` (symlink target, permissions kept, temp + fsync + rename +
  folder sync; in place when only the file is writable). The path, `createdAt` and clean are the
  project's once the write succeeded; clean means the **version written** (an edit made during the
  write stays unsaved). Loads are read and migrated on the worker too (review #10, #13).
- **Title**: `<name>[ *] - Micro DAW` (`Tx::set_title` when it changes; the window starts as
  `Untitled - Micro DAW`). The top bar shows the name and the dirty dot.
- **Dirty**: the doc's state version since the last load / save (master volume and FX edits do
  not count, like the TS).
- **Recent projects**: `micro-daw-recent-projects`, most recent first, de-duplicated by path, 10
  at most, updated on every save and load; the expanded sidebar lists them (`title={filePath}`),
  the palette shows the first 10 with the path under the name, searchable by path.

**Load report** (Rust addition, documented deviation). Electron only logs what a load changes or
cannot do (`console.warn` / `console.error`); the Rust app shows it: "Couldn't open project" with
the `Failed to load project: …` text (the project stays as it was), or "Project opened with
changes" listing the version warning, the dropped legacy drum notes, the coercion warnings
(`$.tracks[0].volume: …`) and the manifest samples that could not be loaded: missing files at
once, files that exist but fail to decode when their decode fails (review #16). A failed save shows
"Couldn't save project" (Electron's IPC rejection is unhandled there).

## Close and quit

Electron: the window's `close` is prevented unless allowed, the renderer asks
(`project:before-close` → `handleBeforeClose`: clean → force close; dirty → Save / Don't Save /
Cancel), `project:force-close` allows it and closes; `window-all-closed` quits except on macOS;
`will-quit` pauses running pack installs (at most 3 s).

Rune cannot veto a window close, and it cannot cancel a quit. The window app therefore runs with
`App::keep_running` and its state holds the app's `AppHandle`:

- **Close button / ⌘W / Alt-F4** → `App::on_window_close`: clean (or allowed) → the window goes:
  `stop_for_close` (typed BPM commits, held notes released, transport stopped, autosave and session
  written); off macOS the app quits (`window-all-closed`), on macOS it keeps running windowless
  and the Dock icon brings the same state back (Electron recreates the window from localStorage).
  Dirty → the window is shown again at once (`AppHandle::open`) with the unsaved prompt; Save
  (with a path, or after the save dialog) / Don't Save close it, Cancel (or a cancelled save
  dialog) keeps it and re-arms the guard. Verified in the real window (below).
- **Quit** (macOS ⌘Q, the Dock's Quit, logout; a Windows session end) → `App::shutdown`:
  everything above, a project write in flight finished (≤ 5 s), the checkpoint written and waited
  for, a running export finished (≤ 10 s) or cancelled (nothing written; the destination keeps
  its old content), `PackService::pause_for_quit`. It runs with or without a window (a hidden
  `keep_running` window still gets `on_quit` in the pinned Rune). **It cannot ask** (gap, Rune).
- **While the window is closed** (macOS) MIDI input is suspended (`Router::set_suspended`):
  Electron's renderer, which routes MIDI, is destroyed with its window. The reopened window
  resumes it.

**What a quit or a crash keeps (the recovery checkpoint, `recovery.rs`, review #1–#4, #6).**
Every autosave (300 ms after a change) and every project transition (open, save, save as, new,
a failed write) writes the arrangement and its session record in **one** settings-store write; a
checkpoint counts as saved only once the store reports it on disk, a failed one shows "Autosave
failed" and is retried every 2 s; the store keeps the previous file as a second slot. The next
launch restores:

| | comes back |
|---|---|
| the arrangement | as of the last durable checkpoint (edits of the last ≤ 300 ms before a crash may be missing; a quit flushes them) |
| the project's path | only when the record matches that arrangement (hash) and the file still has the `modifiedAt` this app last read or wrote; otherwise the work comes back **Untitled and unsaved** with a note, so ⌘S can't write it over a file it didn't come from |
| unsaved (`*`) | yes, with or without a path |
| reverb, chorus, filter cutoff / resonance | yes (the four params a project file restores) |
| generated / processed samples on the timeline | yes (32-bit float WAVs in `<data dir>/recovery/`) |
| not recovered | undo history; the other 16 live-FX params; generated samples only in the Samples library; a cancelled export |

Electron relaunches as a clean "Untitled" with its autosaved arrangement (no path, no dirty
flag, no generated audio).

**One process per data folder** (review #5): a window run locks `<data dir>/instance.lock`
before it opens anything there; a second launch shows "Micro DAW is already running" and quits.

## Electron bugs found (fixed in both apps, user decision 2026-11-18)

1. **A cancelled Open / New swallowed the next edit.** `useProject` set `skipDirtyRef` before
   `open()` / `newProject()` / `openRecent()` / `newProjectFromTemplate()` and only the next
   arrangement change cleared it. When the dialog was cancelled, the user's next edit did not mark
   the project dirty and the close guard let the window close without asking. TS: cleared in a
   `finally` (`src/hooks/useProject.ts`, `useProject.test.ts`, fails on the old code). Rust: dirty
   is a state-version comparison (`a_cancelled_open_does_not_swallow_the_next_edit`).
2. **The close guard was never re-armed (macOS).** `forceCloseAllowed` was a module global set by
   `project:force-close` and never reset; the window `activate` recreated on macOS then closed a
   dirty project without asking. TS: `src/main/closeGuard.ts` (one guard per window, armed again
   after the allowed close; `closeGuard.test.ts`). Rust: `force_close` is cleared by the close it
   allowed and on reopen.
3. **Project names of Windows paths** (inventory §5 gap 4): `getProjectName` /
   `addToRecentProjects` split on `/` only, so `C:\Songs\Demo.mdaw` was named `C:\Songs\Demo`. TS:
   `baseName` (`ProjectManager.test.ts`); Rust: `mdaw_model::migrate::base_name` (model tests).
   `SampleManager`'s sample names (same split) are left as they are (not a project-shell item).

4. **Edits made while an Open / New dialog was up were swallowed** (phase 10 review #15). The
   fix for bug 1 still suppressed dirty from the start of the operation, before its dialogs,
   so a MIDI recording or pad edit made while the Open dialog showed was not marked dirty. TS:
   `ProjectManager.isReplacingProject()` covers only the synchronous replacement, `useProject`
   skips only that (`useProject.test.ts`, fails on the old hook; `ProjectManager.test.ts`). Rust:
   `an_edit_while_the_open_dialog_is_pending_marks_the_project_unsaved`.

Kept as in the TS (judgment calls): only 4 of the 20 `effectParams` are restored on load (gap 3);
`addToRecentProjects` stores `""` for a file without `modifiedAt` (the TS stores `undefined`,
dropped by `JSON.stringify`); the template / New dialogs ask for the path before resetting.

## Window and menus

| Electron | Rust | |
|---|---|---|
| 1360 × 800, min 1360 × 768 | `AppSpec` size (1360, 800), `min_size` (1360, 768) | Rune's transparent title bar fills the frame like `hiddenInset` |
| title `Untitled - Micro DAW`, then `<name>[ *] - Micro DAW` | the same (`Tx::set_title`) | |
| full screen: green button, palette "Toggle Fullscreen", top-bar button with Minimize icon in full screen | `Tx::toggle_full_screen` (palette, top bar); the button shows Minimize when the macOS traffic lights are gone (full screen) | Windows / Linux: always Maximize (no full-screen signal in Rune) |
| `backgroundColor '#100e0c'`, shown on `ready-to-show` | Rune shows the window on its first frame | |
| no app menu (Electron's default: app menu, Edit, View with reload / devtools, Window) | the app menu (About, Services, Hide, Quit) plus **Edit** (Undo, Redo, Cut, Copy, Paste, Select All) and **Window** (Minimize ⌘M, Close ⌘W) | the pinned Rune installs Edit and Window by itself for a regular app whose menu bar is still winit's (`native_menu/standard_macos.rs:29-37`); by Rune's contract the keys still reach the app's handlers first (⌘Z = the app's undo, text fields get ⌘C / ⌘V; a ⌘W nothing takes closes the window through `on_close`, i.e. the close guard). Seen in the running app (System Events: `micro-daw, Edit, Window`; Edit › Undo … Select All); the keys by hand are on the checklist. Gap: no View menu (Reload / DevTools have no meaning in a native app). The phase-10 text claiming "winit's default only" was wrong (review #18) |
| permission handler allows only MIDI | — (no web permissions) | N/A |
| no `.mdaw` file association, no `open-file` | the same; a `.mdaw` dropped on the window opens it (Rust addition, requested) | file association would need Rune support for `application:openFiles:`; not done |

## Settings import from the Electron app

On the Rust app's first launch with this code (`micro-daw-rune:electron-import` absent):
`<config dir>/micro-daw/Local Storage/leveldb` (`MDAW_ELECTRON_PROFILE` overrides; empty = none)
is copied to a temp folder (Electron may be running and holds `LOCK`; the original is never opened)
and read with `rusty-leveldb`. Items are `_<origin>\0<key>` with Chromium's string format byte
(`\x00` UTF-16LE, `\x01` Latin-1). The origin is `file://` (the packaged app) when it has
micro-daw keys, else the origin with the most of them (a dev build's Vite URL). Every key of the
inventory §4.6 list found there is copied unless the Rust store already has it (a Rust value is
kept). The packs folder is **shared, not copied**: if `<userData>/packs` exists the Rust app uses
it from then on (`micro-daw-rune:packs-dir`), under the [packs-folder protocol](packs-dir-protocol.md).
A note ("Imported N settings from the Electron app; its packs folder is shared.") shows at the
bottom right until dismissed; a failure shows the reason instead and the start goes on.

**Robustness** (review #8): the mark is written `in-progress` before reading; the copy is
removed by a drop guard; every log and table of the copy is verified first
(`leveldb_check.rs`: CRC32C of every log record and table block, footer, index entries); a
record cut off by Electron writing is re-copied (3 tries); the read runs on its own thread (a
`rusty-leveldb` panic or a 20 s hang becomes an error); a non-empty database that reads empty is
an error. `done` only after that; an error runs again at the next launch (3 launches in all,
then `failed`), and the note has **Retry import**. 300 single-byte mutations of a profile with
tables: 0 panics, 297 reported as damaged, 3 read exactly the undamaged content.

**Not imported** (listed): the open project's path (Electron never persists it), undo history,
in-memory samples (generated / processed; never persisted by Electron either), the MIDI device
grant (Web MIDI permission, no Rust equivalent), Electron's window state (none kept), the
`electron-updater` cache, Chromium caches / cookies / IndexedDB (unused by the app),
`micro-daw-recordings` (defined but never written by the TS). Later Electron changes are not
synced (one-time import).

The key list includes `micro-daw:midi-split-point` since the review (#9). Verified: a profile written by the real Electron 44.5 (`scripts/electron-profile-fixture`, the
fixture in `rune/crates/mdaw-app/testdata/electron-profile`) — every key, UTF-16 and Latin-1 values,
the last write of a key wins, the profile is untouched; and this Mac's own Electron profile through
a copy (`reads_a_real_profile`, `--ignored`): its compacted `.ldb` tables read, `file://`, 2 keys.

## Check for updates

Settings → Software update → "Check now" (and once at start when "Check for updates
automatically" is on, skipped in `MDAW_SMOKE` runs) asks
`https://api.github.com/repos/yemirhan/micro-daw/releases/latest` on a thread (the pack store's
TLS client, 10 s connect; the whole check ends after 30 s, or 15 s without data, and the UI gives
up at 35 s whatever the thread does, review #14). `tag_name` and the app version are compared with SemVer 2.0 precedence
(what `semver.gt` does; invalid versions such as `1.4.0-01`, `1.4.0+` are rejected, long numeric
prerelease ids compare by value). States and texts are Electron's: "Checking for updates…",
"Version X is available", "You're up to date", errors ("No published versions on GitHub" for a
404, "Check failed: …" otherwise). An available version shows **Open release page** (the release's
`html_url` when it is a page of this repository, else `/releases/latest`) instead of "Download &
install"; there is no download and no install on quit (plan §5, decided 2026-11-18).

## Packaging

`rune/scripts/build-mac-app.sh [out]` → `rune/dist/Micro DAW.app` (48 MB, arm64):
release build, private copy of the binary, the icon rendered by the app itself (`app-icon` scenes
on black and white, matted to a transparent PNG by `icon-matte.py`, `iconutil`), `Info.plist`
(`com.yemirhan.micro-daw`, version from the workspace, `LSMinimumSystemVersion` 12.0, music
category, high-resolution, `NSMicrophoneUsageDescription`),
ad-hoc signature. Fonts (Manrope / JetBrains Mono / Syne TTFs), templates, the bundled pack
catalog and the factory sounds are compiled into the binary; nothing is read from the source tree
at run time (the bundle ran from `rune/dist` through LaunchServices). Judgment calls: the bundle
name `Micro DAW` and id `com.yemirhan.micro-daw` differ from the Electron build's
(`micro-daw.app`, `com.electron.micro-daw`) so both can be installed; the icon is the app mark
(Electron ships the default Electron icon).

**Signing / notarization (manual, needs the user's Apple Developer account):**
```sh
MAC_APP_SIGN="Developer ID Application: <Name> (<TEAMID>)" rune/scripts/build-mac-app.sh
ditto -c -k --keepParent "rune/dist/Micro DAW.app" MicroDAW.zip
xcrun notarytool submit MicroDAW.zip --apple-id <id> --team-id <TEAMID> --password <app-specific> --wait
xcrun stapler staple "rune/dist/Micro DAW.app"
```
With `MAC_APP_SIGN` the script signs with the hardened runtime and
`rune/scripts/mac-entitlements.plist` (audio input). No Bluetooth entitlement and no Bluetooth
usage string (review #18): `com.apple.security.device.bluetooth` is an App Sandbox entitlement
(no effect under the hardened runtime without the sandbox), and Bluetooth MIDI controllers reach
the app through CoreMIDI (the system's Bluetooth MIDI driver), which needs neither. The app opens
no audio input today, so no microphone prompt is expected (checklist). A dmg is not made (Forge's
`MakerDMG`): `hdiutil create -volname "Micro DAW" -srcfolder "rune/dist/Micro DAW.app" MicroDAW.dmg`.

**Linux / Windows (build commands only, unverified here):**
- Linux: `cd rune && cargo build --release -p mdaw-app` (needs ALSA and D-Bus dev headers:
  `libasound2-dev`, `libdbus-1-dev`, `pkg-config`; the file dialogs use the XDG desktop portal).
  Binary: `target/release/micro-daw`. No .deb / .rpm (Forge's makers), no `.desktop` file, no
  icon install. Wayland: file drops don't arrive (Rune).
- Windows: `cd rune && cargo build --release -p mdaw-app` (MSVC toolchain). Binary:
  `target\release\micro-daw.exe`. No installer (Forge's Squirrel), no code signing, no icon
  resource.

## Verification

**Runtime tests** (`src/tests/t_p10_project.rs`, `t_p10_shell.rs`, 26 tests, one of them `--ignored`, plus
3 updater unit tests and 2 import unit tests):

| Flow | Test |
|---|---|
| title + dirty dot, top-bar Save → Save As dialog (title, default name, filter), Windows names | `title_follows_the_name_and_the_dirty_marker` |
| clean close goes | `a_clean_project_closes_without_asking` |
| dirty → close → prompt → Cancel (stays, guard re-armed, Escape = Cancel) | `dirty_close_asks_and_cancel_keeps_the_window` |
| dirty → close → Don't Save (force close, nothing written) | `dirty_close_dont_save_closes_without_writing` |
| dirty → close → Save: save dialog cancelled keeps the window; Enter = Save; with a path no dialog | `dirty_close_save_writes_then_closes_and_cancelling_the_dialog_keeps_it` |
| ⌘S / ⇧⌘S / palette Save and Save As, file content, `createdAt` kept / renewed | `save_and_save_as_keys_write_the_project_file` |
| ⌘N while dirty → Don't Save → path → starter, undo cleared, audio config reset + persisted; sidebar New cancelled | `new_project_asks_saves_resets_and_writes` |
| New from template asks + writes | `new_from_template_asks_and_writes`, `t_p9_dialogs::template_picker_filters_and_loads_a_template` |
| ⌘O on `legacy-v1.0.mdaw` (migrations, report with the dropped GM notes, recent), `legacy-v1.3.mdaw` | `open_loads_legacy_files_with_the_report_and_the_recent_list` |
| invalid / missing file → error report, project unchanged | `a_broken_file_reports_and_keeps_the_project` |
| Electron bug 1 | `a_cancelled_open_does_not_swallow_the_next_edit` |
| recent: 12 saves → 10, order, re-open moves to the front, stored shape, sidebar (expanded only), palette group + search by path | `recent_projects_order_limit_sidebar_and_palette` |
| open while dirty asks first (Cancel / Don't Save) | `opening_while_dirty_asks_first` |
| `.mdaw` drop opens, other files don't | `a_dropped_mdaw_opens_and_audio_files_do_not` |
| quit while dirty → next launch reopens path + changes + `*`; clean session clean | `quit_keeps_the_unsaved_session_for_the_next_launch` |
| the review's crash / recovery / durability / IO / import cases | `t_p10_review` (18 tests; see [`review-phase-10.md`](review-phase-10.md)) |
| Rust save → Rust open identical | `a_rust_saved_project_reopens_identically` |
| min size / title / fullscreen button | `the_fullscreen_button_and_min_size` |
| Rust writer = committed golden; Electron's golden opens identically, re-saved identically | `golden_the_rust_writer_output_is_the_committed_fixture`, `golden_a_project_saved_by_electron_opens_unchanged` |
| LevelDB read untouched, last write wins, UTF-16 / Latin-1 | `the_electron_profile_is_read_without_touching_it` |
| import: every key once, Rust values kept, shared packs folder, mark | `the_import_copies_every_key_once_and_shares_the_packs_folder` |
| no profile / broken profile | `no_profile_marks_the_import_done_silently` |
| imported settings reach the app (arrangement, recent, layout, settings), note shown + dismissed | `the_imported_settings_reach_the_app_and_the_note_shows_once` |
| Check now → available → Open release page URL; up to date; error | `check_now_offers_the_release_page` |
| start-up check follows `autoCheckUpdates` | `the_start_up_check_follows_auto_check_updates` |

**Cross-app goldens.** `rune/parity/fixtures/project-saved-by-rust.mdaw` (hip-hop template with
deterministic ids, a marker, a group, an audio region with its manifest entry, master −9 dB, live
FX) is the Rust writer's exact output; `src/services/ProjectManager.golden.test.ts` opens it with
the real `ProjectManager` + `ArrangementEngine`: the arrangement comes back `toEqual` the file, master
volume, the four FX params, the manifest sample, name, recent entry. The same vitest file saves the
same project through the real `projectManager.saveAs()` (mocked ids and clock) into
`project-saved-by-electron.mdaw`; the Rust test opens it (arrangement equal value for value, the only
report line is the manifest sample's missing file) and saves it again unchanged. `UPDATE_GOLDENS=1`
rewrites each side's file.

**Screenshots** (`docs/parity/phase-10/`, `scripts/parity-phase-10/run.sh`, 1440 × 900, scale 1):

| State | Electron | Remaining differences |
|---|---|---|
| `sidebar-recent` | ✓ | labels and rows within 2 points (the sidebar's group labels / separator gaps were fixed to Electron's here; the Project label was phase 7's) |
| `palette-recent` | ✓ | same recent row (name + mono path); cmdk's fuzzy scorer also lists "Toggle Sample Browser" / "Steel Drum" for "lo-fi", the Rust ranker doesn't (phase 9's documented palette ranking) |
| `unsaved`, `unsaved-untitled` | — | Electron's prompt is a native sheet: not drawn by the renderer, and this machine has no Screen Recording permission for a native capture. The Rust prompt mirrors macOS's compact alert (app icon + caution badge, bold message, detail, Save / Don't Save / Cancel stacked). Texts and button semantics asserted by tests; a side-by-side look is on the manual checklist |
| `open-report`, `open-error`, `import-note`, `settings-update` | — | no Electron counterpart (Electron logs load warnings; has no import; shows "Download & install") |

**Real window** (`MDAW_SMOKE=shell`, through `open` on the built bundle):
```
[smoke] shell: title="Untitled * - Micro DAW" import note=Some("Imported 18 settings from the Electron app; its packs folder is shared.") packs dir=…/smoke-profile/packs
[smoke] shell: edited, dirty=true → closing the window
[smoke] shell: after close: prompt=Some(Close) window visible=Some(true) title="Untitled * - Micro DAW"
[smoke] shell: Don't Save
[smoke] shell: closes let through=1 window visible=Some(false); reopening
[smoke] shell: reopened, window visible=Some(true); quitting
```
`open` returned 0 (the app quit through `on_quit`); the data folder holds the imported keys and the
session record. **This run used `MDAW_AUDIO=null`** (new: a window run with the offline engine,
output discarded): during this session the Mac's audio server hung every client opening the
MiniFuse 4 (`afplay` hung too; the app blocked in CoreAudio's `AudioComponentInstanceNew` before
its first frame), so no run with the real device was possible.

## Deviations and gaps (listed, not dropped)

- **Quit cannot be cancelled** (Rune): no prompt on ⌘Q / Dock Quit / logout; the recovery
  checkpoint brings the work back (see "What a quit or a crash keeps" for the exact list). The
  close button / ⌘W / Alt-F4 are guarded.
- **The unsaved prompt is drawn by the app**, not a native sheet (Rune has no message boxes).
- **App menu**: app menu + Edit + Window (Rune's standard menus); no View menu.
- **No auto-install**: "Open release page" instead of download + install on quit.
- **No `.mdaw` file association / Finder "Open With"**.
- **Load report** and **import note**: Rust additions.
- **Session restore** of the open project's path (Electron forgets it).
- **macOS window close** keeps the same state (Electron recreates the window and reloads from
  localStorage: the same arrangement, but undo history and in-memory samples were lost there).
- Packaging: no dmg / deb / rpm / Squirrel makers, no Windows / Linux icons, signing manual.
- Wayland drops (Rune), unchanged.

## Manual checklist for the user

The consolidated list (this phase and its review) is in
[`review-phase-10.md`](review-phase-10.md#verify-by-hand-before-sign-off-consolidated): crash
recovery, ⌘Q with unsaved work, generated samples after a force-quit, the second instance, an
autosave failure, the real MPK (silent while the window is closed), listening, the close guard,
the native sheet look, menus, VoiceOver, signing (no Bluetooth / microphone prompt expected), the
import on your real profile, updates, Windows / Linux.

## Shared files changed (for the merge)

`app.rs` (`project`, `shell` fields; `poll` → `shell_poll`; `shutdown` = `stop_for_close` + pause
installs; `window()` → `window_with`), `lib.rs` (modules; `app_with_hooks(spec, window)` with
`keep_running`, `on_close` → `on_window_close`, `on_reopen`; window size 1360 × 800, min
1360 × 768, title), `main.rs`, `services.rs` (`packs_dir`, `restored_autosave`, the packs folder
from the import, `MDAW_AUDIO=null`), `keys.rs` (phase-10 dialog keys, ⌘S / ⇧⌘S / ⌘O / ⌘N),
`smoke.rs` (`MDAW_SMOKE=shell`), `scenes.rs` (9 scenes appended), `tests.rs`, `ui/mod.rs` (dialogs,
icon scene, root `.mdaw` drop target), `ui/sidebar.rs` (New / Open wired, Recent group, group
labels and separator gaps as Electron's), `ui/topbar.rs` (project name, Save on click, full-screen
icon), `ui/p9/dialogs.rs` (project + recent palette commands), `ui/p9/settings.rs` (update row),
`views.rs` (`new_from_template` through the project service), `tests/t_p9_dialogs.rs` (template
picker now saves); `mdaw-engine/src/engine.rs` (re-export `MASTER_REFERENCE_DB`);
`mdaw-model/src/migrate_project.rs` + `settings.rs` (`base_name`) and their tests;
`rune/Cargo.lock`, `mdaw-app/Cargo.toml` (`rusty-leveldb`). TS: `src/hooks/useProject.ts`,
`src/main.ts`, `src/services/ProjectManager.ts` (+ new `src/main/closeGuard.ts` and tests).
