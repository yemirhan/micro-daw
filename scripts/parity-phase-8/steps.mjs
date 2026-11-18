// Electron side of the phase-8 parity screenshots (docs/parity/phase-8/<state>-electron.png).
// The Rune side is `micro-daw --snap ... --scene <state>` (rune/crates/mdaw-app/src/scenes_song.rs):
// both open the rock-song template in the Song view and apply the same edits.
import { readFileSync } from 'node:fs';

const STATE = process.env.STATE ?? 'song';
const wait = (ms) => new Promise((r) => setTimeout(r, ms));
const TEMPLATE = JSON.parse(readFileSync(new URL('../../src/data/templates/rock-song.json', import.meta.url), 'utf8'));

export default async (page, { microDaw }) => {
  for (let i = 0; i < 50 && !(await microDaw((d) => d.isAppReady())); i++) await wait(100);
  await wait(400);
  await page.mouse.click(1439, 899);
  await page.keyboard.press('b'); // the scenes start with the browser closed
  await wait(150);
  await microDaw((d, arr) => d.arrangementEngine.loadFromTemplate(arr), TEMPLATE.arrangement);
  await page.keyboard.press('Tab'); // Song view
  await wait(400);
  const ids = await microDaw((d) => Object.fromEntries(d.arrangementEngine.getTracks().map((t) => [t.name, t.id])));
  const first = async (name) => microDaw((d, id) => d.arrangementEngine.getTracks().find((t) => t.id === id).regions[0].id, ids[name]);
  const regions = page.locator('[data-region-id]');
  switch (STATE) {
    case 'song':
      break;
    case 'song-region-selected':
      await regions.nth(0).click({ position: { x: 120, y: 30 } });
      break;
    case 'song-regions-edited': {
      const rd = await first('Drums');
      const rb = await first('Bass');
      await microDaw((d, a) => {
        const e = d.arrangementEngine;
        e.splitRegion(a.d, a.rd, 8);
        e.duplicateRegion(a.b, a.rb);
        e.addTrack('synth', 3);
      }, { d: ids.Drums, b: ids.Bass, rd, rb });
      await wait(200);
      await page.keyboard.press('Meta+a');
      break;
    }
    case 'song-automation':
      await microDaw((d, b) => {
        const e = d.arrangementEngine;
        e.addAutomationLane(b, 'volume');
        e.setAutomationPoint(b, 'volume', 0, 0.8, 1);
        e.setAutomationPoint(b, 'volume', 8, 0.3, 1);
        e.setAutomationPoint(b, 'volume', 16, 0.9, 1);
        e.setAutomationPoint(b, 'volume', 28, 0.5, 1);
      }, ids.Bass);
      break;
    case 'song-markers-loop':
      await microDaw((d) => {
        d.arrangementEngine.setLoopMarkers(8, 24);
        d.arrangementEngine.setLoopEnabled(true);
      });
      break;
    case 'song-track-fx':
      await microDaw((d, b) => {
        d.arrangementEngine.setTrackEffect(b, 'filter', { enabled: true, cutoff: 2400 });
        d.arrangementEngine.setTrackEffect(b, 'reverb', { enabled: true });
      }, ids.Bass);
      await wait(150);
      await page.locator('[aria-label="Track effects"]').nth(1).click();
      break;
    case 'song-piano-roll':
      await regions.nth(1).dblclick({ position: { x: 120, y: 30 } });
      await wait(400);
      break;
    case 'song-piano-velocity':
      await regions.nth(1).dblclick({ position: { x: 120, y: 30 } });
      await wait(400);
      await page.keyboard.press('Meta+a');
      break;
    case 'song-piano-drums':
      await regions.nth(0).dblclick({ position: { x: 120, y: 30 } });
      await wait(400);
      break;
    case 'song-dock-keys':
      await microDaw((d) => {
        d.simulateNoteOn(60, 0.8);
        d.simulateNoteOn(64, 0.8);
      });
      break;
    case 'song-mixer':
      await page.keyboard.press('x');
      break;
    case 'song-empty':
      await microDaw((d) => {
        const e = d.arrangementEngine;
        for (const t of [...e.getTracks()]) e.removeTrack(t.id);
      });
      break;
    case 'song-automation-menu':
      await page.locator('[aria-label="Automation lanes"]').nth(1).click();
      break;
    case 'song-instrument-menu':
      await page.locator('[title="Change instrument"]').nth(1).click();
      break;
    case 'song-region-menu':
      await page.mouse.click(420, 160, { button: 'right' });
      break;
    case 'song-grouped':
      await microDaw((d, a) => d.arrangementEngine.createGroup('Rhythm', [a.d, a.b]), { d: ids.Drums, b: ids.Bass });
      break;
    case 'song-playing':
      await page.keyboard.press('Space');
      await wait(900);
      break;
    default:
      throw new Error(`unknown state ${STATE}`);
  }
  await page.mouse.move(1439, 899);
  await wait(400);
};
