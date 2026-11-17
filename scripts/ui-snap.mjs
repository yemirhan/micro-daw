#!/usr/bin/env node
/**
 * ui-snap — screenshot / drive the micro-daw renderer in headless Chromium,
 * without Electron.
 *
 * Starts its OWN Vite dev server for the renderer (vite.renderer.config.mts) on a
 * free port, so several copies can run concurrently. Opens headless Chromium,
 * clicks through the StartScreen, optionally switches app mode, runs your
 * snippet/steps, saves a screenshot, prints console errors + page errors, and
 * shuts everything down.
 *
 * Usage:
 *   npm run snap -- [options]
 *   node scripts/ui-snap.mjs [options]
 *
 * Options:
 *   --mode <m>        daw | samples | settings | dev | all
 *                     (default: stay on whatever the app shows after Start = daw).
 *                     "all" snaps every mode; --out is then a directory.
 *   --out <path>      Screenshot path (default /tmp/micro-daw-snaps/<mode|start>.png).
 *                     With --mode all: output dir (default /tmp/micro-daw-snaps),
 *                     files are <dir>/<prefix><mode>.png.
 *   --prefix <p>      File name prefix for --mode all (default "").
 *   --eval "<js>"     JS evaluated in the page after the mode switch (awaited;
 *                     the result is printed). window.__microDaw is available.
 *   --steps <file>    ES module whose default export is
 *                     `async (page, { snap, log, microDaw }) => {}`; runs after --eval.
 *                     `snap(name)` saves an extra screenshot next to --out
 *                     (<out-dir>/<name>.png); `microDaw(fn, arg)` = page.evaluate
 *                     with fn(window.__microDaw, arg).
 *   --wait <ms>       Settle time before the final screenshot (default 700).
 *   --width <px>      Viewport width (default 1440; the app's min width is 1360).
 *   --height <px>     Viewport height (default 900).
 *   --scale <n>       deviceScaleFactor (default 1).
 *   --full-page       Capture the full scrollable page.
 *   --no-start        Don't click Start; screenshot the StartScreen.
 *   --headed          Show the browser window (debugging).
 *   --verbose         Also print console warnings/logs and Vite logs.
 *   --strict          Exit with code 2 if any console error / page error was seen.
 *
 * Examples:
 *   npm run snap -- --mode samples --out /tmp/samples.png
 *   npm run snap -- --mode all --prefix baseline-
 *   npm run snap -- --mode daw --eval "window.__microDaw.simulateNoteOn(60, 0.9)"
 *   npm run snap -- --mode daw --steps ./my-steps.mjs --out /tmp/p.png
 *
 *   // my-steps.mjs
 *   export default async (page, { snap, microDaw }) => {
 *     await microDaw((d) => d.simulateNoteOn(64, 0.8));
 *     await snap('note-held');
 *     await microDaw((d) => d.simulateNoteOff(64));
 *   };
 *
 * Exit codes: 0 ok, 1 script/navigation failure, 2 errors seen with --strict.
 *
 * Notes: window.__microDaw (src/dev/debugHandle.ts) exposes the service
 * singletons + simulateNoteOn/Off (same path as real MIDI input), setMode,
 * getMode, getActiveNotes, isAppReady. The StartScreen button is located via
 * [data-testid="start-button"] (keep that attribute when redesigning).
 * Audio runs for real in Chromium (autoplay allowed) but nothing is heard
 * headless; Web MIDI has no devices, which the app tolerates.
 */
import { mkdir } from 'node:fs/promises';
import net from 'node:net';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { parseArgs } from 'node:util';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const MODES = ['daw', 'samples', 'settings', 'dev'];
const DEFAULT_DIR = '/tmp/micro-daw-snaps';

const { values: args } = parseArgs({
  options: {
    mode: { type: 'string' },
    out: { type: 'string' },
    prefix: { type: 'string', default: '' },
    eval: { type: 'string' },
    steps: { type: 'string' },
    wait: { type: 'string', default: '700' },
    width: { type: 'string', default: '1440' },
    height: { type: 'string', default: '900' },
    scale: { type: 'string', default: '1' },
    'full-page': { type: 'boolean', default: false },
    'no-start': { type: 'boolean', default: false },
    headed: { type: 'boolean', default: false },
    verbose: { type: 'boolean', default: false },
    strict: { type: 'boolean', default: false },
    help: { type: 'boolean', short: 'h', default: false },
  },
  allowPositionals: false,
});

