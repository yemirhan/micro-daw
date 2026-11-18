import { describe, expect, it } from 'vitest';
import { packDirPrefix, packRefFromPath, packsDirFromArgv, PACKS_DIR_ARG, resolvePackFilePath } from './packSource';
import { extOf, isAudioFileName, isHiddenName, sortEntries } from './fsEntries';

describe('pack sample references (P1)', () => {
  it('resolves pack id + pack path against the packs folder (POSIX and Windows)', () => {
    expect(resolvePackFilePath('/u/packs', 'tr808-fischer', 'bass-drum/bd0010.wav')).toBe('/u/packs/tr808-fischer/bass-drum/bd0010.wav');
    expect(resolvePackFilePath('/u/packs/', 'p', 'a.wav')).toBe('/u/packs/p/a.wav');
    expect(resolvePackFilePath('C:\\Users\\me\\packs', 'p', 'kicks/bd.wav')).toBe('C:\\Users\\me\\packs\\p\\kicks\\bd.wav');
    expect(packDirPrefix('/u/packs', 'p')).toBe('/u/packs/p/');
  });

  it('refuses unsafe ids and paths', () => {
    expect(resolvePackFilePath('/u/packs', '../x', 'a.wav')).toBeNull();
    expect(resolvePackFilePath('/u/packs', 'p', '../../etc/passwd.wav')).toBeNull();
    expect(resolvePackFilePath('/u/packs', 'p', '/abs.wav')).toBeNull();
    expect(resolvePackFilePath('', 'p', 'a.wav')).toBeNull();
  });

  it('migrates absolute paths inside the packs folder, and only those', () => {
    expect(packRefFromPath('/u/packs', '/u/packs/tr808-fischer/bass-drum/bd0010.wav')).toEqual({ packId: 'tr808-fischer', file: 'bass-drum/bd0010.wav' });
    expect(packRefFromPath('C:\\u\\packs', 'C:\\u\\packs\\p\\kicks\\bd.wav')).toEqual({ packId: 'p', file: 'kicks/bd.wav' });
    expect(packRefFromPath('/u/packs', '/u/packs-other/p/a.wav')).toBeNull();
    expect(packRefFromPath('/u/packs', '/u/packs/p')).toBeNull();
    expect(packRefFromPath('/u/packs', '/u/packs/.partial-p/a.wav')).toBeNull();
    expect(packRefFromPath('/u/packs', '/u/packs/p/../q/a.wav')).toBeNull();
    expect(packRefFromPath('/u/packs', '/Users/me/kick.wav')).toBeNull();
    expect(packRefFromPath(null, '/u/packs/p/a.wav')).toBeNull();
  });

  it('reads the packs folder from the renderer argv', () => {
    expect(packsDirFromArgv(['electron', `${PACKS_DIR_ARG}/u/packs`])).toBe('/u/packs');
    expect(packsDirFromArgv(['electron'])).toBeNull();
    expect(packsDirFromArgv([PACKS_DIR_ARG])).toBeNull();
    expect(packsDirFromArgv(undefined)).toBeNull();
  });
});

describe('fsEntries (shared with the main process, P3)', () => {
  it('classifies and sorts entries', () => {
    expect(extOf('Kick.WAV')).toBe('wav');
    expect(isAudioFileName('a.flac')).toBe(true);
    expect(isAudioFileName('a.txt')).toBe(false);
    expect(isHiddenName('.DS_Store')).toBe(true);
    expect(sortEntries([{ name: 'b2', isDir: false }, { name: 'B10', isDir: false }, { name: 'z', isDir: true }]).map((e) => e.name)).toEqual(['z', 'b2', 'B10']);
  });
});
