/**
 * Pack catalog loading (main process): remote `PACK_CATALOG_URL` with a
 * timeout, validated, falling back to the copy bundled into the main build.
 */
import type { PackCatalog } from '../../types/packs';
import { PACK_CATALOG_URL } from '../../types/packs';
// Bundled by Vite into the main build (works in dev and in the packaged app).
import bundledCatalogJson from '../../../packs/catalog.json';
import { downloadToBuffer, type FetchLike } from './download';
import { MAX_CATALOG_BYTES, mergeCatalogs, validateCatalog } from './validate';

export const CATALOG_TIMEOUT_MS = 8000;

export interface CatalogResult {
  catalog: PackCatalog;
  source: 'remote' | 'bundled';
}

/** The catalog shipped with the app (validated; empty if the bundled file is broken). */
export function bundledCatalog(raw: unknown = bundledCatalogJson): PackCatalog {
  try {
    const { catalog, errors } = validateCatalog(raw);
    if (errors.length) console.warn('[packs] bundled catalog: dropped packs', errors);
    return catalog;
  } catch (err) {
    console.warn('[packs] bundled catalog invalid', err);
    return { version: 1, updatedAt: new Date(0).toISOString(), packs: [] };
  }
}

/** Parses + validates a downloaded catalog body. Throws when the envelope is invalid. */
export function parseCatalog(bytes: Uint8Array): { catalog: PackCatalog; errors: string[] } {
  const raw: unknown = JSON.parse(new TextDecoder().decode(bytes));
  const result = validateCatalog(raw);
  if (result.errors.length) console.warn('[packs] remote catalog: dropped packs', result.errors);
  return result;
}

/**
 * Remote catalog merged over the bundled one (see `mergeCatalogs`): the
 * bundled copy wins when offline, when the remote one is invalid, not newer,
 * or lost every pack; per pack, an invalid or older remote entry keeps the
 * bundled entry.
 */
export async function fetchCatalog(
  opts: { url?: string; fetchImpl?: FetchLike; timeoutMs?: number; bundled?: PackCatalog } = {},
): Promise<CatalogResult> {
  const bundled = opts.bundled ?? bundledCatalog();
  try {
    const bytes = await downloadToBuffer(opts.url ?? PACK_CATALOG_URL, {
      maxBytes: MAX_CATALOG_BYTES,
      signal: AbortSignal.timeout(opts.timeoutMs ?? CATALOG_TIMEOUT_MS),
      idleTimeoutMs: opts.timeoutMs ?? CATALOG_TIMEOUT_MS,
      fetchImpl: opts.fetchImpl,
    });
    const remote = parseCatalog(bytes);
    return mergeCatalogs(bundled, remote.catalog, remote.errors);
  } catch (err) {
    console.warn('[packs] remote catalog unavailable, using bundled copy:', err instanceof Error ? err.message : err);
    return { catalog: bundled, source: 'bundled' };
  }
}
