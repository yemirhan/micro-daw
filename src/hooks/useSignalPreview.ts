import { useEffect, useRef, useCallback } from 'react';
import * as Tone from 'tone';
import { audioEngine } from '@/services/AudioEngine';
import { drumEngine } from '@/services/DrumEngine';
import { midiService } from '@/services/MidiService';
import { midiToFrequency, midiToNoteName } from '@/utils/noteHelpers';
import type { SignalType } from '@/types/samples';

interface UseSignalPreviewParams {
  type: SignalType;
  amplitude: number;
  frequency: number;
  active: boolean;
  onFrequencyDetected?: (hz: number, noteName: string) => void;
}

export function useSignalPreview({
  type,
  amplitude,
  frequency,
  active,
  onFrequencyDetected,
}: UseSignalPreviewParams) {
  const synthRef = useRef<Tone.PolySynth | Tone.NoiseSynth | null>(null);
  const volRef = useRef<Tone.Volume | null>(null);
  const previewNoteRef = useRef<string | null>(null);
  const typeRef = useRef(type);
  const frequencyRef = useRef(frequency);
  const onFreqDetectedRef = useRef(onFrequencyDetected);

  // Keep refs in sync
  useEffect(() => { typeRef.current = type; }, [type]);
  useEffect(() => { frequencyRef.current = frequency; }, [frequency]);
  useEffect(() => { onFreqDetectedRef.current = onFrequencyDetected; }, [onFrequencyDetected]);

  // Recreate synth when type changes (while active)
  useEffect(() => {
    if (!active) return;

    // Dispose old synth
    if (synthRef.current) {
      try { synthRef.current.releaseAll(); } catch { /* NoiseSynth has no releaseAll */ }
      synthRef.current.dispose();
      synthRef.current = null;
    }

    // Create volume node once
    if (!volRef.current) {
      volRef.current = new Tone.Volume(Tone.gainToDb(amplitude)).toDestination();
    }

    // Create synth based on type
    if (type === 'white-noise') {
      const noiseSynth = new Tone.NoiseSynth({
        noise: { type: 'white' },
        envelope: { attack: 0.01, decay: 0.1, sustain: 0.8, release: 0.3 },
      });
      noiseSynth.connect(volRef.current);
      synthRef.current = noiseSynth;
    } else {
      const polySynth = new Tone.PolySynth(Tone.Synth, {
        oscillator: { type } as Tone.OmniOscillatorOptions,
        envelope: { attack: 0.01, decay: 0.1, sustain: 0.8, release: 0.3 },
      });
      polySynth.connect(volRef.current);
      synthRef.current = polySynth;
    }
  }, [type, active]); // eslint-disable-line react-hooks/exhaustive-deps

  // Update volume without recreating synth
  useEffect(() => {
    if (volRef.current) {
      volRef.current.volume.value = Tone.gainToDb(amplitude);
    }
  }, [amplitude]);

  // Mute/unmute AudioEngine + DrumEngine and manage lifecycle
  useEffect(() => {
    if (!active) return;

    audioEngine.mute();
    drumEngine.mute();

    // Subscribe to raw MIDI
    const unsub = midiService.onRawMessage((e) => {
      const synth = synthRef.current;
      if (!synth) return;

      const statusByte = e.message.statusByte;
      const command = statusByte & 0xF0;
      const dataBytes = e.message.rawDataBytes;

      if (command === 0x90) {
        // Note On
        const note = dataBytes[0];
        const velocity = dataBytes[1];
        if (velocity === 0) {
          // Velocity 0 = Note Off
          if (synth instanceof Tone.PolySynth) {
            synth.triggerRelease(midiToNoteName(note), Tone.now());
          } else {
            synth.triggerRelease(Tone.now());
          }
          return;
        }

        if (synth instanceof Tone.PolySynth) {
          synth.triggerAttack(midiToNoteName(note), Tone.now(), velocity / 127);
        } else {
          synth.triggerAttack(Tone.now(), velocity / 127);
        }

        // Auto-fill frequency (skip for noise)
        if (typeRef.current !== 'white-noise') {
          onFreqDetectedRef.current?.(midiToFrequency(note), midiToNoteName(note));
        }
      } else if (command === 0x80) {
        // Note Off
        const note = dataBytes[0];
        if (synth instanceof Tone.PolySynth) {
          synth.triggerRelease(midiToNoteName(note), Tone.now());
        } else {
          synth.triggerRelease(Tone.now());
        }
      }
    });

    return () => {
      unsub();

      // Release and dispose synth
      if (synthRef.current) {
        try { synthRef.current.releaseAll(); } catch { /* NoiseSynth */ }
        synthRef.current.dispose();
        synthRef.current = null;
      }
      if (volRef.current) {
        volRef.current.dispose();
        volRef.current = null;
      }

      audioEngine.unmute();
      drumEngine.unmute();
    };
  }, [active]); // eslint-disable-line react-hooks/exhaustive-deps

  const startPreviewTone = useCallback(() => {
    const synth = synthRef.current;
    if (!synth) return;

    if (synth instanceof Tone.PolySynth) {
      const noteName = Tone.Frequency(frequencyRef.current, 'hz').toNote();
      previewNoteRef.current = noteName;
      synth.triggerAttack(noteName, Tone.now(), 0.8);
    } else {
      previewNoteRef.current = '__noise__';
      synth.triggerAttack(Tone.now(), 0.8);
    }
  }, []);

  const stopPreviewTone = useCallback(() => {
    const synth = synthRef.current;
    if (!synth || !previewNoteRef.current) return;

    if (synth instanceof Tone.PolySynth) {
      synth.triggerRelease(previewNoteRef.current, Tone.now());
    } else {
      synth.triggerRelease(Tone.now());
    }
    previewNoteRef.current = null;
  }, []);

  return { startPreviewTone, stopPreviewTone };
}
