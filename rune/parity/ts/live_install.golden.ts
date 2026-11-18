// @vitest-environment node
/**
 * Live pack install with the Electron installer (network), driven by the Rust test
 * `mdaw-packs/tests/live.rs` (`cargo test -p mdaw-packs --test live -- --ignored`). Does nothing
 * unless MDAW_LIVE_PACK is set:
 *
 * - MDAW_LIVE_PACKS_DIR: install pack MDAW_LIVE_PACK from the real catalog (real `fetchCatalog`,
 *   real fetch) into this folder with the real `PackInstaller`.
 * - MDAW_LIVE_LIST_DIR: list a folder the Rust app installed into and verify every file
 *   (`listInstalled` + `isVerifiedFile`).
 * - MDAW_LIVE_RESUME_DIR: install MDAW_LIVE_PACK into a folder whose staging the Rust app
 *   filled, counting the requests (a resume downloads nothing).
 * - MDAW_LIVE_REPORT: where the JSON report goes.
 */
import fs from 'node:fs/promises';
import path from 'node:path';
import { it } from 'vitest';
import { fetchCatalog } from '@/main/packs/catalog';
import { isVerifiedFile, PackInstaller } from '@/main/packs/installer';

if (typeof globalThis.localStorage?.clear !== 'function') {
  Object.defineProperty(globalThis, 'localStorage', { value: { clear() {} }, configurable: true, writable: true });
}

const env = process.env;

it('live install (Electron installer)', async () => {
  const id = env.MDAW_LIVE_PACK;
  if (!id) return;
  const report: Record<string, unknown> = {};
  const { catalog, source } = await fetchCatalog();
  report.catalogSource = source;
  report.catalogUpdatedAt = catalog.updatedAt;
  const manifest = catalog.packs.find((p) => p.id === id);
  if (!manifest) throw new Error(`no pack ${id} in the catalog`);
  report.version = manifest.version;

  if (env.MDAW_LIVE_PACKS_DIR) {
    const inst = new PackInstaller({ packsDir: env.MDAW_LIVE_PACKS_DIR });
    const p = await inst.install(manifest);
    report.installed = { id: p.id, version: p.version, dir: p.dir, installedAt: p.installedAt };
  }
  if (env.MDAW_LIVE_LIST_DIR) {
    const inst = new PackInstaller({ packsDir: env.MDAW_LIVE_LIST_DIR });
    const list = await inst.listInstalled();
    const verified: Record<string, boolean> = {};
    for (const p of list) {
      for (const f of p.manifest.files) verified[`${p.id}/${f.path}`] = await isVerifiedFile(path.join(p.dir, ...f.path.split('/')), f.size, f.sha256);
    }
    report.listedRust = { packs: list.map((p) => ({ id: p.id, version: p.version, dir: p.dir, installedAt: p.installedAt })), verified };
  }
  if (env.MDAW_LIVE_RESUME_DIR) {
    let requests = 0;
    const inst = new PackInstaller({
      packsDir: env.MDAW_LIVE_RESUME_DIR,
      fetchImpl: (url, init) => {
        requests++;
        return fetch(url, init);
      },
    });
    const p = await inst.install(manifest);
    report.resumed = { id: p.id, requests };
  }
  if (env.MDAW_LIVE_REPORT) await fs.writeFile(env.MDAW_LIVE_REPORT, JSON.stringify(report, null, 2));
}, 300_000);
