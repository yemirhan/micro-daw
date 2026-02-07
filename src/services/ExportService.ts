import * as Tone from 'tone';
import type { Arrangement, Track, RegionNote } from '@/types/arrangement';
import { SYNTH_PRESETS, MAX_POLYPHONY } from '@/utils/constants';
import { midiToNoteName } from '@/utils/noteHelpers';
import { audioBufferToWav } from '@/utils/wavEncoder';

// --- Synth factories (mirror ArrangementEngine.createTrackAudio) ---

function createSynthForTrack(track: Track, destination: Tone.ToneAudioNode): {
  synth: Tone.PolySynth | null;
  drumSynths: Map<number, Tone.Synth | Tone.NoiseSynth | Tone.MembraneSynth | Tone.MetalSynth> | null;
  drumFilter: Tone.Filter | null;
  volume: Tone.Volume;
} {
  const volume = new Tone.Volume(track.volume);
  volume.connect(destination);
  if (track.muted) volume.mute = true;

  if (track.instrument.type === 'synth') {
    const preset = SYNTH_PRESETS[track.instrument.presetIndex] || SYNTH_PRESETS[0];
    const synth = new Tone.PolySynth(Tone.Synth, {
      maxPolyphony: MAX_POLYPHONY,
      oscillator: preset.oscillator as Tone.OmniOscillatorOptions,
      envelope: preset.envelope,
    });
    synth.connect(volume);
    return { synth, drumSynths: null, drumFilter: null, volume };
  }

  // Drums
  const drumSynths = new Map<number, Tone.Synth | Tone.NoiseSynth | Tone.MembraneSynth | Tone.MetalSynth>();
  const drumFilter = new Tone.Filter({ frequency: 8000, type: 'highpass' });
  drumFilter.connect(volume);

  const kick = new Tone.MembraneSynth({
    pitchDecay: 0.05, octaves: 6,
    oscillator: { type: 'sine' },
    envelope: { attack: 0.001, decay: 0.3, sustain: 0, release: 0.1 },
  });
  kick.connect(volume);
  drumSynths.set(36, kick);

  const snare = new Tone.NoiseSynth({
    noise: { type: 'white' },
    envelope: { attack: 0.001, decay: 0.15, sustain: 0, release: 0.05 },
  });
  snare.connect(volume);
  drumSynths.set(37, snare);

  const chh = new Tone.NoiseSynth({
    noise: { type: 'white' },
    envelope: { attack: 0.001, decay: 0.05, sustain: 0, release: 0.01 },
  });
  chh.connect(drumFilter);
  drumSynths.set(38, chh);

  const ohh = new Tone.NoiseSynth({
    noise: { type: 'white' },
    envelope: { attack: 0.001, decay: 0.3, sustain: 0, release: 0.1 },
  });
  ohh.connect(drumFilter);
  drumSynths.set(39, ohh);

  const clap = new Tone.NoiseSynth({
    noise: { type: 'pink' },
    envelope: { attack: 0.001, decay: 0.12, sustain: 0, release: 0.05 },
  });
  clap.connect(volume);
  drumSynths.set(40, clap);

  const tom = new Tone.MembraneSynth({
    pitchDecay: 0.03, octaves: 4,
    oscillator: { type: 'sine' },
    envelope: { attack: 0.001, decay: 0.2, sustain: 0, release: 0.1 },
  });
  tom.connect(volume);
  drumSynths.set(41, tom);

  const crash = new Tone.MetalSynth({
    frequency: 300,
    envelope: { attack: 0.001, decay: 1.2, release: 0.3 },
    harmonicity: 5.1, modulationIndex: 32, resonance: 4000, octaves: 1.5,
  });
  crash.connect(volume);
  drumSynths.set(42, crash);

  const ride = new Tone.MetalSynth({
    frequency: 400,
    envelope: { attack: 0.001, decay: 0.4, release: 0.1 },
    harmonicity: 5.1, modulationIndex: 16, resonance: 5000, octaves: 1,
  });
  ride.connect(volume);
  drumSynths.set(43, ride);

  return { synth: null, drumSynths, drumFilter, volume };
}

function playDrumNote(
  drumSynths: Map<number, Tone.Synth | Tone.NoiseSynth | Tone.MembraneSynth | Tone.MetalSynth>,
  midiNote: number,
  velocity: number,
  time: number,
): void {
  const synth = drumSynths.get(midiNote);
  if (!synth) return;
  const vol = velocity * 0.8 + 0.2;

  if (synth instanceof Tone.MembraneSynth) {
    const pitch = midiNote === 36 ? 'C1' : 'G1';
    synth.triggerAttackRelease(pitch, '8n', time, vol);
  } else if (synth instanceof Tone.NoiseSynth) {
    const dur = midiNote === 38 ? '16n' : midiNote === 39 ? '8n' : '16n';
    synth.triggerAttackRelease(dur, time, vol);
  } else if (synth instanceof Tone.MetalSynth) {
    const dur = midiNote === 42 ? '16n' : '32n';
    synth.triggerAttackRelease(dur, time, vol);
  }
}

/** Find the last note end time in beats */
function getArrangementEndBeat(arrangement: Arrangement): number {
  let maxBeat = 0;
  for (const track of arrangement.tracks) {
    if (track.muted) continue;
    for (const region of track.regions) {
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

    for (const track of arrangement.tracks) {
      if (track.muted) continue;

      const audio = createSynthForTrack(track, destination);

      const events: Array<{ time: number; note: RegionNote }> = [];
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

  return audioBufferToWav(audioBuffer);
}

// --- MIDI Export ---

export async function exportMidi(arrangement: Arrangement): Promise<Blob> {
  // Dynamic import for midi-writer-js
  const MidiWriter = await import('midi-writer-js');

  const midiTracks: InstanceType<typeof MidiWriter.Track>[] = [];

  for (const track of arrangement.tracks) {
    if (track.muted) continue;

    const midiTrack = new MidiWriter.Track();
    midiTrack.setTempo(arrangement.bpm);
    midiTrack.addTrackName(track.name);

    // Drums on channel 10 (index 9 in 0-based)
    const channel = track.instrument.type === 'drums' ? 10 : 1;

    // Collect all notes with absolute positions
    const allNotes: Array<{ absoluteBeat: number; note: RegionNote }> = [];
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
