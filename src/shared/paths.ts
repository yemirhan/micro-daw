/**
 * Pure path helpers for code that runs in the renderer (no node:path) and is
 * shared with the main process. POSIX and Windows separators.
 */

/** Separator used by a path: '\\' for Windows-style paths, otherwise '/'. */
export function separatorOf(p: string): '/' | '\\' {
  return p.includes('\\') && !p.includes('/') ? '\\' : '/';
}

/** Removes trailing separators, keeping filesystem roots ("/", "C:\\") intact. */
export function trimTrailingSep(p: string): string {
  let out = p;
  while (out.length > 1 && /[\\/]$/.test(out) && !/^[A-Za-z]:[\\/]$/.test(out)) out = out.slice(0, -1);
  return out;
}

/** Last path segment ("/a/b/c" → "c"). */
export function basename(p: string): string {
  const trimmed = trimTrailingSep(p);
  const idx = Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\'));
  const base = idx >= 0 ? trimmed.slice(idx + 1) : trimmed;
  return base || trimmed;
}

export function joinPath(dir: string, name: string): string {
  const sep = separatorOf(dir);
  const base = trimTrailingSep(dir);
  return /[\\/]$/.test(base) ? `${base}${name}` : `${base}${sep}${name}`;
}

/** File name without extension ("Kick 01.wav" → "Kick 01"). */
export function displayName(fileName: string): string {
  const dot = fileName.lastIndexOf('.');
  return dot > 0 ? fileName.slice(0, dot) : fileName;
}
