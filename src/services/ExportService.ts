import * as Tone from 'tone';
import type { Arrangement } from '@/types/arrangement';
import { midiToNoteName } from '@/utils/noteHelpers';
import { audioBufferToWav } from '@/utils/wavEncoder';
import { createTrackAudioNodes, disposeTrackAudioNodes, playDrumNote } from '@/utils/trackAudioFactory';
import { sampleManager } from './SampleManager';

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

  const durationSeconds = (endBeat / arrangement.bpm) * 60 + 1; // +1s tail for reverb/release

  const audioBuffer = await Tone.Offline(({ destination }) => {
    const transport = Tone.getTransport();
    transport.bpm.value = arrangement.bpm;

    const trackNodes: ReturnType<typeof createTrackAudioNodes>[] = [];

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
        if (track.instrument.type === 'drums' && audio.drumSynths) {
          playDrumNote(audio.drumSynths, note.note, note.velocity, time);
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
  const MidiWriter = await import('midi-writer-js') as any;

  const midiTracks: any[] = [];

  for (const track of arrangement.tracks) {
    if (track.muted) continue;
    if (track.instrument.type === 'audio') continue; // Skip audio tracks

    const midiTrack = new MidiWriter.Track();
    midiTrack.setTempo(arrangement.bpm);
    midiTrack.addTrackName(track.name);

    // Drums on channel 10 (index 9 in 0-based)
    const channel = track.instrument.type === 'drums' ? 10 : 1;

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
          pitch: [note.note] as unknown as string[],
          duration: `T${durationTicks}`,
          velocity: Math.round(note.velocity * 100),
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
