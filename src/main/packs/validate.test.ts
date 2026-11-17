import { describe, expect, it } from 'vitest';
import {
  declaredPackBytes,
  findCatalogFile,
  isAllowedUrl,
  isGithubSourceUrl,
  isPinnedPackUrl,
  isSafePackColor,
  isSafePackPath,
  isValidPackId,
  MAX_FILE_BYTES,
  mergeCatalogs,
  validateCatalog,
  validateInstalled,
  validateManifest,
} from './validate';
import { COMMIT, manifest, packFile, rawTestUrl } from './testUtils';
import type { PackCatalog } from '../../types/packs';

describe('isAllowedUrl', () => {
  it('accepts https on the allowlisted host only (raw.githubusercontent.com)', () => {
    expect(isAllowedUrl('https://raw.githubusercontent.com/o/r/sha/a.wav')).toBe(true);
    expect(isAllowedUrl('https://RAW.githubusercontent.com:443/a')).toBe(true);
    // S2: redirect targets / other GitHub hosts are no longer allowed.
    expect(isAllowedUrl('https://github.com/o/r/raw/main/a.wav')).toBe(false);
    expect(isAllowedUrl('https://objects.githubusercontent.com/x')).toBe(false);
    expect(isAllowedUrl('https://codeload.github.com/x')).toBe(false);
  });

  it('rejects other schemes, hosts, ports and credentials', () => {
    expect(isAllowedUrl('http://raw.githubusercontent.com/a')).toBe(false);
    expect(isAllowedUrl('https://evil.com/a')).toBe(false);
    expect(isAllowedUrl('https://raw.githubusercontent.com.evil.com/a')).toBe(false);
    expect(isAllowedUrl('https://evil.com@raw.githubusercontent.com/a')).toBe(false);
    expect(isAllowedUrl('https://github.com:8443/a')).toBe(false);
    expect(isAllowedUrl('file:///etc/passwd')).toBe(false);
    expect(isAllowedUrl('not a url')).toBe(false);
    expect(isAllowedUrl(42)).toBe(false);
  });
});

describe('isSafePackPath', () => {
  it.each(['kick.wav', 'kicks/bd 01.wav', 'a/b/c/Snare-1.WAV', 'LICENSE.txt', 'hats/ch.flac'])('accepts %s', (p) => {
    expect(isSafePackPath(p)).toBe(true);
  });

  it.each([
    '',
    '/etc/x.wav',
    '../x.wav',
    'a/../../x.wav',
    'a/./x.wav',
    'a//x.wav',
    'a\\..\\x.wav',
    'C:/x.wav',
    'c:x.wav',
    '.hidden.wav',
    'a/.git/x.wav',
    'x.exe',
    'kick',
    'pack.json',
    'con.wav',
    'a/nul',
    'trailing.wav ',
    'x\u0000.wav',
    'a/b/c/d/e/f/g/h/i.wav',
  ])('rejects %j', (p) => {
    expect(isSafePackPath(p)).toBe(false);
  });
});

