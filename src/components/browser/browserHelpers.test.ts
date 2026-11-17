import { describe, expect, it } from 'vitest';
import {
  basename,
  buildBreadcrumbs,
  displayName,
  extOf,
  filterEntries,
  findRootFor,
  isAudioFileName,
  isHiddenName,
  isWithin,
  joinPath,
  moveIndex,
  normalizeRoots,
  parentPath,
  sortEntries,
} from './browserHelpers';

const f = (name: string) => ({ name, isDir: false });
const d = (name: string) => ({ name, isDir: true });

describe('extensions / names', () => {
  it('extracts lower-case extensions', () => {
    expect(extOf('Kick.WAV')).toBe('wav');
    expect(extOf('a.b.aiff')).toBe('aiff');
    expect(extOf('noext')).toBe('');
    expect(extOf('.hidden')).toBe('');
    expect(extOf('trailing.')).toBe('');
  });

  it('recognises audio files', () => {
    for (const n of ['a.wav', 'a.aif', 'a.AIFF', 'a.mp3', 'a.flac', 'a.ogg', 'a.m4a']) expect(isAudioFileName(n)).toBe(true);
    expect(isAudioFileName('a.txt')).toBe(false);
    expect(isAudioFileName('wav')).toBe(false);
  });

  it('flags dotfiles as hidden', () => {
    expect(isHiddenName('.DS_Store')).toBe(true);
    expect(isHiddenName('._kick.wav')).toBe(true);
    expect(isHiddenName('kick.wav')).toBe(false);
  });

  it('strips the extension for display', () => {
    expect(displayName('Kick 01.wav')).toBe('Kick 01');
    expect(displayName('noext')).toBe('noext');
  });
});

describe('sortEntries', () => {
  it('puts folders first, then files, case-insensitive with natural numbers', () => {
    const out = sortEntries([f('snare.wav'), d('zeta'), f('Kick 10.wav'), d('Alpha'), f('kick 2.wav'), d('beta')]);
    expect(out.map((e) => e.name)).toEqual(['Alpha', 'beta', 'zeta', 'kick 2.wav', 'Kick 10.wav', 'snare.wav']);
  });

  it('does not mutate the input and is deterministic for case-only differences', () => {
    const input = [f('b.wav'), f('B.wav'), f('a.wav')];
    const out = sortEntries(input);
    expect(input.map((e) => e.name)).toEqual(['b.wav', 'B.wav', 'a.wav']);
    expect(out.map((e) => e.name)).toEqual(['a.wav', 'B.wav', 'b.wav']);
  });
});

describe('filterEntries', () => {
  const list = [f('Kick Hard.wav'), f('Snare Tight.wav'), d('Kicks'), f('hat closed.wav')];

  it('returns everything for an empty query', () => {
    expect(filterEntries(list, '   ')).toHaveLength(4);
  });

  it('matches case-insensitively', () => {
    expect(filterEntries(list, 'KICK').map((e) => e.name)).toEqual(['Kick Hard.wav', 'Kicks']);
  });

  it('requires every term', () => {
    expect(filterEntries(list, 'kick hard').map((e) => e.name)).toEqual(['Kick Hard.wav']);
    expect(filterEntries(list, 'kick snare')).toEqual([]);
  });
});

describe('paths', () => {
  it('basename / parentPath on POSIX paths', () => {
    expect(basename('/Users/me/Samples/')).toBe('Samples');
    expect(basename('/')).toBe('/');
    expect(parentPath('/Users/me/Samples')).toBe('/Users/me');
    expect(parentPath('/Users')).toBe('/');
    expect(parentPath('/')).toBeNull();
  });

  it('basename / parentPath on Windows paths', () => {
    expect(basename('C:\\Samples\\Drums')).toBe('Drums');
    expect(parentPath('C:\\Samples\\Drums')).toBe('C:\\Samples');
    expect(parentPath('C:\\Samples')).toBe('C:\\');
    expect(parentPath('C:\\')).toBeNull();
  });

  it('joinPath uses the path separator', () => {
    expect(joinPath('/a/b', 'c')).toBe('/a/b/c');
    expect(joinPath('/a/b/', 'c')).toBe('/a/b/c');
    expect(joinPath('/', 'c')).toBe('/c');
    expect(joinPath('C:\\x', 'y')).toBe('C:\\x\\y');
  });

  it('isWithin does not match sibling prefixes', () => {
    expect(isWithin('/a/b', '/a/b')).toBe(true);
    expect(isWithin('/a/b', '/a/b/c')).toBe(true);
    expect(isWithin('/a/b', '/a/bc')).toBe(false);
    expect(isWithin('/', '/a')).toBe(true);
  });

  it('findRootFor picks the deepest containing root', () => {
    const roots = ['/m', '/m/drums', '/x'];
    expect(findRootFor(roots, '/m/drums/kicks')).toBe('/m/drums');
    expect(findRootFor(roots, '/m/keys')).toBe('/m');
    expect(findRootFor(roots, '/y')).toBeNull();
  });
});

describe('buildBreadcrumbs', () => {
  it('builds a trail from the root to the current folder', () => {
    expect(buildBreadcrumbs('/Users/me/Splice', '/Users/me/Splice/sounds/packs')).toEqual([
      { name: 'Splice', path: '/Users/me/Splice' },
      { name: 'sounds', path: '/Users/me/Splice/sounds' },
      { name: 'packs', path: '/Users/me/Splice/sounds/packs' },
    ]);
  });

  it('is just the root when at the root', () => {
    expect(buildBreadcrumbs('/a/b/', '/a/b')).toEqual([{ name: 'b', path: '/a/b' }]);
  });

  it('handles Windows paths', () => {
    expect(buildBreadcrumbs('D:\\Samples', 'D:\\Samples\\Kicks')).toEqual([
      { name: 'Samples', path: 'D:\\Samples' },
      { name: 'Kicks', path: 'D:\\Samples\\Kicks' },
    ]);
  });

  it('returns [] outside the root', () => {
    expect(buildBreadcrumbs('/a/b', '/a/c')).toEqual([]);
  });
});

describe('normalizeRoots', () => {
  it('keeps unique non-empty strings without trailing separators', () => {
    expect(normalizeRoots(['/a/', '/a', 3, '', '/b', null])).toEqual(['/a', '/b']);
  });

  it('rejects non-arrays', () => {
    expect(normalizeRoots('nope')).toEqual([]);
    expect(normalizeRoots(null)).toEqual([]);
  });
});

describe('moveIndex', () => {
  it('clamps to the list', () => {
    expect(moveIndex(0, -1, 5)).toBe(0);
    expect(moveIndex(4, 1, 5)).toBe(4);
    expect(moveIndex(2, 10, 5)).toBe(4);
  });

  it('starts from the ends when nothing is selected', () => {
    expect(moveIndex(-1, 1, 5)).toBe(0);
    expect(moveIndex(-1, -1, 5)).toBe(4);
    expect(moveIndex(0, 1, 0)).toBe(-1);
  });
});
