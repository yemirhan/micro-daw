/**
 * Golden vectors for rune/crates/mdaw-model/src/packs/validate.rs (src/shared/packValidate.ts):
 * the real packs/catalog.json, a battery of broken manifests / files / kits / envelopes /
 * pack.json files (error messages must match), the URL / path / colour / id predicates and
 * mergeCatalogs.
 */
import { it } from 'vitest';
import catalogJson from '../../../packs/catalog.json';
import {
  declaredPackBytes,
  findCatalogFile,
  isAllowedUrl,
  isGithubSourceUrl,
  isPinnedPackUrl,
  isSafePackColor,
  isValidPackId,
  mergeCatalogs,
  safePackPathSegments,
  validateCatalog,
  validateInstalled,
  validateManifest,
} from '@/shared/packValidate';
import { COMMIT, manifest, packFile, rawTestUrl } from '@/main/packs/testUtils';
import type { PackCatalog } from '@/types/packs';
import { writeFixture } from './fixture';

type Outcome = { ok: unknown } | { error: string };
const run = (f: () => unknown): Outcome => {
  try {
    return { ok: f() };
  } catch (e) {
    return { error: (e as Error).message };
  }
};
const clone = <T>(v: T): T => JSON.parse(JSON.stringify(v)) as T;
const UNDEF = '__undefined__';
/** JSON can't carry `undefined`: the marker deletes the key instead. */
function setKey(o: Record<string, unknown>, k: string, v: unknown) {
  if (v === UNDEF) delete o[k];
  else o[k] = v;
}

const BAD_VALUES: unknown[] = [UNDEF, null, 42, 1.5, true, '', '   ', '\u00a0', 'x'.repeat(201), 'x'.repeat(5001), [], {}, ['a'], 'ok value'];

