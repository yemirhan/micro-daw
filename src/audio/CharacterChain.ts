import * as Tone from 'tone';
import type { CharacterParams } from '@/types/audio';
import { clamp, safeDispose } from './voices/common';

/**
 * Common output trim for every instrument. Presets are loudness-matched with
 * their own `gain` (see src/audio/qa.ts `calibrate`) so a single note at
 * velocity 0.9 sits around -12 dBFS RMS at the instrument output, i.e. about
 * -24 dBFS RMS at the live engine's default -12 dB volume -- chords and
 * several tracks still leave headroom before the master bus.
 */
export const INSTRUMENT_HEADROOM_DB = 0;

const DEFAULT_CHARACTER: CharacterParams = {
  drive: 0,
  chorus: 0,
  tremolo: 0,
  tremoloRate: 4.5,
  tone: 20000,
  presence: 0,
};

export function withCharacterDefaults(c: Partial<CharacterParams> | undefined): CharacterParams {
  return { ...DEFAULT_CHARACTER, ...(c ?? {}) };
}

/** Soft saturation curve with roughly level-matched output for typical voice levels. */
export function driveCurve(drive: number): (x: number) => number {
  const k = 1 + clamp(drive, 0, 1) * 9;
  const ref = 0.35;
  const norm = ref / Math.tanh(k * ref);
  return (x: number) => clamp(Math.tanh(k * x) * norm, -1, 1);
}

/**
 * Instrument output processing, built only from the stages a preset uses:
 *   input -> [drive] -> [presence shelf] -> [tone lowpass] -> [tremolo] -> [chorus] -> trim -> output
 */
export class CharacterChain {
  readonly input: Tone.Gain;
  readonly output: Tone.Volume;
  private c: CharacterParams;
  private drive: Tone.WaveShaper | null = null;
  private presence: Tone.Filter | null = null;
  private tone: Tone.Filter | null = null;
  private tremolo: Tone.Tremolo | null = null;
  private chorus: Tone.Chorus | null = null;
  private driveAmount = 0;

  constructor(character: Partial<CharacterParams> | undefined, gainDb: number) {
    this.c = withCharacterDefaults(character);
    this.input = new Tone.Gain(1);
    this.output = new Tone.Volume(INSTRUMENT_HEADROOM_DB + gainDb);
    this.sync(true);
  }

  update(character: Partial<CharacterParams> | undefined, gainDb: number): void {
    this.c = withCharacterDefaults(character);
    this.output.volume.rampTo(INSTRUMENT_HEADROOM_DB + gainDb, 0.05);
    this.sync(false);
  }

  dispose(): void {
    safeDispose(this.drive, this.presence, this.tone, this.tremolo, this.chorus, this.input, this.output);
  }

  /** Create/remove stages to match params; rewire if the topology changed. */
  private sync(force: boolean): void {
    const c = this.c;
    let changed = force;

    if (c.drive > 0.001) {
      if (!this.drive) {
        this.drive = new Tone.WaveShaper(driveCurve(c.drive), 2048);
        this.drive.oversample = '2x';
        changed = true;
      } else if (Math.abs(c.drive - this.driveAmount) > 1e-4) {
        this.drive.setMap(driveCurve(c.drive), 2048);
      }
      this.driveAmount = c.drive;
    } else if (this.drive) {
      safeDispose(this.drive);
      this.drive = null;
      changed = true;
    }

    if (Math.abs(c.presence) > 0.05) {
      if (!this.presence) {
        this.presence = new Tone.Filter({ type: 'highshelf', frequency: 3200, gain: c.presence });
        changed = true;
      } else {
        this.presence.gain.rampTo(c.presence, 0.05);
      }
    } else if (this.presence) {
      safeDispose(this.presence);
      this.presence = null;
      changed = true;
    }

    if (c.tone < 19000) {
      const f = clamp(c.tone, 200, 20000);
      if (!this.tone) {
        this.tone = new Tone.Filter({ type: 'lowpass', frequency: f, rolloff: -12, Q: 0.3 });
        changed = true;
      } else {
        this.tone.frequency.rampTo(f, 0.05);
      }
    } else if (this.tone) {
      safeDispose(this.tone);
      this.tone = null;
      changed = true;
    }

    if (c.tremolo > 0.001) {
      if (!this.tremolo) {
        this.tremolo = new Tone.Tremolo({ frequency: c.tremoloRate, depth: c.tremolo, spread: 140 }).start();
        changed = true;
      } else {
        this.tremolo.depth.rampTo(clamp(c.tremolo, 0, 1), 0.05);
        this.tremolo.frequency.rampTo(clamp(c.tremoloRate, 0.1, 20), 0.05);
      }
    } else if (this.tremolo) {
      safeDispose(this.tremolo);
      this.tremolo = null;
      changed = true;
    }

    if (c.chorus > 0.001) {
      const amt = clamp(c.chorus, 0, 1);
      if (!this.chorus) {
        this.chorus = new Tone.Chorus({
          frequency: 0.6,
          delayTime: 3.2,
          depth: 0.25 + 0.45 * amt,
          spread: 180,
          wet: 0.2 + 0.35 * amt,
        }).start();
        changed = true;
      } else {
        this.chorus.depth = 0.25 + 0.45 * amt;
        this.chorus.wet.rampTo(0.2 + 0.35 * amt, 0.05);
      }
    } else if (this.chorus) {
      safeDispose(this.chorus);
      this.chorus = null;
      changed = true;
    }

    if (changed) this.rewire();
  }

  private rewire(): void {
    const stages = [this.drive, this.presence, this.tone, this.tremolo, this.chorus].filter(
      (n): n is NonNullable<typeof n> => n !== null,
    );
    this.input.disconnect();
    for (const s of stages) s.disconnect();
    let prev: Tone.ToneAudioNode = this.input;
    for (const s of stages) {
      prev.connect(s);
      prev = s;
    }
    prev.connect(this.output);
  }
}
