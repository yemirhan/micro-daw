#!/usr/bin/env node
/**
 * Electron file:// smoke test (`npm run test:electron`).
 *
 * Builds the production renderer (vite.renderer.config.mts, base './') and
 * the main + preload bundles the way Forge lays them out:
 *
 *   .vite/smoke/build/main.cjs, preload.cjs
 *   .vite/smoke/renderer/main_window/index.html (+ assets)
 *
 * then launches Electron with scripts/electron-smoke-harness.cjs, which loads
 * the real main process (MICRO_DAW_SMOKE=1: hidden window, no DevTools) and
 * checks the CSP, the navigation / window.open guards and the IPC sender
 * guard (see the harness for the list). Exit code 0 = all checks passed.
 * Needs a desktop session (macOS / Windows; on Linux run under xvfb-run).
 *
 *   node scripts/electron-smoke.mjs              # build + run
 *   node scripts/electron-smoke.mjs --no-build   # reuse the last build
 */
import { spawn } from 'node:child_process';
import { mkdirSync, rmSync, writeFileSync, existsSync } from 'node:fs';
import { createRequire } from 'node:module';
import { builtinModules } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const OUT = join(ROOT, '.vite', 'smoke');
const require = createRequire(import.meta.url);
const args = process.argv.slice(2);

async function buildAll() {
  rmSync(OUT, { recursive: true, force: true });
  mkdirSync(join(OUT, 'build'), { recursive: true });

  const t0 = Date.now();
  const { build: viteBuild } = await import('vite');
  await viteBuild({
    configFile: join(ROOT, 'vite.renderer.config.mts'),
    root: ROOT,
    base: './',
    mode: 'production',
    logLevel: 'warn',
    build: { outDir: join(OUT, 'renderer', 'main_window'), emptyOutDir: true },
  });
  console.log(`[smoke] renderer built (${((Date.now() - t0) / 1000).toFixed(1)}s)`);

  const { build } = await import('rolldown');
  const builtins = new Set([...builtinModules, ...builtinModules.map((m) => `node:${m}`)]);
  const common = {
    platform: 'node',
    // electron + node built-ins + npm packages stay external (resolved from the repo's node_modules).
    external: (id) => id === 'electron' || builtins.has(id) || (!id.startsWith('.') && !id.startsWith('/') && !id.startsWith('@/') && !id.startsWith('\0')),
    transform: { define: { MAIN_WINDOW_VITE_DEV_SERVER_URL: 'undefined', MAIN_WINDOW_VITE_NAME: '"main_window"' } },
    logLevel: 'warn',
  };
  await build({ ...common, input: join(ROOT, 'src', 'main.ts'), output: { file: join(OUT, 'build', 'main.cjs'), format: 'cjs' } });
  await build({ ...common, input: join(ROOT, 'src', 'preload.ts'), output: { file: join(OUT, 'build', 'preload.cjs'), format: 'cjs' } });
  console.log('[smoke] main + preload built');
}

async function main() {
  if (!args.includes('--no-build') || !existsSync(join(OUT, 'build', 'main.cjs'))) await buildAll();
  rmSync(join(OUT, 'userdata'), { recursive: true, force: true });
  rmSync(join(OUT, 'result.json'), { force: true });
  writeFileSync(join(OUT, 'foreign.html'), '<!doctype html><meta charset="utf-8"><title>foreign</title><h1>foreign page</h1>\n');

  const electronBin = require('electron'); // path of the Electron binary
  const child = spawn(electronBin, [join(ROOT, 'scripts', 'electron-smoke-harness.cjs')], {
    cwd: ROOT,
    stdio: ['ignore', 'pipe', 'pipe'],
    env: { ...process.env, MICRO_DAW_SMOKE: '1', MICRO_DAW_SMOKE_OUT: OUT, ELECTRON_ENABLE_LOGGING: '0', ELECTRON_RUN_AS_NODE: '' },
  });
  const verbose = args.includes('--verbose');
  child.stdout.on('data', (d) => process.stdout.write(d));
  child.stderr.on('data', (d) => {
    const text = String(d);
    // Chromium / macOS sandbox noise is only shown with --verbose; [security] logs always.
    if (verbose || /\[security\]|Error|FAIL/.test(text)) process.stderr.write(text.split('\n').filter((l) => verbose || /\[security\]|FAIL|Uncaught/.test(l)).map((l) => `${l}\n`).join(''));
  });
  const code = await new Promise((resolve) => child.on('exit', (c, sig) => resolve(c ?? (sig ? 1 : 0))));
  console.log(`[smoke] result: ${join(OUT, 'result.json')}`);
  process.exit(code);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
