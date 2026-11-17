/**
 * Dev/QA helpers (not imported by the app). Load from the dev server in a
 * browser, e.g. `await import('/src/audio/qa.ts')`, to render instruments
 * offline and measure their levels -- used to calibrate preset gains and to
 * check nothing clips or is silent.
 */
import * as Tone from 'tone';
import { SYNTH_PRESETS, FALLBACK_PIANO } from './presets';
import type { SynthPreset } from '@/types/audio';
import { createSynthFromPreset } from '@/utils/synthFactory';
import { DrumKit } from './drumKit';
import { installMasterBus } from './masterBus';
import { loadPianoSamples } from './pianoSamples';
import { DEFAULT_VOLUME } from '@/utils/constants';

export interface LevelStats {
  peakDb: number;
  rmsDb: number;
  /** RMS over the first 400 ms (perceived "hit" loudness). */
  attackRmsDb: number;
  /** RMS over 1.0-2.0 s (sustain loudness for pads). */
  sustainRmsDb: number;
  /** Seconds until the signal falls below -70 dBFS for good. */
  tail: number;
}

const toDb = (x: number) => (x > 0 ? 20 * Math.log10(x) : -Infinity);

function stats(buffer: Tone.ToneAudioBuffer): LevelStats {
  const chans = buffer.toArray() as Float32Array[] | Float32Array;
  const data = Array.isArray(chans) ? chans : [chans];
  const sr = buffer.sampleRate;
  let peak = 0;
  let sum = 0;
  let n = 0;
  let attackSum = 0;
  let attackN = 0;
  let susSum = 0;
  let susN = 0;
  let lastLoud = 0;
  const thresh = Math.pow(10, -70 / 20);
  for (const ch of data) {
    for (let i = 0; i < ch.length; i++) {
      const a = Math.abs(ch[i]);
      if (a > peak) peak = a;
      sum += ch[i] * ch[i];
      n++;
      if (i < sr * 0.45) {
        attackSum += ch[i] * ch[i];
        attackN++;
      } else if (i >= sr * 1.05 && i < sr * 2.05) {
        susSum += ch[i] * ch[i];
        susN++;
      }
      if (a > thresh && i > lastLoud) lastLoud = i;
    }
  }
  return {
    peakDb: toDb(peak),
    rmsDb: toDb(Math.sqrt(sum / Math.max(1, n))),
    attackRmsDb: toDb(Math.sqrt(attackSum / Math.max(1, attackN))),
    sustainRmsDb: toDb(Math.sqrt(susSum / Math.max(1, susN))),
    tail: lastLoud / sr,
  };
}

/**
 * Render a preset offline. `notes` play together for `hold` seconds.
 * `master`: route through the master bus (as the app does) at the engine's
 * default volume, else measure the raw instrument output.
 */
export async function renderPreset(
  index: number | SynthPreset,
  { notes = [60], velocity = 0.9, hold = 1, duration = 3, master = false } = {},
): Promise<LevelStats> {
  const preset = typeof index === 'number' ? SYNTH_PRESETS[index] : index;
  if (preset.engine === 'sampler') await loadPianoSamples();
  const buf = await Tone.Offline(({ destination }) => {
    let out: Tone.ToneAudioNode = destination;
    if (master) {
      installMasterBus(destination);
      const vol = new Tone.Volume(DEFAULT_VOLUME).connect(destination);
      out = vol;
    }
    const inst = createSynthFromPreset(preset);
    inst.connect(out);
    for (const n of notes) inst.noteOn(n, velocity, 0.05);
    for (const n of notes) inst.noteOff(n, 0.05 + hold);
  }, duration);
  return stats(buf);
}

export async function renderDrum(pad: number, velocity = 1): Promise<LevelStats> {
  const buf = await Tone.Offline(() => {
    const kit = new DrumKit();
    kit.connect(Tone.getDestination());
    kit.trigger(pad as 0, velocity, 0.05);
  }, 2.5);
  return stats(buf);
}

/** Level table for every preset: single note + 4-note chord. */
export async function presetLevelTable(opts: { master?: boolean } = {}) {
  const rows = [];
  for (let i = 0; i < SYNTH_PRESETS.length; i++) {
    const p = SYNTH_PRESETS[i];
    const lowCat = p.category === 'bass';
    const single = await renderPreset(i, { notes: [lowCat ? 36 : 60], master: opts.master });
    const chord = await renderPreset(i, { notes: lowCat ? [36] : [48, 60, 64, 67, 72], master: opts.master });
    rows.push({ i, id: p.id, engine: p.engine, gain: p.gain, single, chord });
  }
  return rows;
}

/** Worst case: 10-finger fortissimo chord through the master bus at 0 dB volume. */
export async function stressChord(index: number): Promise<LevelStats> {
  const preset = SYNTH_PRESETS[index];
  if (preset.engine === 'sampler') await loadPianoSamples();
  const buf = await Tone.Offline(({ destination }) => {
    installMasterBus(destination);
    const inst = createSynthFromPreset(preset);
    inst.connect(destination);
    const notes = [36, 43, 48, 52, 55, 60, 64, 67, 72, 76];
    for (const n of notes) inst.noteOn(n, 1, 0.05);
    for (const n of notes) inst.noteOff(n, 1.5);
  }, 3);
  return stats(buf);
}

/** Perceived loudness used for calibration: louder of the attack (0-450 ms) and the sustain (1-2 s). */
export function loudness(s: LevelStats): number {
  return Math.max(s.attackRmsDb, s.sustainRmsDb);
}

/** Single note (C4, C2 for basses) held 2 s at velocity 0.9 -> loudness + suggested gain for `target` dB. */
export async function calibrate(target = -12) {
  const out: { id: string; gain: number; loudness: number; suggested: number; peakDb: number }[] = [];
  for (const p of [...SYNTH_PRESETS, FALLBACK_PIANO]) {
    const note = p.category === 'bass' ? 36 : 60;
    const s = await renderPreset(p, { notes: [note], hold: 2, duration: 3.5 });
    const l = loudness(s);
    out.push({ id: p.id, gain: p.gain, loudness: l, suggested: Math.round((p.gain + target - l) * 2) / 2, peakDb: s.peakDb });
  }
  return out;
}
