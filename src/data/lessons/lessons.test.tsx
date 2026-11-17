/**
 * Validates every lesson JSON and proves each one can be completed end to
 * end through the real lesson player by feeding it the input its steps ask
 * for (as note events, exactly what a MIDI keyboard / pads produce).
 */
import { describe, expect, it, vi } from 'vitest';
import { act, renderHook } from '@testing-library/react';
import { ALL_LESSONS } from '@/data/lessons';
import { useLessonPlayer } from '@/hooks/useLessonPlayer';
import { noteEventBus } from '@/services/NoteEventBus';
import { chordPitchClasses, CHORD_QUALITY_INTERVALS, noteNameToPitchClass, pitchClass } from '@/utils/lessonMatching';
import { SCALE_DEFINITIONS } from '@/utils/scales';
import { DRUM_SOUNDS, PIANO_END, PIANO_START } from '@/utils/constants';
import type { Lesson, LessonStep } from '@/types/appMode';

vi.mock('@/services/AudioEngine', async () => (await import('@/test/fakeAudioEngine')).audioEngineModule());

const STEP_TYPES = ['info', 'play-notes', 'play-chord', 'play-scale', 'play-drums', 'quiz', 'listen-and-identify'];
const CATEGORIES = ['piano', 'drums', 'theory', 'ear-training', 'songs'];

