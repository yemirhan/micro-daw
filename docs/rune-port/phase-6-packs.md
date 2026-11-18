# Phase 6: packs (`rune/crates/mdaw-packs`) vs the Electron app

**Scope** (plan §4 row 6, inventory [§2 Pack store](inventory/main-packs-midi-project.md#2-pack-store) and the `packs:*` channels of §1.6):
- `src/main/packs/download.ts`: allowlisted, size-capped, sha256-verified downloads, manual redirects, idle timeout, retries.
- `src/main/packs/catalog.ts`: remote catalog with timeout, validated, merged over the bundled copy.
- `src/main/packs/installer.ts`: staging, resume, atomic swap, crash recovery, pause on quit, uninstall, `pausedIds`, `listInstalled`.
- `src/main/packs/preview.ts`: the remote preview fetcher (LRU, in-flight sharing).
- `src/main/packs/ipc.ts` + `quit.ts`: the store API behind the `packs:*` channels, startup cleanup, `packs:states`, pause on quit.
- `src/services/PackService.ts`: the renderer state machine: install states from progress events, installed-list refresh with buffer invalidation, uninstall errors, previews, **Get & load**.

Validation, catalog merge and the pure service reductions were ported and golden-tested in phase 1 (`mdaw_model::packs`); this crate reuses them unchanged.

## Method

**Design.**

| Module | Port of | Notes |
|---|---|---|
| `http.rs` | Node `fetch` (undici) | A small blocking HTTP/1.1 GET client over rustls (`webpki-roots` trust anchors). Every socket read waits at most 20 ms, then asks the caller whether to give up (see deviation 1) |
| `signal.rs` | `AbortController` / `AbortSignal.timeout` | First reason wins: `Cancel`, `Paused` (`InstallPausedError`), `Timeout`, `Error(msg)` |
| `download.rs` | `download.ts` | Same order of checks and the same messages and `retryable` classes; `DownloadError::{Failed, Timeout, Aborted}` |
| `catalog.rs` | `catalog.ts` | `packs/catalog.json` compiled in (`include_str!`) like the Vite main build |
| `installer.rs` | `installer.ts` | One thread per install, `concurrency` download threads in a scope (the install settles after every worker, B2). Fault hooks (`fault.rs`) stand in for the vitest `fs` spies |
| `json.rs` | `JSON.stringify(v, null, 2)`, `toISOString` | pack.json is written with JS number formatting |
| `preview.rs` | `preview.ts` | 24 entries / 48 MiB LRU, ≥ 1 entry kept, in-flight sharing, 20 s timeout |
| `store.rs` | `ipc.ts`, `quit.ts` | `PackStore`: blocking calls taking only an id or a preview URL (`assertId`, catalog-only preview URLs kept); cleanup on a thread at construction, awaited by list / install / uninstall / states |
| `service.rs` | `PackService.ts` | `PackService` on the UI thread; store calls run on worker threads and report through a channel applied by `poll`. `PackHost` is what it needs from the app (implemented for `ArrangementEngine`) |

**On disk: the same layout and files.** `<packs>/<id>/`, `.partial-<id>`, `.old-<id>-<8hex>`, `.del-<id>-<8hex>`, legacy `.tmp-*`; pack.json `{id, version, dir, installedAt, manifest}` pretty-printed with 2 spaces. A packs folder written by either app is read, resumed, updated and cleaned up by the other (live test below).

**Threads.** The UI thread never blocks on IO: `PackService::install / uninstall / load_catalog / refresh_installed / preview_*` start a worker and return. Install progress (`on_state`, throttled to 100 ms) is sent to the same channel as the call results, so the order of `packs:progress` events and replies is the order Electron delivers them. Previews decode on the worker (`PadBufferCache::decode`, device rate) and play on a new engine slot; the audio thread only renders.

**Engine addition** (`mdaw-engine/src/engine/preview.rs`): `ArrangementEngine::play_preview / stop_preview / is_preview_playing`. A `PreviewVoice` on its own live-only slot: the buffer at gain 0.9 × −6 dB (the TS `ToneBufferSource` gain into `Tone.Volume(-6)`), stopped with a 10 ms linear fade; a new preview replaces the slot's instrument. `MAX_SLOTS` 258 → 259 for that slot; `MAX_TRACKS` stays 256.

**Get & load.** `get_and_load(pack, kit, host)` captures `{active drum track, its kit id, session}`, installs when the pack is missing or older than the catalog, then:
- drops the load if the request was superseded, cancelled or the target no longer holds;
- waits while a take records (`waiting_for_recording`), loading after it stops;
- applies with `KitSelectionSource::PackLoad` and reports a `KitLoadedEvent`.

The TS watches engine events (`onStateChange`, `onKitChange`, `onKitSelected`). The Rust app calls `PackService::check_target(host)` after engine edits (`poll` does it every frame) and forwards `ArrangementEngine::take_kit_selected()` events to `on_kit_selected` (see deviation 4).

## Parity evidence

| Area | Comparison | Result |
|---|---|---|
| pack.json bytes | `rune/parity/ts/packs_io.golden.ts` → `packs_io.json`: `JSON.stringify(installed, null, 2)` of all 4 real catalog packs, the test manifest and one with escapes (`"`, `\u0001`, `\n`, `\t`, U+2028, é), CC-BY attribution and pad settings `-3.5`, `1.0`, `1e21`, `1e-7`, `null`, `true` | **6 / 6 byte-identical** |
| Downloads | Same fixture: 32 scripted response sets fed to the real `downloadToBuffer` / `downloadToFileWithRetry` (fake fetch) and replayed by Rust on a local HTTP server: ok with / without content-length, empty body, upper-case expected sha, 403 / 404 / 408 / 429 / 500 / 503 (with per-hit sequences), redirects (absolute, relative, root-relative, 5 hops ok, 6 too many, no location, to another host, to `http:`, to port 8443), disallowed initial URLs (scheme, host, credentials, garbage), declared and streamed oversize, exactly the cap, size and checksum mismatch (retried), stalls (idle timeout, retried, then ok) | **32 / 32 identical**: outcome (bytes, sha256, file bytes / message, `retryable`, error class) and the requested URLs in order (retry counts included) |
| Catalog | 12 remote bodies over the real bundled catalog through the real `fetchCatalog`: offline, invalid JSON, invalid envelope, not newer, unparseable date, newer with bumped / older versions, newer but empty, one withdrawn, one invalid (bundled kept), a new pack, over 5 MiB | **12 / 12 identical** (source, updatedAt, pack ids@versions in order) |
| Installer | 18 cases / 36 steps on the real `PackInstaller` (concurrency 1 and 4) over seeded folders: fresh install, resume after 404, tampered staging + junk + stale pack.json, checksum retries, 503 ×2 then ok, oversize, redirects, update then failed update, invalid manifests, S5 restore, newest of 3 backups (one with an unparseable date), M1 invalid present folder, obsolete backup + staging expiry (8 vs 6 days) + invalid folders, uninstall (twice, invalid id), `pausedIds` layouts, `listInstalled` order (`localeCompare`: `Alpha`, `alpha 2`, `Äpfel`, `beta`, `Beta`, `zeta`) and validity (id mismatch, bad JSON, no pack.json, version fallback) | **36 / 36 steps identical**: outcome, state events (runs of `downloading` with one `fileIndex` collapsed to the last, see deviation 2; in the concurrency-4 case only the last `downloading` event and the others, since parallel progress interleaves by timing in both apps; its URLs are compared as a set), requested URLs, and the whole folder tree after the step (names with hex suffixes masked; every file's content; pack.json with `dir` / fresh `installedAt` masked) |
| Ported vitest suites | `installer.test.ts` (PackInstaller, downloads / previews / catalog, fault injection L-f) → `tests/installer.rs` against a real local HTTP server; fs spies → `FsFaults` hooks (rename, open, writeFile, slow close for B2) | 32 tests pass, incl. B2 (immediate Retry during a slow close), M1 both cases, failed swap, ENOSPC on open (not retried), failed pack.json write, pause keeps staging, cancel removes it, stalled reads cancelled in < 1 s |
| | `PackService.test.ts` (`packService`: progress, errors, previews, P1, P2, uninstall errors, Get & load M2 + N2, pack names N6, L-c) and `quit.test.ts` → `tests/service.rs` with the real store, a local server and the real `ArrangementEngine` as host (the TS mocks the API and the engine) | 34 tests pass. N2 cases run on real doc edits: kit chosen / re-chosen by the user, project reset, pad edit keeps, active drum track switch, session change, null target (loads creating a track; created / switched tracks drop), recording waits (forced, R3 restart, and a **real** loop take on the engine), cancel while waiting. The loaded kit equals `kitFromPack` and its pads decode from disk |
| Kill mid-install | `tests/kill.rs`: the install runs in a child process that is SIGKILLed (a) while a file streams, (b) between the two renames of an update | (a) next start: staging kept, `pausedIds = [test-pack]`, the resume downloads only the missing file. (b) next start: `cleanupStale` restores v1 from `.old-*`, the staged v2 resumes with **no** download |
| Live catalog install | `tests/live.rs` (`--ignored`): the real catalog URL over TLS (136,291 bytes, updatedAt 2026-11-18, not newer than the bundled copy, so both apps used the bundled one), then `uzu-drumkit` v2 (30 files, 1.33 MB) installed by the Rust store and by the Electron `PackInstaller` (`live_install.golden.ts`, real Node fetch) | **30 / 30 files byte-identical**; pack.json bytes identical apart from `dir` and `installedAt`. Electron lists the Rust folder (right `dir`, `installedAt`) and verifies all 30 files; Rust lists the Electron folder, verifies all 30 and decodes its kit's 16 pads. Each app resumes the other's staging folder with **0** requests |
| Preview playback | `engine/preview.rs` unit + engine tests | gain 0.9 × −6 dB on both channels until the buffer ends; stop = linear fade to 0 over 10 ms; transport stop does not cut it; `is_preview_playing` clears at the buffer end (`onended`) |

Golden fixtures are regenerated with `npx vitest run --config rune/parity/vitest.golden.config.mts` (`packs_io.json`; regenerating the whole set changes no other fixture). The live test: `cd rune && cargo test -p mdaw-packs --test live -- --ignored --nocapture` (network + `npx`).

## Electron bugs

None found in this phase. The standing decision (fix in both apps) had nothing to apply to; `src/` is unchanged.

## Deviations (what the user notices)

1. **HTTP client.** The plan named ureq. The download rules need a body read that a cancel, a pause or the idle timer can interrupt at any time (the TS aborts the fetch and cancels the reader), which ureq's blocking reads cannot do without a thread per read. The crate has its own minimal HTTP/1.1 client over rustls instead (GET only, `Connection: close`, content-length / chunked / until-close bodies). Effects:
   - A cancel or pause takes effect within 20 ms during the TLS handshake, the wait for the response head and the body. **DNS resolution itself is not interruptible** (OS resolver): a cancel during a slow lookup waits for it.
   - No `Accept-Encoding`: bodies come uncompressed. Node's fetch asks for gzip and decompresses, so the byte counts and hashes the checks see are the same.
   - Transport errors use Node's texts (`fetch failed` before a response, `terminated` when a body breaks off). OS errors use Rust's texts (`No space left on device (os error 28)` instead of `ENOSPC: no space left on device, open '…'`); they are shown verbatim in the pack card.
   - Trust anchors: `webpki-roots` (Mozilla's list) instead of Node's bundled copy of the same list. No proxy support, as in Node's fetch.
2. **Progress granularity.** `downloading` events are emitted per network read in both apps; reads are split differently (Node stream chunks vs 64 KiB reads), so the number of intermediate events differs. Start, resume, per-file, retry and final events are the same (golden compares them with runs collapsed).
3. **Installed-list refresh order.** Each listing carries a sequence number and an older listing arriving after a newer one is ignored. The TS applies whatever arrives last (a stale list could win a rare race between the `installed` progress refresh and the install reply). Hardening, not a behaviour change in normal use.
4. **Get & load target watch is polled.** The TS reacts synchronously to each engine event; Rust checks the target after engine edits and every frame. A change undone within one frame (active drum track A → B → A) keeps the request where the TS drops it. Explicit kit choices are still caught: their `user` kit-selection events are forwarded (`on_kit_selected`), as `onKitSelected` does.
5. **Preview errors** are kept in `snapshot().preview_error` (key, message) instead of rejecting a promise.
6. **No IPC.** The trusted-sender check has nothing to guard (plan §5); `assertId` and the catalog-only preview URL rule are kept on the store API. `packs:fetch-catalog` keeps its semantics: every call refetches, concurrent calls share one fetch, failures fall back to the bundled copy.

## Verified / not verified

**Verified here (this Mac):** everything in the evidence table, including the live install against the real catalog and GitHub over TLS, a real process kill at two points, and both directions of folder compatibility.

**Not verified:**
- **Windows and Linux.** Paths go through `std::path` and the validator rejects Windows-reserved names, but neither platform was run. On Windows, renaming a folder with an open file fails; the TS has the same exposure, untested in both.
- **Real network failures.** 5xx, redirects, stalls, truncation and timeouts are simulated on a local server (and the TS side by the fake fetch); the live run had none. A truncated body with a declared length is a retryable `terminated` in Rust (as in real undici); the TS fake fetch cannot produce one, so that case is not in the golden.
- **A slow or failing DNS** (see deviation 1), IPv6-only networks, captive portals.
- **Full-size packs.** The biggest pack installed live was 1.33 MB; the 500 MiB / 5000-file limits are tested with the validator and small files only. Release-build throughput not measured.
- **Preview sound vs Chromium.** The preview voice's gain and fade follow Tone's code; no render comparison against Chromium was made.
- **Kit preview (the 2-bar demo groove) and remote kit loading** (`kitPreview.ts` playback, `remoteKit.ts` 32-entry decoded LRU): the pure parts are in `mdaw_model::packs::kit_preview` (phase 1); scheduling the groove on the engine comes with the Packs page (phase 9).
- **UI wiring.** The Packs page, pack detail, missing-pack chips and the "Getting…" chip are phases 7 and 9. The app loop must: build one `PackService` at start, call `init()` and `poll(&mut engine)` every frame, forward `engine.take_kit_selected()` to `on_kit_selected` and call `check_target` after engine edits, and call `pause_for_quit()` on quit (phase 10).
