/**
 * Shared contract for the beat-making side of micro-daw: the 16-pad sampler,
 * pad/knob MIDI mapping and the loop recorder. Every work package codes
 * against these types; change them only in coordination (see docs/beat-mode-plan.md).
 */

/** Number of sampler pads (2 banks of 8, matching MPK-style controllers). */
export const PAD_COUNT = 16;
/** Pads per bank. Bank A = pads 0..7, bank B = pads 8..15. */
export const PADS_PER_BANK = 8;
/**
 * Note number used *inside drum regions* for pad i (36 + i). This is the
 * arrangement's internal representation, independent of what notes the
 * hardware sends (see PadMidiMap for that). Pads 0..15 → notes 36..51.
 */
export const PAD_BASE_NOTE = 36;

/** 0..15 */
export type PadIndex = number;

export const padToNote = (pad: PadIndex): number => PAD_BASE_NOTE + pad;
export const noteToPad = (note: number): PadIndex | null => {
  const pad = note - PAD_BASE_NOTE;
  return pad >= 0 && pad < PAD_COUNT ? pad : null;
};

/** Built-in synthesized voices (rendered to buffers for the factory kit). */
export type FactoryVoiceId =
  | 'kick' | 'kick-sub' | 'snare' | 'rim' | 'clap' | 'chh' | 'phh' | 'ohh'
  | 'tom-lo' | 'tom-mid' | 'tom-hi' | 'crash' | 'ride' | 'shaker' | 'perc' | 'snap';

export type PadSampleSource =
  | { kind: 'factory'; voice: FactoryVoiceId }
  /** A file on disk. `sampleId` is the SampleManager id once loaded (runtime only, may be stale on reload). */
  | { kind: 'file'; path: string; sampleId?: string };

export interface PadSample {
  source: PadSampleSource;
  /** Display name (file name without extension, or factory voice label). */
  name: string;
}

export type PadPlayMode = 'oneshot' | 'gate';

export interface PadSettings {
  sample: PadSample | null;
  /** -60..+12 dB */
  gainDb: number;
  /** -1..1 */
  pan: number;
  /** Semitones, -24..+24 (playback-rate pitch, like an MPC). */
  pitch: number;
  /** Trim start, seconds into the buffer. */
  startSec: number;
  /** Trim end, seconds into the buffer; null = end of buffer. */
  endSec: number | null;
  /** Amp release/decay in seconds applied at the end of the slice or on gate release. 0.005..4 */
  decay: number;
  /** Lowpass cutoff in Hz, 20..20000 (20000 = open). */
  filterCutoff: number;
  reverse: boolean;
  mode: PadPlayMode;
  /** 0 = none; pads sharing a group > 0 cut each other off (e.g. open/closed hat). */
  chokeGroup: number;
  /** Optional color override (CSS color). */
  color?: string;
}

export interface KitState {
  id: string;
  name: string;
  /** Always PAD_COUNT entries. */
  pads: PadSettings[];
}

/** Parameters a knob (or the pad editor) can change on a pad. */
export type PadParam = 'gainDb' | 'pan' | 'pitch' | 'decay' | 'filterCutoff' | 'startSec';

export type KnobTarget =
  | { kind: 'pad'; param: PadParam } // applies to the currently selected pad
  | { kind: 'swing' }
  | { kind: 'bpm' }
  | { kind: 'masterVolume' }
  | { kind: 'trackVolume' }; // the active drum track

export interface CCMapping {
  cc: number;
  /** 1..16, or null = any channel */
  channel: number | null;
  target: KnobTarget;
}

/** How incoming hardware notes map to pads. */
export interface PadMidiMap {
  /** notes[pad] = incoming MIDI note number that triggers that pad (null = unassigned). */
  notes: (number | null)[];
  /** Only notes on this channel (1..16) are treated as pad hits; null = any channel. */
  channel: number | null;
}

export interface ControllerProfile {
  id: string;
  name: string;
  padMap: PadMidiMap;
  ccMappings: CCMapping[];
}

export type QuantizeGrid = 'off' | '1/4' | '1/8' | '1/8T' | '1/16' | '1/16T' | '1/32';

export interface LoopRecordSettings {
  /** Loop length in bars (4/4). */
  bars: 1 | 2 | 4 | 8;
  quantize: QuantizeGrid;
  /** 0..0.75 – delays every other grid step (MPC-style swing, 0 = straight). */
  swing: number;
  /** Bars of count-in before recording starts (0 = none). */
  countInBars: 0 | 1 | 2;
  metronome: boolean;
  /** true = each loop pass adds hits; false = first pass replaces the pattern. */
  overdub: boolean;
}

export type NoteRepeatRate = '1/4' | '1/8' | '1/8T' | '1/16' | '1/16T' | '1/32';
