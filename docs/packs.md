# Pack catalog

The Packs page lists downloadable sample packs from `packs/catalog.json` (type
`PackCatalog`, `src/types/packs.ts`). The app fetches the catalog from
`PACK_CATALOG_URL` (this file on `main`) and falls back to the copy bundled into
the main-process build. Design: `docs/packs-plan.md`.

```
packs/
  sources/<id>.json   curated pack specs, one per pack (edit these)
  catalog.json        generated, do not edit by hand
scripts/
  pack-lib.mjs        shared checks + helpers (also used by src/test/packCatalog.test.ts)
  pack-build.mjs      sources -> catalog.json (downloads files, records size + sha256)
  pack-check.mjs      validates catalog.json (+ --online / --deep URL checks)
```

## How the app uses packs

- **Catalog**: main fetches `PACK_CATALOG_URL` and merges it over the bundled
  copy (`mergeCatalogs` in `src/shared/packValidate.ts`): the remote copy is only
  used when its `updatedAt` is strictly newer; per pack, an older `version` or
  an entry the validator dropped keeps the bundled entry; a pack the newer
  remote catalog leaves out is withdrawn.
- **Downloads** (`src/main/packs/installer.ts`): every file is fetched from its
  commit-pinned URL, size + sha256 verified, retried up to 3 times with backoff
  on network errors, timeouts, 5xx/429 and corrupt bodies. Files land in
  `<packs>/.partial-<id>`; after a failure Retry only downloads what is missing
  (verified files are kept), a cancel discards it. An install settles only
  after every download worker settled (so an immediate Retry never races the
  failed attempt's cleanup). Quitting the app mid-install pauses it
  (`pauseAll` on `will-quit`, awaited): the staging folder is kept and the
  next start resumes. The finished folder is renamed into place; an update
  interrupted between the renames is restored from `.old-<id>-*` at the next
  start. A backup is deleted only once a valid install of that pack is
  confirmed (a failed restore keeps every backup for the next start).
- **Previews**: the renderer may only preview URLs of files in the loaded
  catalog (checked in main, size + sha256 verified).
- **Pads**: a pack kit stores `{ kind: 'pack', packId, file }` sources, resolved
  against `<userData>/packs` when loaded (`src/shared/packSource.ts`), so
  projects stay portable. Old projects with absolute paths into the packs
  folder are migrated on load. A pad whose pack is not installed fails with
  `MissingPackSampleError`; `packService.missingPackOf(source)` /
  `missingPacks()` / `isUsedByProject(id)` drive the "Pack not installed — Get"
  and uninstall warnings. `packStatus(id)` is `'unavailable'` for a pack that
  is not installed and not in the loaded catalog (withdrawn / unknown: offer
  clear / replace instead of Get). Installing, updating or removing a pack
  drops its cached pad audio (`invalidatePackBuffers`) so open kits reload;
  SampleManager samples under the pack folder (Song audio regions) are
  reloaded in place with the same `sampleId`
  (`sampleManager.onSamplesReloaded`), and a sample whose file is gone keeps
  its loaded audio (`sampleManager.isUnavailable(id)`), so the Song never goes
  silent.
- **Get & load**: `packService.getAndLoad(packId, kitId)` installs the pack
  if needed and loads the kit on the active drum track at app level (it
  completes after leaving the Packs page). Only the latest request loads
  (`pendingLoad`, `cancelPendingLoad()`); `onKitLoaded` lets a mounted page
  navigate to the pads.

## Licence rules

Only material whose licence you can **quote** goes in.

1. Allowed licences: `CC0-1.0`, `public-domain` (CC0-equivalent dedications such as
   The Unlicense) and `CC-BY-4.0`. CC-BY packs **must** have `attribution`.
   Not allowed: CC-BY-SA, CC-BY-NC, GPL, "royalty-free", "free to use", no licence.
2. The licence must come from the people who made the recordings (repo LICENSE,
   README or per-sound metadata). Record it in `licenseEvidence` as
   `{ url, quote }`, with the URL pinned to the same commit as the files.
3. No recordings of a machine's factory **PCM ROM** sounds (LinnDrum, DMX, RX5,
   SP-1200 factory disks, etc.): the recordist cannot license the manufacturer's
   samples. This includes modern machines whose voices play stored PCM wave
   data (e.g. the TR-8's 707/727 voices and 909 cymbals, see the rejected
   `tr8-mckaudio` pack). Recordings of analog circuits (TR-808) are fine when
   the recordist licenses them.
