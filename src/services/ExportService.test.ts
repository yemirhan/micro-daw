import { describe, expect, it, vi } from 'vitest';
import type { Arrangement, RegionNote, Track } from '@/types/arrangement';
import { DEFAULT_TRACK_EFFECTS } from '@/types/effects';
import { padToNote, type PadSettings } from '@/types/sampler';
import { createFactoryKit } from '@/audio/sampler/factoryKit';
import { normalizeBeatState } from '@/utils/arrangementHelpers';
import { effectsTailSeconds, exportMidi, getArrangementEndSeconds } from './ExportService';

/** midi-writer-js stand-in: records the note events written. */
const midiEvents = vi.hoisted(() => [] as { pitch: unknown; startTick: number; duration: string }[]);
vi.mock('midi-writer-js', () => {
  class Track {
    setTempo() {}
    addTrackName() {}
    addEvent(e: { opts: { pitch: unknown; startTick: number; duration: string } }) {
      midiEvents.push(e.opts);
    }
  }
  class NoteEvent {
    constructor(public opts: unknown) {}
  }
  class Writer {
    dataUri() {
      return 'data:audio/midi;base64,AAAA';
    }
  }
  return { default: { Track, NoteEvent, Writer }, Track, NoteEvent, Writer };
});

const BPM = 120; // 0.5 s per beat

function drumTrack(notes: RegionNote[], patch: (pads: PadSettings[]) => void = () => undefined): Track {
  const kit = createFactoryKit();
  patch(kit.pads);
  return {
    id: 'd',
    name: 'Drums',
    instrument: { type: 'drums', presetIndex: 0, kit },
    regions: [{ id: 'r', startBeat: 0, lengthBeats: 4, notes }],
    volume: 0,
    pan: 0,
    muted: false,
    solo: false,
    color: '#f00',
    effects: structuredClone(DEFAULT_TRACK_EFFECTS),
  };
}

const hit = (pad: number, startBeat: number, durationBeats = 0.25): RegionNote => ({
  note: padToNote(pad),
  velocity: 0.8,
  startBeat,
  durationBeats,
  isDrum: true,
});

const arrangementOf = (...tracks: Track[]): Arrangement => ({ id: 'a', name: 'x', bpm: BPM, lengthBeats: 16, tracks });

describe('WAV export length', () => {
  it('a long one-shot sample outlives its note: render until the sample ends', () => {
    const arr = arrangementOf(drumTrack([hit(0, 2)]));
    // 6 s sample on pad 0, note at 1 s lasting 0.125 s
    expect(getArrangementEndSeconds(arr, () => 6)).toBeCloseTo(1 + 6, 6);
    // unknown buffer: falls back to the note end
    expect(getArrangementEndSeconds(arr, () => null)).toBeCloseTo(1.125, 6);
  });

  it('honours trim and pitch (playback rate) of the pad', () => {
    const arr = arrangementOf(
      drumTrack([hit(0, 0)], (pads) => {
        pads[0] = { ...pads[0], startSec: 1, endSec: 3, pitch: 12 }; // 2 s slice at double speed
      }),
    );
    expect(getArrangementEndSeconds(arr, () => 10)).toBeCloseTo(1, 6);
  });

  it('gate pads stop at note end + decay (never past the slice)', () => {
    const arr = arrangementOf(
      drumTrack([hit(0, 0, 2)], (pads) => {
        pads[0] = { ...pads[0], mode: 'gate', decay: 0.5 };
      }),
    );
    expect(getArrangementEndSeconds(arr, () => 10)).toBeCloseTo(1 + 0.5, 6);
    expect(getArrangementEndSeconds(arr, () => 1.2)).toBeCloseTo(1.2, 6);
  });

  it('applies swing to drum note start times like playback', () => {
    const arr = arrangementOf(drumTrack([hit(0, 3.75)]));
    arr.beat = normalizeBeatState({ loop: { swing: 0.5, quantize: '1/16' } }, arr.tracks);
    // odd 1/16 step: sounds 0.125 beat later
    expect(getArrangementEndSeconds(arr, () => 1)).toBeCloseTo((3.75 + 0.125) * 0.5 + 1, 6);
  });

  it('adds a feedback-delay tail and skips muted tracks', () => {
    const t = drumTrack([hit(0, 0)]);
    t.effects!.delay = { ...t.effects!.delay, enabled: true, wet: 0.5, time: 0.5, feedback: 0.5 };
    expect(effectsTailSeconds(t)).toBeCloseTo(0.5 * (Math.log(1e-3) / Math.log(0.5)), 6);
    expect(getArrangementEndSeconds(arrangementOf(t), () => 1)).toBeCloseTo(1 + effectsTailSeconds(t), 6);
    t.muted = true;
    expect(getArrangementEndSeconds(arrangementOf(t), () => 1)).toBe(0);
  });
});

describe('MIDI export', () => {
  it('writes drum notes swung, as they sound (same transform as playback and WAV)', async () => {
    midiEvents.length = 0;
    const arr = arrangementOf(drumTrack([hit(0, 0), hit(0, 0.25), hit(0, 0.5)]));
    arr.beat = normalizeBeatState({ loop: { swing: 0.5, quantize: '1/4', swingGrid: '1/16' } }, arr.tracks);
    await exportMidi(arr);
    // 128 ticks per beat: the odd 1/16 (0.25) is 0.125 beat late; 0.5 is straight
    expect(midiEvents.map((e) => e.startTick)).toEqual([0, 48, 64]);
  });
});
