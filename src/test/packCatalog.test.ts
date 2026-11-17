/**
 * The curated pack catalog (packs/catalog.json, built from packs/sources by
 * scripts/pack-build.mjs) must pass the app's own validation, the curation
 * rules in scripts/pack-lib.mjs, stay in sync with its sources and turn into
 * playable 16-pad kits. See docs/packs.md.
 */
import { describe, expect, it } from 'vitest';
import catalogJson from '../../packs/catalog.json';
import {
  checkCatalog,
  checkSource,
  checkSync,
  isPinnedRawUrl,
  packPathError,
  rawUrl,
  sortSources,
} from '../../scripts/pack-lib.mjs';
import { validateCatalog } from '@/main/packs/validate';
import { kitFromPack } from '@/services/PackService';
import { PAD_COUNT } from '@/types/sampler';
import type { InstalledPack, PackCatalog, PackManifest } from '@/types/packs';

const catalog = catalogJson as unknown as PackCatalog;
const sourceModules = import.meta.glob<{ default: Record<string, unknown> }>('../../packs/sources/*.json', { eager: true });
const sources = Object.entries(sourceModules).map(([file, mod]) => ({ fileName: file.split('/').pop() as string, spec: mod.default }));

const clone = <T>(v: T): T => JSON.parse(JSON.stringify(v)) as T;

describe('packs/catalog.json', () => {
  it('passes the app validator without dropping any pack', () => {
    const { catalog: valid, errors } = validateCatalog(catalogJson);
    expect(errors).toEqual([]);
    expect(valid.packs.map((p) => p.id)).toEqual(catalog.packs.map((p) => p.id));
    expect(valid.packs.length).toBeGreaterThanOrEqual(4);
  });

  it('passes the curation checks (pinned URLs, sizes, sha256, < 60 MB, 16-pad kits)', () => {
    expect(checkCatalog(catalog)).toEqual([]);
  });

  it('is in sync with packs/sources and in source order', () => {
    expect(sources.length).toBe(catalog.packs.length);
    for (const { spec, fileName } of sources) expect(checkSource(spec, fileName)).toEqual([]);
    expect(checkSync(sources.map((s) => s.spec), catalog)).toEqual([]);
    expect(sortSources(sources).map((s) => s.spec.id)).toEqual(catalog.packs.map((p) => p.id));
  });

  it('records licence evidence for every source', () => {
    for (const { spec } of sources) {
      const evidence = spec.licenseEvidence as { url: string; quote: string }[];
      expect(evidence.length).toBeGreaterThan(0);
      const upstream = spec.upstream as { commit: string };
      // Evidence links are pinned to the same commit the files come from.
      expect(evidence.some((e) => e.url.includes(upstream.commit))).toBe(true);
    }
  });

  it('ships each pack with its upstream licence or readme text', () => {
    for (const p of catalog.packs) {
      expect(p.files.some((f) => /^(LICENSE\.txt|README\.md)$/.test(f.path))).toBe(true);
    }
  });

  it('puts hi-hats in one choke group in every kit', () => {
    for (const p of catalog.packs) {
      for (const kit of p.kits) {
        const hats = kit.pads.filter((pad) => /\b(Closed|Open|Pedal|Tick) Hat\b/.test(pad.name ?? ''));
        for (const hat of hats) expect(hat.settings?.chokeGroup, `${p.id}/${kit.id}/${hat.name}`).toBe(1);
      }
    }
  });

  it('turns every kit into a full KitState of file pads', () => {
    for (const manifest of catalog.packs as PackManifest[]) {
      const installed: InstalledPack = { id: manifest.id, version: manifest.version, dir: '/packs/' + manifest.id, installedAt: '2026-11-17T00:00:00.000Z', manifest };
      for (const kit of manifest.kits) {
        const state = kitFromPack(installed, kit.id);
        expect(state).not.toBeNull();
        expect(state!.pads).toHaveLength(PAD_COUNT);
        for (const pad of state!.pads) {
          expect(pad.sample?.source.kind).toBe('file');
          if (pad.sample?.source.kind === 'file') expect(pad.sample.source.path.startsWith(`/packs/${manifest.id}/`)).toBe(true);
        }
      }
    }
  });
});