4. No re-uploads: take files from the original source, not from a mirror that
   re-encoded or renamed them.
5. Never present a pack as an official product. `name` and `tagline` must not
   contain manufacturer names (checked: Roland, Linn, Yamaha, Korg, Akai, E-mu,
   Oberheim, Sequential, Boss). Use "808-style" or "TR-808 Recordings", and give the factual credit in
   `description` / `attribution`.
6. No copyrighted loops or breaks (e.g. the Amen break), even if a repo says
   CC0.
7. Every recording must be covered by the licence of the person who made it.
   Files a repo only hosts on behalf of someone else (another pack, another
   licence) are left out unless that person's own licence allows it.

## Curation rules (enforced by `pack-check`)

- Files come from `raw.githubusercontent.com/<owner>/<repo>/<40-hex commit>/…`
  (the only allowed host, pinned to a commit so the bytes never change). Every
  file has `size` and `sha256`. The app enforces the same rules at runtime
  (`src/shared/packValidate.ts`): a catalog entry without a pinned URL, size or
  sha256 is dropped, and the installer verifies the exact byte count and
  sha256 of every download. Commit-pinned raw URLs are served without
  redirects (`pack-check --online` uses `redirect: 'error'`), so no other host
  is allowlisted.
- `source` is an `https://github.com/…` link and `color` is `#hex` or
  `oklch(L C H)` (the app rejects anything else, so a catalog cannot inject CSS).
- Optional per-file `label` (at most 80 characters): the friendly name shown in
  sound lists instead of the file name, e.g. `"Kick · tone 0 · decay 10"` for
  the Fischer 808 file `bd0010.wav`, `"Closed Hat 12"` for uzu
  `hihat-closed/12_hh_switchangel.wav`, `"High Tom · soft"` for Sonic Pi
  `drum_tom_hi_soft.flac`. Every audio file of a curated pack has one. Set it in the source spec next to
  `path`/`from`; pack-build copies it into the catalog.
- A pack is at most **60 MB** (the app allows 500 MB, but packs must download
  quickly); single files at most 50 MB.
- Audio paths are lower-case, no whitespace, no hidden files, allowed
  extensions only (`wav aif aiff flac mp3 ogg m4a`, plus `txt md` for the
  upstream LICENSE/README). Every pack ships its upstream licence text (a
  LICENSE file, or the licence section of the README when that is where the
  licence is stated).
- 1 to 4 kits per pack, each filling all 16 pads, unique kit ids, display names
  of at most 24 characters, pad settings within `PadSettings` ranges.
- Pad layout convention (bank A = the essentials, like the factory kit):
  `0 kick · 1 snare · 2 closed hat · 3 open hat · 4 clap · 5 rim · 6 crash/cymbal · 7 ride/perc`,
  bank B (pads 9-16) for alternates, toms and percussion. Kits that are not drum
  kits (727 Latin percussion, Tabla) use the pads freely.
- Hi-hats (closed, open, pedal) share **choke group 1**. Other mutually exclusive
  sounds use groups 2+ (tuned 808 bass on bank B, open/muted conga, short/long
  whistle).
- Balance the kit with `gainDb` (normalised hats/cymbals are typically -3 to
  -6 dB); tuned pads use `pitch` (semitones).

## Adding a pack

1. Find the source and its licence. Note the commit SHA:
   `gh api repos/<owner>/<repo>/commits/HEAD --jq .sha`, and list files with
   `gh api "repos/<owner>/<repo>/git/trees/<sha>?recursive=1"`.
2. Download the candidates and **listen**. Check format/level, e.g.
   `ffprobe` and `ffmpeg -af volumedetect`, and check for leading silence.
3. Write `packs/sources/<id>.json`:

   ```jsonc
   {
     "id": "my-pack",                  // kebab-case, = file name
     "name": "…", "tagline": "…", "description": "…",
     "tags": ["808", "drum machine"],  // lower-case
     "author": "who recorded/made the sounds",
     "source": "https://github.com/owner/repo",
     "license": "CC0-1.0",
     "attribution": "…",               // required for CC-BY-4.0
     "color": "oklch(0.68 0.19 35)",
     "version": 1,                     // bump when files or kits change
     "order": 60,                      // catalog position
     "upstream": { "repo": "owner/repo", "commit": "<40-hex sha>" },
     "licenseEvidence": [{ "url": "https://github.com/owner/repo/blob/<sha>/LICENSE", "quote": "CC0 1.0 Universal" }],
     "files": [
       { "path": "LICENSE.txt", "from": "LICENSE" },
       { "path": "kick/bd-01.wav", "from": "Kicks/BD 01.WAV" }   // pack path <- upstream path
     ],
     "kits": [
       { "id": "main", "name": "Main Kit", "pads": [
         { "pad": 0, "file": "kick/bd-01.wav", "name": "Kick" },
         { "pad": 2, "file": "hats/ch.wav", "name": "Closed Hat", "settings": { "chokeGroup": 1, "gainDb": -4 } }
       ] }
     ]
   }
   ```

