import { describe, expect, it } from 'vitest';
import {
  declaredPackBytes,
  isAllowedUrl,
  isSafePackPath,
  isValidPackId,
  MAX_FILE_BYTES,
  validateCatalog,
  validateInstalled,
  validateManifest,
} from './validate';
import { manifest } from './testUtils';

describe('isAllowedUrl', () => {
  it('accepts https on allowlisted hosts', () => {
    expect(isAllowedUrl('https://raw.githubusercontent.com/o/r/sha/a.wav')).toBe(true);
    expect(isAllowedUrl('https://github.com/o/r/raw/main/a.wav')).toBe(true);
    expect(isAllowedUrl('https://objects.githubusercontent.com/x')).toBe(true);
    expect(isAllowedUrl('https://RAW.githubusercontent.com:443/a')).toBe(true);
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
      validateManifest(manifest({ files: [{ path: 'a.wav', url: 'https://evil.com/a.wav' }], kits: [] })),
    ).toThrow(/not allowed/);
    expect(() =>
      validateManifest(manifest({ files: [{ path: '../a.wav', url: 'https://github.com/a.wav' }], kits: [] })),
    ).toThrow(/Unsafe/);
  });

  it('rejects duplicate (case-insensitive) paths and file/folder clashes', () => {
    const url = 'https://github.com/x';
    expect(() => validateManifest(manifest({ files: [{ path: 'A.wav', url }, { path: 'a.wav', url }], kits: [] }))).toThrow(/Duplicate/);
    expect(() => validateManifest(manifest({ files: [{ path: 'a.wav', url }, { path: 'a.wav/b.wav', url }], kits: [] }))).toThrow(/clash/);
  });

  it('enforces size caps and sha256 format', () => {
    const url = 'https://github.com/x';
    expect(() => validateManifest(manifest({ files: [{ path: 'a.wav', url, size: MAX_FILE_BYTES + 1 }], kits: [] }))).toThrow(/too large/);
    const many = Array.from({ length: 11 }, (_, i) => ({ path: `f${i}.wav`, url, size: MAX_FILE_BYTES }));
    expect(() => validateManifest(manifest({ files: many, kits: [] }))).toThrow(/Pack too large/);
    expect(() => validateManifest(manifest({ files: [{ path: 'a.wav', url, sha256: 'XYZ' }], kits: [] }))).toThrow(/sha256/);
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

  it('sums sizes only when all are known', () => {
    expect(declaredPackBytes(manifest())).toBeNull();
    const url = 'https://github.com/x';
    expect(declaredPackBytes(manifest({ files: [{ path: 'a.wav', url, size: 3 }, { path: 'b.wav', url, size: 4 }] }))).toBe(7);
  });
});
