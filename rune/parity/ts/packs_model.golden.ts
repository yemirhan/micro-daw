/**
 * Golden vectors for rune/crates/mdaw-model/src/packs/{model,service,kit_preview,sender}.rs:
 * Packs page filtering / tags / sizes / grouping / button states over the real catalog,
 * kitFromPack + remoteKitPlan for every real catalog kit, drum-role detection and demo
 * grooves, IPC error cleanup, pack file paths / labels and the renderer-URL check.
 */
import { it } from 'vitest';
import catalogJson from '../../../packs/catalog.json';
import { validateCatalog } from '@/shared/packValidate';
import {
  builtinItem,
  filterItems,
  formatBytes,
  groupFilesByFolder,
  litPads,
  packButtonState,
  packFileLabel as modelPackFileLabel,
  remoteItems,
  soundLabel,
  topTags,
  type ClassicMachineInfo,
} from '@/components/packs/packsModel';
import { classifyPadName, demoPattern, kitRoles } from '@/components/packs/kitPreview';
import { remoteKitPlan } from '@/components/packs/remoteKit';
import { ipcErrorMessage, kitFromPack, packFileLabel as servicePackFileLabel, packFilePath } from '@/services/PackService';
import { isTrustedRendererUrl } from '@/main/packs/sender';
import { CLASSIC_MACHINES } from '@/audio/sampler/classic';
import type { InstalledPack, PackInstallState, PackManifest } from '@/types/packs';
import { writeFixture } from './fixture';