4. Build and check:

   ```sh
   node scripts/pack-build.mjs --only my-pack   # downloads my-pack, reuses the others' sizes/hashes
   node scripts/pack-check.mjs --deep           # re-downloads everything and verifies sha256
   npm test -- packCatalog
   ```

   `pack-build` without flags re-downloads every file. `--date YYYY-MM-DD` sets
   `updatedAt`. `pack-check --online` only sends HEAD requests (status +
   Content-Length). Run `pack-check --online` after every catalog change.
5. Add the pack and its licence evidence to the table below, and any rejected
   candidates to the rejected list.

## Included packs

Verified 2026-11-17 (all 286 URLs checked with `pack-check --online`, sizes and
sha256 recorded).

| Pack | Sounds / size | Kits | Licence |
|---|---|---|---|
| `tr808-fischer`: TR-808 Recordings (v2: file labels) | 116 / 12.1 MB | 808 Classic, 808 Boom (tuned 808 bass on bank B) | CC0-1.0 |
| `uzu-drumkit`: uzu Drumkit (v2: Mot4i and unattributed files removed) | 28 / 1.3 MB | uzu Kit A, uzu Kit B | public-domain (Unlicense) |
| `sonic-pi-drums`: Sonic Pi Drums & Percussion (v2: ships LICENSE.md) | 113 / 3.8 MB | Acoustic, Electro, Kick & Hat Lab, Tabla | CC0-1.0 |
| `freepats-synth-percussion`: FreePats Synth Percussion | 21 / 1.5 MB | Synth Percussion Kit | CC0-1.0 |

### Licence evidence

