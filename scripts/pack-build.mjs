#!/usr/bin/env node
/**
 * Builds packs/catalog.json from packs/sources/*.json.
 *
 * For every file in every source spec it downloads the commit-pinned raw
 * GitHub URL, records the byte size and sha256, then writes the catalog and
 * validates it (see scripts/pack-lib.mjs, docs/packs.md).
 *
 *   node scripts/pack-build.mjs                 # download everything, write catalog
 *   node scripts/pack-build.mjs --reuse         # reuse size/sha256 from the current catalog when the URL is unchanged
 *   node scripts/pack-build.mjs --only <id>     # (re)download one pack, reuse the rest
 *   node scripts/pack-build.mjs --date 2026-11-17
 */
import { createHash } from 'node:crypto';
import { readFileSync, readdirSync, writeFileSync, existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { checkCatalog, checkSource, manifestFromSource, packBytes, rawUrl, sortSources } from './pack-lib.mjs';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const SOURCES_DIR = join(ROOT, 'packs', 'sources');
const CATALOG_PATH = join(ROOT, 'packs', 'catalog.json');
const CONCURRENCY = 8;

const args = process.argv.slice(2);
const flag = (name) => args.includes(name);
const opt = (name) => {
  const i = args.indexOf(name);
  return i >= 0 ? args[i + 1] : undefined;
};

export function loadSources(dir = SOURCES_DIR) {
  return sortSources(
    readdirSync(dir)
      .filter((f) => f.endsWith('.json'))
      .map((f) => ({ fileName: f, spec: JSON.parse(readFileSync(join(dir, f), 'utf8')) })),
  );
}

async function fetchInfo(url) {
  for (let attempt = 1; ; attempt++) {
    try {
      const res = await fetch(url, { redirect: 'error' });
      if (res.status !== 200) throw new Error(`HTTP ${res.status}`);
      const buf = Buffer.from(await res.arrayBuffer());
      return { size: buf.length, sha256: createHash('sha256').update(buf).digest('hex') };
    } catch (err) {
      if (attempt >= 3) throw new Error(`${url}: ${err.message}`, { cause: err });
      await new Promise((r) => setTimeout(r, 500 * attempt));
    }
  }
}

async function mapLimit(items, limit, fn) {
  const out = new Array(items.length);
  let next = 0;
  await Promise.all(
    Array.from({ length: Math.min(limit, items.length) }, async () => {
      while (next < items.length) {
        const i = next++;
        out[i] = await fn(items[i], i);
      }
    }),
  );
  return out;
}

async function main() {
  const sources = loadSources();
  const sourceErrors = sources.flatMap(({ spec, fileName }) => checkSource(spec, fileName));
  if (sourceErrors.length) {
    console.error(sourceErrors.map((e) => `  ✗ ${e}`).join('\n'));
    process.exit(1);
  }

  const only = opt('--only');
  const reuse = flag('--reuse') || !!only;
  const previous = new Map();
  if (reuse && existsSync(CATALOG_PATH)) {
    for (const p of JSON.parse(readFileSync(CATALOG_PATH, 'utf8')).packs ?? []) {
      for (const f of p.files ?? []) if (f.size && f.sha256) previous.set(f.url, { size: f.size, sha256: f.sha256 });
    }
  }

  const packs = [];
  for (const { spec } of sources) {
    const info = new Map();
    const todo = [];
    for (const f of spec.files) {
      const url = rawUrl(spec.upstream.repo, spec.upstream.commit, f.from);
      const prev = previous.get(url);
      if (prev && (!only || only !== spec.id)) info.set(f.path, prev);
      else todo.push({ path: f.path, url });
    }
    process.stdout.write(`${spec.id}: ${spec.files.length} files (${todo.length} to download)… `);
    await mapLimit(todo, CONCURRENCY, async ({ path, url }) => info.set(path, await fetchInfo(url)));
    const manifest = manifestFromSource(spec, (p) => info.get(p));
    console.log(`${(packBytes(manifest) / 1048576).toFixed(1)} MB, ${manifest.kits.length} kit(s)`);
    packs.push(manifest);
  }

  const catalog = {
    version: 1,
    updatedAt: opt('--date') ?? new Date().toISOString().slice(0, 10),
    packs,
  };
  const errors = checkCatalog(catalog);
  if (errors.length) {
    console.error(errors.map((e) => `  ✗ ${e}`).join('\n'));
    process.exit(1);
  }
  writeFileSync(CATALOG_PATH, `${JSON.stringify(catalog, null, 2)}\n`);
  console.log(`Wrote ${CATALOG_PATH} (${packs.length} packs)`);
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main().catch((err) => {
    console.error(err);
    process.exit(1);
  });
}
