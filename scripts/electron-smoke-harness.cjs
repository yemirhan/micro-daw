/**
 * Electron smoke harness (run by scripts/electron-smoke.mjs, not directly).
 *
 * Loads the REAL built main process (`$MICRO_DAW_SMOKE_OUT/build/main.cjs`,
 * which loads the production renderer over file://), drives it, and checks:
 *
 *   1. CSP: no Content-Security-Policy violation while the app boots and is
 *      used (Start, pads, Packs page, classic kit render worker); then that
 *      the CSP really blocks eval, inline/remote scripts, remote fetch/images
 *      and frames.
 *   2. Navigation guards: file://, https:// and sibling-file navigations are
 *      blocked; window.open never creates a window (https goes to
 *      shell.openExternal, stubbed here); <iframe> to a remote page is refused.
 *   3. IPC sender guard: a foreign file:// page with the app's preload gets
 *      "Untrusted IPC sender" from every handler and cannot force-close the
 *      app; the real renderer still works after a reload.
 *
 * Writes `$MICRO_DAW_SMOKE_OUT/result.json` and exits 0 when every check
 * passed, 1 otherwise.
 */
/* eslint-disable @typescript-eslint/no-require-imports */
const electron = require('electron');
const path = require('node:path');
const fs = require('node:fs');

const { app, BrowserWindow, shell } = electron;
const OUT = process.env.MICRO_DAW_SMOKE_OUT;
if (!OUT) {
  console.error('MICRO_DAW_SMOKE_OUT is not set (run scripts/electron-smoke.mjs)');
  process.exit(2);
}
const TIMEOUT_MS = Number(process.env.MICRO_DAW_SMOKE_TIMEOUT_MS || 120000);

const result = { checks: [], console: [], cspConsole: [], external: [], windowsCreated: 0 };
let phase = 'boot';
const check = (name, ok, detail) => {
  result.checks.push({ name, ok: !!ok, detail });
  console.log(`${ok ? 'PASS' : 'FAIL'} ${name}${detail !== undefined ? ` — ${JSON.stringify(detail).slice(0, 300)}` : ''}`);
};
let finished = false;
const finish = (code) => {
  if (finished) return;
  finished = true;
  const failed = result.checks.filter((c) => !c.ok);
  result.ok = code === 0 && failed.length === 0;
  fs.writeFileSync(path.join(OUT, 'result.json'), JSON.stringify(result, null, 2));
  console.log(`\n${result.ok ? 'ELECTRON SMOKE OK' : 'ELECTRON SMOKE FAILED'} (${result.checks.length - failed.length}/${result.checks.length} checks)`);
  app.exit(result.ok ? 0 : 1);
};
setTimeout(() => {
  check('finished before timeout', false, { phase });
  finish(1);
}, TIMEOUT_MS).unref();
process.on('uncaughtException', (err) => {
  check('no uncaught exception in main', false, String(err && err.stack ? err.stack : err).slice(0, 400));
  finish(1);
});

