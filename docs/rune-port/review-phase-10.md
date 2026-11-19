# Phase 10 review (merge 4d24e26): findings and fixes

| Reviewer | Data safety | Parity | UI / UX | Docs |
|---|---|---|---|---|
| Opus 5.5 | 5 | 7.5 | 7 | 5 |
| GPT-6.1 Sol | 5 | 7 | 7 | 6 |

**The core problem (both reviewers, reproduced):** the phase doc said a quit that can't be
cancelled was safe because "the autosave and the session record are written together". They
weren't: the arrangement was autosaved every 300 ms, the session record (path, dirty) only on
close / quit. After a crash the next launch paired the newest arrangement with an old record: B's
arrangement came back under A's path, clean, and ⌘S wrote it over A. Unsaved edits came back
clean; an untitled unsaved project came back clean even after a normal quit. The fix is a
recovery **checkpoint** (`rune/crates/mdaw-app/src/recovery.rs`): the arrangement and its record
in one store write, at every autosave and every project transition, validated on restore.

**End-of-port assessment** (summarized from the review brief: the scores and the findings each reviewer filed): both rated data safety 5/10, the
lowest axis, for the reasons above plus unacknowledged autosave failures, two instances
overwriting each other's store, a panicking / silently partial Electron import and non-durable
writes; both rated parity (7–7.5) and UI (7) as close to Electron with listed gaps (uncancellable
quit, app-drawn prompt, menus, Dev monitor, start-screen dialogs); docs (5–6) were marked down for
claims the code didn't back ("Nothing is lost", the menu limitation, a sandbox-only entitlement,
statuses ✅ without evidence). Neither recommended deleting `src/` yet; Sol named the remaining
`src/` dependencies of the Rust tree as the prerequisite (finding 19).

"Before" is the reviewers' reproduction, or what the new test asserts against the old code path;
"after" comes from the named tests. Paths are under `rune/crates/mdaw-app/src/` unless noted.
New tests: `tests/t_p10_review.rs` (18), `updater.rs` (1 + more cases), `instance.rs` (1),
`tests/single_instance.rs` (1, two processes), `mdaw-model` `fsio.rs` (5), `settings_tests.rs`
(2), `templates_data.rs` (1); TS: `useProject.test.ts` (1), `ProjectManager.test.ts` (1).

After the fixes:
- `rune/scripts/check-without-src.sh` (the repository's `src/` renamed away, every crate rebuilt,
  `cargo test --workspace --no-fail-fast`): **1,406 passed, 0 failed, 26 ignored**. With `src/`
  in place: **1,406 passed, 0 failed, 26 ignored** (`templates_match_the_electron_copies` then
  compares the copies). One earlier full run had `t_perf_song_with_48_tracks_and_a_32k_note_selection`
  fail its relative bound under parallel load (43.1 vs 19.8 ms); alone it passes every time (17 vs
  10.5 ms, three runs) and the rerun of the whole workspace was clean.
  `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all --check` clean.
- `npm test`: **935 passed**; `tsc --noEmit` clean.

Electron bug fixed in both apps (user decision 2026-11-18): finding 15.