/** "C#4" / "Eb5" -> MIDI number. */
function labelToMidi(label: string): number | undefined {
  const m = label.match(/^([A-Ga-g])([#b]?)(-?\d+)$/);
  if (!m) return undefined;
  const pc = noteNameToPitchClass(m[1] + m[2]);
  if (pc === undefined) return undefined;
  return (parseInt(m[3], 10) + 1) * 12 + pc;
}

function validateStep(step: LessonStep): string[] {
  const errors: string[] = [];
  if (!STEP_TYPES.includes(step.type)) errors.push(`unknown type ${step.type}`);
  if (!step.title?.trim()) errors.push('missing title');
  if (!step.description?.trim()) errors.push('missing description');
  switch (step.type) {
    case 'play-notes': {
      const notes = step.expectedNotes ?? [];
      if (notes.length === 0) errors.push('play-notes without expectedNotes');
      for (const n of notes) {
        if (!Number.isInteger(n.midi) || n.midi < PIANO_START || n.midi > PIANO_END) {
          errors.push(`note ${n.midi} outside the on-screen keyboard (${PIANO_START}-${PIANO_END})`);
        }
        if (n.label !== undefined) {
          const fromLabel = labelToMidi(n.label);
          if (step.anyOctave ? pitchClass(fromLabel ?? -1) !== pitchClass(n.midi) : fromLabel !== n.midi) {
            errors.push(`label ${n.label} does not match midi ${n.midi}`);
          }
        }
      }
      if (step.noteMode && !['together', 'sequence'].includes(step.noteMode)) errors.push('bad noteMode');
      if ((step.noteMode ?? 'together') === 'together' && new Set(notes.map((n) => n.midi)).size !== notes.length) {
        errors.push('duplicate notes in a "together" step (use noteMode: "sequence" for melodies)');
      }
      break;
    }
    case 'play-chord': {
      const chord = step.expectedChord;
      if (!chord) errors.push('play-chord without expectedChord');
      else {
        if (!chordPitchClasses(chord)) errors.push(`unknown chord ${chord.root}${chord.quality}`);
        if (!(chord.quality in CHORD_QUALITY_INTERVALS)) errors.push(`unknown quality ${chord.quality}`);
        const expectedDisplay = `${chord.root}${chord.quality === 'maj' ? '' : chord.quality}`;
        if (chord.display !== expectedDisplay) errors.push(`display ${chord.display} != ${expectedDisplay}`);
      }
      break;
    }
    case 'play-scale': {
      const scale = step.expectedScale;
      if (!scale) {
        errors.push('play-scale without expectedScale');
        break;
      }
      if (scale.notes[0] !== scale.root) errors.push('scale does not start on its root');
      const def = SCALE_DEFINITIONS.find((d) => d.name === scale.name);
      if (!def) errors.push(`unknown scale ${scale.name}`);
      else {
        const expected = def.intervals.map((i) => (scale.root + i) % 12);
        const body = scale.notes.length === expected.length + 1 && scale.notes.at(-1) === scale.root
          ? scale.notes.slice(0, -1)
          : scale.notes;
        if (JSON.stringify(body) !== JSON.stringify(expected)) {
          errors.push(`scale notes ${JSON.stringify(scale.notes)} != ${scale.name} from ${scale.root}: ${JSON.stringify(expected)}`);
        }
      }
      break;
    }
    case 'play-drums': {
      const pads = step.expectedDrumPattern?.pads ?? [];
      if (pads.length === 0) errors.push('play-drums without pads');
      if (pads.some((p) => !DRUM_SOUNDS[p])) errors.push(`unknown pad in ${JSON.stringify(pads)}`);
      if (new Set(pads).size !== pads.length) errors.push('duplicate pads');
      break;
    }
    case 'quiz':
    case 'listen-and-identify': {
      const options = step.quizOptions ?? [];
      if (options.length < 2) errors.push('quiz needs at least two options');
      if (options.filter((o) => o.correct).length !== 1) errors.push('quiz needs exactly one correct option');
      if (new Set(options.map((o) => o.label)).size !== options.length) errors.push('duplicate quiz labels');
      if (step.type === 'listen-and-identify') {
        const prompt = step.audioPrompt;
        if (!prompt || prompt.notes.length === 0) errors.push('listen step without audioPrompt');
        else if (prompt.notes.some((n) => !Number.isInteger(n) || n < 21 || n > 108)) errors.push('prompt note out of range');
      }
      break;
    }
  }
  return errors;
}

describe('lesson data', () => {
  it('has unique ids and valid metadata', () => {
    const ids = ALL_LESSONS.map((l) => l.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const lesson of ALL_LESSONS) {
      expect(CATEGORIES, lesson.id).toContain(lesson.category);
      expect(['beginner', 'intermediate', 'advanced'], lesson.id).toContain(lesson.difficulty);
      expect(lesson.steps.length, lesson.id).toBeGreaterThan(0);
      expect(lesson.title.trim(), lesson.id).not.toBe('');
    }
  });

  it.each(ALL_LESSONS.map((l) => [l.id, l] as const))('%s: every step is well-formed', (_id, lesson) => {
    const problems = lesson.steps.flatMap((step, i) => validateStep(step).map((e) => `step ${i} "${step.title}": ${e}`));
    expect(problems).toEqual([]);
  });

  it('ear-training answers match what the prompt actually plays', () => {
    const INTERVAL_NAMES: Record<number, string> = {
      2: 'Major 2nd', 3: 'Minor 3rd', 4: 'Major 3rd', 5: 'Perfect 4th', 7: 'Perfect 5th', 9: 'Major 6th', 10: 'Minor 7th', 11: 'Major 7th', 12: 'Octave',
    };
    for (const lesson of ALL_LESSONS) {
      for (const step of lesson.steps) {
        if (step.type !== 'listen-and-identify' || !step.audioPrompt) continue;
        const correct = step.quizOptions!.find((o) => o.correct)!.label;
        const notes = step.audioPrompt.notes;
        if (step.audioPrompt.type === 'interval') {
          expect(correct, `${lesson.id}: ${step.title}`).toBe(INTERVAL_NAMES[Math.abs(notes[1] - notes[0])]);
        } else if (step.audioPrompt.type === 'chord') {
          const third = (notes[1] - notes[0] + 12) % 12;
          expect(correct, `${lesson.id}: ${step.title}`).toBe(third === 4 ? 'Major' : 'Minor');
        }
      }
    }
  });
});

// ---------------------------------------------------------------------------
// Completability: play each lesson through the real lesson player
// ---------------------------------------------------------------------------

let clock = 0;
const emit = (event: Parameters<typeof noteEventBus.emit>[0]) => act(() => noteEventBus.emit(event));
const press = (notes: number[]) => {
  for (const note of notes) emit({ type: 'noteon', note, velocity: 0.8, channel: 1, time: (clock += 20) });
};
const release = (notes: number[]) => {
  for (const note of notes) emit({ type: 'noteoff', note, channel: 1, time: (clock += 20) });
};
const playInOrder = (notes: number[]) => {
  for (const note of notes) {
    press([note]);
    release([note]);
  }
};

/** Feed the input a step asks for. */
function perform(step: LessonStep, completeQuiz: (correct: boolean) => void) {
  clock += 5000; // never inside the previous step's "together" window
  switch (step.type) {
    case 'play-notes': {
      const notes = step.expectedNotes!.map((n) => n.midi);
      if (step.noteMode === 'sequence') playInOrder(notes);
      else {
        press(notes);
        release(notes);
      }
      break;
    }
    case 'play-chord': {
      const pcs = [...chordPitchClasses(step.expectedChord!)!];
      const root = noteNameToPitchClass(step.expectedChord!.root)!;
      // Root-position voicing above C4.
      const notes = pcs.map((pc) => 60 + root + ((pc - root + 12) % 12));
      press(notes);
      release(notes);
      break;
    }
    case 'play-scale': {
      let prev = -1;
      const notes = step.expectedScale!.notes.map((pc) => {
        let n = 48 + pc;
        while (n <= prev) n += 12;
        prev = n;
        return n;
      });
      playInOrder(notes);
      break;
    }
    case 'play-drums':
      for (const padId of step.expectedDrumPattern!.pads) {
        emit({ type: 'drum', padId, note: DRUM_SOUNDS[padId].midiNote, velocity: 0.9, time: (clock += 5) });
      }
      break;
    case 'quiz':
    case 'listen-and-identify':
      act(() => completeQuiz(false));
      act(() => completeQuiz(true));
      break;
    default:
      break;
  }
}

function playThrough(lesson: Lesson) {
  const { result, unmount } = renderHook(() => useLessonPlayer());
  act(() => result.current.startLesson(lesson));
  const stuck: string[] = [];
  for (let i = 0; i < lesson.steps.length; i++) {
    expect(result.current.currentStepIndex).toBe(i);
    const step = lesson.steps[i];
    perform(step, (c) => result.current.completeQuiz(c));
    if (!result.current.stepCompleted) stuck.push(`step ${i} "${step.title}" (${step.type})`);
    act(() => result.current.nextStep());
    if (stuck.length) break;
  }
  const completed = result.current.isLessonCompleted(lesson.id);
  const active = result.current.activeLesson;
  unmount();
  return { stuck, completed, active };
}

describe('every lesson can be completed', () => {
  it.each(ALL_LESSONS.map((l) => [l.id, l] as const))('%s', (_id, lesson) => {
    const { stuck, completed, active } = playThrough(lesson);
    expect(stuck).toEqual([]);
    expect(completed).toBe(true);
    expect(active).toBeNull();
  });
});
