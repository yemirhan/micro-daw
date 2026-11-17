/**
 * Pack catalog loading (main process): remote `PACK_CATALOG_URL` with a
 * timeout, validated, falling back to the copy bundled into the main build.
 */
import type { PackCatalog } from '../../types/packs';
import { PACK_CATALOG_URL } from '../../types/packs';
// Bundled by Vite into the main build (works in dev and in the packaged app).
import bundledCatalogJson from '../../../packs/catalog.json';
import { downloadToBuffer, type FetchLike } from './download';
import { MAX_CATALOG_BYTES, validateCatalog } from './validate';

export const CATALOG_TIMEOUT_MS = 8000;

export interface CatalogResult {
  catalog: PackCatalog;
  source: 'remote' | 'bundled';
}

/** The catalog shipped with the app (validated; empty if the bundled file is broken). */
export function bundledCatalog(): PackCatalog {
  try {
    const { catalog, errors } = validateCatalog(bundledCatalogJson as unknown);
    if (errors.length) console.warn('[packs] bundled catalog: dropped packs', errors);
    return catalog;
  } catch (err) {
    console.warn('[packs] bundled catalog invalid', err);
    return { version: 1, updatedAt: new Date(0).toISOString(), packs: [] };
  }
}

/** Parses + validates a downloaded catalog body. Throws when the envelope is invalid. */
export function parseCatalog(bytes: Uint8Array): PackCatalog {
  const raw: unknown = JSON.parse(new TextDecoder().decode(bytes));
  const { catalog, errors } = validateCatalog(raw);
  if (errors.length) console.warn('[packs] remote catalog: dropped packs', errors);
  return catalog;
}

/**
 * Remote catalog, or the bundled one when offline / invalid / older. A remote
 * catalog that validates but lost every pack (all invalid) is not trusted
 * over a non-empty bundled one.
 */
export async function fetchCatalog(opts: { url?: string; fetchImpl?: FetchLike; timeoutMs?: number } = {}): Promise<CatalogResult> {
  const bundled = bundledCatalog();
  try {
    const bytes = await downloadToBuffer(opts.url ?? PACK_CATALOG_URL, {
      maxBytes: MAX_CATALOG_BYTES,
      signal: AbortSignal.timeout(opts.timeoutMs ?? CATALOG_TIMEOUT_MS),
      idleTimeoutMs: opts.timeoutMs ?? CATALOG_TIMEOUT_MS,
      fetchImpl: opts.fetchImpl,
    });
    const remote = parseCatalog(bytes);
    if (remote.packs.length === 0 && bundled.packs.length > 0) return { catalog: bundled, source: 'bundled' };
    return { catalog: remote, source: 'remote' };
  } catch (err) {
    console.warn('[packs] remote catalog unavailable, using bundled copy:', err instanceof Error ? err.message : err);
    return { catalog: bundled, source: 'bundled' };
  }
}
