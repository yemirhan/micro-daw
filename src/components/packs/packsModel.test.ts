import { describe, expect, it } from 'vitest';
import type { PackFile } from '@/types/packs';
import { FAKE_MACHINE, SHA, fakeInstalled, fakeManifest } from './fixtures';
import {
  builtinItem,
  downloadProgress,
  filterItems,
  formatBytes,
  groupFilesByFolder,
  installButtonState,
  litPads,
  matchesQuery,
  kitsUsePack,
  packFileLabel,
  packIdOfSource,
  packSizeBytes,
  remoteItem,
  remoteItems,
  soundLabel,
  topTags,
} from './packsModel';

describe('packsModel', () => {
  const m808 = fakeManifest();
  const m909 = fakeManifest({ id: 'tr909-x', name: '909 Hits', tagline: 'Punchy techno', tags: ['909', 'techno', 'Analog'], author: 'Jane', license: 'CC-BY-4.0' });

  it('builds built-in items from classic machines', () => {
    const item = builtinItem(FAKE_MACHINE);
    expect(item).toMatchObject({ kind: 'builtin', id: 'builtin:tr808', name: '808-style', soundCount: 4 });
    expect(item.tags).toContain('808');
    expect(item.tags).toContain('built-in');
  });

  it('sums known file sizes', () => {
    expect(packSizeBytes(m808.files)).toBe(1.5 * 1024 * 1024);
    expect(packSizeBytes([{ path: 'a.wav', url: 'https://x' } as PackFile])).toBeNull();
    expect(formatBytes(1.5 * 1024 * 1024)).toBe('1.5 MB');
    expect(formatBytes(2048)).toBe('2 KB');
    expect(formatBytes(null)).toBe('');
  });

  it('lists catalog packs and keeps installed packs the catalog no longer has', () => {
    const orphan = fakeInstalled(fakeManifest({ id: 'gone', name: 'Gone' }));
    const items = remoteItems([m808, m909, m808], [fakeInstalled(m808), orphan]);
    expect(items.map((i) => i.id)).toEqual(['tr808-fischer', 'tr909-x', 'gone']);
    expect(remoteItems(null, [orphan]).map((i) => i.id)).toEqual(['gone']);
  });

  it('searches name, tags, description, author and licence; every word must match', () => {
    const a = remoteItem(m808);
    const b = remoteItem(m909);
    expect(matchesQuery(a, 'fischer')).toBe(true);
    expect(matchesQuery(a, '808 hip')).toBe(true);
    expect(matchesQuery(a, '808 techno')).toBe(false);
    expect(matchesQuery(b, 'cc-by')).toBe(true);
    expect(filterItems([a, b], '', 'analog').map((i) => i.id)).toEqual(['tr808-fischer', 'tr909-x']);
    expect(filterItems([a, b], 'punchy', 'ANALOG').map((i) => i.id)).toEqual(['tr909-x']);
    expect(filterItems([a, b], '  ', null)).toHaveLength(2);
  });

  it('ranks tags by count, case-insensitively', () => {
    const tags = topTags([remoteItem(m808), remoteItem(m909), builtinItem(FAKE_MACHINE)], 3);
    expect(tags[0]).toEqual({ tag: '808', count: 2 });
    expect(tags[1]).toEqual({ tag: 'analog', count: 2 });
    expect(tags).toHaveLength(3);
  });

  it('groups sounds by folder, root first, natural order', () => {
    const groups = groupFilesByFolder([
      { path: 'sd/SD10.wav', url: 'u', size: 0, sha256: SHA },
      { path: 'sd/SD2.wav', url: 'u', size: 0, sha256: SHA },
      { path: 'root.wav', url: 'u', size: 0, sha256: SHA },
      { path: 'bd/BD.wav', url: 'u', size: 0, sha256: SHA },
    ]);
    expect(groups.map((g) => g.folder)).toEqual(['', 'bd', 'sd']);
    expect(groups[2].files.map((f) => f.path)).toEqual(['sd/SD2.wav', 'sd/SD10.wav']);
    expect(soundLabel('sd/Snare 01.wav')).toBe('Snare 01');
  });

  it('derives the install button state', () => {
    const item = remoteItem(m808);
    expect(installButtonState(builtinItem(FAKE_MACHINE), undefined, false)).toEqual({ kind: 'builtin' });
    expect(installButtonState(item, { state: 'idle' }, false)).toEqual({ kind: 'get' });
    expect(installButtonState(item, { state: 'idle' }, true)).toEqual({ kind: 'installed' });
    expect(installButtonState(item, { state: 'error', message: 'Timed out' }, false)).toEqual({ kind: 'error', message: 'Timed out' });
    expect(
      installButtonState(item, { state: 'downloading', receivedBytes: 50, totalBytes: 200, fileIndex: 0, fileCount: 3 }, false),
    ).toEqual({ kind: 'downloading', progress: 0.25, label: '25%' });
    expect(
      installButtonState(item, { state: 'downloading', receivedBytes: 0, totalBytes: null, fileIndex: 1, fileCount: 4 }, false),
    ).toMatchObject({ progress: 0.25 });
    expect(downloadProgress({ state: 'downloading', receivedBytes: 0, totalBytes: null, fileIndex: 0, fileCount: 0 })).toBeNull();
  });

  it('lights the pads a kit fills', () => {
    const lit = litPads(remoteItem(m808));
    expect(lit.slice(0, 4)).toEqual([true, true, true, false]);
    expect(litPads(builtinItem(FAKE_MACHINE)).filter(Boolean)).toHaveLength(4);
  });
});

describe('pack usage + labels', () => {
  const pad = (source: unknown) => ({ sample: source ? { source } : null });
  it('finds pads that use a pack (pack sources or files inside its folder)', () => {
    const dir = '/data/packs/tr808-fischer';
    expect(kitsUsePack([{ pads: [pad(null), pad({ kind: 'pack', packId: 'tr808-fischer', file: 'bd/a.wav' })] }], 'tr808-fischer', null)).toBe(true);
    expect(kitsUsePack([{ pads: [pad({ kind: 'file', path: `${dir}/bd/a.wav` })] }], 'tr808-fischer', dir)).toBe(true);
    expect(kitsUsePack([{ pads: [pad({ kind: 'file', path: `${dir}-other/bd/a.wav` })] }], 'tr808-fischer', dir)).toBe(false);
    expect(kitsUsePack([{ pads: [pad({ kind: 'pack', packId: 'other', file: 'x.wav' })] }, undefined], 'tr808-fischer', dir)).toBe(false);
    expect(kitsUsePack([{ pads: [pad({ kind: 'file', path: 'C:\\packs\\p\\a.wav' })] }], 'p', 'C:\\packs\\p')).toBe(true);
  });

  it('reads the pack id of a pack source', () => {
    expect(packIdOfSource({ kind: 'pack', packId: 'a', file: 'x' })).toBe('a');
    expect(packIdOfSource({ kind: 'file', path: '/x' })).toBeNull();
    expect(packIdOfSource(null)).toBeNull();
  });

  it('labels a sound by its catalog label, else its file name', () => {
    expect(packFileLabel({ path: 'bd/bd0010.wav', url: 'u', size: 0, sha256: SHA, label: 'Kick · tone 0 · decay 10' })).toBe('Kick · tone 0 · decay 10');
    expect(packFileLabel({ path: 'bd/bd0010.wav', url: 'u', size: 0, sha256: SHA, label: '  ' })).toBe('bd0010');
    expect(packFileLabel({ path: 'bd/bd0010.wav', url: 'u', size: 0, sha256: SHA })).toBe('bd0010');
  });
});
