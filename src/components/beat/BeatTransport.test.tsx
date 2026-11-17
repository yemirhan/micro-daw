import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, within } from '@testing-library/react';
import type { LoopRecordSettings } from '@/types/sampler';
import { BeatTransport, type BeatTransportProps } from './BeatTransport';

const LOOP: LoopRecordSettings = { bars: 2, quantize: '1/16', swing: 0.5, swingGrid: '1/16', countInBars: 1, metronome: true, overdub: true };

function setup(extra: Partial<BeatTransportProps> = {}) {
  const props: BeatTransportProps = {
    playing: false,
    recording: false,
    countInRemaining: 0,
    bpm: 120,
    loop: LOOP,
    patternStartBeat: 0,
    onTogglePlay: vi.fn(),
    onToggleRecord: vi.fn(),
    onBpmChange: vi.fn(),
    onLoopChange: vi.fn(),
    ...extra,
  };
  render(<BeatTransport {...props} />);
  return props;
}

describe('BeatTransport', () => {
  it('shows swing as MPC percent next to its knob', () => {
    setup();
    expect(screen.getByRole('slider', { name: 'Swing' })).toHaveAttribute('aria-valuetext', '75%');
  });

  it('shows the count-in instead of the loop position while counting in', () => {
    setup({ countInRemaining: 3.2 });
    expect(screen.getByText('COUNT-IN').parentElement).toHaveTextContent('COUNT-IN4');
    expect(screen.getByRole('button', { name: 'Stop recording' })).toHaveAttribute('aria-pressed', 'true');
  });

  it('loop settings controls patch the loop', () => {
    const props = setup();
    fireEvent.click(screen.getByRole('radio', { name: '4' }));
    expect(props.onLoopChange).toHaveBeenCalledWith({ bars: 4 });
    fireEvent.click(screen.getByRole('radio', { name: 'Replace' }));
    expect(props.onLoopChange).toHaveBeenCalledWith({ overdub: false });
    fireEvent.click(screen.getByRole('button', { name: 'Metronome' }));
    expect(props.onLoopChange).toHaveBeenCalledWith({ metronome: false });
  });

  it('swing grid selector patches swingGrid (default 1/16), independent of quantize', () => {
    const props = setup();
    const group = screen.getByRole('radiogroup', { name: 'Swing grid' });
    const radios = within(group).getAllByRole('radio');
    expect(radios.map((r) => r.textContent)).toEqual(['1/8', '1/16']);
    expect(within(group).getByRole('radio', { name: '1/16' })).toHaveAttribute('aria-checked', 'true');
    fireEvent.click(within(group).getByRole('radio', { name: '1/8' }));
    expect(props.onLoopChange).toHaveBeenCalledWith({ swingGrid: '1/8' });
    expect(props.onLoopChange).not.toHaveBeenCalledWith(expect.objectContaining({ quantize: expect.anything() }));
  });

  it('offers Export only when a handler is given', () => {
    setup();
    expect(screen.queryByRole('button', { name: 'Export' })).toBeNull();
  });

  it('Export opens the export dialog', () => {
    const onExport = vi.fn();
    setup({ onExport });
    fireEvent.click(screen.getByRole('button', { name: 'Export' }));
    expect(onExport).toHaveBeenCalledTimes(1);
  });
});
