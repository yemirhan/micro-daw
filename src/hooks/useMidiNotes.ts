import { useState, useCallback, useEffect, useLayoutEffect, useMemo, useRef } from 'react';
import type { MidiCallbacks } from '@/types/midi';
import type { EffectParams } from '@/types/effects';
import { DEFAULT_EFFECT_PARAMS } from '@/types/effects';
import type { DrumPadId } from '@/types/drums';
import { audioEngine } from '@/services/AudioEngine';
import { drumEngine } from '@/services/DrumEngine';
import { arrangementEngine } from '@/services/ArrangementEngine';
import { noteEventBus, eventTime } from '@/services/NoteEventBus';
import {
  CC_MAPPINGS,
  VOLUME_CCS,
  DRUM_NOTE_TO_PAD,
  DRUM_CHANNEL,
  DRUM_SOUNDS,
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
  muted?: boolean;
}

/** MIDI CC numbers with special meaning for note handling. */
export const CC_SUSTAIN = 64;
export const CC_ALL_SOUND_OFF = 120;
export const CC_RESET_ALL_CONTROLLERS = 121;
export const CC_ALL_NOTES_OFF = 123;

function isRecording(): boolean {
  return arrangementEngine.getState() === 'recording';
}

/**
 * One entry per pitch that is (or, when muted, would be) sounding on the live
 * synth. This is the single source of truth for live notes: every attack and
 * every release of the live synth goes through it, so a pitch can never have
 * more than one voice and every voice we started can always be released.
 */
interface LiveVoice {
  velocity: number;
  /** We actually called audioEngine.noteOn (false when muted/keyboard mode). */
  attacked: boolean;
  /** The key (MIDI key / on-screen key) is physically down. */
  held: boolean;
}

/**
 * Note routing + live note state. Owns:
 *  - which notes are held (UI state `activeNotes`) and which are sounding,
 *  - routing MIDI notes to the synth or drums (decided once at note-on, so a
 *    routing/mute change while a key is held can't orphan the note-off),
 *  - sustain pedal (CC64) and MIDI panic CCs (120/121/123),
 *  - `allNotesOff()`: releases everything we started + audioEngine.releaseAll().
 *
 * Every performance event is also published on `noteEventBus` so lessons and
 * practice activities can score MIDI and on-screen input identically.
 */
