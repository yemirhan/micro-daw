// Writes a synthetic Electron profile whose localStorage holds every micro-daw key, through
// Electron's own Chromium (so the LevelDB layout is the real one). Used by the Rust app's
// settings-import test (phase 10).
//
//   node_modules/.bin/electron scripts/electron-profile-fixture/main.cjs <out profile dir>
//
// The page is loaded from a file:// URL like the packaged app (origin "file://").
const { app, BrowserWindow } = require('electron');
const path = require('node:path');
const fs = require('node:fs');

const out = path.resolve(process.argv[process.argv.length - 1]);
fs.mkdirSync(out, { recursive: true });
app.setPath('userData', out);

app.whenReady().then(async () => {
  const win = new BrowserWindow({ show: false, webPreferences: { sandbox: true, contextIsolation: true } });
  await win.loadFile(path.join(__dirname, 'page.html'));
  const ok = await win.webContents.executeJavaScript(`(() => {
    const items = {
      'micro-daw-arrangement': JSON.stringify({ id: 'imp-arr', name: 'Imported', bpm: 97, lengthBeats: 32, tracks: [] }),
      'micro-daw-recent-projects': JSON.stringify([
        { filePath: '/Users/şule/Müzik/Ritim.mdaw', name: 'Ritim', modifiedAt: '2026-11-01T10:00:00.000Z' },
        { filePath: '/tmp/old.mdaw', name: 'old', modifiedAt: '2026-10-01T10:00:00.000Z' },
      ]),
      'micro-daw-settings': JSON.stringify({ general: { autoCheckUpdates: false }, audio: { defaultMasterVolume: -9, bufferSizeHint: 512 }, midi: { autoConnectLastDevice: false } }),
      'micro-daw-sample-library': JSON.stringify([{ sampleId: 's1', samplePath: '/tmp/kick.wav', sampleName: 'kick', addedAt: 1 }]),
      'micro-daw-piano-roll': JSON.stringify({ snap: 0.5, drawLength: 2, follow: false }),
      'micro-daw:controller': JSON.stringify({ version: 1, profile: null }),
      'micro-daw:kits': '[]',
      'micro-daw:pack-names': JSON.stringify({ 'lofi-kit': 'Lo-Fi Kit' }),
      'micro-daw:masterVolume': '-7',
      'micro-daw:browser-open': 'false',
      'micro-daw:browser-width': '300',
      'micro-daw:browser-roots': JSON.stringify(['/Users/şule/Samples']),
      'micro-daw:browser-last-path': '/Users/şule/Samples',
      'micro-daw:browser-auto-preview': '0',
      'micro-daw:dock-pad-bank': 'B',
      'micro-daw:beat-pad-layout': '2x8',
      'micro-daw:beat-step-grid': '1/32',
      'micro-daw:beat-key-hints': 'on',
      'micro-daw:midi-split-point': '52',
      'unrelated-key': 'x',
    };
    for (const [k, v] of Object.entries(items)) localStorage.setItem(k, v);
    // An overwritten value: only the last one may be read back.
    localStorage.setItem('micro-daw:beat-step-grid', '1/8');
    return localStorage.length;
  })()`);
  console.log('[fixture] items written:', ok);
  win.destroy();
  // localStorage is committed to LevelDB on shutdown.
  setTimeout(() => app.quit(), 300);
});