app.setPath('userData', path.join(OUT, 'userdata'));
shell.openExternal = async (url) => {
  result.external.push(url);
};
app.on('browser-window-created', () => {
  result.windowsCreated++;
});
app.on('web-contents-created', (_e, wc) => {
  wc.on('console-message', (ev, level, message) => {
    const msg = String(typeof ev?.message === 'string' ? ev.message : message);
    const lvl = ev?.level ?? level;
    if (/Content Security Policy|Refused to/.test(msg)) result.cspConsole.push({ phase, msg: msg.slice(0, 300) });
    if (lvl === 'error' || lvl === 3) result.console.push({ phase, msg: msg.slice(0, 300) });
  });
});

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function run() {
  // The app's main window (created by main.cjs on 'ready').
  let win = BrowserWindow.getAllWindows()[0];
  for (let i = 0; !win && i < 100; i++) {
    await sleep(100);
    win = BrowserWindow.getAllWindows()[0];
  }
  if (!win) {
    check('main window created', false);
    return finish(1);
  }
  const wc = win.webContents;
  if (wc.isLoading()) await new Promise((r) => wc.once('did-finish-load', r));
  const js = (code) => wc.executeJavaScript(code, true);
  const waitFor = async (expr, ms = 15000) => {
    const t0 = Date.now();
    while (Date.now() - t0 < ms) {
      if (await js(expr).catch(() => false)) return true;
      await sleep(100);
    }
    return false;
  };
  const appUrl = wc.getURL();
  check('renderer loads over file://', appUrl.startsWith('file://') && appUrl.endsWith('/index.html'), appUrl.replace(OUT, '<out>'));
  check('preload API exposed', (await js('typeof window.electronAPI')) === 'object');
  check('no dev debug handle in production', (await js('typeof window.__microDaw')) === 'undefined');
  await js(`window.__csp = []; document.addEventListener('securitypolicyviolation', (e) => window.__csp.push(e.violatedDirective + ' ' + String(e.blockedURI).slice(0, 80))); 0`);

  // --- 1. Normal use: no CSP violation ---
  phase = 'use';
  await js(`document.querySelector('[data-testid="start-button"]')?.click(); 0`);
  check('Start shows the pads', await waitFor(`!!document.querySelector('[data-pad]')`), { pads: await js(`document.querySelectorAll('[data-pad]').length`) });
  await sleep(800);
  const packsNav = await js(`(() => { const b = document.querySelector('button[aria-label="Packs"]'); if (b) b.click(); return !!b; })()`);
  if (packsNav) {
    const cards = await waitFor(`!!document.querySelector('[data-pack-id]')`);
    check('Packs page lists packs (catalog via IPC)', cards, { cards: await js(`document.querySelectorAll('[data-pack-id]').length`) });
    // Load a built-in classic kit: renders its voices in a blob: worker under the CSP.
    const opened = await js(`(() => { const c = document.querySelector('[data-pack-id^="builtin:"] > button'); if (c) c.click(); return !!c; })()`);
    await sleep(400);
    const loaded =
      opened && (await js(`(() => { const b = [...document.querySelectorAll('aside button')].find((x) => x.textContent.trim() === 'Load on pads'); if (b) b.click(); return !!b; })()`));
    if (loaded) {
      await waitFor(`!!document.querySelector('[data-pad]')`);
      await sleep(2500); // classic voices render in the worker
    } else {
      console.log('note: could not find a built-in kit "Load on pads" button; skipped the worker render step');
    }
  } else {
    console.log('note: no Packs nav button found; skipped the Packs page step');
  }
  const catalog = await js(`window.electronAPI.packsFetchCatalog().then((c) => c.packs.length, (e) => 'ERR ' + e.message)`);
  check('pack catalog loads in main (remote or bundled)', typeof catalog === 'number' && catalog > 0, catalog);
  const useViolations = [...(await js('window.__csp.slice()')), ...result.cspConsole.filter((c) => c.phase !== 'probe').map((c) => c.msg)];
  check('no CSP violations while booting and using the app', useViolations.length === 0, useViolations);
  const useErrors = result.console.filter((c) => c.phase !== 'probe' && !/Pack not installed|MissingPackSample|net::ERR_|Failed to load resource/.test(c.msg));
  check('no renderer console errors while using the app', useErrors.length === 0, useErrors);

  // --- CSP enforcement probes ---
  phase = 'probe';
  const csp = await js(`(async () => {
    const r = {};
    try { r.eval = String(eval('1+1')); } catch (e) { r.eval = 'blocked'; }
    try { r.newFunction = String(new Function('return 2')()); } catch (e) { r.newFunction = 'blocked'; }
    r.fetchRemote = await fetch('https://example.com/').then(() => 'ALLOWED', () => 'blocked');
    const s = document.createElement('script'); s.textContent = 'window.__inline = 1'; document.head.appendChild(s);
    r.inlineScript = window.__inline === 1 ? 'RAN' : 'blocked';
    r.remoteScript = await new Promise((res) => { const t = document.createElement('script'); t.src = 'https://cdnjs.cloudflare.com/ajax/libs/lodash.js/4.17.21/lodash.min.js'; t.onload = () => res('LOADED'); t.onerror = () => res('blocked'); document.head.appendChild(t); setTimeout(() => res('timeout'), 4000); });
    r.remoteImg = await new Promise((res) => { const i = new Image(); i.onload = () => res('LOADED'); i.onerror = () => res('blocked'); i.src = 'https://example.com/x.png'; setTimeout(() => res('timeout'), 4000); });
    const f = document.createElement('iframe'); f.src = 'https://example.com/'; document.body.appendChild(f);
    await new Promise((x) => setTimeout(x, 600));
    f.remove();
    r.violations = window.__csp.length;
    return r; })()`);
  for (const k of ['eval', 'newFunction', 'fetchRemote', 'inlineScript', 'remoteScript', 'remoteImg']) check(`CSP blocks ${k}`, csp[k] === 'blocked', csp[k]);
  check('CSP reports the blocked probes', csp.violations >= 5, csp.violations);
  check('remote <iframe> refused (frame-src none)', result.cspConsole.some((c) => c.phase === 'probe' && /frame-src|Framing/.test(c.msg)));

  // --- 2. Navigation / window.open guards ---
  phase = 'nav';
  const urlNow = () => wc.getURL();
  await js(`location.href = 'file:///etc/hosts'; 0`).catch(() => undefined);
  await sleep(500);
  check('file:// navigation blocked', urlNow() === appUrl, urlNow());
  await js(`location.assign('https://example.com/'); 0`).catch(() => undefined);
  await sleep(500);
  check('https navigation blocked', urlNow() === appUrl, urlNow());
  const sibling = path.join(OUT, 'foreign.html');
  await js(`location.href = ${JSON.stringify(`file://${sibling}`)}; 0`).catch(() => undefined);
  await sleep(500);
  check('navigation to a sibling file:// page blocked', urlNow() === appUrl, urlNow());
  const before = BrowserWindow.getAllWindows().length;
  await js(`window.open('https://example.org/popup'); window.open('file:///etc/passwd'); window.open('http://example.org/plain'); window.open('https://u:p@example.net/cred'); 0`);
  await sleep(600);
  check('window.open never creates a window', BrowserWindow.getAllWindows().length === before);
  check('only plain https links go to the system browser', JSON.stringify(result.external) === JSON.stringify(['https://example.org/popup']), result.external);
  check('app still running after navigation attempts', await js(`!!document.querySelector('[data-pad]')`));

  // --- 3. IPC sender guard ---
  phase = 'ipc';
  const foreign = new BrowserWindow({ show: false, webPreferences: { preload: path.join(OUT, 'build', 'preload.cjs'), sandbox: true, contextIsolation: true } });
  await foreign.loadFile(sibling);
  const fj = (code) => foreign.webContents.executeJavaScript(code, true);
  const calls = {
    readFile: 'window.electronAPI.sampleReadFile("/etc/hosts")',
    listDir: 'window.electronAPI.fsListDir("/")',
    homeDir: 'window.electronAPI.fsHomeDir()',
    projectSave: `window.electronAPI.projectSave(${JSON.stringify(path.join(OUT, 'pwned.mdaw'))}, "x")`,
    packsDir: 'window.electronAPI.packsDir()',
    packsInstall: 'window.electronAPI.packsInstall("uzu-drumkit")',
    packsList: 'window.electronAPI.packsListInstalled()',
  };
  for (const [name, expr] of Object.entries(calls)) {
    const r = await fj(`(async () => { try { const api = window.electronAPI; if (!api) return 'NO API'; await (${expr}); return 'ALLOWED'; } catch (e) { return 'ERR ' + e.message; } })()`);
    check(`foreign page IPC refused: ${name}`, /Untrusted IPC sender/.test(r), r);
  }
  check('foreign page could not write a file', !fs.existsSync(path.join(OUT, 'pwned.mdaw')));
  await fj('window.electronAPI && window.electronAPI.forceClose && window.electronAPI.forceClose(); 0').catch(() => undefined);
  await sleep(400);
  check('foreign page cannot force-close the app', !win.isDestroyed());
  foreign.destroy();

  wc.reload();
  await new Promise((r) => wc.once('did-finish-load', r));
  await sleep(300);
  const trusted = await js(`window.electronAPI.packsListInstalled().then((l) => 'OK ' + l.length, (e) => 'ERR ' + e.message)`);
  check('app renderer stays trusted after reload', /^OK/.test(trusted), trusted);

  finish(0);
}

app.whenReady().then(() => {
  setTimeout(() => {
    run().catch((err) => {
      check('harness ran', false, String(err && err.stack ? err.stack : err).slice(0, 400));
      finish(1);
    });
  }, 300);
});

require(path.join(OUT, 'build', 'main.cjs'));