function manifestBattery(): unknown[] {
  const out: unknown[] = [];
  const base = () => clone(manifest()) as unknown as Record<string, unknown>;
  out.push(base());
  for (const key of ['id', 'name', 'tagline', 'description', 'tags', 'author', 'source', 'license', 'attribution', 'color', 'files', 'kits', 'version']) {
    for (const v of BAD_VALUES) {
      const m = base();
      setKey(m, key, v);
      out.push(m);
    }
  }
  // ids
  for (const id of ['Bad_Id', '../x', 'a--b', '-a', 'a-', 'ok-1', '808', 'x'.repeat(64), 'x'.repeat(65), 'é']) out.push({ ...base(), id });
  // licences
  for (const license of ['CC0-1.0', 'public-domain', 'CC-BY-4.0', 'GPL', 'cc0-1.0', ['CC0-1.0']]) out.push({ ...base(), license });
  out.push({ ...base(), license: 'CC-BY-4.0', attribution: 'by X' });
  out.push({ ...base(), license: 'CC-BY-4.0', attribution: '' });
  out.push({ ...base(), license: 'CC-BY-4.0', attribution: '  ' });
  // sources
  for (const source of ['https://github.com/o/r', 'http://github.com/o/r', 'https://GitHub.com/o', 'https://github.com:443/o', 'https://github.com:8443/o', 'https://user@github.com/o', 'https://evil.example/repo', 'github.com/o/r', 'https://github.com.evil.example/o']) out.push({ ...base(), source });
  // tags
  for (const tags of [[], ['a', 'b'], ['x'.repeat(64)], ['x'.repeat(65)], ['a', 1], 'a', null]) out.push({ ...base(), tags });
  // colours
  for (const color of ['#f80', '#ff8800', '#FF8800CC', '#ff88', '#ff880', 'oklch(0.68 0.19 35)', 'oklch(68% 0.19 35deg / 0.5)', 'OKLCH(.5 .1 30DEG)', 'oklch(0.6 0.1)', 'red', 'var(--x)', 'oklch(0.6 0.1 30); color: red', ' #f80', '#f80 ']) out.push({ ...base(), color });
  // versions
  for (const version of [0, 1, 2.0, -1, 1.5, '1', 1e3, 9007199254740991]) out.push({ ...base(), version });
  // files
  const f = packFile('a.wav', 'a');
  const fileVariants: Record<string, unknown>[] = [
    { ...f },
    { ...f, size: 0 },
    { ...f, size: -1 },
    { ...f, size: 1.5 },
    { ...f, size: '4' },
    { ...f, size: 50 * 1024 * 1024 },
    { ...f, size: 50 * 1024 * 1024 + 1 },
    { ...f, sha256: 'XYZ' },
    { ...f, sha256: f.sha256.toUpperCase() },
    { ...f, sha256: 42 },
    { ...f, url: 'https://evil.com/a.wav' },
    { ...f, url: 'https://raw.githubusercontent.com/o/r/main/a.wav' },
    { ...f, url: rawTestUrl('a.wav') + '?x=1' },
    { ...f, url: rawTestUrl('a.wav') + '?' },
    { ...f, url: rawTestUrl('a.wav') + '#' },
    { ...f, url: rawTestUrl('a/../a.wav') },
    { ...f, url: 42 },
    { ...f, path: '../a.wav' },
    { ...f, path: 42 },
    { ...f, path: '' },
    { ...f, label: 'Kick · tone 0' },
    { ...f, label: 'x'.repeat(80) },
    { ...f, label: 'x'.repeat(81) },
    { ...f, label: '  ' },
    { ...f, label: 'a\nb' },
    { ...f, label: null },
    { ...f, label: 3 },
    'not a file',
    null,
  ];
  for (const v of ['size', 'sha256', 'url', 'path']) {
    const copy: Record<string, unknown> = { ...f };
    delete copy[v];
    fileVariants.push(copy);
  }
  for (const fv of fileVariants) out.push({ ...base(), files: [fv], kits: [] });
  out.push({ ...base(), files: [packFile('A.wav', 'a'), packFile('a.wav', 'a')], kits: [] });
  out.push({ ...base(), files: [packFile('a.wav', 'a'), packFile('a.wav/b.wav', 'b')], kits: [] });
  out.push({ ...base(), files: [packFile('A.WAV/b.wav', 'a'), packFile('a.wav', 'b')], kits: [] });
  // Unicode normalization (phase-6 review finding 11): NFC and NFD spellings are one name on macOS.
  const nfc = 'caf\u00e9.wav';
  const nfd = 'cafe\u0301.wav';
  out.push({ ...base(), files: [packFile(nfc, 'a', rawTestUrl('u1.wav')), packFile(nfd, 'b', rawTestUrl('u2.wav'))], kits: [] });
  out.push({ ...base(), files: [packFile('cafe\u0301.wav/b.wav', 'a', rawTestUrl('u3.wav')), packFile('caf\u00e9.wav', 'b', rawTestUrl('u4.wav'))], kits: [] });
  out.push({ ...base(), files: [packFile('caf\u00e9.wav/b.wav', 'a', rawTestUrl('u5.wav')), packFile('CAFE\u0301.WAV', 'b', rawTestUrl('u6.wav'))], kits: [] });
  out.push({ ...base(), files: [packFile('CAF\u00c9.wav', 'a', rawTestUrl('u7.wav')), packFile('cafe\u0301.wav', 'b', rawTestUrl('u8.wav'))], kits: [] });
  out.push({ ...base(), files: [packFile(nfc, 'a', rawTestUrl('u9.wav')), packFile('cafe.wav', 'b', rawTestUrl('u10.wav')), packFile('caf\u00e8.wav', 'c', rawTestUrl('u11.wav'))], kits: [] });
  out.push({ ...base(), files: Array.from({ length: 11 }, (_, i) => ({ ...packFile(`f${i}.wav`, 'x'), size: 50 * 1024 * 1024 })), kits: [] });
  out.push({ ...base(), files: Array.from({ length: 10 }, (_, i) => ({ ...packFile(`f${i}.wav`, 'x'), size: 50 * 1024 * 1024 })), kits: [] });
  // kits
  const kitVariants: unknown[] = [
    [],
    'x',
    [null],
    [{ id: 'k', name: 'K', pads: [] }],
    [{ id: 'k', name: 'K' }],
    [{ id: '', name: 'K', pads: [] }],
    [{ id: 'k'.repeat(65), name: 'K', pads: [] }],
    [{ id: 'k', name: 'n'.repeat(201), pads: [] }],
    [{ id: 'k', name: 'K', pads: [] }, { id: 'k', name: 'K2', pads: [] }],
    [{ id: 'k', name: 'K', pads: [{ pad: 16, file: 'sd.wav' }] }],
    [{ id: 'k', name: 'K', pads: [{ pad: -1, file: 'sd.wav' }] }],
    [{ id: 'k', name: 'K', pads: [{ pad: 1.5, file: 'sd.wav' }] }],
    [{ id: 'k', name: 'K', pads: [{ pad: '1', file: 'sd.wav' }] }],
    [{ id: 'k', name: 'K', pads: [{ pad: 0, file: 'nope.wav' }] }],
    [{ id: 'k', name: 'K', pads: [{ pad: 0, file: 'SD.wav' }] }],
    [{ id: 'k', name: 'K', pads: [{ pad: 0, file: 'sd.wav' }, { pad: 0, file: 'sd.wav' }] }],
    [{ id: 'k', name: 'K', pads: [{ pad: 3, file: 'sd.wav' }, { pad: 3.0, file: 'kicks/bd.wav' }] }],
    [{ id: 'k', name: 'K', pads: [null] }],
    [{ id: 'k', name: 'K', pads: [{ pad: 0, file: 'sd.wav', name: '' }] }],
    [{ id: 'k', name: 'K', pads: [{ pad: 0, file: 'sd.wav', name: null }] }],
    [{ id: 'k', name: 'K', pads: [{ pad: 0, file: 'sd.wav', name: 'Snare' }] }],
    [{ id: 'k', name: 'K', pads: [{ pad: 0, file: 'sd.wav', settings: 'x' }] }],
    [{ id: 'k', name: 'K', pads: [{ pad: 0, file: 'sd.wav', settings: null }] }],
    [{ id: 'k', name: 'K', pads: [{ pad: 0, file: 'sd.wav', settings: [] }] }],
    [{ id: 'k', name: 'K', pads: [{ pad: 0, file: 'sd.wav', settings: { chokeGroup: 1, sample: { evil: true }, nested: { a: 1 }, gainDb: 3.5, mode: 'gate', reverse: true, color: null, list: [1] } }] }],
    [{ id: 'k', name: 'K', pads: [{ pad: 0, file: 'sd.wav', settings: { 10: 1, 2: 2, b: 3, a: 4 } }] }],
  ];
  for (const kits of kitVariants) out.push({ ...base(), kits });
  // whole-value oddities
  out.push(null, 'x', [], 42);
  return out;
}

