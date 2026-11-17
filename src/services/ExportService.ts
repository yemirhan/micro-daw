import * as Tone from 'tone';
import type { Arrangement, Track } from '@/types/arrangement';
import { noteToPad, type PadSettings } from '@/types/sampler';
import { midiToNoteName } from '@/utils/noteHelpers';
import { audioBufferToWav } from '@/utils/wavEncoder';
import { createTrackAudioNodes, playDrumNote } from '@/utils/trackAudioFactory';
import { presetAt } from '@/utils/synthFactory';
import { installMasterBus } from '@/audio/masterBus';
import { loadPianoSamples } from '@/audio/pianoSamples';
import { getCachedPadBuffer, preloadKitBuffers } from '@/audio/sampler/bufferCache';
import { createFactoryKit } from '@/audio/sampler/factoryKit';
import { computeSlice } from '@/audio/sampler/SamplerKit';
import { DEFAULT_LOOP_SETTINGS, drumNoteToGm, swingOffsetBeats } from '@/utils/arrangementHelpers';
import { sampleManager } from './SampleManager';

/** Seconds rendered after the last audible sound so synth releases and reverb tails ring out. */
const EXPORT_TAIL_SECONDS = 3;
/** Longest feedback-delay tail we render (s). */
const MAX_DELAY_TAIL_SECONDS = 12;

/** Buffer duration (s) of a pad's sample, or null when it is unknown/not loaded. */
export type PadBufferDuration = (pad: PadSettings) => number | null;

const cachedPadDuration: PadBufferDuration = (pad) =>
  pad.sample ? getCachedPadBuffer(pad.sample.source)?.duration ?? null : null;

/** Beat a drum-track note sounds at: patterns are stored straight, swing is applied at playback (as live). */
function drumPlayBeat(arrangement: Arrangement, beat: number): number {
  const loop = arrangement.beat?.loop ?? DEFAULT_LOOP_SETTINGS;
  return beat + swingOffsetBeats(beat, loop.quantize, loop.swing);
}

/**
 * Seconds a drum note keeps sounding after it starts: one-shots play their
 * whole slice (buffer slice / playback rate); gate pads stop at note end +
 * decay (never past the slice). Unknown samples fall back to the note length.
 */
function drumNoteAudibleSeconds(pad: PadSettings | undefined, noteSeconds: number, padDuration: PadBufferDuration): number {
  if (!pad?.sample) return noteSeconds;
  const bufferDuration = padDuration(pad);
  if (bufferDuration == null || !(bufferDuration > 0)) return noteSeconds;
  const slice = computeSlice(pad, bufferDuration);
  if (!slice) return 0;
  if (pad.mode === 'gate') return Math.min(slice.realDuration, noteSeconds + Math.max(0, pad.decay));
  return slice.realDuration;
}

/** Extra render time for a track's effects (feedback delay repeats; reverb is covered by the base tail). */
export function effectsTailSeconds(track: Track): number {
  const delay = track.effects?.delay;
  if (!delay?.enabled || !(delay.wet > 0) || !(delay.time > 0)) return 0;
  const fb = Math.min(0.99, Math.max(0, delay.feedback));
  // Repeats until -60 dB (at least one echo).
  const repeats = fb > 0 ? Math.max(1, Math.log(1e-3) / Math.log(fb)) : 1;
  return Math.min(MAX_DELAY_TAIL_SECONDS, delay.time * repeats);
}

/**
 * Seconds from the start until the last unmuted sound ends, including the
 * full length of drum samples (a long one-shot outlives its note) and each
 * track's effects tail. The render adds EXPORT_TAIL_SECONDS on top.
 */
export function getArrangementEndSeconds(arrangement: Arrangement, padDuration: PadBufferDuration = cachedPadDuration): number {
  const secPerBeat = 60 / arrangement.bpm;
  let end = 0;
  for (const track of arrangement.tracks) {
    if (track.muted) continue;
    let trackEnd = 0;
    const isDrums = track.instrument.type === 'drums';
    for (const region of track.regions) {
      // Audio regions
      if (region.audio) {
        trackEnd = Math.max(trackEnd, (region.startBeat + region.lengthBeats) * secPerBeat);
        continue;
      }
      // MIDI regions
      for (const note of region.notes) {
        const beat = region.startBeat + note.startBeat;
        const noteSeconds = Math.max(0, note.durationBeats) * secPerBeat;
        if (isDrums) {
          const pad = noteToPad(Math.round(note.note));
          const settings = pad === null ? undefined : track.instrument.kit?.pads[pad];
          const start = drumPlayBeat(arrangement, beat) * secPerBeat;
          trackEnd = Math.max(trackEnd, start + drumNoteAudibleSeconds(settings, noteSeconds, padDuration));
        } else {
          trackEnd = Math.max(trackEnd, beat * secPerBeat + noteSeconds);
        }
      }
    }
    if (trackEnd > 0) end = Math.max(end, trackEnd + effectsTailSeconds(track));
  }
  return end;
}

// --- WAV Export ---

