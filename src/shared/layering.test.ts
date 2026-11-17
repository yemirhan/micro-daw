/**
 * P3: the main process, preload and shared modules must not import renderer
 * code (components, hooks, services, '@/…' aliases the main build cannot
 * resolve), and renderer services must not import main-process modules.
 */
import { readdirSync, readFileSync, statSync } from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';

const SRC = path.resolve(__dirname, '..');

function files(dir: string): string[] {
  return readdirSync(dir).flatMap((n) => {
    const full = path.join(dir, n);
    return statSync(full).isDirectory() ? files(full) : /\.tsx?$/.test(n) && !/\.test\.tsx?$/.test(n) ? [full] : [];
  });
}

const importsOf = (file: string) => [...readFileSync(file, 'utf8').matchAll(/^\s*(?:import|export)[^'"]*from\s+['"]([^'"]+)['"]/gm)].map((m) => m[1]);

describe('layering', () => {
  it('main process, preload and shared code import only main/shared/types (relative)', () => {
    const offenders: string[] = [];
    const roots = [path.join(SRC, 'main'), path.join(SRC, 'shared')];
    const list = [...roots.flatMap(files), path.join(SRC, 'main.ts'), path.join(SRC, 'preload.ts')];
    for (const f of list) {
      if (f.endsWith('testUtils.ts')) continue;
      for (const spec of importsOf(f)) {
        if (spec.startsWith('@/')) offenders.push(`${path.relative(SRC, f)} -> ${spec}`);
        if (!spec.startsWith('.')) continue;
        const target = path.relative(SRC, path.resolve(path.dirname(f), spec));
        // The bundled catalog JSON (packs/catalog.json) is data, not code.
        if (spec.endsWith('.json')) continue;
        if (!/^(main|shared|types)([/\\]|$)/.test(target)) offenders.push(`${path.relative(SRC, f)} -> ${spec}`);
      }
    }
    expect(offenders).toEqual([]);
  });

  it('renderer services do not import main-process modules', () => {
    const offenders = files(path.join(SRC, 'services'))
      .flatMap((f) => importsOf(f).map((spec) => ({ f, spec })))
      .filter(({ spec }) => spec.startsWith('@/main/') || spec.includes('/main/packs/'))
      .map(({ f, spec }) => `${path.relative(SRC, f)} -> ${spec}`);
    expect(offenders).toEqual([]);
  });
});