it('dumps pack validator vectors', () => {
  const catalog = run(() => validateCatalog(catalogJson));

  const manifests = manifestBattery().map((input) => ({ input, out: run(() => validateManifest(input)) }));

  const m = () => clone(manifest());
  const envelopes = [
    null,
    [],
    'x',
    { version: 2, updatedAt: 'x', packs: [] },
    { version: '1', updatedAt: 'x', packs: [] },
    { version: 1.0, updatedAt: 'x', packs: [] },
    { updatedAt: 'x', packs: [] },
    { version: 1, updatedAt: 5, packs: [] },
    { version: 1, updatedAt: 'x'.repeat(65), packs: [] },
    { version: 1, updatedAt: 'x' },
    { version: 1, updatedAt: '2026-01-01', packs: {} },
    { version: 1, updatedAt: '2026-01-01', packs: [m(), m(), { ...m(), id: 'other', license: 'nope' }, { ...m(), id: 'good-two' }, null, { ...m(), id: 'BAD' }] },
    { version: [1], updatedAt: 'x', packs: [] },
  ].map((input) => ({ input, out: run(() => validateCatalog(input)) }));

  const installed = [
    { id: 'test-pack', version: 1, dir: '/evil', installedAt: 'now', manifest: m() },
    { id: 'other', manifest: m() },
    { id: 'test-pack', manifest: m() },
    { id: 'test-pack', version: 7, manifest: m() },
    { id: 'test-pack', version: 1.5, installedAt: 5, manifest: m() },
    { id: 'test-pack', version: -3, manifest: m() },
    { id: 'test-pack' },
    { id: 'test-pack', manifest: { ...m(), color: 'red' } },
    null,
    [],
  ].map((input) => ({ input, out: run(() => validateInstalled(input, '/real/test-pack')) }));

  const ids = ['', 'a', 'ok-1', 'Bad_Id', '../x', 'a--b', '-a', 'a-', '808', 'x'.repeat(64), 'x'.repeat(65), 'é', ' a', 'a b', 42, null].map((v) => [v, isValidPackId(v)]);

  const urlInputs: unknown[] = [
    'https://raw.githubusercontent.com/o/r/sha/a.wav',
    'https://RAW.githubusercontent.com:443/a',
    'https://raw.githubusercontent.com:444/a',
    'https://github.com/o/r/raw/main/a.wav',
    'https://objects.githubusercontent.com/x',
    'https://codeload.github.com/x',
    'http://raw.githubusercontent.com/a',
    'https://evil.com/a',
    'https://raw.githubusercontent.com.evil.com/a',
    'https://evil.com@raw.githubusercontent.com/a',
    'https://user:pw@raw.githubusercontent.com/a',
    'https://:@raw.githubusercontent.com/a',
    'https://github.com:8443/a',
    'file:///etc/passwd',
    'not a url',
    '',
    ' https://raw.githubusercontent.com/o/r/' + COMMIT + '/a.wav',
    'https://raw.githubusercontent.com/o/r/' + COMMIT + '/a.wav ',
    rawTestUrl('a/b%20c.wav'),
    rawTestUrl('a b.wav'),
    rawTestUrl('a.wav'),
    rawTestUrl('a.wav') + '?x=1',
    rawTestUrl('a.wav') + '?',
    rawTestUrl('a.wav') + '#x',
    rawTestUrl('a.wav') + '#',
    rawTestUrl('a/%2e%2e/b.wav'),
    rawTestUrl('a/%2E%2E/b.wav'),
    rawTestUrl('a%2fb.wav'),
    rawTestUrl('a%5Cb.wav'),
    rawTestUrl('a/../b.wav'),
    rawTestUrl('a/./b.wav'),
    rawTestUrl('a//b.wav'),
    rawTestUrl(''),
    `https://raw.githubusercontent.com/o/r/${COMMIT}`,
    `https://raw.githubusercontent.com/o/r/${COMMIT}/`,
    `https://raw.githubusercontent.com/o/r/main/a.wav`,
    `https://raw.githubusercontent.com/o/r/${COMMIT.toUpperCase()}/a.wav`,
    `https://raw.githubusercontent.com/-o/r/${COMMIT}/a.wav`,
    `https://raw.githubusercontent.com/${'o'.repeat(39)}/r/${COMMIT}/a.wav`,
    `https://raw.githubusercontent.com/${'o'.repeat(40)}/r/${COMMIT}/a.wav`,
    `https://raw.githubusercontent.com/o/r.x_y-z/${COMMIT}/a.wav`,
    `https://raw.githubusercontent.com/o/r%20x/${COMMIT}/a.wav`,
    `https://RAW.githubusercontent.com/o/r/${COMMIT}/a.wav`,
    `https://raw.githubusercontent.com:443/o/r/${COMMIT}/a.wav`,
    `https://github.com/o/r/${COMMIT}/a.wav`,
    'https://github.com/o/r/tree/abc/dir',
    'https://github.com.evil.example/o/r',
    'https://user@github.com/o/r',
    'https://GITHUB.com/o',
    'https://github.com:443/o',
    'javascript:alert(1)',
    'https://' + 'a'.repeat(4100),
    'https://github.com/' + 'a'.repeat(1990),
    42,
    null,
  ];
  const urls = urlInputs.map((u) => [u, isAllowedUrl(u), isPinnedPackUrl(u), isGithubSourceUrl(u)]);

  const colorInputs: unknown[] = [
    '#f80', '#ff8800', '#ff8800cc', '#FFF', '#ffff', '#fffff', '#ggg', '#', '', 'red', 'rgb(1,2,3)', 'url(x)',
    'oklch(0.68 0.19 35)', 'oklch(68% 0.19 35deg / 0.5)', 'oklch(0.6 0.1)', 'oklch(0.6 0.1 30); color: red', 'oklch(var(--x) 0.1 30)',
    'OKLCH(0.6 0.1 30)', 'oklch( 0.6  0.1  30 )', 'oklch(0.6\t0.1\n30)', 'oklch(.6 .1 30.)', 'oklch(+0.6 -0.1 30)', 'oklch(0.6% 0.1% 30%)',
    'oklch(0.6 0.1 30DEG)', 'oklch(0.6 0.1 30 /0.5)', 'oklch(0.6 0.1 30/ 50%)', 'oklch(0.6 0.1 30 / )', 'oklch(0.6 0.1 30 / 0.5 / 1)',
    'oklch(0.60.1 30)', 'oklch(0.6 0.1 30deg)', 'oklch(0.6 0.1 30 deg)', 'oklch(. 0.1 30)', 'oklch(1e2 0.1 30)', 'oklch(0.6 0.1 30)x',
    'oklch(0.6\u00a00.1\u00a030)', 'oklch(' + '1'.repeat(60) + ' 0 0)', 42, null,
  ];
  const colors = colorInputs.map((c) => [c, isSafePackColor(c)]);

  const pathInputs: unknown[] = [
    'kick.wav', 'kicks/bd 01.wav', 'a/b/c/Snare-1.WAV', 'LICENSE.txt', 'README.md', 'hats/ch.flac', 'x.wave', 'x.aif', 'x.ogg', 'x.m4a', 'x.mp3',
    '', '/etc/x.wav', '../x.wav', 'a/../../x.wav', 'a/./x.wav', 'a//x.wav', 'a\\..\\x.wav', 'C:/x.wav', 'c:x.wav', '.hidden.wav', 'a/.git/x.wav',
    'x.exe', 'kick', 'pack.json', 'PACK.JSON', 'a/pack.json', 'con.wav', 'CON', 'a/nul', 'com1.wav', 'lpt9.x.wav', 'com.wav', 'conx.wav',
    'trailing.wav ', 'trailing.', 'x\u0000.wav', 'x\u007f.wav', 'a*b.wav', 'a?.wav', 'a"b.wav', 'a<b.wav', 'a|b.wav', 'a/b/c/d/e/f/g/h.wav',
    'a/b/c/d/e/f/g/h/i.wav', 'x'.repeat(128) + '.wav', 'x'.repeat(125) + '.wav', 'd/' + 'x'.repeat(125) + '.wav', 'a/'.repeat(200) + 'x.wav',
    'é.wav', '音.wav', '.wav', 'a.', 'a.WAV', 'a .wav', 'con.\u2028.wav', 42, null,
  ];
  const paths = pathInputs.map((p) => [p, run(() => safePackPathSegments(p))]);

  const cat = (updatedAt: string, packs = [manifest()]): PackCatalog => ({ version: 1, updatedAt, packs });
  const mergeInputs: [PackCatalog, PackCatalog, string[]][] = [
    [cat('2026-11-17'), cat('2026-11-16', [manifest({ version: 9 })]), []],
    [cat('2026-11-17'), cat('2026-11-17', [manifest({ version: 9 })]), []],
    [cat('2026-11-17'), cat('not a date', [manifest({ version: 9 })]), []],
    [cat('2026-11-17'), cat('2026-12-01', []), []],
    [cat('2026-11-17', []), cat('2026-12-01', []), []],
    [cat('x'), cat('2026-12-01', [manifest({ version: 2 })]), []],
    [cat('x'), cat('y', [manifest({ version: 2 })]), []],
    [cat('2026-11-17T10:00:00Z'), cat('2026-11-17T10:00:01Z', [manifest({ version: 2 })]), []],
    [cat('2026-11-17T10:00:00.000Z'), cat('2026-11-17T11:00:00+02:00', [manifest({ version: 2 })]), []],
    [cat('2026'), cat('2026-01', [manifest({ version: 2 })]), []],
    [
      cat('2026-11-17', [manifest({ version: 3 }), manifest({ id: 'other', version: 1 })]),
      cat('2026-12-01', [manifest({ version: 2 }), manifest({ id: 'new-pack' })]),
      ['Pack other: Invalid sha256 for a.wav'],
    ],
    [cat('2026-11-17', [manifest(), manifest({ id: 'withdrawn' })]), cat('2026-12-01', [manifest({ version: 2 })]), []],
    [cat('2026-11-17', [manifest(), manifest({ id: 'dup' })]), cat('2026-12-01', [manifest({ version: 2 })]), ['Duplicate pack id: dup']],
    [cat('2026-11-17', [manifest(), manifest({ id: 'dup' })]), cat('2026-12-01', [manifest({ version: 2 })]), ['Pack dup:x', 'Pack dupe: y', 'Duplicate pack id: dup2']],
    [cat('2026-11-17', [manifest({ version: 1 }), manifest({ version: 5 })]), cat('2026-12-01', [manifest({ version: 2 }), manifest({ version: 7 })]), []],
  ];
  const merges = mergeInputs.map(([bundled, remote, errors]) => ({ bundled, remote, errors, out: mergeCatalogs(bundled, remote, errors) }));

  const c = cat('2026-11-17');
  const finds = [c.packs[0].files[1].url, c.packs[0].files[0].url, rawTestUrl('other.wav'), 42].map((u) => [u, findCatalogFile(c, u)]);
  const declared = [declaredPackBytes(manifest()), ...(catalogJson as unknown as PackCatalog).packs.map((p) => declaredPackBytes(p))];

  writeFixture('packs_validate', { catalog, manifests, envelopes, installed, ids, urls, colors, paths, merges, finds, declared, findCatalog: c });
});
