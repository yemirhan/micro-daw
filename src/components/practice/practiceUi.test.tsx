/**
 * Practice UI added in the redesign: readable session names in the stats,
 * the live tally in the exercise header and "new personal best".
 */
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, within } from '@testing-library/react';
import { resetFakeEngines } from '@/test/fakeAudioEngine';
import { useMidiNotes } from '@/hooks/useMidiNotes';
import { useMidi } from '@/hooks/useMidi';
import { useLearningMode } from '@/hooks/useLearningMode';
import { midiService } from '@/services/MidiService';
import { PracticeView } from '@/components/practice/PracticeView';
import { RecentSessions } from '@/components/practice/stats/RecentSessions';
import { sessionTitle } from '@/components/practice/practiceMeta';
import { getPracticeById } from '@/data/practices';

vi.mock('@/services/AudioEngine', async () => (await import('@/test/fakeAudioEngine')).audioEngineModule());
vi.mock('@/services/DrumEngine', async () => (await import('@/test/fakeAudioEngine')).drumEngineModule());
vi.mock('@/services/ArrangementEngine', async () =>
  (await import('@/test/fakeAudioEngine')).arrangementEngineModule(),
);

beforeEach(() => {
  resetFakeEngines();
  localStorage.clear();
});

function Harness() {
  const notes = useMidiNotes();
  useMidi(notes.callbacks);
  const learning = useLearningMode(notes.activeNotes);
  return (
    <PracticeView
      activeNotes={notes.activeNotes}
      onNoteOn={notes.noteOn}
      onNoteOff={notes.noteOff}
      activePads={new Set()}
      onDrumHit={notes.drumHit}
      detectedChord={learning.detectedChord}
      highlightedNotes={learning.highlightedNotes}
      selectedRoot={learning.selectedRoot}
      setSelectedRoot={learning.setSelectedRoot}
      selectedScale={learning.selectedScale}
      setSelectedScale={learning.setSelectedScale}
      onAllNotesOff={notes.allNotesOff}
    />
  );
}

const play = (note: number) => {
  act(() => midiService.simulateNoteOn(note, 0.8));
  act(() => midiService.simulateNoteOff(note));
};

describe('practice UI', () => {
  it('session ids are shown as exercise / activity names', () => {
    expect(sessionTitle('chord-prog-cgamf')).toBe(getPracticeById('chord-prog-cgamf')!.title);
    expect(sessionTitle('free-free-play')).toBe('Free Play');
    expect(sessionTitle('unknown-id')).toBe('unknown-id');
    render(
      <RecentSessions
        sessions={[
          { exerciseId: 'scale-c-major', timestamp: new Date().toISOString(), durationSeconds: 75, accuracy: 90, stars: 3, category: 'scales' },
        ]}
      />,
    );
    expect(screen.getByText('C Major Scale')).toBeInTheDocument();
    expect(screen.getByText('90%')).toBeInTheDocument();
  });

  it('the exercise header tallies correct notes and misses live', () => {
    render(<Harness />);
    fireEvent.click(screen.getByText(getPracticeById('piano-white-keys-cg')!.title));
    play(60);
    play(61); // wrong
    const header = screen.getByRole('banner');
    expect(within(header).getByText(/correct/i)).toHaveTextContent('1');
    expect(within(header).getByText(/misses/i)).toHaveTextContent('1');
    expect(within(header).getByText('50%')).toBeInTheDocument();
  });

  it('beating the saved best shows "new personal best"', () => {
    localStorage.setItem(
      'micro-daw-practice-progress',
      JSON.stringify({ 'piano-find-c': { bestStars: 1, bestAccuracy: 0.5, attempts: 1 } }),
    );
    render(<Harness />);
    fireEvent.click(screen.getByText(getPracticeById('piano-find-c')!.title));
    play(60);
    expect(screen.getByText('100%')).toBeInTheDocument();
    expect(screen.getByText(/new personal best/i)).toBeInTheDocument();
    // Retry, do worse: no badge.
    fireEvent.click(screen.getByRole('button', { name: /try again/i }));
    play(61);
    play(60);
    expect(screen.getByText('50%')).toBeInTheDocument();
    expect(screen.queryByText(/new personal best/i)).not.toBeInTheDocument();
  });
});
