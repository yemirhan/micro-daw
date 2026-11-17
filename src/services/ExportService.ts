import * as Tone from 'tone';
import type { Arrangement } from '@/types/arrangement';
import { midiToNoteName } from '@/utils/noteHelpers';
import { audioBufferToWav } from '@/utils/wavEncoder';
import { createTrackAudioNodes, playDrumNote } from '@/utils/trackAudioFactory';
import { presetAt } from '@/utils/synthFactory';
import { installMasterBus } from '@/audio/masterBus';
import { loadPianoSamples } from '@/audio/pianoSamples';
import { preloadKitBuffers } from '@/audio/sampler/bufferCache';
import { createFactoryKit } from '@/audio/sampler/factoryKit';
import { drumNoteToGm } from '@/utils/arrangementHelpers';
import { sampleManager } from './SampleManager';

/** Seconds rendered after the last note so releases and reverb tails ring out. */
const EXPORT_TAIL_SECONDS = 3;

/** Find the last note/audio end time in beats */
function getArrangementEndBeat(arrangement: Arrangement): number {
  let maxBeat = 0;
  for (const track of arrangement.tracks) {
    if (track.muted) continue;
    for (const region of track.regions) {
      // Audio regions
      if (region.audio) {
        maxBeat = Math.max(maxBeat, region.startBeat + region.lengthBeats);
        continue;
      }
      // MIDI regions
      for (const note of region.notes) {
        const end = region.startBeat + note.startBeat + note.durationBeats;
        maxBeat = Math.max(maxBeat, end);
      }
    }
  }
  return maxBeat;
}

// --- WAV Export ---

export async function exportWav(arrangement: Arrangement): Promise<Blob> {
  const endBeat = getArrangementEndBeat(arrangement);
  if (endBeat <= 0) throw new Error('Nothing to export');

  const durationSeconds = (endBeat / arrangement.bpm) * 60 + EXPORT_TAIL_SECONDS;

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
      for (const region of track.regions) {
        for (const note of region.notes) {
          const absoluteBeat = region.startBeat + note.startBeat;
          events.push({ time: absoluteBeat * (60 / arrangement.bpm), note });
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
