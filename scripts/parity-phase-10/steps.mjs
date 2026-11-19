// Electron side of the phase-10 parity screenshots (docs/parity/phase-10/<state>-electron.png).
// The Rune side is `micro-daw --snap ... --scene <state>` (rune/crates/mdaw-app/src/scenes_shell.rs).
// Only the states the renderer draws have an Electron shot: the recent projects in the sidebar
// and in the palette. The unsaved prompt is a native sheet in Electron (not drawn by the
// renderer), the load report / import note / update link do not exist there.
const STATE = process.env.STATE ?? 'sidebar-recent';
const wait = (ms) => new Promise((r) => setTimeout(r, ms));
const RECENT = ['Night Drive', 'Boom Bap Sketch', 'Lo-Fi Study', 'Untitled 3'];

async function recents(page) {
  await page.evaluate(async (names) => {
    const { projectManager } = await import('/src/services/ProjectManager.ts');
    // `addToRecentProjects` puts the newest first: add the oldest first.
    for (const n of [...names].reverse()) {
      projectManager['addToRecentProjects'](`/Users/you/Music/Micro DAW/${n}.mdaw`, '2026-11-10T10:00:00.000Z');
    }
    projectManager['emit']();
  }, RECENT);
}

export default async (page, { microDaw }) => {
  for (let i = 0; i < 50 && !(await microDaw((d) => d.isAppReady())); i++) await wait(100);
  await wait(400);
  await page.mouse.click(1439, 899);
  await page.keyboard.press('b'); // the scenes start with the browser closed
  await wait(150);
  switch (STATE) {
    case 'sidebar-recent':
      await recents(page);
      await page.evaluate(async () => {
        const { projectManager } = await import('/src/services/ProjectManager.ts');
        projectManager['filePath'] = '/Users/you/Music/Micro DAW/Night Drive.mdaw';
        projectManager['emit']();
      });
      await page.getByRole('button', { name: 'Toggle Sidebar' }).first().click();
      await wait(500);
      break;
    case 'palette-recent':
      await recents(page);
      await page.keyboard.press('Meta+k');
      await wait(300);
      await page.keyboard.type('lo-fi');
      await wait(300);
      break;
  }
};