describe('pack-lib checks', () => {
  const base = () => clone(catalog);

  it('builds encoded, commit-pinned raw URLs', () => {
    const sha = 'a'.repeat(40);
    expect(rawUrl('o/r', sha, 'Samples/Hat One.wav')).toBe(`https://raw.githubusercontent.com/o/r/${sha}/Samples/Hat%20One.wav`);
    expect(isPinnedRawUrl(rawUrl('o/r', sha, 'a.wav'))).toBe(true);
    expect(isPinnedRawUrl('https://raw.githubusercontent.com/o/r/main/a.wav')).toBe(false);
    expect(isPinnedRawUrl(`http://raw.githubusercontent.com/o/r/${sha}/a.wav`)).toBe(false);
    expect(isPinnedRawUrl(`https://example.com/o/r/${sha}/a.wav`)).toBe(false);
  });

  it('rejects unsafe or non-audio pack paths', () => {
    expect(packPathError('kick/bd.wav')).toBeNull();
    expect(packPathError('LICENSE.txt')).toBeNull();
    expect(packPathError('../x.wav')).not.toBeNull();
    expect(packPathError('/abs.wav')).not.toBeNull();
    expect(packPathError('a\\b.wav')).not.toBeNull();
    expect(packPathError('.hidden.wav')).not.toBeNull();
    expect(packPathError('Kick.WAV')).not.toBeNull();
    expect(packPathError('a b.wav')).not.toBeNull();
    expect(packPathError('tool.exe')).not.toBeNull();
    expect(packPathError('pack.json')).not.toBeNull();
  });

  it('flags a branch URL, a missing sha256 and an oversize pack', () => {
    const c = base();
    c.packs[0].files[0].url = c.packs[0].files[0].url.replace(/\/[0-9a-f]{40}\//, '/main/');
    delete c.packs[0].files[1].sha256;
    c.packs[1].files[0].size = 40 * 1024 * 1024;
    c.packs[1].files[1].size = 40 * 1024 * 1024;
    const errors = checkCatalog(c).join('\n');
    expect(errors).toMatch(/pinned to a commit SHA/);
    expect(errors).toMatch(/sha256 missing/);
    expect(errors).toMatch(/60 MB/);
  });

  it('flags broken kits', () => {
    const c = base();
    const kit = c.packs[0].kits[0];
    kit.pads[1].pad = kit.pads[0].pad; // duplicate pad
    kit.pads[2].file = 'nope/missing.wav';
    kit.pads[3].settings = { gainDb: 40, mode: 'loop' as never };
    c.packs[0].kits[1] = { ...c.packs[0].kits[1], pads: c.packs[0].kits[1].pads.slice(0, 8) };
    const errors = checkCatalog(c).join('\n');
    expect(errors).toMatch(/assigned twice/);
    expect(errors).toMatch(/not in files/);
    expect(errors).toMatch(/gainDb out of range/);
    expect(errors).toMatch(/mode must be/);
    expect(errors).toMatch(/fill all 16 pads/);
  });

  it('flags duplicate ids, unsupported licences and trademark names', () => {
    const c = base();
    c.packs[1].id = c.packs[0].id;
    (c.packs[2] as { license: string }).license = 'CC-BY-NC-4.0';
    c.packs[3].name = 'Roland Drums';
    const errors = checkCatalog(c).join('\n');
    expect(errors).toMatch(/duplicate id/);
    expect(errors).toMatch(/licence must be/);
    expect(errors).toMatch(/trademark/);
  });

  it('detects a catalog that is out of date with its sources', () => {
    const specs = clone(sources.map((s) => s.spec));
    (specs[0].kits as { name: string }[])[0].name = 'Renamed';
    expect(checkSync(specs, catalog).join('\n')).toMatch(/out of date/);
    expect(checkSync(specs.slice(1), catalog).join('\n')).toMatch(/has no packs\/sources/);
  });
});