export async function exportWav(arrangement: Arrangement): Promise<Blob> {
  if (getArrangementEndSeconds(arrangement, () => null) <= 0) throw new Error('Nothing to export');

  // Sampled instruments must be decoded before the offline render starts.
  const needsPiano = arrangement.tracks.some(
    (t) => !t.muted && t.instrument.type === 'synth' && presetAt(t.instrument.presetIndex).engine === 'sampler',
  );
  if (needsPiano) await loadPianoSamples().catch(() => undefined);
  // Pad samples too: SamplerKit then picks every buffer up synchronously inside
  // the offline context (missing files are logged and stay silent).
  await Promise.all(
    arrangement.tracks
      .filter((t) => !t.muted && t.instrument.type === 'drums')
      .map((t) => preloadKitBuffers(t.instrument.kit ?? createFactoryKit())),
  );
  // Now that pad buffers are loaded: long samples are not cut off at their note's end.
  const durationSeconds = getArrangementEndSeconds(arrangement) + EXPORT_TAIL_SECONDS;

  const audioBuffer = await Tone.Offline(({ destination }) => {
    // Same master processing (glue compressor + true-peak ceiling) as live playback.
    installMasterBus(destination);
    const transport = Tone.getTransport();
    transport.bpm.value = arrangement.bpm;

    const trackNodes: ReturnType<typeof createTrackAudioNodes>[] = [];
    const secPerBeat = 60 / arrangement.bpm;

    for (const track of arrangement.tracks) {
      if (track.muted) continue;

      const audio = createTrackAudioNodes(track, destination);
      trackNodes.push(audio);

      // Audio track — schedule players
      if (track.instrument.type === 'audio') {
        for (const region of track.regions) {
          if (!region.audio) continue;
          const toneBuffer = sampleManager.getToneBuffer(region.audio.sampleId);
          if (!toneBuffer) continue;

          const player = new Tone.Player(toneBuffer);
          if (region.audio.gainDb !== 0) {
            player.volume.value = region.audio.gainDb;
          }
          player.connect(audio.filter);

          const startTimeSec = (region.startBeat / arrangement.bpm) * 60;
          const regionDurSec = (region.lengthBeats / arrangement.bpm) * 60;
          const offset = region.audio.offsetSeconds;

          transport.schedule((time) => {
            player.start(time, offset, regionDurSec);
          }, startTimeSec);
        }
        continue;
      }

      // MIDI track — schedule note events
      const events: Array<{ time: number; note: import('@/types/arrangement').RegionNote }> = [];
      const isDrums = track.instrument.type === 'drums';
      for (const region of track.regions) {
        for (const note of region.notes) {
          const absoluteBeat = region.startBeat + note.startBeat;
          const playBeat = isDrums ? drumPlayBeat(arrangement, absoluteBeat) : absoluteBeat;
          events.push({ time: playBeat * secPerBeat, note });
        }
      }

      if (events.length === 0) continue;

      const part = new Tone.Part((time, value) => {
        const { note } = value;
        if (track.instrument.type === 'drums') {
          if (audio.sampler) playDrumNote(audio.sampler, note.note, note.velocity, time, note.durationBeats * secPerBeat);
        } else if (audio.synth) {
          const noteName = midiToNoteName(note.note);
          const dur = (note.durationBeats * 60) / arrangement.bpm;
          audio.synth.triggerAttackRelease(noteName, dur, time, note.velocity);
        }
      }, events.map((e) => ({ time: e.time, note: e.note })));

      part.start(0);
    }

    transport.start(0);
  }, durationSeconds);

  return audioBufferToWav(audioBuffer as unknown as AudioBuffer);
}

// --- MIDI Export ---

export async function exportMidi(arrangement: Arrangement): Promise<Blob> {
  // Dynamic import for midi-writer-js
  const MidiWriter = await import('midi-writer-js');

  const midiTracks: unknown[] = [];

  for (const track of arrangement.tracks) {
    if (track.muted) continue;
    if (track.instrument.type === 'audio') continue; // Skip audio tracks

    const midiTrack = new MidiWriter.Track();
    midiTrack.setTempo(arrangement.bpm);
    midiTrack.addTrackName(track.name);

    // Drums on channel 10 (index 9 in 0-based)
    const isDrums = track.instrument.type === 'drums';
    const channel = isDrums ? 10 : 1;

    // Collect all notes with absolute positions
    const allNotes: Array<{ absoluteBeat: number; note: import('@/types/arrangement').RegionNote }> = [];
    for (const region of track.regions) {
      for (const note of region.notes) {
        allNotes.push({
          absoluteBeat: region.startBeat + note.startBeat,
          note,
        });
      }
    }

    // Sort by time
    allNotes.sort((a, b) => a.absoluteBeat - b.absoluteBeat);

    // Convert beats to ticks (128 ticks per beat is MidiWriter default)
    for (const { absoluteBeat, note } of allNotes) {
      const startTick = Math.round(absoluteBeat * 128);
      const durationTicks = Math.max(1, Math.round(note.durationBeats * 128));

      midiTrack.addEvent(
        new MidiWriter.NoteEvent({
          // Drum pads → GM note of their factory voice (else the pad note 36..51).
          pitch: [isDrums ? drumNoteToGm(note.note, track.instrument.kit) : note.note] as unknown as string[],
          duration: `T${durationTicks}`,
          velocity: Math.max(1, Math.min(100, Math.round(note.velocity * 100))),
          startTick,
          channel: channel as 1,
        }),
      );
    }

    midiTracks.push(midiTrack);
  }

  if (midiTracks.length === 0) throw new Error('Nothing to export');

  const writer = new MidiWriter.Writer(midiTracks);
  const dataUri = writer.dataUri();
  // dataUri format: data:audio/midi;base64,...
  const base64 = dataUri.split(',')[1];
  const binaryString = atob(base64);
  const bytes = new Uint8Array(binaryString.length);
  for (let i = 0; i < binaryString.length; i++) {
    bytes[i] = binaryString.charCodeAt(i);
  }

  return new Blob([bytes], { type: 'audio/midi' });
}

// --- File download helper ---

export function downloadBlob(blob: Blob, filename: string): void {
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
  URL.revokeObjectURL(url);
}