describe('validateManifest', () => {
  it('accepts a valid manifest', () => {
    const m = validateManifest(manifest());
    expect(m.id).toBe('test-pack');
    expect(m.kits[0].pads).toHaveLength(2);
  });

  it('rejects bad ids, licences, hosts and traversal', () => {
    expect(isValidPackId('Bad_Id')).toBe(false);
    expect(() => validateManifest(manifest({ id: '../x' }))).toThrow(/pack id/);
    expect(() => validateManifest({ ...manifest(), license: 'GPL' })).toThrow(/licence/);
    expect(() =>
      validateManifest(manifest({ files: [{ ...packFile('a.wav', 'a'), url: 'https://evil.com/a.wav' }], kits: [] })),
    ).toThrow(/not allowed/);
    expect(() => validateManifest(manifest({ files: [packFile('../a.wav', 'a', rawTestUrl('a.wav'))], kits: [] }))).toThrow(/Unsafe/);
  });

  it('rejects duplicate (case-insensitive) paths and file/folder clashes', () => {
    expect(() => validateManifest(manifest({ files: [packFile('A.wav', 'a'), packFile('a.wav', 'a')], kits: [] }))).toThrow(/Duplicate/);
    expect(() => validateManifest(manifest({ files: [packFile('a.wav', 'a'), packFile('a.wav/b.wav', 'b')], kits: [] }))).toThrow(/clash/);
  });

  it('enforces size caps and sha256 format', () => {
    const f = packFile('a.wav', 'a');
    expect(() => validateManifest(manifest({ files: [{ ...f, size: MAX_FILE_BYTES + 1 }], kits: [] }))).toThrow(/too large/);
    const many = Array.from({ length: 11 }, (_, i) => ({ ...packFile(`f${i}.wav`, 'x'), size: MAX_FILE_BYTES }));
    expect(() => validateManifest(manifest({ files: many, kits: [] }))).toThrow(/Pack too large/);
    expect(() => validateManifest(manifest({ files: [{ ...f, sha256: 'XYZ' }], kits: [] }))).toThrow(/sha256/);
  });

  it('S2: requires size, sha256 and a commit-pinned raw URL for every file', () => {
    const f = packFile('a.wav', 'a');
    const without = (k: 'size' | 'sha256') => {
      const copy: Record<string, unknown> = { ...f };
      delete copy[k];
      return copy as never;
    };
    expect(() => validateManifest(manifest({ files: [without('size')], kits: [] }))).toThrow(/size/);
    expect(() => validateManifest(manifest({ files: [without('sha256')], kits: [] }))).toThrow(/sha256/);
    expect(() => validateManifest(manifest({ files: [{ ...f, size: 0 }], kits: [] }))).toThrow(/size/);
    expect(() =>
      validateManifest(manifest({ files: [{ ...f, url: 'https://raw.githubusercontent.com/o/r/main/a.wav' }], kits: [] })),
    ).toThrow(/pinned/);
    expect(validateManifest(manifest({ files: [f], kits: [] })).files[0]).toEqual(f);
  });

  it('S2/S4: source must be a github.com link and color #hex or oklch()', () => {
    expect(() => validateManifest(manifest({ source: 'https://evil.example/repo' }))).toThrow(/github/);
    expect(() => validateManifest(manifest({ source: 'http://github.com/o/r' }))).toThrow(/github/);
    expect(() => validateManifest(manifest({ color: 'red; background: url(https://evil.example/x)' }))).toThrow(/color/);
    expect(() => validateManifest(manifest({ color: 'var(--x)' }))).toThrow(/color/);
    expect(validateManifest(manifest({ color: 'oklch(0.68 0.19 35)' })).color).toBe('oklch(0.68 0.19 35)');
  });

  it('P4: keeps a valid file label and rejects bad ones', () => {
    const f = { ...packFile('a.wav', 'a'), label: 'Kick · tone 0 · decay 10' };
    expect(validateManifest(manifest({ files: [f], kits: [] })).files[0].label).toBe('Kick · tone 0 · decay 10');
    expect(() => validateManifest(manifest({ files: [{ ...f, label: 'x'.repeat(81) }], kits: [] }))).toThrow(/label/);
    expect(() => validateManifest(manifest({ files: [{ ...f, label: 3 as never }], kits: [] }))).toThrow(/label/);
  });

  it('validates kits against files and pad range', () => {
    const base = manifest();
    expect(() => validateManifest({ ...base, kits: [{ id: 'k', name: 'K', pads: [{ pad: 16, file: 'sd.wav' }] }] })).toThrow(/pad index/);
    expect(() => validateManifest({ ...base, kits: [{ id: 'k', name: 'K', pads: [{ pad: 0, file: 'nope.wav' }] }] })).toThrow(/unknown file/);
    expect(() =>
      validateManifest({ ...base, kits: [{ id: 'k', name: 'K', pads: [{ pad: 0, file: 'sd.wav' }, { pad: 0, file: 'sd.wav' }] }] }),
    ).toThrow(/twice/);
  });

  it('keeps only scalar pad settings', () => {
    const m = validateManifest({
      ...manifest(),
      kits: [{ id: 'k', name: 'K', pads: [{ pad: 0, file: 'sd.wav', settings: { chokeGroup: 1, sample: { evil: true }, nested: { a: 1 } } as never }] }],
    });
    expect(m.kits[0].pads[0].settings).toEqual({ chokeGroup: 1 });
  });

  it('requires attribution for CC-BY', () => {
    expect(() => validateManifest(manifest({ license: 'CC-BY-4.0' }))).toThrow(/attribution/);
    expect(validateManifest(manifest({ license: 'CC-BY-4.0', attribution: 'by X' })).attribution).toBe('by X');
  });
});

describe('validateCatalog', () => {
  it('drops invalid and duplicate packs but keeps the rest', () => {
    const { catalog, errors } = validateCatalog({
      version: 1,
      updatedAt: '2026-01-01',
      packs: [manifest(), manifest(), manifest({ id: 'other', license: 'nope' as never }), manifest({ id: 'good-two' })],
    });
    expect(catalog.packs.map((p) => p.id)).toEqual(['test-pack', 'good-two']);
    expect(errors).toHaveLength(2);
  });

  it('throws on an invalid envelope', () => {
    expect(() => validateCatalog(null)).toThrow();
    expect(() => validateCatalog({ version: 2, updatedAt: 'x', packs: [] })).toThrow(/version/);
    expect(() => validateCatalog({ version: 1, updatedAt: 'x' })).toThrow(/packs/);
  });
});

describe('validateInstalled / declaredPackBytes', () => {
  it('uses the folder it was read from, not the stored dir', () => {
    const pack = validateInstalled({ id: 'test-pack', version: 1, dir: '/evil', installedAt: 'now', manifest: manifest() }, '/real/test-pack');
    expect(pack.dir).toBe('/real/test-pack');
    expect(() => validateInstalled({ id: 'other', manifest: manifest() }, '/x')).toThrow(/mismatch/);
  });

  it('sums declared sizes', () => {
    expect(declaredPackBytes(manifest())).toBe(9); // "kick" + "snare"
  });
});