| # | Finding | Fix | Test / evidence (before → after) |
|---|---|---|---|
| 1 | **Critical** (both): crash recovery pairs B's arrangement with A's path, shown clean; ⌘S overwrites A (`shell.rs:121-160`, `services.rs:378-426`) | `recovery.rs`: every autosave (300 ms debounce) and every transition (open, save, save as, new, a failed write) writes **one checkpoint**: `micro-daw-arrangement` + `micro-daw-rune:session` v2 (`filePath`, `createdAt`, `fileModifiedAt`, `dirty`, `arrangementHash` = FNV-1a 64 of the arrangement JSON written with it, `gen`, `savedAt`, live FX, recovery assets) through `SettingsStore::set_items` (one file write). Restore trusts the record only when the hash matches the autosaved arrangement, and the path only when the file still has `fileModifiedAt`; otherwise the work comes back **Untitled + unsaved** with a "Recovered work" note | `a_crash_after_opening_another_project_reopens_that_project_not_the_first` (A saved, B opened + edited, dropped without hooks: **before: B's arrangement as "A", clean → after: path B, bpm 133, `B * - Micro DAW`; ⌘S writes B, A keeps 101**); `a_checkpoint_that_doesnt_match_its_arrangement_restores_untitled_and_unsaved` (a foreign arrangement write: Untitled, dirty, note, ⌘S opens Save As); `a_file_changed_on_disk_since_the_checkpoint_is_not_overwritten` (another `modifiedAt` / a deleted file → Untitled, dirty, note naming "A") |
| 2 | High (both): after a crash, unsaved edits return clean | Same checkpoint: `dirty` is written with the arrangement it describes | `unsaved_edits_survive_a_crash_as_unsaved` (**clean → dirty with its path**; a clean project still comes back clean) |
| 3 | High (both): an untitled dirty project returns clean after a quit (`shell.rs:149` early return) | Restore applies `createdAt` / dirty without a path | `an_untitled_unsaved_project_comes_back_unsaved_after_a_quit_and_a_crash` (quit and crash: **clean → `Untitled * - Micro DAW`**, and the close guard asks) |
| 4 | High (Sol): failed autosaves acknowledged and never retried (`services.rs:398` cleared pending before writing, `save_now` dropped errors) | `SettingsStore` keeps a generation per change and a `WriteStatus` (`durable`, `failed`); `Services` holds the checkpoint in flight `(generation, doc version)` and acknowledges it only once `durable ≥ generation`; a failure keeps it pending, shows **"Autosave failed"** (alert card) and retries every 2 s (`AUTOSAVE_RETRY`). The store's writes go through `fsio::write_atomic` (file + folder fsync) and keep the replaced file as `local-storage.json.bak` (second slot; `open` falls back to it when the main file is missing or unreadable) | `a_failed_autosave_is_shown_kept_pending_and_retried` (folder replaced by a file: error kept, `save_pending`, note shown, still failing after a retry; folder back → written, note gone, store holds bpm 123); `mdaw-model` `write_status_tracks_generations_failures_and_retries`, `store_persists_atomically_and_reloads` (corrupt main → the `.bak` slot) |
| 5 | High (both): two instances overwrite each other's whole store (`settings.rs:121,210`, `main.rs:11`) | `instance.rs`: before opening anything, a window run takes an exclusive `File::try_lock` (flock / LockFileEx) on `<data dir>/instance.lock`, held until exit (released by the OS on a crash). A second launch prints the message and opens a small "Micro DAW is already running" window with Quit, without touching the store. `--instance-probe` (test hook) | `instance::tests::a_second_lock_on_the_same_folder_is_refused_until_the_first_is_dropped`; `tests/single_instance.rs` runs the **real binary** against a folder this process locked: `busy` / exit 3, then `acquired` once released. Live: a second `micro-daw` on the first one's temp data folder printed the message and never opened the store (the refusal comes before `SettingsStore::open`). Its window was not seen by System Events here (unverified on screen) |
| 6 | High (Sol): recovery loses generated / processed samples and live FX (reverb 0.7 → 0.12) (`shell.rs:208`, `samples.rs:830`) | The checkpoint writes each timeline sample that has no file (generated / processed, `samplePath ""`) as a 32-bit float WAV into `<data dir>/recovery/<id>/<name>.wav` (IO worker, `fsio`), lists it in `assets` once written, and restore loads it by id (name from the file); copies no record lists are deleted at start. The four live-FX parameters a project file restores (reverb, chorus, cutoff, resonance) are in `audio`. Still not recovered (listed in the phase doc): samples only in the Samples library, the other 16 effect params (a project file doesn't restore them either), undo, the last ≤ 300 ms of edits | `timeline_generated_samples_and_live_fx_survive_a_restart` (record lists the asset; next launch: 4,800 frames back under the same id and name, **reverb 0.7 instead of 0.12**) |
| 7 | High / Low (both): quit abandons a running export; `fs::write` can truncate an existing destination (`views.rs:632-641`, `app.rs:783`) | Exports go through `fsio::write_atomic` (temp + fsync + rename + folder sync: the old file survives a failure). The export carries a cancel flag; `shutdown` waits up to 10 s for it (`finish_export_for_quit`), then cancels: the render's result is dropped, nothing is written | `an_export_replaces_the_file_atomically_and_a_quit_cancels_one_that_runs_on` (existing file replaced by a MIDI file, no temp left; a never-ending export is cancelled at quit, flag set, `running` cleared). Residual: a render can't be interrupted; it ends with the process |
| 8 | High (Opus): the Electron import can panic (`rusty-leveldb` asserts: 385 / 3,000 mutations) and crash every launch; temp copy leaked; (Sol) a flipped WAL byte read as `Ok([])` and the import was marked done (`electron_import.rs:93,113,173-245`) | `electron_import.rs` + new `leveldb_check.rs`: the mark is written **`in-progress` (attempt n) before reading**; the copy is a drop-guarded `TempCopy`; every log (`*.log`, `MANIFEST-*`: record CRC32C, type, length) and table (`*.ldb`: footer magic, metaindex / index / every data block in bounds with its CRC, snappy, entries well formed) is verified first; a record cut off mid-write is *transient* (re-copied, 3 tries); the read runs on its own thread (a panic or a 20 s hang becomes an error; its panic message is not printed); a non-empty database that reads empty is an error. `done` only after that; `error` runs again next launch, 3 launches in all, then `failed`; the note has **Retry import** (`import_retry`) | `mutated_profiles_never_panic_or_read_partially` (300 single-byte mutations of a profile with `.ldb` tables, log and manifest: **0 panics, 297 reported as damaged, 3 read exactly the undamaged content, 0 partial reads**); `the_import_detects_a_damaged_log_instead_of_importing_nothing` (**Sol's repro: `Ok([])` + done → "damaged", mark `error`, attempt 1**); `an_import_that_crashed_is_counted_and_given_up_after_three_launches` (in-progress 3 → `failed` once with a note, then nothing; Retry imports 19 keys; in-progress 1 → retried, `done` attempt 2) |
| 9 | Medium (Sol): `micro-daw:midi-split-point` (both apps persist it) not imported | Added to `mdaw_model::settings::ALL_KEYS` (now 20; `mdaw-midi` uses the model's constant), the module's key table, inventory §4.6; the fixture regenerated **with the real Electron** (`scripts/electron-profile-fixture` writes `'52'`) | `the_import_copies_every_key_once_and_shares_the_packs_folder` (18 imported, split point 52); `a_compacted_profile_imports_the_midi_split_point` (tables) |
| 10 | Medium (both): `write_atomic` (`project.rs:446-457`, the store too): no fsync, symlinks replaced, permissions lost, no fallback, stale temps | `mdaw_model::fsio::write_atomic`: follows symlinks (replaces the target, keeps the link), gives the temp the old file's permissions, `sync_all` the file, renames, syncs the folder; an unwritable folder with a writable file falls back to an in-place write (truncate + write + sync; Electron always writes in place); temps `.<name>.tmp-<pid>-<n>` older than 10 min are removed once per folder per process. Used by project files, the settings store, exports and recovery WAVs | `fsio::tests` (`replaces_the_file_and_leaves_no_temp`, `follows_symlinks_and_keeps_permissions` (link kept, 0640 kept), `an_unwritable_folder_falls_back_to_writing_in_place` (0555 folder), `stale_temp_files_are_removed`, `a_missing_folder_is_an_error`) |
| 11 | Medium (both): New / template marked a nonexistent path clean before writing (`project.rs:240-254`) | The new project is Untitled and unsaved until its write succeeds; `finish_write` then sets path, `createdAt`, clean, recent list. Save As likewise commits the path only after the write | `new_into_a_missing_folder_stays_untitled_and_unsaved_with_the_error` (**path taken + clean → `Untitled * - Micro DAW`, "Couldn't save project" shown, no file**) |
| 12 | Medium (Sol): a restored dirty project's close prompt is invisible on the start screen (`ui/mod.rs:100`, `keys.rs:89`) | The start screen view draws `ui::shell::layers` (notes, load report, unsaved prompt) over itself; `dialog_keys` runs before the start screen's any-key handling | `a_restored_unsaved_project_prompts_over_the_start_screen` (close on the start screen: prompt drawn, Escape cancels without starting; a load report shows and Enter closes it, still on the start screen) |
| 13 | Medium (Sol): project IO blocks the UI thread (`project.rs:289,321`; autosave rewrote settings synchronously) | `io.rs`: one worker thread for project writes / reads (read + migrate + coerce + manifest file checks) and recovery WAVs; the UI builds an immutable snapshot (JSON, doc version) and applies the result in `shell_poll`; clean is set to the **version written**, so an edit made during the write keeps the project dirty; one operation at a time (`project.busy`). The settings file is written by its own thread (`SettingsStore::use_background_writer`, newest content wins) in window runs; quit waits for both (5 s). Headless runs keep the inline worker | `saves_and_loads_run_on_the_io_worker_and_an_edit_during_a_save_stays_unsaved` (threaded worker: submit returns at once, file has the snapshot's 125, dirty because of the 126 edit; a threaded load applies); `mdaw-model` `the_background_writer_writes_the_newest_content` (50 writes → generation 50 durable, file "49"). Residual: the JSON serialization of the arrangement and of the store still runs on the UI thread; the start-up restore reads the project file synchronously |
| 14 | Medium (both): update check can hang forever (`updater.rs:168-175,238`); Low: SemVer parser accepts `1.4.0-01`, `1.4.0+`, `1.4.0+a..b`, mis-orders huge numeric ids | The fetch's check (polled by every wait of `mdaw-packs` `http.rs`: DNS, connect, TLS, headers, body) ends it after 30 s in all or 15 s without data; the UI gives up at 35 s whatever the thread does ("Check failed: timed out") and "Check now" works again. The parser follows SemVer 2.0 (no leading zeros in numeric prerelease ids, non-empty `[0-9A-Za-z-]` build identifiers); numeric ids compare by length then digits (no `u64` overflow) | `a_stuck_check_times_out_and_can_run_again` (a fetcher sleeping an hour: **Checking forever → error at the deadline, next check available**); `semver_precedence_matches_the_spec_order` (+13 rejected, 5 accepted edge cases, 20-digit ids ordered) |
| 15 | Medium (Sol, **Electron bug, both apps**): `useProject.ts:30,58` suppressed dirty from the start of Open / New, before its dialogs: an edit (a MIDI recording) during a pending Open was swallowed | TS: `ProjectManager.isReplacingProject()` is true only around the synchronous replacement (`loadProject` / `resetToStarter` / `loadFromTemplate` + `undoManager.clear()`); `useProject` skips only those changes (`skipDirtyRef` removed). Rust already compares state versions; the behaviour is now asserted | TS `useProject.test.ts` › "an edit while the open dialog is still showing marks the project dirty" (**fails on the old hook: `markDirty` 0 calls → 1**), `ProjectManager.test.ts` › "flags only the replacement itself" (false while the file is read, true inside `loadProject`); Rust `an_edit_while_the_open_dialog_is_pending_marks_the_project_unsaved` |
| 16 | Medium (Sol): a referenced sample that exists but can't be decoded produced no report (`project.rs:353`, `samples.rs:763`) | `Samples::region_failures` carries async decode failures; the load records its manifest samples in `pending_samples` (reset per load, `load_gen`), and a failure of one of them is appended to the report (or opens one) | `a_manifest_sample_that_cannot_be_decoded_is_in_the_load_report` (a corrupt `RIFF` file: **no report → "could not load sample \"Broken\" from …: …"**) |
| 17 | Low (Opus): with the window closed (macOS) MIDI kept sounding; ⌘Q from the Dock while windowless said to skip `pause_for_quit` | `Router::set_suspended` (a third silence flag, like the mutes): the window's close releases notes and suspends input; `on_reopen` resumes. Quit: by the pinned Rune's code a hidden (`keep_running`) window is not `Finished`, so `on_quit` runs (`window/any_host.rs:131`); `shutdown` runs its cleanups with or without a window and records it | `a_closed_window_suspends_midi_and_a_windowless_quit_still_runs_its_cleanups` (suspended after close, resumed on reopen; close + quit → `quit_paused`). The Dock quit itself was not driven (no automation of the Dock here) |
| 18 | Low docs / bundle: `com.apple.security.device.bluetooth` is a sandbox entitlement; Rune's menus; statuses | Bluetooth entitlement and the unused `NSBluetoothAlwaysUsageDescription` removed (Bluetooth MIDI comes through CoreMIDI: no entitlement, no Info.plist key); a mic-prompt item added to the checklist. **Menus:** the pinned Rune installs the standard Edit (Undo, Redo, Cut, Copy, Paste, Select All) and Window (Minimize ⌘M, Close ⌘W) menus by itself for a regular app whose menu bar is still winit's (`native_menu/standard_macos.rs:29-37`), and the keys still reach the app's handlers first; **seen in the running app**: System Events listed the menu bar `micro-daw, Edit, Window` and Edit › Undo, Redo, Cut, Copy, Paste, Select All (+ the system's Writing Tools / AutoFill / Dictation / Emoji). Phase doc, plan §5.8 and parity rows corrected; statuses reconciled (below); "Nothing is lost" replaced by what is and isn't recovered | `rune/scripts/mac-entitlements.plist`, `build-mac-app.sh`; `docs/rune-parity.md`, `phase-10-app-shell.md`, `rune-port-plan.md` |
| 19 | Prerequisite for deleting `src/` (Sol): `views.rs:348`, `scenes_song.rs`, Rust tests read `src/data/templates` | The six templates are copied into `rune/crates/mdaw-model/assets/templates/` and embedded from there (`mdaw_model::templates_data::TEMPLATE_FILES`), used by the app, the scenes and the model / engine tests; `templates_match_the_electron_copies` keeps them byte-equal while `src/` exists. `rune/scripts/check-without-src.sh` hides `src/`, rebuilds every crate and runs the workspace tests | The script's run: **1,406 passed, 0 failed** with `src/` renamed away. The plan lists what still needs `src/` (TS goldens, parity harness, Chromium captures) and the condition for deleting it |

**Parity table statuses reconciled** (`docs/rune-parity.md`): "Recent projects, autosave/localStorage
restore" and "Import of an existing Electron profile's settings" are ✅ with this review's tests
as evidence; "MIDI monitor" and "Dev view" are 🟡 (no "Jump to latest", scrolling away isn't
detected); Slicer, Linear recording, Top bar and Instrument dock were 🟡 while their notes said
every part was done: now ✅ (the real-MPK checks stay on the manual list); the Window row's menu
text and the Close guard row's recovery text corrected; keys "all 20".

**Shared-file changes** (for the merge): `app.rs` (`io`, `recovery_dir`, `recovery_assets`;
`poll` → `autosave_tick`; `shutdown` drains IO, waits for the export and the store, records
`quit_paused`), `lib.rs` (modules `instance`, `io`, `leveldb_check`, `recovery`; the IO waker),
`main.rs` (instance lock, `--instance-probe`), `services.rs` (autosave scheduling / ack instead of
writing), `shell.rs` (session code moved to `recovery.rs`, MIDI suspension, `retry_electron_import`,
store background writer), `keys.rs`, `ui/mod.rs`, `ui/shell.rs` (layers over the start screen,
notes), `samples.rs` (`region_failures`), `views.rs` (templates from `mdaw-model`, atomic +
cancellable export, decode failures to the report), `scenes_song.rs`, `Cargo.toml` (`crc32c`,
`snap`); `mdaw-model` (`fsio.rs`, `templates_data.rs` + `assets/templates/`, `settings.rs`
store durability + `MIDI_SPLIT_POINT_KEY`, tests); `mdaw-midi` (`router.rs` suspension,
`settings.rs` key constant); `mdaw-engine` `synth/preset_tests.rs`; `rune/scripts/`
(`check-without-src.sh`, entitlements, bundle script); fixture
`testdata/electron-profile` (regenerated). TS: `src/services/ProjectManager.ts`,
`src/hooks/useProject.ts` and their tests; `scripts/electron-profile-fixture/main.cjs`.

**Still open / not fixed:**
- A quit still can't be cancelled (Rune); the checkpoint makes it recoverable, not askable.
- The last ≤ 300 ms of edits before a crash, undo history, Samples-library-only generated samples
  and 16 of the 20 effect params are not recovered (listed in the phase doc).
- The autosave's JSON serialization (arrangement + store) still runs on the UI thread; only the
  file IO moved. The start-up restore reads the project file synchronously.
- An export's render can't be interrupted at quit (cancelled = its result dropped).
- The second-instance window was not seen on screen in this session (the lock refusal and the
  message were).

## Verify by hand before sign-off (consolidated)

1. **Crash recovery**: open a saved project, edit, force-quit the app (`kill -9`, or Activity
   Monitor › Force Quit); relaunch: same project, `*`, the edit present. Then: open A, open B, edit
   B, force-quit; relaunch shows B (never A). Edit a project in another app (or touch its JSON's
   `modifiedAt`) between a force-quit and the relaunch: it must come back Untitled with the note.
2. **⌘Q with unsaved work** (no prompt, Rune): relaunch → `*`; an untitled project comes back
   `Untitled *`, and closing the window then asks.
3. **Generated sample on the timeline**: Generate a tone, Send to arrangement, set Reverb to 0.7,
   force-quit; relaunch: the region plays and Reverb reads 0.7.
4. **Second instance**: with the bundle running, launch the binary again
   (`open -n "rune/dist/Micro DAW.app"`): a small "Micro DAW is already running" window, Quit;
   the first app's work untouched.
5. **Autosave failure**: make `~/Library/Application Support/micro-daw-rune` read-only (`chmod
   a-w`), edit: "Autosave failed" appears; restore the permission: it goes away within 2 s.
6. **Real audio + MPK mini Play mk3**: Start, pads / keys / record a loop; close the window (macOS):
   the MPK must be silent; reopen from the Dock: it plays again. Save, quit, reopen from Recent.
7. **Listening**: an Electron-saved project vs Electron by ear; master volume and live FX.
8. **Close guard by hand**: red close button → prompt; Cancel / Don't Save / Save (with and
   without a file); Save into a folder you can't write: the error, the window stays.
9. **Native sheet look**: the app-drawn prompt next to Electron's.
10. **Menus**: Edit › Undo / Redo undo the app's last edit; ⌘C / ⌘V in a text field; Window ›
    Minimize / Close (Close goes through the guard).
11. **VoiceOver**: the prompt, load report, notes (Autosave failed is an alert), Recent items,
    palette recent rows.
12. **Signing**: Developer ID + notarize; on a second Mac: Gatekeeper, MIDI (no Bluetooth or
    microphone prompt expected: the app opens no audio input; if macOS asks for the microphone,
    report it).
13. **Import on your real profile**: the note, Recent, browser roots, kits, the split point,
    installed packs shared. A damaged copy shows "damaged" with Retry import.
14. **Updates**: once a release newer than 1.4.0 exists, "Check now" offers it; offline, the check
    ends within ~30 s with an error and can be retried.
15. **Windows / Linux** builds (commands in the phase doc) were not run.