if (args.help) {
  const { readFile } = await import('node:fs/promises');
  const src = await readFile(fileURLToPath(import.meta.url), 'utf8');
  console.log(src.split('*/')[0].replace(/^#!.*\n\/\*\*\n?/, '').replace(/^ \* ?/gm, ''));
  process.exit(0);
}

if (args.mode && args.mode !== 'all' && !MODES.includes(args.mode)) {
  console.error(`[ui-snap] unknown --mode "${args.mode}". Use one of: ${MODES.join(', ')}, all`);
  process.exit(1);
}

const log = (...m) => console.log('[ui-snap]', ...m);

async function getFreePort() {
  return new Promise((resolve, reject) => {
    const srv = net.createServer();
    srv.unref();
    srv.on('error', reject);
    srv.listen(0, '127.0.0.1', () => {
      const { port } = srv.address();
      srv.close(() => resolve(port));
    });
  });
}

async function startVite() {
  const { createServer } = await import('vite');
  const port = await getFreePort();
  const server = await createServer({
    root: ROOT,
    configFile: path.join(ROOT, 'vite.renderer.config.mts'),
    // Separate dep-optimizer cache from `npm start`, shared between ui-snap runs.
    cacheDir: path.join(ROOT, 'node_modules/.vite-ui-snap'),
    mode: 'development',
    logLevel: args.verbose ? 'info' : 'error',
    clearScreen: false,
    server: {
      host: '127.0.0.1',
      port,
      strictPort: false,
      hmr: false,
      watch: null,
    },
  });
  await server.listen();
  const url = server.resolvedUrls?.local?.[0] ?? `http://127.0.0.1:${port}/`;
  return { server, url };
}

const errors = [];
let server;
let browser;
let exitCode = 0;

async function shutdown() {
  await browser?.close().catch(() => {});
  await server?.close().catch(() => {});
}

process.on('SIGINT', async () => {
  await shutdown();
  process.exit(130);
});

try {
  const { chromium } = await import('playwright');

  const started = Date.now();
  const vite = await startVite();
  server = vite.server;
  const url = vite.url;
  log(`vite dev server at ${url}`);

  browser = await chromium.launch({
    headless: !args.headed,
    args: ['--autoplay-policy=no-user-gesture-required', '--use-fake-ui-for-media-stream'],
  });
  const context = await browser.newContext({
    viewport: { width: Number(args.width), height: Number(args.height) },
    deviceScaleFactor: Number(args.scale),
    colorScheme: 'dark',
  });
  await context.grantPermissions(['midi', 'midi-sysex'], { origin: new URL(url).origin }).catch(() => {});

  await context.addInitScript(() => {
    try {
      const key = 'micro-daw-settings';
      const cur = JSON.parse(localStorage.getItem(key) || '{}');
      cur.general = { ...(cur.general || {}), autoCheckUpdates: false };
      localStorage.setItem(key, JSON.stringify(cur));
    } catch {
      /* ignore */
    }
  });

  const page = await context.newPage();
  page.on('console', (msg) => {
    const type = msg.type();
    if (type === 'error') {
      errors.push(`console.error: ${msg.text()}`);
    } else if (args.verbose && (type === 'warning' || type === 'log' || type === 'info')) {
      console.log(`[page:${type}] ${msg.text()}`);
    }
  });
  page.on('pageerror', (err) => errors.push(`pageerror: ${err.stack || err.message}`));
  page.on('requestfailed', (req) => {
    if (args.verbose) console.log(`[page:requestfailed] ${req.url()} ${req.failure()?.errorText ?? ''}`);
  });

  await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 120_000 });

  const startBtn = page
    .locator('[data-testid="start-button"]')
    .or(page.getByRole('button', { name: /start/i }))
    .first();
  await startBtn.waitFor({ state: 'visible', timeout: 120_000 });

  if (!args['no-start']) {
    await startBtn.click();
    await page.waitForFunction(() => window.__microDaw?.isAppReady?.() === true, null, { timeout: 30_000 });
    log(`app ready (${((Date.now() - started) / 1000).toFixed(1)}s)`);
  } else {
    await page.waitForTimeout(800); // let the intro animation settle
  }

  const microDaw = (fn, arg) =>
    page.evaluate(
      ([src, a]) => {
        const f = new Function(`return (${src})`)();
        return f(window.__microDaw, a);
      },
      [fn.toString(), arg],
    );

  async function setMode(mode) {
    await page.evaluate((m) => window.__microDaw.setMode(m), mode);
    await page.waitForFunction((m) => window.__microDaw.getMode() === m, mode, { timeout: 10_000 });
  }

  const modes = args.mode === 'all' ? MODES : args.mode ? [args.mode] : [null];
  const outFor = (mode) => {
    if (args.mode === 'all') return path.resolve(args.out ?? DEFAULT_DIR, `${args.prefix}${mode}.png`);
    return path.resolve(args.out ?? path.join(DEFAULT_DIR, `${mode ?? (args['no-start'] ? 'start' : 'daw')}.png`));
  };

  for (const mode of modes) {
    if (mode && !args['no-start']) {
      await setMode(mode);
      log(`mode -> ${mode}`);
    }
    const out = outFor(mode);
    await mkdir(path.dirname(out), { recursive: true });

    if (args.eval) {
      const result = await page.evaluate(async (code) => {
        // Accept an expression or a statement body (use `return` for a result).
        let fn;
        try {
          fn = new Function(`return (async () => (${code}))()`);
        } catch {
          fn = new Function(`return (async () => { ${code} })()`);
        }
        const r = await fn();
        try {
          return JSON.stringify(r);
        } catch {
          return String(r);
        }
      }, args.eval);
      log(`eval result: ${result}`);
    }

    if (args.steps) {
      const mod = await import(pathToFileURL(path.resolve(args.steps)).href);
      const steps = mod.default ?? mod.steps;
      if (typeof steps !== 'function') throw new Error(`--steps module must export a default async function`);
      const snap = async (name, opts = {}) => {
        const file = path.join(path.dirname(out), name.endsWith('.png') ? name : `${name}.png`);
        await page.screenshot({ path: file, fullPage: args['full-page'], ...opts });
        log(`extra screenshot: ${file}`);
        return file;
      };
      await steps(page, { snap, log, microDaw, setMode, url });
    }

    await page.waitForTimeout(Number(args.wait));
    await page.screenshot({ path: out, fullPage: args['full-page'] });
    log(`screenshot: ${out}`);
  }
} catch (err) {
  console.error('[ui-snap] FAILED:', err?.stack || err);
  exitCode = 1;
} finally {
  if (errors.length) {
    console.log(`[ui-snap] ${errors.length} console/page error(s):`);
    for (const e of errors) console.log('  - ' + e.split('\n').join('\n    '));
    if (args.strict && exitCode === 0) exitCode = 2;
  } else {
    console.log('[ui-snap] no console errors / page errors');
  }
  await shutdown();
}
process.exit(exitCode);
