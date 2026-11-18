import { it } from 'vitest';
import { isBlackKey, midiToFrequency, midiToNoteName, noteNameToMidi, velocityToNormalized } from '@/utils/noteHelpers';
import { computePeaksFromBuffer } from '@/utils/waveformHelpers';
import { detectChord } from '@/utils/chordDetection';
import { BUFFER_SPECS, N, genBuffer, mulberry32 } from './bufgen';
import { writeFixture } from './fixture';

it('dumps note / chord / buffer-peak vectors', () => {
  const out: Record<string, unknown> = {};

  const midis: number[] = [];
  for (let m = -30; m <= 160; m++) midis.push(m);
  midis.push(0.5, 60.5, -0.5, 1e6, -1e6, 1e21 * 12, -0);
  out.midiToNoteName = midis.map((m) => [N(m), midiToNoteName(m)]);

  const names: string[] = [];
  for (const n of ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B', 'Db', 'E#', 'B#', 'H', 'c', 'c#', '']) {
    for (const o of ['-1', '-2', '0', '4', '9', '10', '-', '', '4 ', ' 4', '-0', '٣']) names.push(`${n}${o}`);
  }
  names.push('C4x', 'xC4', 'C##4', 'C#-1', 'G9', 'A-9');
  out.noteNameToMidi = names.map((s) => [s, noteNameToMidi(s)]);

  out.isBlackKey = midis.filter(Number.isInteger).map((m) => [m, isBlackKey(m)]);
  out.midiToFrequency = [...midis.filter((m) => Math.abs(m) < 1000), 69.5, 0.25].map((m) => [m, midiToFrequency(m)]);
  out.velocityToNormalized = [-10, -0, 0, 1, 63.5, 64, 100, 126.9, 127, 128, 1000, NaN].map((v) => [N(v), N(velocityToNormalized(v))]);

  out.bufferSpecs = BUFFER_SPECS;
  const peaks = [];
  BUFFER_SPECS.forEach((spec, b) => {
    const buf = genBuffer(spec) as unknown as AudioBuffer;
    for (const n of [1, 2, 3, 7, 64, 100, 257, 1000]) peaks.push([b, n, Array.from(computePeaksFromBuffer(buf, n))]);
  });
  out.computePeaksFromBuffer = peaks;

  const sets: number[][] = [[], [60], [60, 72], [60, 64], [60, 67], [60, 64, 67], [64, 67, 72], [57, 60, 64], [59, 62, 65], [67, 71, 74, 77], [60, 64, 67, 71], [-12, -8, -5], [-1, 3, 6], [-3, 0, 4]];
  for (let root = 0; root < 12; root++) {
    for (const pat of [[0, 4, 7], [0, 3, 7], [0, 3, 6], [0, 4, 8], [0, 2, 7], [0, 5, 7], [0, 7], [0, 4, 7, 10], [0, 3, 7, 10], [0, 4, 7, 11], [0, 3, 6, 9], [0, 3, 6, 10], [0, 1], [0, 1, 2], [0, 4, 7, 9], [0, 2, 4, 7]]) {
      sets.push(pat.map((i) => 48 + root + i));
      sets.push(pat.map((i, k) => 36 + root + i + 12 * (k % 3)).reverse());
    }
  }
  const rnd = mulberry32(7);
  for (let i = 0; i < 1500; i++) {
    const n = 1 + Math.floor(rnd() * 6);
    sets.push(Array.from({ length: n }, () => Math.floor(rnd() * 128)));
  }
  for (let i = 0; i < 200; i++) {
    const n = 2 + Math.floor(rnd() * 3);
    sets.push(Array.from({ length: n }, () => Math.floor(rnd() * 60) - 30));
  }
  out.detectChord = sets.map((s) => [s, detectChord(s)]);

  writeFixture('notes', out);
});
