/**
 * Event-driven chord attempt judge for chord practice exercises.
 *
 * An "attempt" is judged as soon as the notes struck since the last attempt
 * (held keys + notes clicked within TOGETHER_WINDOW_MS, so the one-pointer
 * on-screen keyboard works too) either form the target chord (correct) or
 * reach as many pitch classes as the target without matching (wrong).
 * After an attempt every key must be released before the next one counts,
 * so holding a chord is one attempt, not many.
 */
import type { NoteEvent } from '@/services/NoteEventBus';
import type { ExpectedChord } from '@/types/appMode';
import { chordMatches, chordPitchClasses, describeChord, pitchClass } from '@/utils/lessonMatching';

export const CHORD_WINDOW_MS = 1500;

export interface ChordAttempt {
  correct: boolean;
  /** Name of the chord that was played, if recognisable. */
  display: string | null;
}

export function createChordJudge() {
  const held = new Set<number>();
  let recent: { note: number; time: number }[] = [];
  let armed = true;

  const judge = {
    /** Index of the next target chord. */
    index: 0,
    handle(event: NoteEvent, target: ExpectedChord | undefined): ChordAttempt | null {
      if (event.type === 'noteoff') {
        held.delete(event.note);
        if (held.size === 0) armed = true;
        return null;
      }
      if (event.type === 'allnotesoff') {
        held.clear();
        recent = [];
        armed = true;
        return null;
      }
      if (event.type !== 'noteon') return null;

      held.add(event.note);
      recent = [...recent.filter((r) => event.time - r.time <= CHORD_WINDOW_MS), { note: event.note, time: event.time }];
      if (!armed || !target) return null;

      const targetPcs = chordPitchClasses(target);
      if (!targetPcs) return null;
      const candidates = [...new Set([...held, ...recent.map((r) => r.note)])];
      const correct = chordMatches(target, held) || chordMatches(target, candidates);
      const playedPcs = new Set(candidates.map(pitchClass));
      if (!correct && playedPcs.size < targetPcs.size) return null; // still building the chord

      armed = false;
      recent = [];
      if (correct) judge.index += 1;
      return { correct, display: describeChord(correct ? [...held, ...candidates] : candidates) };
    },
  };
  return judge;
}
