// Electron side of the phase-7 parity screenshots (docs/parity/phase-7/<state>-electron.png).
// The Rune side is `micro-daw --snap ... --scene <state>` (rune/crates/mdaw-app/src/scenes.rs):
// both put the app in the same state. Run every state with scripts/parity-phase-7/run.sh.
const STATE = process.env.STATE ?? 'beat';
const wait = (ms) => new Promise((r) => setTimeout(r, ms));

async function demoPattern(microDaw) {
  await microDaw((d) => {
    const e = d.arrangementEngine;
    const id = e.getActiveDrumTrackId() ?? e.ensureDrumTrack();
    for (let s = 0; s < 32; s += 4) e.toggleStep(id, 0, s, 0.25);
    for (const s of [4, 12, 20, 28]) e.toggleStep(id, 1, s, 0.25);
    for (let s = 0; s < 32; s += 2) e.toggleStep(id, 2, s, 0.25);
    e.setStepVelocity(id, 2, 6, 0.25, 0.35);
    e.toggleStep(id, 3, 14, 0.25);
    e.toggleStep(id, 3, 30, 0.25);
  });
}

export default async (page, { microDaw }) => {
  for (let i = 0; i < 50 && !(await microDaw((d) => d.isAppReady())); i++) await wait(100);
  await wait(400);
  await page.mouse.click(1439, 899); // focus the page body (no control)
  // The scenes start with the browser closed (its Electron view has no file system).
  if (STATE !== 'browser') {
    await page.keyboard.press('b');
    await wait(150);
  }
  const click = async (sel) => {
    await page.click(sel);
    await wait(250);
  };
  switch (STATE) {
    case 'beat':
    case 'browser':
      break;
    case 'pad-selected':
      // Low Tom (B6): a tonal tail, so both apps render the same length (the factory Snap is
      // Tone.Noise, Math.random, trimmed at -60 dBFS: 210–216 ms from run to run).
      await microDaw((d) => d.arrangementEngine.setSelectedPad(13));
      break;
    case 'pattern':
      await demoPattern(microDaw);
      break;
    case 'playing':
      await demoPattern(microDaw);
      await page.keyboard.press('Space');
      await wait(900);
      break;
    case 'pad-menu':
      await page.mouse.click(104, 415, { button: 'right' });
      await wait(250);
      break;
    case 'kit-menu':
      await click('[aria-label^="Kit:"]');
      break;
    case 'drum-track-menu':
      await click('[aria-label^="Drum track:"]');
      break;
    case 'quantize-menu':
      await click('[aria-label="Quantize"]');
      break;
    case 'chop':
      await click('[aria-label="Chop to pads"]');
      break;
    case 'count-in':
      await page.keyboard.press('r');
      await wait(600);
      break;
    case 'recording':
      await microDaw((d) => d.arrangementEngine.setLoopSettings({ countInBars: 0 }));
      await demoPattern(microDaw);
      await page.keyboard.press('r');
      await wait(900);
      break;
    case 'note-repeat':
      await page.keyboard.press('n');
      break;
    case 'key-hints':
      await click('[aria-label="Computer-key hints"]');
      break;
    case 'layout-2x8':
      await click('[role="radiogroup"][aria-label="Pad layout"] [role="radio"]:nth-child(2)');
      break;
    case 'dock-open':
      await click('[aria-label="Expand instrument dock"]');
      break;
    case 'edited-pad':
      await microDaw((d) => {
        const e = d.arrangementEngine;
        e.setSelectedPad(0);
        e.updatePad(e.getActiveDrumTrackId(), 0, { gainDb: -6, pitch: 5, pan: -0.4, startSec: 0.05 });
      });
      break;
    default:
      throw new Error(`unknown state ${STATE}`);
  }
  await page.mouse.move(1439, 899);
  await wait(300);
};
