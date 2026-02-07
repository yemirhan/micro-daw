import { useCallback, useEffect, useRef, useState } from 'react';
import * as Tone from 'tone';
import type { RhythmFeedback } from '@/types/appMode';

const PERFECT_WINDOW = 30;  // ms
const GOOD_WINDOW = 80;     // ms
const ACCEPTABLE_WINDOW = 150; // ms

interface RhythmTrainerOptions {
  bpm?: number;
  totalBeats?: number;
  subdivision?: '4n' | '8n';
  onComplete?: (accuracy: number, feedbackHistory: RhythmFeedback[]) => void;
}

export function useRhythmTrainer(bpmOrOptions: number | RhythmTrainerOptions = 120) {
  const opts = typeof bpmOrOptions === 'number' ? { bpm: bpmOrOptions } : bpmOrOptions;
  const bpm = opts.bpm ?? 120;
  const totalBeats = opts.totalBeats;
  const subdivision = opts.subdivision ?? '4n';
  const onComplete = opts.onComplete;

  const [isRunning, setIsRunning] = useState(false);
  const [currentBeat, setCurrentBeat] = useState(0);
  const [feedbackHistory, setFeedbackHistory] = useState<RhythmFeedback[]>([]);
  const beatTimesRef = useRef<number[]>([]);
  const loopRef = useRef<Tone.Loop | null>(null);
  const beatCountRef = useRef(0);
  const completedRef = useRef(false);
  const feedbackRef = useRef<RhythmFeedback[]>([]);

  const stopInternal = useCallback(() => {
    loopRef.current?.stop();
    loopRef.current?.dispose();
    loopRef.current = null;
    Tone.getTransport().stop();
    setIsRunning(false);
  }, []);

  const start = useCallback(() => {
    Tone.getTransport().bpm.value = bpm;
    beatTimesRef.current = [];
    feedbackRef.current = [];
    setFeedbackHistory([]);
    setCurrentBeat(0);
    beatCountRef.current = 0;
    completedRef.current = false;

    const loop = new Tone.Loop((time) => {
      beatTimesRef.current.push(time * 1000); // store in ms
      beatCountRef.current += 1;
      const beat = beatCountRef.current;
      setCurrentBeat(beat);

      if (totalBeats && beat >= totalBeats) {
        // Schedule stop slightly after to avoid cutting off last beat
        setTimeout(() => {
          if (!completedRef.current) {
            completedRef.current = true;
            stopInternal();
            const hist = feedbackRef.current;
            const acc = hist.length > 0
              ? hist.filter((f) => f.timing === 'perfect' || f.timing === 'good').length / hist.length
              : 0;
            onComplete?.(acc, hist);
          }
        }, 200);
      }
    }, subdivision);

    loopRef.current = loop;
    loop.start(0);
    Tone.getTransport().start();
    setIsRunning(true);
  }, [bpm, totalBeats, subdivision, onComplete, stopInternal]);

  const stop = useCallback(() => {
    stopInternal();
  }, [stopInternal]);

  const recordHit = useCallback((): RhythmFeedback | null => {
    if (!isRunning || beatTimesRef.current.length === 0) return null;

    const hitTime = Tone.now() * 1000;
    // Find closest beat
    let closestIdx = 0;
    let closestDiff = Infinity;
    for (let i = 0; i < beatTimesRef.current.length; i++) {
      const diff = Math.abs(hitTime - beatTimesRef.current[i]);
      if (diff < closestDiff) {
        closestDiff = diff;
        closestIdx = i;
      }
    }

    // Also check next expected beat
    const beatInterval = (60 / bpm) * 1000;
    const lastBeatTime = beatTimesRef.current[beatTimesRef.current.length - 1];
    const nextBeatTime = lastBeatTime + beatInterval;
    const nextDiff = Math.abs(hitTime - nextBeatTime);
    if (nextDiff < closestDiff) {
      closestDiff = nextDiff;
    }

    const offsetMs = hitTime - beatTimesRef.current[closestIdx];
    let timing: RhythmFeedback['timing'];
    if (closestDiff <= PERFECT_WINDOW) timing = 'perfect';
    else if (closestDiff <= GOOD_WINDOW) timing = 'good';
    else if (closestDiff <= ACCEPTABLE_WINDOW) timing = offsetMs < 0 ? 'early' : 'late';
    else timing = 'miss';

    const feedback: RhythmFeedback = { beat: closestIdx, timing, offsetMs };
    feedbackRef.current = [...feedbackRef.current, feedback];
    setFeedbackHistory(feedbackRef.current);
    return feedback;
  }, [isRunning, bpm]);

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      loopRef.current?.stop();
      loopRef.current?.dispose();
    };
  }, []);

  const accuracy = feedbackHistory.length > 0
    ? feedbackHistory.filter((f) => f.timing === 'perfect' || f.timing === 'good').length / feedbackHistory.length
    : 0;

  return {
    isRunning,
    currentBeat,
    feedbackHistory,
    accuracy,
    start,
    stop,
    recordHit,
  };
}
