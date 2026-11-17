#!/usr/bin/env node
/**
 * Validates packs/catalog.json (and that it is in sync with packs/sources).
 *
 *   node scripts/pack-check.mjs            # schema, unique ids, kit files exist, pinned URLs, sizes, sync with sources
 *   node scripts/pack-check.mjs --online   # + HEAD every URL: HTTP 200 and Content-Length == size
 *   node scripts/pack-check.mjs --deep     # + download every file and verify sha256
 *
 * Exit code 1 on any problem. See docs/packs.md.
 */
import { createHash } from 'node:crypto';
import { readFileSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { checkCatalog, checkSource, checkSync, packBytes, sortSources } from './pack-lib.mjs';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const CATALOG_PATH = join(ROOT, 'packs', 'catalog.json');
const SOURCES_DIR = join(ROOT, 'packs', 'sources');
const CONCURRENCY = 8;

const args = process.argv.slice(2);
const deep = args.includes('--deep');
const online = deep || args.includes('--online');

async function checkUrl(file) {
  for (let attempt = 1; ; attempt++) {
    try {
      const res = await fetch(file.url, { method: deep ? 'GET' : 'HEAD', redirect: 'error' });
      if (res.status !== 200) return `${file.url}: HTTP ${res.status}`;
      if (deep) {
        const buf = Buffer.from(await res.arrayBuffer());
        if (buf.length !== file.size) return `${file.url}: size ${buf.length} != ${file.size}`;
        const sha = createHash('sha256').update(buf).digest('hex');
        if (sha !== file.sha256) return `${file.url}: sha256 mismatch`;
      } else {
        // Text files are served gzip-encoded; Content-Length is then the compressed size.
        const len = Number(res.headers.get('content-length'));
        const encoded = (res.headers.get('content-encoding') ?? 'identity') !== 'identity';
        if (len && !encoded && len !== file.size) return `${file.url}: Content-Length ${len} != ${file.size}`;
      }
      return null;
    } catch (err) {
      if (attempt >= 3) return `${file.url}: ${err.message}`;
      await new Promise((r) => setTimeout(r, 500 * attempt));
    }
  }
}

async function main() {
  const catalog = JSON.parse(readFileSync(CATALOG_PATH, 'utf8'));
  const sources = sortSources(
    readdirSync(SOURCES_DIR)
      .filter((f) => f.endsWith('.json'))
      .map((f) => ({ fileName: f, spec: JSON.parse(readFileSync(join(SOURCES_DIR, f), 'utf8')) })),
  );
  const errors = [
    ...checkCatalog(catalog),
    ...sources.flatMap(({ spec, fileName }) => checkSource(spec, fileName)),
    ...checkSync(
      sources.map((s) => s.spec),
      catalog,
    ),
  ];
  const order = sources.map((s) => s.spec.id).join(',');
  if (order !== (catalog.packs ?? []).map((p) => p.id).join(',')) errors.push('catalog pack order differs from sources (run node scripts/pack-build.mjs --reuse)');

  for (const p of catalog.packs ?? []) {
    const sounds = p.files.filter((f) => !/\.(md|txt)$/.test(f.path)).length;
    console.log(`  ${p.id.padEnd(28)} ${String(sounds).padStart(4)} sounds  ${(packBytes(p) / 1048576).toFixed(1).padStart(5)} MB  ${p.kits.length} kit(s)  ${p.license}`);
  }

  if (online && errors.length === 0) {
    const files = catalog.packs.flatMap((p) => p.files);
    process.stdout.write(`Checking ${files.length} URLs ${deep ? '(download + sha256)' : '(HEAD)'}… `);
    let next = 0;
    const urlErrors = [];
    await Promise.all(
      Array.from({ length: CONCURRENCY }, async () => {
        while (next < files.length) {
          const err = await checkUrl(files[next++]);
          if (err) urlErrors.push(err);
        }
      }),
    );
    console.log(urlErrors.length ? `${urlErrors.length} failed` : 'all OK');
    errors.push(...urlErrors);
  }

  if (errors.length) {
    console.error(errors.map((e) => `  ✗ ${e}`).join('\n'));
    process.exit(1);
  }
  console.log(`catalog OK (${catalog.packs.length} packs${online ? ', URLs verified' : ''})`);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