export function useMidiNotes(options: UseMidiNotesOptions = {}) {
  const { onVolumeChange, onEffectChange, onDrumFlash, muted = false } = options;
  const [activeNotes, setActiveNotes] = useState<Map<number, ActiveNote>>(() => new Map());
  const [effectParams, setEffectParams] = useState<EffectParams>({ ...DEFAULT_EFFECT_PARAMS });
  const [routingMode, setRoutingMode] = useState<RoutingMode>('auto');
  const [sustainOn, setSustainOn] = useState(false);
  const routingModeRef = useRef<RoutingMode>('auto');
  const mutedRef = useRef(muted);

  // Stable refs for callback options — avoids re-creating MIDI callbacks on prop changes
  const onVolumeChangeRef = useRef(onVolumeChange);
  const onEffectChangeRef = useRef(onEffectChange);
  const onDrumFlashRef = useRef(onDrumFlash);
  useLayoutEffect(() => {
    mutedRef.current = muted;
    onVolumeChangeRef.current = onVolumeChange;
    onEffectChangeRef.current = onEffectChange;
    onDrumFlashRef.current = onDrumFlash;
  });

  /** Live synth voices by MIDI note. */
  const voicesRef = useRef<Map<number, LiveVoice>>(new Map());
  /** MIDI notes currently down that were routed to drums at note-on time. */
  const drumNotesDownRef = useRef<Set<number>>(new Set());
  const sustainRef = useRef(false);
  /** Latest MIDI handlers, so the `callbacks` object handed to MidiService is stable forever. */
  const midiNoteOnRef = useRef<(note: number, velocity: number, channel: number) => void>(() => undefined);
  const midiNoteOffRef = useRef<(note: number, channel: number) => void>(() => undefined);
  const handleCCRef = useRef<(cc: number, value: number) => void>(() => undefined);

  const updateRoutingMode = useCallback((mode: RoutingMode) => {
    routingModeRef.current = mode;
    setRoutingMode(mode);
  }, []);

  const syncActiveNotes = useCallback(() => {
    const next = new Map<number, ActiveNote>();
    for (const [note, v] of voicesRef.current) {
      if (v.held) next.set(note, { note, velocity: v.velocity });
    }
    setActiveNotes(next);
  }, []);

  /** Release the live-synth voice for `note` (if we started one) and forget it. */
  const releaseVoice = useCallback((note: number) => {
    const voice = voicesRef.current.get(note);
    if (!voice) return;
    voicesRef.current.delete(note);
    if (voice.attacked) audioEngine.noteOff(note);
  }, []);

  // --- Drums -------------------------------------------------------------

  const hitDrum = useCallback((padId: DrumPadId, velocity: number, note: number) => {
    if (!mutedRef.current) drumEngine.hit(padId, velocity);
    onDrumFlashRef.current?.(padId);
    if (isRecording()) arrangementEngine.captureNoteOn(note, velocity, true);
    noteEventBus.emit({ type: 'drum', padId, note, velocity, time: eventTime() });
  }, []);

  /** On-screen drum pad (or any explicit pad trigger): always a drum, never the synth. */
  const drumHit = useCallback((padId: DrumPadId, velocity: number) => {
    const sound = DRUM_SOUNDS[padId];
    if (!sound) return;
    hitDrum(padId, velocity, sound.midiNote);
  }, [hitDrum]);

  // --- Synth notes ---------------------------------------------------------

  /** Press a key on the live synth. Idempotent per pitch: re-pressing re-triggers, never stacks voices. */
  const keyDown = useCallback((note: number, velocity: number, channel: number) => {
    const existing = voicesRef.current.get(note);
    if (existing) {
      // Re-trigger: release the old voice first so the synth never holds two
      // voices for one pitch (Tone.PolySynth would only release one of them).
      if (existing.attacked) audioEngine.noteOff(note);
      if (existing.held && isRecording()) arrangementEngine.captureNoteOff(note);
    }
    const attacked = !mutedRef.current;
    if (attacked) audioEngine.noteOn(note, velocity);
    voicesRef.current.set(note, { velocity, attacked, held: true });
    if (isRecording()) arrangementEngine.captureNoteOn(note, velocity, false);
    syncActiveNotes();
    noteEventBus.emit({ type: 'noteon', note, velocity, channel, time: eventTime() });
  }, [syncActiveNotes]);

  /** Lift a key. Idempotent: a second note-off (or one after a panic) does nothing. */
  const keyUp = useCallback((note: number, channel: number) => {
    const voice = voicesRef.current.get(note);
    if (!voice || !voice.held) return;
    voice.held = false;
    if (isRecording()) arrangementEngine.captureNoteOff(note);
    if (!sustainRef.current) releaseVoice(note);
    syncActiveNotes();
    noteEventBus.emit({ type: 'noteoff', note, channel, time: eventTime() });
  }, [releaseVoice, syncActiveNotes]);

  const isDrumNote = useCallback((note: number, channel: number): boolean => {
    if (routingModeRef.current === 'keys') return false;
    // Channel 10 = drums (GM standard), or note is in drum map
    return channel === DRUM_CHANNEL || DRUM_NOTE_TO_PAD.has(note);
  }, []);

  /** MIDI input note-on (routing applies: drum notes go to the drum engine). */
  const midiNoteOn = useCallback((note: number, velocity: number, channel: number = 1) => {
    if (velocity <= 0) {
      // Running-status note-off sent as note-on velocity 0.
      midiNoteOffRef.current(note, channel);
      return;
    }
    if (isDrumNote(note, channel)) {
      drumNotesDownRef.current.add(note);
      const padId = DRUM_NOTE_TO_PAD.get(note);
      if (padId !== undefined) hitDrum(padId, velocity, note);
      return;
    }
    keyDown(note, velocity, channel);
  }, [isDrumNote, hitDrum, keyDown]);

  /** MIDI input note-off. Uses the route chosen at note-on, not the current routing mode. */
  const midiNoteOff = useCallback((note: number, channel: number = 1) => {
    if (drumNotesDownRef.current.delete(note)) {
      // Drums are one-shots: nothing to release on the audio side.
      if (isRecording()) arrangementEngine.captureNoteOff(note);
      return;
    }
    keyUp(note, channel);
  }, [keyUp]);

  /** On-screen keyboard note-on: always the synth (C2..D#3 are piano keys, not drums). */
  const noteOn = useCallback((note: number, velocity: number) => {
    keyDown(note, velocity, 0);
  }, [keyDown]);

  const noteOff = useCallback((note: number) => {
    keyUp(note, 0);
  }, [keyUp]);

  // --- Sustain / panic -----------------------------------------------------

  const setSustain = useCallback((on: boolean) => {
    if (sustainRef.current === on) return;
    sustainRef.current = on;
    setSustainOn(on);
    if (!on) {
      // Pedal up: release every note that is only ringing because of the pedal.
      for (const [note, v] of [...voicesRef.current]) {
        if (!v.held) releaseVoice(note);
      }
    }
  }, [releaseVoice]);

  /**
   * Panic: release every live note we started (held or sustained), clear the
   * pedal and held-key state, and hard-stop the live synth via
   * audioEngine.releaseAll() so even voices whose note-off was lost go silent.
   */
  const allNotesOff = useCallback(() => {
    const recording = isRecording();
    for (const [note, v] of [...voicesRef.current]) {
      if (v.held && recording) arrangementEngine.captureNoteOff(note);
      releaseVoice(note);
    }
    if (recording) {
      for (const note of drumNotesDownRef.current) arrangementEngine.captureNoteOff(note);
    }
    voicesRef.current.clear();
    drumNotesDownRef.current.clear();
    sustainRef.current = false;
    setSustainOn(false);
    audioEngine.releaseAll();
    setActiveNotes((prev) => (prev.size === 0 ? prev : new Map()));
    noteEventBus.emit({ type: 'allnotesoff', time: eventTime() });
  }, [releaseVoice]);

  /** Notes currently sounding on the live synth through this hook (held or sustained). */
  const getSoundingNotes = useCallback((): number[] => {
    return [...voicesRef.current.entries()]
      .filter(([, v]) => v.attacked)
      .map(([note]) => note)
      .sort((a, b) => a - b);
  }, []);

  // Never leave voices behind if the owner unmounts.
  useEffect(() => () => allNotesOff(), [allNotesOff]);

  // --- Effect CCs ----------------------------------------------------------

  // Pickup mode (soft takeover): ignore CC until the physical knob
  // crosses the current software value, preventing jumps on first touch.
  const ccPickup = useRef<Map<number, { lastRaw: number; pickedUp: boolean }>>(new Map());

  const effectToNormalized = useCallback((effect: keyof EffectParams, params: EffectParams): number => {
    switch (effect) {
      case 'reverbWet': return params.reverbWet;
      case 'chorusDepth': return params.chorusDepth;
      case 'filterCutoff': return Math.log(params.filterCutoff / 60) / Math.log(18000 / 60);
      case 'filterResonance': return params.filterResonance / 20;
      default: return 0;
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
    if (cc === CC_SUSTAIN) {
      setSustain(value >= 64);
      return;
    }
    if (cc === CC_ALL_SOUND_OFF || cc === CC_ALL_NOTES_OFF) {
      allNotesOff();
      return;
    }
    if (cc === CC_RESET_ALL_CONTROLLERS) {
      setSustain(false);
      return;
    }

    // Check volume CCs
    if (VOLUME_CCS.includes(cc)) {
      const db = Math.round(-40 + (value / 127) * 40);
      audioEngine.setVolume(db);
      onVolumeChangeRef.current?.(db);
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
      onEffectChangeRef.current?.(params);
    }
  }, [checkPickup, setSustain, allNotesOff]);

  // Stable callbacks for MidiService — prevents useMidi from re-registering on every render
  useLayoutEffect(() => {
    midiNoteOnRef.current = midiNoteOn;
    midiNoteOffRef.current = midiNoteOff;
    handleCCRef.current = handleCC;
  }, [midiNoteOn, midiNoteOff, handleCC]);

  const callbacks: MidiCallbacks = useMemo(() => ({
    onNoteOn: (note: number, velocity: number, channel: number) => midiNoteOnRef.current(note, velocity, channel),
    onNoteOff: (note: number, channel: number) => midiNoteOffRef.current(note, channel),
    onControlChange: (cc: number, value: number) => handleCCRef.current(cc, value),
  }), []); // stable forever — delegates through refs

  return {
    activeNotes,
    /** On-screen keyboard: press a key (always the synth). */
    noteOn,
    /** On-screen keyboard: release a key. */
    noteOff,
    /** On-screen drum pad hit (always the drum engine). */
    drumHit,
    /** Panic: silence every live note. */
    allNotesOff,
    getSoundingNotes,
    sustainOn,
    setSustain,
    callbacks,
    effectParams,
    setEffectParams,
    routingMode,
    setRoutingMode: updateRoutingMode,
  };
}
