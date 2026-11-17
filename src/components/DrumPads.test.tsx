import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { DrumPads, shortPadName } from './DrumPads';

describe('shortPadName', () => {
  it.each([
    ['Kick', 'KCK'],
    ['Snare', 'SNR'],
    ['Closed HH', 'CHH'],
    ['Open HH', 'OHH'],
    ['Sub Kick', 'SKC'],
    ['Mid Tom', 'MTM'],
    ['Ride', 'RD'],
    ['big_snare-02', 'BS02'],
    ['   ', '—'],
  ])('%s → %s', (name, tag) => {
    expect(shortPadName(name)).toBe(tag);
  });
});

describe('DrumPads', () => {
  it('uses the built-in labels without looks', () => {
    render(<DrumPads activePads={new Set()} onHit={vi.fn()} compact />);
    expect(screen.getByRole('button', { name: 'Kick' })).toHaveTextContent('KCK');
  });

  it('shows the kit names when looks are given (empty pads say so)', () => {
    const looks = [{ name: 'Big Snare', color: 'red' }, { name: '', color: 'blue' }];
    render(<DrumPads activePads={new Set()} onHit={vi.fn()} looks={looks} compact />);
    expect(screen.getByRole('button', { name: 'Big Snare' })).toHaveTextContent('BSN');
    expect(screen.getByRole('button', { name: 'Empty' })).toHaveTextContent('—');
    // Pads without a look keep their defaults.
    expect(screen.getByRole('button', { name: 'Closed HH' })).toBeInTheDocument();
  });
});
