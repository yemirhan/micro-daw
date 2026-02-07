import { useState, useCallback, useMemo, useRef } from 'react';
import type { MidiCallbacks } from '@/types/midi';
import type { EffectParams } from '@/types/effects';
import type { DrumPadId } from '@/types/drums';
import { audioEngine } from '@/services/AudioEngine';
import { drumEngine } from '@/services/DrumEngine';
import { arrangementEngine } from '@/services/ArrangementEngine';
import {
  CC_MAPPINGS,
  VOLUME_CCS,
  DRUM_NOTE_TO_PAD,
  DRUM_CHANNEL,
  type RoutingMode,
} from '@/utils/constants';

export interface ActiveNote {
  note: number;
  velocity: number;
}

interface UseMidiNotesOptions {
  onVolumeChange?: (db: number) => void;
  onEffectChange?: (params: EffectParams) => void;
  onDrumFlash?: (padId: DrumPadId) => void;
}

export function useMidiNotes(options: UseMidiNotesOptions = {}) {
  const { onVolumeChange, onEffectChange, onDrumFlash } = options;
  const [activeNotes, setActiveNotes] = useState<Map<number, ActiveNote>>(new Map());
  const [effectParams, setEffectParams] = useState<EffectParams>({
    reverbWet: 0,
    chorusDepth: 0,
    filterCutoff: 18000,
    filterResonance: 1,
  });
  const [routingMode, setRoutingMode] = useState<RoutingMode>('auto');
  const routingModeRef = useRef<RoutingMode>('auto');

  // Keep ref in sync for use in callbacks without re-creating them
  const updateRoutingMode = useCallback((mode: RoutingMode) => {
    routingModeRef.current = mode;
    setRoutingMode(mode);
  }, []);

  const isDrumNote = useCallback((note: number, channel: number): boolean => {
    const mode = routingModeRef.current;
    if (mode === 'keys') return false;
    // Channel 10 = drums (GM standard), or note is in drum map
    if (channel === DRUM_CHANNEL) return true;
    return DRUM_NOTE_TO_PAD.has(note);
  }, []);

  const noteOn = useCallback((note: number, velocity: number, channel: number = 1) => {
    if (isDrumNote(note, channel)) {
      // Route to drums — look up pad ID from GM note map
      const padId = DRUM_NOTE_TO_PAD.get(note);
      if (padId !== undefined) {
        drumEngine.hit(padId, velocity);
        onDrumFlash?.(padId);
      }
      // Forward to arrangement engine if recording
      if (arrangementEngine.getState() === 'recording') {
        arrangementEngine.captureNoteOn(note, velocity, true);
      }
    } else {
      // Route to synth
      audioEngine.noteOn(note, velocity);
      // Forward to arrangement engine if recording
      if (arrangementEngine.getState() === 'recording') {
        arrangementEngine.captureNoteOn(note, velocity, false);
      }
      setActiveNotes((prev) => {
        const next = new Map(prev);
        next.set(note, { note, velocity });
        return next;
      });
    }
  }, [isDrumNote, onDrumFlash]);

  const noteOff = useCallback((note: number, channel: number = 1) => {
    if (isDrumNote(note, channel)) {
      // Drums are percussive, no noteOff needed for audio
      if (arrangementEngine.getState() === 'recording') {
        arrangementEngine.captureNoteOff(note);
      }
    } else {
      audioEngine.noteOff(note);
      if (arrangementEngine.getState() === 'recording') {
        arrangementEngine.captureNoteOff(note);
      }
      setActiveNotes((prev) => {
        const next = new Map(prev);
        next.delete(note);
        return next;
      });
    }
  }, [isDrumNote]);

  // Pickup mode (soft takeover): ignore CC until the physical knob
  // crosses the current software value, preventing jumps on first touch.
  const ccPickup = useRef<Map<number, { lastRaw: number; pickedUp: boolean }>>(new Map());

  const effectToNormalized = useCallback((effect: keyof EffectParams, params: EffectParams): number => {
    switch (effect) {
      case 'reverbWet': return params.reverbWet;
      case 'chorusDepth': return params.chorusDepth;
      case 'filterCutoff': return Math.log(params.filterCutoff / 60) / Math.log(18000 / 60);
      case 'filterResonance': return params.filterResonance / 20;
    }
  }, []);

  const checkPickup = useCallback((cc: number, rawValue: number, effect: keyof EffectParams): boolean => {
    const normalized = rawValue / 127;
    const state = ccPickup.current.get(cc);
    if (state?.pickedUp) return true;

    const currentNorm = effectToNormalized(effect, audioEngine.getEffectParams());
    const THRESHOLD = 0.05; // ~6 out of 127

    if (!state) {
      // First message for this CC — check if already close enough
      const close = Math.abs(normalized - currentNorm) < THRESHOLD;
      ccPickup.current.set(cc, { lastRaw: rawValue, pickedUp: close });
      return close;
    }

    // Check if knob crossed the current value since last message
    const lastNorm = state.lastRaw / 127;
    const crossed = (lastNorm <= currentNorm && normalized >= currentNorm) ||
                    (lastNorm >= currentNorm && normalized <= currentNorm) ||
                    Math.abs(normalized - currentNorm) < THRESHOLD;
    state.lastRaw = rawValue;
    if (crossed) state.pickedUp = true;
    return crossed;
  }, [effectToNormalized]);

  const handleCC = useCallback((cc: number, value: number) => {
    // Check volume CCs
    if (VOLUME_CCS.includes(cc)) {
      const db = Math.round(-40 + (value / 127) * 40);
      audioEngine.setVolume(db);
      onVolumeChange?.(db);
      return;
    }

    // Check effect CCs
    const mapping = CC_MAPPINGS.find((m) => m.cc === cc);
    if (mapping) {
      if (!checkPickup(cc, value, mapping.effect)) return;

      const normalized = value / 127;
      switch (mapping.effect) {
        case 'reverbWet':
          audioEngine.setReverbWet(normalized);
          break;
        case 'chorusDepth':
          audioEngine.setChorusDepth(normalized);
          break;
        case 'filterCutoff': {
          // Log-scale: 60 Hz to 18000 Hz
          const hz = 60 * Math.pow(18000 / 60, normalized);
          audioEngine.setFilterCutoff(hz);
          break;
        }
        case 'filterResonance':
          audioEngine.setFilterResonance(normalized * 20);
          break;
      }
      const params = audioEngine.getEffectParams();
      setEffectParams(params);
      onEffectChange?.(params);
    }
  }, [onVolumeChange, onEffectChange, checkPickup]);

  const callbacks: MidiCallbacks = useMemo(() => ({
    onNoteOn: (note: number, velocity: number, channel: number) => noteOn(note, velocity, channel),
    onNoteOff: (note: number, channel: number) => noteOff(note, channel),
    onControlChange: handleCC,
  }), [noteOn, noteOff, handleCC]);

  return {
    activeNotes,
    noteOn,
    noteOff,
    callbacks,
    effectParams,
    setEffectParams,
    routingMode,
    setRoutingMode: updateRoutingMode,
  };
}