**tr808-fischer**: `tidalcycles/sounds-tr808-fischer@85fbecf1bec32553395625ea659e2a56dfd7c0e1`
- [LICENSE](https://github.com/tidalcycles/sounds-tr808-fischer/blob/85fbecf1bec32553395625ea659e2a56dfd7c0e1/LICENSE): "CC0 1.0 Universal" (the GitHub licence is detected as `CC0-1.0`).
- [_soundmeta/bd8.json](https://github.com/tidalcycles/sounds-tr808-fischer/blob/85fbecf1bec32553395625ea659e2a56dfd7c0e1/_soundmeta/bd8.json): `"copyright": "(c) Michael Fischer / Technopolis"`, `"license": "cc0"`.
- [README.md](https://github.com/tidalcycles/sounds-tr808-fischer/blob/85fbecf1bec32553395625ea659e2a56dfd7c0e1/README.md) (Fischer's 1994 text): "These samples were taken _DIRECTLY_ from a Roland TR-808 Rhythm Composer (SERIAL NO. 103852)" … "these samples are ABSOLUTELY FREE!"
- Caveat: Fischer's 1994 text is a freeware release, not a formal licence. The CC0 dedication was added by the TidalCycles maintainers, who publish the set as CC0. The TR-808 is analog, so there is no factory sample content.

**uzu-drumkit**: `tidalcycles/uzu-drumkit@2f3e05c70ab4d73ad053a1467adec89bd27377a0`
- [LICENSE](https://github.com/tidalcycles/uzu-drumkit/blob/2f3e05c70ab4d73ad053a1467adec89bd27377a0/LICENSE): "This is free and unencumbered software released into the public domain." (The Unlicense, GitHub: `Unlicense`; catalogued as `public-domain`.)
- [README.md](https://github.com/tidalcycles/uzu-drumkit/blob/2f3e05c70ab4d73ad053a1467adec89bd27377a0/README.md): "Switch Angel (provided synthesized drum sounds processed with a la76 style analog compressor)" … "(Ludens + Switch Angel) recorded the misc sounds on modular synthesizer". The contributors made the sounds themselves.
- Excluded: `brk/10_break_amen_pprocessed.wav` (Amen break) and `.DS_Store` files.
- Excluded since v2 (review round 3): the ten `*_mot4i.wav` files (kicks, clap,
  crash, hats, toms). The README says Mot4i "provided" them; they come from
  Mot4i's own pack [mot4i/garden](https://github.com/mot4i/garden), which has no
  licence file and asks for credit ("You are free to use, share, and adapt the
  samples, as long as you credit the source"). They were committed by the repo
  owner, not by Mot4i, so the repo's Unlicense cannot be shown to cover them.
  Also excluded: `sd/14_sd.wav` and `tb/10_tb.wav` (added in an "acoustic kit"
  commit with no credit, provenance unknown).

**sonic-pi-drums**: `sonic-pi-net/sonic-pi@974b31345059ccae6f6b81f3f63411698611f354` (`etc/samples`, FLAC originals)
- [LICENSE.md](https://github.com/sonic-pi-net/sonic-pi/blob/974b31345059ccae6f6b81f3f63411698611f354/LICENSE.md), section "Samples": "All the bundled samples are individually licensed under a CC0 1.0 Universal Public Domain Dedication."
- [etc/samples/README.md](https://github.com/sonic-pi-net/sonic-pi/blob/974b31345059ccae6f6b81f3f63411698611f354/etc/samples/README.md): "All other samples in this directory are from http://freesound.org and have also been placed in the public domain via the Creative Commons 0 License". The file lists the freesound link of every sample and ships in the pack.
- Since v2 the pack also ships the pinned upstream `LICENSE.md` (whose "Samples" section is the licence statement), not only the samples README.
- Included prefixes: `bd_ drum_ sn_ hat_ elec_ tabla_ perc_snap*`. Excluded: loops (incl. `loop_amen*`), ambient, bass, guitar and vinyl sounds.

**freepats-synth-percussion**: `freepats/synthesizer-percussion@39bbabce8e0e10aa259d5d94fca329f668733185`
- [README.md](https://github.com/freepats/synthesizer-percussion/blob/39bbabce8e0e10aa259d5d94fca329f668733185/README.md): "These sounds had been created by Roberto <roberto@zenvoid.org> with Yoshimi and Geonkick free software synthesizers" … "Published under the terms of Creative Commons CC0 public domain dedication".
- [LICENSE](https://github.com/freepats/synthesizer-percussion/blob/39bbabce8e0e10aa259d5d94fca329f668733185/LICENSE): "CC0 1.0 Universal".

## Coverage and gaps

| Wanted | Status |
|---|---|
| 808 | ✓ Fischer recordings (real TR-808) |
| 909 / 707 / 606 | Built-in 909-, 707- and 606-style synth kits. No freely licensed recording with clear rights was found (the TR-8 recordings were withdrawn, see below) |
| LinnDrum / LM-2 | ✗ No source with clear rights (ROM samples; see BushDrum below). The built-in LinnDrum-style synth kit covers the sound |
| CR-78 / CR-8000 | ✗ No licensed source found (GitHub search for cr78, cr-78, compurhythm). The built-in CR-78-style kit covers the sound |
| Trap / hip-hop | ✓ "808 Boom" (long-decay 808 kick tuned across bank B as 808 bass), 808 Classic, uzu Kit A |
| Oddball machines | ✓ Tabla (Sonic Pi), FreePats analog-synth kit, uzu modular hits |

## Rejected candidates

| Candidate | Reason |
|---|---|
| `MckAudio/MckSamplePacks` TR8 folder (CC0 repo; shipped as `tr8-mckaudio` until review round 3, then **withdrawn**) | Roland documents that the TR-8's 707/727 voices are built from the original machines' PCM wave data, and the original 909's cymbals were PCM samples too. The recordist's CC0 cannot clear rights in that underlying sample data (rule 3). The whole pack was removed rather than shipping a partial 909/606 kit of uncertain provenance; the built-in 909-, 707- and 606-style synth kits cover these sounds. Installed copies stay on users' disks but the pack is no longer offered. |
| `Boochi44/free-drum-samples` (README says CC0, no LICENSE file) | 30 of 57 files are byte-identical copies of the Fischer 808 set under new names, some mislabelled (`01-hard-trap/claps/cl.wav` and `clap-01.wav` are the 808 **claves**). The rest are lo-fi or downsampled copies of the same files. The README credits the set to "Edward Loveall" instead of Michael Fischer and contains a stray machine-generated citation token. The only new material is a few sine "808 bass" files. The included Fischer pack covers it better (tuned 808 bass kit). |
| `tidalcycles/sounds-repetition` (CC0) | 9 speech-synth vocal phrases ("algorave generation…"), not drums; not enough for a kit. |
| `smpldsnds/drum-machines` (no LICENSE) | The README's "A collection of public domain samples" is the only statement. There is no provenance or licence for LM-2, CR-8000, RZ-1, Drumtraks, MR-10, MFB-512, Micro-Rhythmer or SK-1 (several are PCM-ROM machines). The TR-808 folder is the Fischer set re-encoded to ogg/m4a (`TR808.TXT`), so the lossless original is used instead. |
| `smpldsnds/sonic-pi-samples` | Lossy ogg/m4a mirror of Sonic Pi; the upstream FLACs are used instead. |
| `EwonRael/BushDrum` (repo LICENSE CC0, LinnDrum LM-2) | "made from 44.1 kHz samples of the original LinnDrum drum machine". LinnDrum voices are factory PCM ROM recordings, and the repo does not say who sampled them. File names match the anonymous LM-2 set mirrored elsewhere. A CC0 file from the uploader cannot cover rights it may not hold. |
| `sfzinstruments/TicTokMen.RetroDrums1` (CR-8000, RZ-1, Drumtraks, MR-10, …) | "Not all recordings were done in house (but all are free use licenses)": no specific licence, no LICENSE. |
| `sfzinstruments/TicTokMen.Moogdrums1` | No licence statement at all. |
| `smpldsnds/drum-abuse-vol3` (LinnDrum, TR-909, DMX…) | Extracted from the "Synth Abuse" collection (Google Drive). There is no free licence ("licensing details per machine"), and it is 741 MB. |
| `tidalcycles/Dirt-Samples`, `geikha/tidal-drum-machines`, `ritchse/tidal-drum-machines`, `felixroos/dough-samples` (machine banks) | No licence for the drum-machine banks (mixed provenance). |
| `MckAudio/MckSamplePacks` RX5 and DR5 folders (CC0 repo) | Yamaha RX5 and Alesis DR-5 play factory PCM ROM samples (rule 3). RD6 folder (Behringer RD-6): unlabeled numbered files with no README, and the 808 sound is already covered. |
| `lukesnc/volca-beats` (CC0) | Only a single `KorgVolcaBeats.zip`, but the catalog needs individual audio files. Reconsider if the files are published unzipped. |
| `sykesgabri/PSS-470-Drums` (CC0) | Only 5 samples, not enough for a kit. |
| `sfzinstruments/karoryfer.unruly-drums` (CC0) | Deferred. A multi-mic SFZ library (top, bottom and overhead L/R per drum, 6,300 files): single-mic files sound partial without the SFZ mix, so it needs an offline mixdown hosted on an allowed host. A good future oddball pack (snare wires on every drum). |
| `Gradigang-Solutions/kringel-samples` (CC0, 2026) | Re-processed copies of Fischer, Sonic Pi and body-percussion files, which come from their originals here. |
| `octalmusic/octal-samples` (CC0) | Synthesized from 808/909 "recipes"; overlaps with the built-in classic kits. Not evaluated further. |
| `rit-am/DrumKit` | Salamander Drumkit, CC BY-SA 3.0 (ShareAlike not allowed). |
| `cleary/samples-hydrogen-drums` | GPL-2.0 (not allowed). |
| `sfzinstruments/DrumGizmo.*`, `WilkinsonAudio.NakedDrums` (CC-BY-4.0) | Multi-mic acoustic libraries of 300 MB to 1.2 GB that need a mixdown; acoustic is covered by Sonic Pi. Possible later with attribution. |
| freesound.org packs | Not on an allowed download host. |

## Tests

- `npm test` covers the validator, catalog sync, installer (incl. fault
  injection: concurrent Retry, failed swap / restore, disk errors, pause on
  quit) and PackService.
- `npm run test:electron` (scripts/electron-smoke.mjs) builds the production
  renderer + main and runs the real app in Electron over file:// with
  `MICRO_DAW_SMOKE=1` (hidden window): no CSP violations while booting and
  using the app, the CSP blocks eval / inline + remote scripts / remote
  fetch, images and frames, navigation and window.open are blocked, and a
  foreign page with the preload gets "Untrusted IPC sender". Needs a desktop
  session (xvfb-run on Linux).
