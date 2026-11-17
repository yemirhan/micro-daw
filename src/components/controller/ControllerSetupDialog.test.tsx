import { beforeEach, describe, expect, it } from 'vitest';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { ControllerSetupDialog } from '@/components/controller/ControllerSetupDialog';
import { controllerMapping, createDefaultProfile } from '@/services/ControllerMapping';

beforeEach(() => {
  controllerMapping.cancelLearn();
  controllerMapping.setProfile(createDefaultProfile());
});

describe('ControllerSetupDialog', () => {
  it('runs the pad wizard: prompt, advance on hits, Backspace back, Esc cancels', () => {
    render(<ControllerSetupDialog open onOpenChange={() => undefined} />);
    expect(screen.getByLabelText(/Pad 1: note 36/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: /learn pads/i }));
    expect(screen.getByText('Hit pad 1')).toBeInTheDocument();

    act(() => {
      controllerMapping.handleNoteOn(60, 0.8, 10);
    });
    expect(screen.getByText('Hit pad 2')).toBeInTheDocument();
    expect(screen.getByLabelText(/Pad 1: note 60/)).toBeInTheDocument();
    expect(screen.getByTestId('midi-activity')).toHaveTextContent('Note C4 · 60');

    fireEvent.keyDown(window, { key: 'Backspace' });
    expect(screen.getByText('Hit pad 1')).toBeInTheDocument();

    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' });
    expect(controllerMapping.isLearning()).toBe(false);
    expect(screen.queryByText('Hit pad 1')).not.toBeInTheDocument();
  });
});
