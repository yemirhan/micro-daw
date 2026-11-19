// Electron side of the phase-9 parity screenshots (docs/parity/phase-9/<state>-electron.png).
// The Rune side is `micro-daw --snap ... --scene <state>` (rune/crates/mdaw-app/src/scenes_views.rs):
// the same states reached through the UI / the services.
import { readFileSync } from 'node:fs';

const STATE = process.env.STATE ?? 'samples-empty';
const wait = (ms) => new Promise((r) => setTimeout(r, ms));
const TEMPLATE = JSON.parse(readFileSync(new URL('../../src/data/templates/rock-song.json', import.meta.url), 'utf8'));
const CATALOG = JSON.parse(readFileSync(new URL('../../packs/catalog.json', import.meta.url), 'utf8'));

/** A pack API with the bundled catalog and nothing installed (ui-snap has no Electron main). */
async function packsWithCatalog(page) {
  await page.evaluate(async (catalog) => {
    window.electronAPI = {
      ...(window.electronAPI ?? {}),
      packsListInstalled: async () => [],
      onPacksProgress: () => () => {},
      packsDir: async () => '/tmp/packs',
      packsActiveStates: async () => ({}),
      packsFetchCatalog: async () => catalog,
      packsInstall: async () => { throw new Error('offline'); },
      packsCancel: async () => true,
      packsUninstall: async () => {},
      packsFetchPreview: async () => { throw new Error('offline'); },
    };
    const { packService } = await import('/src/services/PackService.ts');
    await packService.init();
    await packService.loadCatalog({ force: true });
  }, CATALOG);
}

async function song(page, microDaw) {
  await microDaw((d, arr) => d.arrangementEngine.loadFromTemplate(arr), TEMPLATE.arrangement);
  await page.keyboard.press('Tab');
  await wait(400);
}

async function generateSine(page, microDaw) {
  await microDaw((d) => d.setMode('samples'));
  await wait(300);
  await page.getByRole('button', { name: 'Generate', exact: true }).click();
  await wait(600);
}

export default async (page, { microDaw }) => {
  for (let i = 0; i < 50 && !(await microDaw((d) => d.isAppReady())); i++) await wait(100);
  await wait(400);
  await page.mouse.click(1439, 899);
  await page.keyboard.press('b'); // the scenes start with the browser closed
  await wait(150);
  switch (STATE) {
    case 'samples-empty':
      await microDaw((d) => d.setMode('samples'));
      break;
    case 'samples-sine':
      await generateSine(page, microDaw);
      await page.getByRole('tab', { name: 'Edit' }).click();
      break;
    case 'samples-trim':
      await generateSine(page, microDaw);
      await page.getByRole('tab', { name: 'Edit' }).click();
      await page.getByLabel('Start (s)').fill('0.2');
      await page.getByLabel('End (s)').fill('0.7');
      await page.mouse.click(1300, 800);
      break;
    case 'samples-process':
      await generateSine(page, microDaw);
      await page.getByRole('tab', { name: 'Process' }).click();
      await page.locator('#fx-filter').click();
      await page.locator('#fx-reverb').click();
      break;
    case 'samples-generate':
      await microDaw((d) => d.setMode('samples'));
      await wait(300);
      await page.getByRole('radio', { name: 'Square' }).click();
      break;
    case 'packs':
      await packsWithCatalog(page);
      await microDaw((d) => d.setMode('packs'));
      break;
    case 'packs-builtin':
      await packsWithCatalog(page);
      await microDaw((d) => d.setMode('packs'));
      await wait(300);
      await page.locator('[data-pack-id="builtin:tr808"] > button').click();
      break;
    case 'packs-remote':
      await packsWithCatalog(page);
      await microDaw((d) => d.setMode('packs'));
      await wait(300);
      await page.locator('section[aria-label="Downloadable packs"] [data-pack-id] > button').first().click();
      break;
    case 'packs-search':
      await packsWithCatalog(page);
      await microDaw((d) => d.setMode('packs'));
      await wait(300);
      await page.getByLabel('Search packs').fill('909');
      break;
    case 'settings':
      await microDaw((d) => d.setMode('settings'));
      break;
    case 'dev':
      await microDaw((d) => d.setMode('dev'));
      break;
    case 'dev-events':
      await microDaw((d) => d.setMode('dev'));
      await wait(300);
      await microDaw((d) => {
        const rows = [
          [[0x90, 60, 100], 1234.5],
          [[0x80, 60, 0], 1480.25],
          [[0xb0, 74, 64], 1600],
          [[0xe0, 0x00, 0x50], 1700.75],
          [[0xc0, 5], 1810],
          [[0x91, 36, 0], 1900.5],
        ];
        const real = performance.now;
        for (const [bytes, t] of rows) {
          performance.now = () => t;
          const message = { rawData: Uint8Array.from(bytes), statusByte: bytes[0], rawDataBytes: Uint8Array.from(bytes.slice(1)) };
          for (const cb of d.midiService.rawCallbacks) cb({ message, timestamp: t });
        }
        performance.now = real;
      });
      await wait(200);
      break;
    case 'palette':
      await page.keyboard.press('Meta+k');
      break;
    case 'palette-search':
      await page.keyboard.press('Meta+k');
      await wait(200);
      await page.keyboard.type('temp');
      break;
    case 'export':
      await song(page, microDaw);
      await page.keyboard.press('Meta+e');
      break;
    case 'templates':
      await page.keyboard.press('Meta+k');
      await wait(200);
      await page.keyboard.type('New from Template');
      await page.keyboard.press('Enter');
      break;
    case 'controller':
      await page.getByRole('button', { name: 'Controller setup' }).first().click();
      break;
    case 'synth-editor':
      await page.getByRole('button', { name: 'Toggle synth editor' }).click();
      break;
    case 'instrument-menu':
      await page.getByRole('combobox', { name: 'Choose instrument sound' }).click();
      break;
    case 'live-fx':
      await song(page, microDaw);
      await page.getByRole('tab', { name: 'LFO' }).click();
      break;
    case 'song-audio':
      await microDaw((d, arr) => d.arrangementEngine.loadFromTemplate(arr), TEMPLATE.arrangement);
      await generateSine(page, microDaw);
      await page.getByRole('tab', { name: 'Edit' }).click();
      await page.getByRole('button', { name: 'Send to arrangement' }).click();
      await wait(300);
      await microDaw((d) => d.setMode('daw'));
      await wait(200);
      await page.keyboard.press('Tab');
      await wait(400);
      break;
    default:
      throw new Error(`unknown state ${STATE}`);
  }
  await wait(500);
};
