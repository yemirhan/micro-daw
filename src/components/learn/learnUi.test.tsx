/**
 * Learn UI behaviour added in the redesign: multiple-choice answers, live
 * wrong-note feedback, the lesson-complete screen and its "next lesson".
 * Driven through the real note path (MidiService -> useMidi -> useMidiNotes
 * -> note bus) against the fake audio engine.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { resetFakeEngines, fakeAudio } from '@/test/fakeAudioEngine';
import { useMidiNotes } from '@/hooks/useMidiNotes';
import { useMidi } from '@/hooks/useMidi';
import { midiService } from '@/services/MidiService';
import { audioPromptPlayer } from '@/services/AudioPromptPlayer';
import { LearnView } from '@/components/learn/LearnView';
import { AnswerOptions } from '@/components/learn/AnswerOptions';
import { nextLessonAfter } from '@/components/learn/lessonMeta';
import { ALL_LESSONS, getLessonById } from '@/data/lessons';
import type { Lesson } from '@/types/appMode';

vi.mock('@/services/AudioEngine', async () => (await import('@/test/fakeAudioEngine')).audioEngineModule());
vi.mock('@/services/DrumEngine', async () => (await import('@/test/fakeAudioEngine')).drumEngineModule());
vi.mock('@/services/ArrangementEngine', async () =>
  (await import('@/test/fakeAudioEngine')).arrangementEngineModule(),
);

beforeEach(() => {
  resetFakeEngines();
  audioPromptPlayer.stop();
  localStorage.clear();
  vi.useRealTimers();
});

function LearnHarness() {
  const notes = useMidiNotes();
  useMidi(notes.callbacks);
  return (
    <LearnView
      activeNotes={notes.activeNotes}
      onNoteOn={notes.noteOn}
      onNoteOff={notes.noteOff}
      activePads={new Set()}
      onDrumHit={notes.drumHit}
      detectedChord={null}
      onAllNotesOff={notes.allNotesOff}
    />
  );
}

const key = {
  down: (note: number) => act(() => midiService.simulateNoteOn(note, 0.8)),
  up: (note: number) => act(() => midiService.simulateNoteOff(note)),
};

function resumeAt(lesson: Lesson, stepIndex: number) {
  localStorage.setItem(
    'micro-daw-lesson-progress',
    JSON.stringify({
      lessons: {
        [lesson.id]: {
          lessonId: lesson.id,
          completedSteps: lesson.steps.slice(0, stepIndex).map((_, i) => i),
          completed: false,
          lastStepIndex: Math.max(0, stepIndex - 1),
        },
      },
    }),
  );
}

describe('AnswerOptions', () => {
  const options = [
    { label: 'Major 3rd', correct: false },
    { label: 'Perfect 5th', correct: true },
    { label: 'Octave', correct: false },
  ];

  it('a wrong pick is marked and disabled, the others stay available, the answer is not revealed', () => {
    const onAnswer = vi.fn();
    render(<AnswerOptions options={options} completed={false} onAnswer={onAnswer} />);
    fireEvent.click(screen.getByRole('button', { name: 'Major 3rd' }));
    expect(onAnswer).toHaveBeenLastCalledWith(false);
    expect(screen.getByRole('button', { name: 'Major 3rd' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Perfect 5th' })).toBeEnabled();
    expect(screen.getByRole('button', { name: 'Perfect 5th' })).toHaveAttribute('aria-pressed', 'false');
    expect(screen.getByRole('status')).toHaveTextContent(/not quite/i);
  });

  it('the correct pick locks the question', () => {
    const onAnswer = vi.fn();
    render(<AnswerOptions options={options} completed={false} onAnswer={onAnswer} />);
    fireEvent.click(screen.getByRole('button', { name: 'Perfect 5th' }));
    expect(onAnswer).toHaveBeenLastCalledWith(true);
    for (const o of options) expect(screen.getByRole('button', { name: o.label })).toBeDisabled();
    expect(screen.getByRole('status')).toHaveTextContent(/correct/i);
  });

  it('letter and number keys pick answers', () => {
    const onAnswer = vi.fn();
    render(<AnswerOptions options={options} completed={false} onAnswer={onAnswer} />);
    fireEvent.keyDown(window, { key: 'a' });
    expect(onAnswer).toHaveBeenLastCalledWith(false);
    fireEvent.keyDown(window, { key: '2' });
    expect(onAnswer).toHaveBeenLastCalledWith(true);
  });

  it('a step already completed shows the right answer, locked', () => {
    render(<AnswerOptions options={options} completed onAnswer={vi.fn()} />);
    expect(screen.getByRole('button', { name: 'Perfect 5th' })).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByRole('button', { name: 'Octave' })).toBeDisabled();
  });
});

describe('live feedback in play steps', () => {
  it('a wrong MIDI note flashes "not quite", then the step completes on release', () => {
    const lesson = getLessonById('piano-two-hand-notes')!;
    resumeAt(lesson, 1); // "C and D Together"
    render(<LearnHarness />);
    fireEvent.click(screen.getByText(lesson.title));

    key.down(61);
    expect(screen.getByText(/not quite/i)).toHaveTextContent('C#4');
    key.up(61);

    key.down(60);
    key.down(62);
    expect(screen.getByTestId('release-keys-hint')).toBeInTheDocument();
    expect(screen.getByTestId('lesson-next')).toBeDisabled();
    key.up(60);
    key.up(62);
    expect(screen.getByTestId('lesson-next')).toBeEnabled();
    expect(screen.getByText(/step complete/i, { selector: '[role="status"] *' })).toBeInTheDocument();
  });

  it('Enter advances once the step is complete', () => {
    const lesson = getLessonById('piano-two-hand-notes')!;
    render(<LearnHarness />);
    fireEvent.click(screen.getByText(lesson.title));
    // Step 0 is an info step: complete on entry.
    expect(screen.getByRole('heading', { level: 3, name: lesson.steps[0].title })).toBeInTheDocument();
    fireEvent.keyDown(window, { key: 'Enter' });
    expect(screen.getByRole('heading', { level: 3, name: lesson.steps[1].title })).toBeInTheDocument();
    // Not complete yet: Enter does nothing.
    fireEvent.keyDown(window, { key: 'Enter' });
    expect(screen.getByRole('heading', { level: 3, name: lesson.steps[1].title })).toBeInTheDocument();
  });
});

describe('lesson complete screen', () => {
  it('offers the next lesson, which starts directly; nothing is left sounding', () => {
    const lesson = getLessonById('theory-intervals')!;
    const last = lesson.steps.length - 1;
    resumeAt(lesson, last);
    render(<LearnHarness />);
    fireEvent.click(screen.getByText(lesson.title));
    key.down(60);
    const correct = lesson.steps[last].quizOptions!.find((o) => o.correct)!;
    fireEvent.click(screen.getByRole('button', { name: correct.label }));
    fireEvent.click(screen.getByRole('button', { name: /finish/i }));
    expect(fakeAudio.sounding()).toEqual([]);

    const banner = screen.getByTestId('lesson-complete-banner');
    expect(banner).toHaveTextContent(lesson.title);
    const next = nextLessonAfter(lesson.id, (id) => id === lesson.id)!;
    expect(banner).toHaveTextContent(next.title);

    fireEvent.click(screen.getByRole('button', { name: /next lesson/i }));
    expect(screen.getByRole('heading', { level: 2, name: next.title })).toBeInTheDocument();
    key.up(60);
  });

  it('"Back to lessons" returns to the browser with the lesson marked done', () => {
    const lesson = getLessonById('theory-intervals')!;
    const last = lesson.steps.length - 1;
    resumeAt(lesson, last);
    render(<LearnHarness />);
    fireEvent.click(screen.getByText(lesson.title));
    const correct = lesson.steps[last].quizOptions!.find((o) => o.correct)!;
    fireEvent.click(screen.getByRole('button', { name: correct.label }));
    fireEvent.click(screen.getByRole('button', { name: /finish/i }));
    fireEvent.click(screen.getByRole('button', { name: /back to lessons/i }));
    const card = document.querySelector(`[data-lesson-id="${lesson.id}"]`)!;
    expect(card).toHaveTextContent(/done/i);
  });
});

describe('nextLessonAfter', () => {
  it('prefers the next unfinished lesson in the same category, then any unfinished lesson', () => {
    const piano = ALL_LESSONS.filter((l) => l.category === 'piano');
    expect(nextLessonAfter(piano[0].id, () => false)?.id).toBe(piano[1].id);
    const done = new Set(piano.map((l) => l.id));
    const next = nextLessonAfter(piano[0].id, (id) => done.has(id));
    expect(next && next.category).not.toBe('piano');
    expect(nextLessonAfter(piano[0].id, () => true)).toBeUndefined();
  });
});
