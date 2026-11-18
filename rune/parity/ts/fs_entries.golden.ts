/**
 * Golden vectors for rune/crates/mdaw-model/src/fs_entries.rs (src/shared/fsEntries.ts):
 * sortEntries over tricky names (numbers, case, accents, punctuation, dots, expansions),
 * pairwise collator results for every Latin-1 / Latin Extended character against a set of
 * probes, and ext / audio / hidden flags.
 */
import { it } from 'vitest';
import { extOf, isAudioFileName, isHiddenName, sortEntries } from '@/shared/fsEntries';
import { writeFixture } from './fixture';

const TRICKY = [
  'Kick 10.wav', 'kick 2.wav', 'Kick 1.wav', 'kick 01.wav', 'kick 001.wav', 'kick1.wav', 'kick-1.wav', 'kick_1.wav', 'kick.1.wav', 'Kick.wav', 'kick.WAV',
  'KICK.wav', 'kíck.wav', 'kïck 2.wav', 'Ärger.wav', 'arger.wav', 'Zebra.wav', 'zebra.wav', 'éclair.wav', 'eclair.wav', 'Eclair.wav', 'straße.wav',
  'strasse.wav', 'Straße 2.wav', 'æther.wav', 'aether.wav', 'Œuvre.wav', 'oeuvre.wav', 'øre.wav', 'ore.wav', 'ñandu.wav', 'nandu.wav', '808 bd.wav',
  '808bd.wav', '80 8.wav', '9.wav', '10.wav', '010.wav', '1.5.wav', '1.10.wav', '1.05.wav', 'a.b.c.wav', 'a b.wav', 'ab.wav', 'a-b.wav', 'a_b.wav',
  'a(1).wav', 'a[1].wav', 'a{1}.wav', 'a@b.wav', 'a#b.wav', 'a$b.wav', 'a%b.wav', 'a&b.wav', 'a+b.wav', 'a=b.wav', 'a~b.wav', 'a!b.wav', "a'b.wav",
  'a,b.wav', 'a;b.wav', '_hidden.wav', '-dash.wav', '(paren).wav', '~tilde.wav', 'Ω.wav', 'Ж.wav', '音.wav', 'snare 12345678901234567890.wav',
  'snare 12345678901234567891.wav', 'snare 9.wav', 'ĳssel.wav', 'ijssel.wav', 'þorn.wav', 'thorn.wav', 'đuro.wav', 'duro.wav', 'Łódź.wav', 'lodz.wav',
  'ı.wav', 'i.wav', 'İ.wav', 'x­Y.wav', 'xy.wav', 'µ.wav', 'm.wav', '²nd.wav', '2nd.wav', '½.wav', 'ﬁle.wav', 'file.wav', 'á.wav', 'á.wav',
];

function rng(seed: number) {
  return () => {
    seed = (seed * 1103515245 + 12345) & 0x7fffffff;
    return seed / 0x7fffffff;
  };
}

it('dumps fs entry vectors', () => {
  const sorts: { input: { name: string; isDir: boolean }[]; sorted: string[] }[] = [];
  const all = TRICKY.map((name, i) => ({ name: i % 7 === 0 ? name.replace('.wav', '') : name, isDir: i % 7 === 0 }));
  sorts.push({ input: all, sorted: sortEntries(all).map((e) => e.name) });
  const r = rng(42);
  for (let k = 0; k < 60; k++) {
    const pick = Array.from({ length: 2 + Math.floor(r() * 12) }, () => ({ name: TRICKY[Math.floor(r() * TRICKY.length)], isDir: r() < 0.2 }));
    sorts.push({ input: pick, sorted: sortEntries(pick).map((e) => e.name) });
  }

  const collator = new Intl.Collator(undefined, { sensitivity: 'base', numeric: true });
  const pairs: [string, string, number][] = [];
  const probes = ['a', 'e', 'o', 's', 'z', 'ss', 'ae', '0', '9', '10', ' ', '_', '-', '.', '~', '$', 'µ', 'þ', 'Ʒ'];
  for (let cp = 0x20; cp < 0x250; cp++) {
    const c = String.fromCharCode(cp);
    for (const p of probes) pairs.push([c, p, collator.compare(c, p)]);
    pairs.push([`x${c}1`, 'x1', collator.compare(`x${c}1`, 'x1')]);
  }
  const words = ['file 2', 'file 10', 'File 2', 'file 02', 'file2b', 'file 2a', 'a1b2', 'a01b02', 'a1b10', '', ' ', 'a ', 'a', 'A', 'á', 'b', '0', '00', '007', '7', 'x9y', 'x10y'];
  for (const a of words) for (const b of words) pairs.push([a, b, collator.compare(a, b)]);

  const names = ['Kick.WAV', '.wav', 'kick.', 'kick', 'a.b.Mp3', 'x.aiff', 'x.aif', 'x.wave', 'x.txt', 'x.flac', 'x.ogg', 'x.m4a', '.DS_Store', '._kick.wav', 'kick.wav', 'é.WAV', 'a.WaV ', ''].map((name) => ({
    name,
    ext: extOf(name),
    audio: isAudioFileName(name),
    hidden: isHiddenName(name),
  }));

  writeFixture('fs_entries', { sorts, pairs, names });
});
