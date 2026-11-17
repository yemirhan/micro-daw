/**
 * Pack sample references (pure, shared by main, preload and renderer).
 *
 * A pad that plays a pack sound stores `{ kind: 'pack', packId, file }`
 * (PadSampleSource in src/types/sampler.ts): the pack id plus the POSIX path
 * inside the pack, never an absolute path. It resolves against the packs
 * folder (`<userData>/packs`) when loaded, so projects survive a moved user
 * folder, another machine, or an uninstall + reinstall.
 */
import { isSafePackPath, isValidPackId } from './packValidate';
import { separatorOf, trimTrailingSep } from './paths';

/** Command-line switch carrying the packs folder from main to the preload (read synchronously there). */
export const PACKS_DIR_ARG = '--mdaw-packs-dir=';

/** The packs folder from a renderer's argv (preload), or null. */
export function packsDirFromArgv(argv: readonly string[] | undefined): string | null {
  const arg = argv?.find((a) => a.startsWith(PACKS_DIR_ARG));
  const dir = arg?.slice(PACKS_DIR_ARG.length) ?? '';
  return dir.length > 0 ? dir : null;
}

/** `<packsDir>/<packId>/` with a trailing separator (prefix of every file of the pack); null for an unsafe id. */
export function packDirPrefix(packsDir: string, packId: string): string | null {
  if (!packsDir || !isValidPackId(packId)) return null;
  const sep = separatorOf(packsDir);
  return `${trimTrailingSep(packsDir)}${sep}${packId}${sep}`;
}

/**
 * Absolute path of `file` in pack `packId` under `packsDir` (separators follow
 * `packsDir`). Null when the id or the path is unsafe.
 */
export function resolvePackFilePath(packsDir: string, packId: string, file: string): string | null {
  const prefix = packDirPrefix(packsDir, packId);
  if (!prefix || !isSafePackPath(file)) return null;
  return prefix + (separatorOf(packsDir) === '\\' ? file.replace(/\//g, '\\') : file);
}

/**
 * Inverse of `resolvePackFilePath`, for migrating old projects that stored
 * absolute paths into the packs folder: `{ packId, file }` when `absPath` is a
 * safe file inside `<packsDir>/<id>/`, else null.
 */
export function packRefFromPath(packsDir: string | null | undefined, absPath: string): { packId: string; file: string } | null {
  if (!packsDir || !absPath) return null;
  const root = trimTrailingSep(packsDir);
  if (absPath.length <= root.length + 1 || !absPath.startsWith(root)) return null;
  const sepChar = absPath[root.length];
  if (sepChar !== '/' && sepChar !== '\\') return null;
  const segs = absPath.slice(root.length + 1).split(/[\\/]/);
  if (segs.length < 2) return null;
  const [packId, ...rest] = segs;
  const file = rest.join('/');
  if (!isValidPackId(packId) || !isSafePackPath(file)) return null;
  return { packId, file };
}
