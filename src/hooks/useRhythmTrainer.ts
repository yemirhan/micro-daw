import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import * as Tone from 'tone';
import { useNoteEvents } from './useNoteEvents';
import type { NoteEvent } from '@/services/NoteEventBus';
import type { RhythmFeedback } from '@/types/appMode';
import type { DrumPadId } from '@/types/drums';
import { DRUM_NOTE_TO_PAD } from '@/utils/constants';
import { RhythmScorer } from '@/utils/rhythmScoring';

/** Delay before the first beat so the first click is never late/cut off. */
const START_DELAY_S = 0.1;

interface RhythmTrainerOptions {
  bpm?: number;
  /** Number of ticks (quarter or eighth notes) in an exercise; undefined = endless. */
  totalBeats?: number;
  subdivision?: '4n' | '8n';
  /** Drum pattern practice: pads expected per tick (cycled). */
  pattern?: DrumPadId[][];
  /**
   * Which performance events count as hits (from the shared note event bus,
   * so the MIDI keyboard/pads and the on-screen ones are scored the same):
   * 'keys' = any note-on, 'drums' = drum hits (and pad notes routed as keys),
   * 'none' = only manual recordHit() calls.
   */
  input?: 'keys' | 'drums' | 'none';
  /** Audible click on every tick (default true). */
  metronome?: boolean;
  onComplete?: (accuracy: number, feedbackHistory: RhythmFeedback[]) => void;
}

/**
 * Metronome + timing judge for rhythm training and drum pattern practice.
 *
 * Runs on its own Tone.Clock, NOT on Tone.Transport: the transport belongs
 * to the arrangement (DAW) and must not be started/stopped/re-tempo'd by
 * practice mode.
 */
export function useRhythmTrainer(bpmOrOptions: number | RhythmTrainerOptions = 120) {
  const opts: RhythmTrainerOptions = typeof bpmOrOptions === 'number' ? { bpm: bpmOrOptions } : bpmOrOptions;
  const bpm = opts.bpm ?? 120;
  const totalBeats = opts.totalBeats;
  const subdivision = opts.subdivision ?? '4n';
  const input = opts.input ?? 'keys';
  const metronome = opts.metronome ?? true;
  const pattern = opts.pattern;

  const [isRunning, setIsRunning] = useState(false);
  /** Index of the tick currently sounding (-1 before the first one). */
  const [currentBeat, setCurrentBeat] = useState(-1);
  const [beatsPlayed, setBeatsPlayed] = useState(0);
  const [feedbackHistory, setFeedbackHistory] = useState<RhythmFeedback[]>([]);
  const [accuracy, setAccuracy] = useState(0);

  const onComplete = opts.onComplete;
  const onCompleteRef = useRef(onComplete);
  useLayoutEffect(() => {
    onCompleteRef.current = onComplete;
  }, [onComplete]);

  const clockRef = useRef<Tone.Clock | null>(null);
  const clickRef = useRef<Tone.Synth | null>(null);
  const scorerRef = useRef<RhythmScorer | null>(null);
  const runningRef = useRef(false);

  const disposeClock = useCallback(() => {
    const clock = clockRef.current;
    clockRef.current = null;
    if (clock) {
      clock.stop();
      clock.dispose();
    }
  }, []);

  const stop = useCallback(() => {
    runningRef.current = false;
    disposeClock();
    setIsRunning(false);
  }, [disposeClock]);

  const finish = useCallback(() => {
    if (!runningRef.current) return;
    const scorer = scorerRef.current;
    stop();
    if (!scorer) return;
    const acc = scorer.accuracy();
    setAccuracy(acc);
    onCompleteRef.current?.(acc, [...scorer.feedback]);
  }, [stop]);

  const start = useCallback(() => {
    disposeClock();
    const interval = 60 / bpm / (subdivision === '8n' ? 2 : 1);
    const startTime = Tone.now() + START_DELAY_S;
    scorerRef.current = new RhythmScorer({ startTime, interval, totalBeats, pattern });
    setFeedbackHistory([]);
    setAccuracy(0);
    setCurrentBeat(-1);
    setBeatsPlayed(0);

    if (metronome && !clickRef.current) {
      clickRef.current = new Tone.Synth({
        oscillator: { type: 'triangle' },
        envelope: { attack: 0.001, decay: 0.06, sustain: 0, release: 0.02 },
        volume: -8,
      }).toDestination();
    }

    let tick = 0;
    const ticksPerBar = subdivision === '8n' ? 8 : 4;
    const clock = new Tone.Clock((time: number) => {
      const index = tick;
      tick += 1;
      if (totalBeats !== undefined && index >= totalBeats) {
        // One tick after the last beat: its timing window is over.
        Tone.getDraw().schedule(() => finish(), time);
        return;
      }
      if (metronome) {
        clickRef.current?.triggerAttackRelease(index % ticksPerBar === 0 ? 'C6' : 'G5', 0.03, time);
      }
      Tone.getDraw().schedule(() => {
        if (!runningRef.current) return;
        setCurrentBeat(index);
        setBeatsPlayed(index + 1);
        const scorer = scorerRef.current;
        if (scorer) setAccuracy(scorer.accuracy(time));
      }, time);
    }, 1 / interval);
    clockRef.current = clock;
    runningRef.current = true;
    clock.start(startTime);
    setIsRunning(true);
  }, [bpm, subdivision, totalBeats, pattern, metronome, disposeClock, finish]);

  /** Judge a hit happening now. `padId` for drum pattern practice. */
  const recordHit = useCallback((padId?: DrumPadId): RhythmFeedback | null => {
    const scorer = scorerRef.current;
    if (!runningRef.current || !scorer) return null;
    const now = Tone.getContext().currentTime;
    const fb = scorer.recordHit(now, padId);
    setFeedbackHistory([...scorer.feedback]);
    setAccuracy(scorer.accuracy(now));
    return fb;
  }, []);

  const handleEvent = useCallback((event: NoteEvent) => {
    if (input === 'keys') {
      if (event.type === 'noteon') recordHit();
      return;
    }
    if (input === 'drums') {
      if (event.type === 'drum') recordHit(event.padId);
      else if (event.type === 'noteon') {
        const padId = DRUM_NOTE_TO_PAD.get(event.note);
        if (padId !== undefined) recordHit(padId);
      }
    }
  }, [input, recordHit]);

  useNoteEvents(handleEvent, isRunning && input !== 'none');

  // Cleanup on unmount: stop the clock and free the click synth.
  useEffect(() => () => {
    runningRef.current = false;
    disposeClock();
    clickRef.current?.dispose();
    clickRef.current = null;
  }, [disposeClock]);

  return {
    isRunning,
    currentBeat,
    beatsPlayed,
    feedbackHistory,
    accuracy,
    start,
    stop,
    recordHit,
  };
}