it('dumps packs model vectors', () => {
  const catalog = validateCatalog(catalogJson).catalog;
  const machines = (CLASSIC_MACHINES as unknown as ClassicMachineInfo[]).map((m) => ({
    id: m.id,
    name: m.name,
    tagline: m.tagline,
    color: m.color,
    ...(m.description !== undefined ? { description: m.description } : {}),
    voices: m.voices.map((v) => ({ id: v.id, label: v.label, ...(v.gm !== undefined ? { gm: v.gm } : {}) })),
    ...(m.pads ? { pads: [...m.pads] } : {}),
  }));
  const orphanManifest: PackManifest = { ...catalog.packs[0], id: 'orphan-pack', name: 'Orphan Pack', tags: ['Orphan', 'Ünïcode', '808'] };
  const installed: InstalledPack[] = [
    { id: catalog.packs[0].id, version: 1, dir: '/data/packs/x', installedAt: 'x', manifest: catalog.packs[0] },
    { id: 'orphan-pack', version: 1, dir: '/data/packs/orphan-pack', installedAt: 'x', manifest: orphanManifest },
  ];
  const items = [...machines.map((m) => builtinItem(m)), ...remoteItems(catalog.packs, installed)];
  const queries = ['', '  ', '808', 'drum', 'DRUM machine', 'cc0', 'cc-by', 'public', 'analog 808', 'ünicode', 'unicode', 'xyz', 'built-in', 'kit', 'synth', 'sonic', '9', 'é'];
  const allTags = [...new Set(items.flatMap((i) => i.tags))];
  const tags: (string | null)[] = [null, '', 'NOPE', ...allTags, ...allTags.map((t) => t.toUpperCase())];
  const filters: { query: string; tag: string | null; ids: string[] }[] = [];
  for (const query of queries) for (const tag of tags) filters.push({ query, tag, ids: filterItems(items, query, tag).map((i) => i.id) });

  const top = [1, 3, 10, 100].map((limit) => topTags(items, limit));
  const sizes = [null, undefined, NaN, Infinity, -1, 0, 1, 1023, 1024, 1536, 1048575, 1048576, 1.25 * 1048576, 1.45 * 1048576, 9.96 * 1048576, 9.94 * 1048576, 10 * 1048576, 10.5 * 1048576, 123456789, 1.5, 0.5].map((b) => [
    b === undefined || (typeof b === 'number' && !Number.isFinite(b)) ? String(b) : b,
    formatBytes(b as number | null),
  ]);
  const itemSummaries = items.map((i) => ({
    id: i.id,
    kind: i.kind,
    name: i.name,
    description: i.description,
    tags: i.tags,
    soundCount: i.soundCount,
    sizeBytes: i.kind === 'remote' ? i.sizeBytes : null,
    lit: litPads(i),
  }));
  const groups = catalog.packs.map((p) => groupFilesByFolder(p.files).map((g) => ({ folder: g.folder, files: g.files.map((f) => f.path) })));
  const labels = catalog.packs.flatMap((p) => p.files.map((f) => [f.path, soundLabel(f.path), modelPackFileLabel(f), servicePackFileLabel(f)]));
  const extraLabels = ['a/b/Kick 01.WAV', 'x.toolong', 'x.ab', 'x.a', '.wav', 'a.b.c', 'noext', 'dir.d/file', 'é.wåv', 'x.mp3 '].map((p) => [p, soundLabel(p), servicePackFileLabel({ path: p })]);

  const d = (receivedBytes: number, totalBytes: number | null, fileIndex: number, fileCount: number): PackInstallState => ({ state: 'downloading', receivedBytes, totalBytes, fileIndex, fileCount });
  const states: (PackInstallState | undefined)[] = [
    undefined, { state: 'idle' }, { state: 'installed' }, { state: 'error', message: 'Timed out' }, { state: 'error', message: 'Download paused' },
    { state: 'error', message: 'download paused' }, d(0, null, 0, 0), d(50, 200, 0, 3), d(200, 200, 3, 3), d(300, 200, 3, 3), d(1, 3, 0, 0), d(2, 3, 0, 0),
    d(0, 0, 1, 4), d(0, null, 4, 4), d(0, null, 5, 4), d(5, 1000, 0, 10), d(995, 1000, 9, 10), d(1, 200, 0, 0), d(3, 200, 0, 0),
  ];
  const buttons = states.flatMap((s) => [false, true].map((inst) => [s ?? null, inst, packButtonState(s, inst)]));

  const kits = catalog.packs.flatMap((m) => m.kits.map((k) => ({ pack: m.id, kit: k.id, state: kitFromPack({ id: m.id, manifest: m }, k.id) })));
  const plans = catalog.packs.flatMap((m) => m.kits.map((k) => ({ pack: m.id, kit: k.id, plan: remoteKitPlan(m, k.id) })));
  const roles = kits.map((k) => ({ roles: kitRoles(k.state!), demo: demoPattern(k.state!) }));
  const padNames = [
    ...new Set([
      ...catalog.packs.flatMap((p) => p.kits.flatMap((k) => k.pads.map((pd) => pd.name ?? ''))),
      ...catalog.packs.flatMap((p) => p.files.map((f) => f.label ?? soundLabel(f.path))),
      'Kick', 'BD0010', '808 bd long', 'SD0025', 'Clap', 'Closed HH', 'CH', 'Open HH', 'OH_02', 'Cowbell', 'Crash', 'hi-hat', 'Hi - Hat', 'hihat', 'HiHat 2',
      'bass drum', 'BassDrum', 'bass\tdrum', 'kk', 'kkk', 'sn', 'snr1', 'cp_01', 'rs.wav', 'lt', 'MT', 'ht', 'ohh', 'ohhh', 'x_oh', 'oh.', 'chhh', 'phh',
      'Tom', 'atom', 'rim shot', 'Tamb', 'perc', 'agogo', 'cabasa', 'conga', 'bongo', 'maraca', 'shaker', 'clave', 'ma', 'cl', 'cb', 'Ride', 'ﬁ', 'éoh', 'ÖH',
      'KICK__01', 'snare..2', '', ' ', 'pedal', 'closed', 'open', 'opened hat', 'kickback',
    ]),
  ];
  const classified = padNames.map((n) => [n, classifyPadName(n)]);

  const ipc = [
    "Error invoking remote method 'packs:install': Error: HTTP 404",
    "Error invoking remote method 'packs:install': PackValidationError: Pack x: bad",
    "Error invoking remote method 'packs:install': plain",
    "Error invoking remote method 'packs:install': Error: ",
    "Error invoking remote method 'x': ",
    "Error invoking remote method '': Error: x",
    "Error invoking remote method 'a'b': Error: x",
    "Error invoking remote method 'a': Error:x",
    "Error invoking remote method 'a': _Error: x",
    "Error invoking remote method 'a': My Error: x",
    'Timed out',
    '',
  ].map((m) => [m, ipcErrorMessage(new Error(m))]);

  const paths = [
    ['C:\\Users\\me\\packs\\p', 'kicks/bd.wav'], ['/data/packs/p', 'kicks/bd.wav'], ['/data/packs/p/', 'a/b/c.wav'], ['/', 'a.wav'], ['C:\\', 'a/b.wav'],
    ['rel', 'a.wav'], ['C:/mixed\\dir', 'a/b.wav'],
  ].map(([dir, rel]) => [dir, rel, packFilePath(dir, rel)]);

  const DEV = 'http://localhost:5173';
  const PROD = 'file:///Applications/Micro%20DAW.app/Contents/Resources/app.asar/.vite/renderer/main_window/index.html';
  const senders = [
    ['http://localhost:5173/', DEV], ['http://localhost:5173/index.html#/packs', DEV], ['http://localhost:5174/', DEV], ['https://localhost:5173/', DEV],
    ['https://evil.example/', DEV], ['file:///tmp/index.html', DEV], [PROD, PROD], [`${PROD}#/beat`, PROD], [`${PROD}?x=1`, PROD], ['file:///tmp/evil.html', PROD],
    [PROD.replace('index.html', 'other.html'), PROD], ['https://raw.githubusercontent.com/x/index.html', PROD], [null, DEV], ['', DEV], ['not a url', DEV],
    [DEV, 'not a url'], ['HTTP://LOCALHOST:5173/x', DEV], ['http://localhost:80/', 'http://localhost/'], ['http://127.0.0.1:5173/', DEV], ['file://host/a.html', 'file://host/a.html'],
    ['file://other/a.html', 'file://host/a.html'], ['data:text/html,x', 'data:text/html,x'], ['app://main/index.html', 'app://main/index.html'],
  ].map(([s, r]) => [s, r, isTrustedRendererUrl(s, r as string)]);

  writeFixture('packs_model', { machines, installed, items: itemSummaries, filters, top, sizes, groups, labels, extraLabels, buttons, kits, plans, roles, classified, ipc, paths, senders });
});
