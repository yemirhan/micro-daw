/**
 * Pure helpers shared by the main process (`fs:list-dir`) and the renderer's
 * sample browser: which files are audio, which are hidden, listing order.
 * No DOM, no Node APIs, no path aliases (the main build resolves relative
 * imports only).
 */

/** Audio file extensions the browser lists (lower-case, no dot). */
export const AUDIO_EXTENSIONS: readonly string[] = ['wav', 'aif', 'aiff', 'mp3', 'flac', 'ogg', 'm4a'];

/** Lower-case extension without the dot ("Kick.WAV" → "wav"); '' when there is none. */
export function extOf(name: string): string {
  const dot = name.lastIndexOf('.');
  if (dot <= 0 || dot === name.length - 1) return '';
  return name.slice(dot + 1).toLowerCase();
}

export function isAudioFileName(name: string): boolean {
  return AUDIO_EXTENSIONS.includes(extOf(name));
}

/** Hidden entries (dotfiles such as .DS_Store, ._foo.wav AppleDouble files). */
export function isHiddenName(name: string): boolean {
  return name.startsWith('.');
}

const collator = new Intl.Collator(undefined, { sensitivity: 'base', numeric: true });

interface NamedEntry {
  name: string;
  isDir: boolean;
}

/** Case-insensitive comparison: folders first, then files, natural number order. */
export function compareEntries(a: NamedEntry, b: NamedEntry): number {
  if (a.isDir !== b.isDir) return a.isDir ? -1 : 1;
  const byName = collator.compare(a.name, b.name);
  return byName !== 0 ? byName : a.name < b.name ? -1 : a.name > b.name ? 1 : 0;
}

/** Returns a new array sorted folders-first, then by name (case-insensitive). */
export function sortEntries<T extends NamedEntry>(entries: readonly T[]): T[] {
  return [...entries].sort(compareEntries);
}
