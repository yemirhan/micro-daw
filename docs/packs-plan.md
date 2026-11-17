# Packs plan (in-app pack store)

Goal: the user never has to search the web for drum-machine packs. Two sources, one "Packs" page:

1. **Built-in classic kits** — original syntheses of classic drum-machine *styles*
   (808-, 909-, 707-, 606-, LinnDrum-, CR-78-style), rendered offline once and cached, like
   the factory kit. Always available, offline, no licensing questions. Pad sample source
   `{ kind: 'classic', machine, voice }` (see `src/types/sampler.ts`).
2. **Downloadable packs** — real recordings with explicit free licences (CC0 / public domain /
   CC-BY with attribution), listed in a curated catalog (`packs/catalog.json`, served from
   `PACK_CATALOG_URL` with the bundled copy as fallback). Contract: `src/types/packs.ts`.

Naming: never present a pack as an official Roland/Linn/etc. product. Use "808-style",
"TR-808 recordings (Michael Fischer, CC0)" etc. Trademarks only in factual source credits.

## Work packages (file ownership)

### P1 — Built-in classic kits
Owns: `src/audio/sampler/classic/**`, `src/audio/sampler/bufferCache.ts`, `src/audio/sampler/factoryKit.ts`,
`src/components/beat/beatHelpers.ts` (only the sample-name helper), `src/utils/arrangementHelpers.ts` (only GM mapping for classic voices).
- Voice synthesis per machine (Tone.js offline): 808 (long tuned sine kick w/ pitch env + optional drive, snappy noise+tone snare, metallic 6-square-osc hats, clap with multi-burst, cowbell 2 squares, toms/congas, rim, clave, maracas, cymbal), 909 (punchy kick w/ click, noisier snare, hats/crash/ride from filtered noise+metallic), 707, 606, LinnDrum-style, CR-78-style. Each machine ≥ 12 voices.
- `CLASSIC_MACHINES: { id, name: '808-style', tagline, color, voices: {id,label,gm}[] }[]`, `createClassicKit(machine): KitState` (16 pads, sensible layout: kick, snare, ch, oh, clap, toms…, hats share choke group), `renderClassicVoice(machine, voice): Promise<AudioBuffer>`.
- bufferCache/normalizeKit/sample-name/GM export support for `kind: 'classic'`. Unit tests.

### P2 — Pack service (main + renderer)
Owns: `src/main.ts` (pack IPC section), `src/preload.ts`, `src/types/electron.d.ts`, `src/main/packs/**` (main-process pack code, create), `src/services/PackService.ts`, `src/hooks/usePacks.ts`, `src/hooks/useSampleBrowser.ts` + `src/components/browser/**` (only to add the "Installed packs" root).
- IPC: `packsFetchCatalog() → PackCatalog` (remote with 8 s timeout, validate schema, fall back to bundled `packs/catalog.json` — make sure it's included in the packaged app, e.g. via Vite import of the JSON in main), `packsListInstalled() → InstalledPack[]`, `packsInstall(id)` (download every file with Electron `net`/fetch into `<userData>/packs/.tmp-<id>`, only from `PACK_ALLOWED_HOSTS` over https, follow redirects only to allowed hosts, max 50 MB per file / 500 MB per pack, verify sha256 when given, reject path traversal in `PackFile.path`, write `pack.json` (InstalledPack), then atomic rename to `<userData>/packs/<id>`), progress events `packs:progress` (`{id, state: PackInstallState}`), `packsCancel(id)`, `packsUninstall(id)`, `packsFetchPreview(url) → ArrayBuffer` (allowed hosts, ≤ 10 MB, small in-memory LRU), `packsDir() → string`.
- Renderer `packService`: catalog + installed + per-pack install state, subscribe, `install/cancel/uninstall`, `kitFromPack(installed, kitId) → KitState` (file sources with absolute paths + pad settings), `previewRemote(url)` (decode + play via drumEngine-compatible preview), `isInstalled(id)`.
- Sample browser: when packs are installed, show a pinned "Installed packs" root (not removable) pointing at `<userData>/packs`.
- Tests for pure parts (validation, path safety, host allowlist, kitFromPack).

### P3 — Catalog curation
Owns: `packs/**`, `docs/packs.md`.
- Build `packs/catalog.json` (type `PackCatalog`) from sources with VERIFIED licences. Pin raw URLs to a commit SHA (`https://raw.githubusercontent.com/<owner>/<repo>/<sha>/<path>`), verify each URL with an HTTP HEAD/GET (status 200, size), fill `size` (and `sha256` if cheap).
- Candidates: `tidalcycles/sounds-tr808-fischer` (CC0), `Boochi44/free-drum-samples` (CC0; 3 kits), `tidalcycles/sounds-repetition` (CC0), `smpldsnds/drum-machines` (states public domain — include only machines whose licence text you can confirm, e.g. TR808.TXT), `smpldsnds/sonic-pi-samples` if CC0. Search for CC0/public-domain 909, 707, LinnDrum, CR-78 recordings; include only with an explicit licence statement. Never include unclear-licence material; list rejected candidates + reason in docs/packs.md.
- Each pack: 1–3 sensible 16-pad kits (kick, snare, closed/open hat with choke group 1, clap, toms, cymbals, percussion, 808 bass hits where available with `mode: 'oneshot'`), tags, colour, honest description and credit.
- docs/packs.md: how to add a pack (schema, licence rules, script), and a script `scripts/pack-check.mjs` that validates the catalog (schema, unique ids, kit files exist in `files`, URLs reachable with `--online`).

### P4 — Packs page UI
Owns: `src/components/packs/**`, `src/types/appMode.ts`, `src/components/sidebar/Sidebar.tsx`, `src/config/commands.ts`, `src/app/App.tsx` (mount + command wiring), `src/components/beat/**` kit menu (only to list built-in kits + installed pack kits).
- New AppMode `'packs'`, sidebar entry (Package icon) + shortcut, command "Browse packs".
- PacksView: header with search + tag filter chips; sections "Built-in kits" (instant, no download) and "Downloadable packs". Cards: colour accent, name, tagline, tags, licence badge, size, sound count, state button (Get / progress ring + Cancel / Installed ✓ / Retry). Card click → detail panel: description, credit + licence link, kit list with "Load on pads" (calls `arrangementEngine.setKit(ensureDrumTrack(), kit)` then switches to Beat view), sound list grouped by folder with per-sound play (remote preview before install, local after), "Preview kit" plays a short built-in 2-bar demo pattern through a temporary SamplerKit (stop on leave), Uninstall.
- Kit menu in Beat view gains sections: Built-in (classic kits), Installed packs (their kits), User kits (KitStore), plus "Browse packs…" which opens the Packs page.
- Offline / catalog error state with retry; empty states. Must look like the rest of the app (screenshots with scripts/ui-snap.mjs; mock `electronAPI` packs calls in the snap if needed).