describe('pinned URLs, sources and colors', () => {
  it('isPinnedPackUrl: raw.githubusercontent.com/<owner>/<repo>/<40-hex>/<path> only', () => {
    expect(isPinnedPackUrl(rawTestUrl('a/b%20c.wav'))).toBe(true);
    expect(isPinnedPackUrl(`https://raw.githubusercontent.com/o/r/${COMMIT}`)).toBe(false);
    expect(isPinnedPackUrl('https://raw.githubusercontent.com/o/r/main/a.wav')).toBe(false);
    expect(isPinnedPackUrl(`https://raw.githubusercontent.com/o/r/${COMMIT.toUpperCase()}/a.wav`)).toBe(false);
    expect(isPinnedPackUrl(`${rawTestUrl('a.wav')}?x=1`)).toBe(false);
    expect(isPinnedPackUrl(`${rawTestUrl('a.wav')}#x`)).toBe(false);
    expect(isPinnedPackUrl(`https://raw.githubusercontent.com/o/r/${COMMIT}/a/%2e%2e/b.wav`)).toBe(false);
    expect(isPinnedPackUrl(`https://github.com/o/r/${COMMIT}/a.wav`)).toBe(false);
    expect(isPinnedPackUrl(42)).toBe(false);
  });

  it('isGithubSourceUrl', () => {
    expect(isGithubSourceUrl('https://github.com/o/r/tree/abc/dir')).toBe(true);
    expect(isGithubSourceUrl('https://github.com.evil.example/o/r')).toBe(false);
    expect(isGithubSourceUrl('https://user@github.com/o/r')).toBe(false);
    expect(isGithubSourceUrl('javascript:alert(1)')).toBe(false);
  });

  it.each(['#f80', '#ff8800', '#ff8800cc', 'oklch(0.68 0.19 35)', 'oklch(68% 0.19 35deg / 0.5)'])('accepts color %s', (c) => {
    expect(isSafePackColor(c)).toBe(true);
  });

  it.each(['red', '#ggg', 'rgb(1,2,3)', 'oklch(0.6 0.1 30); color: red', 'oklch(var(--x) 0.1 30)', 'url(x)', 'oklch(0.6 0.1)', ''])(
    'rejects color %j',
    (c) => {
      expect(isSafePackColor(c)).toBe(false);
    },
  );
});

describe('mergeCatalogs (S2: remote must not override with weaker data)', () => {
  const cat = (updatedAt: string, packs = [manifest()]): PackCatalog => ({ version: 1, updatedAt, packs });

  it('keeps the bundled catalog when the remote one is not newer or undated', () => {
    const bundled = cat('2026-11-17');
    expect(mergeCatalogs(bundled, cat('2026-11-16', [manifest({ version: 9 })])).source).toBe('bundled');
    expect(mergeCatalogs(bundled, cat('2026-11-17', [manifest({ version: 9 })])).source).toBe('bundled');
    expect(mergeCatalogs(bundled, cat('not a date', [manifest({ version: 9 })])).source).toBe('bundled');
    expect(mergeCatalogs(bundled, cat('2026-12-01', [])).source).toBe('bundled');
  });

  it('per pack: an older remote version or an entry dropped as invalid keeps the bundled one', () => {
    const bundled = cat('2026-11-17', [manifest({ version: 3 }), manifest({ id: 'other', version: 1 })]);
    const remote = cat('2026-12-01', [manifest({ version: 2 }), manifest({ id: 'new-pack' })]);
    const merged = mergeCatalogs(bundled, remote, ['Pack other: Invalid sha256 for a.wav']);
    expect(merged.source).toBe('remote');
    expect(merged.catalog.packs.map((p) => `${p.id}@${p.version}`)).toEqual(['test-pack@3', 'new-pack@1', 'other@1']);
  });

  it('a pack the newer remote catalog removes cleanly stays removed (withdrawal)', () => {
    const bundled = cat('2026-11-17', [manifest(), manifest({ id: 'withdrawn' })]);
    const merged = mergeCatalogs(bundled, cat('2026-12-01', [manifest({ version: 2 })]));
    expect(merged.catalog.packs.map((p) => p.id)).toEqual(['test-pack']);
    expect(merged.catalog.packs[0].version).toBe(2);
  });

  it('findCatalogFile (S3: only catalog URLs may be previewed)', () => {
    const c = cat('2026-11-17');
    expect(findCatalogFile(c, c.packs[0].files[1].url)).toEqual(c.packs[0].files[1]);
    expect(findCatalogFile(c, rawTestUrl('other.wav'))).toBeNull();
    expect(findCatalogFile(c, 42)).toBeNull();
  });
});
