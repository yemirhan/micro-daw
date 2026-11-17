import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { memo } from 'react';
import type { KitState } from '@/types/sampler';

// BeatView's heavy children are irrelevant here; KitMenu is a stub that exposes its load callbacks.
const kitMenu: { onLoadFactoryKit?: () => void; onLoadKit?: (k: KitState) => void } = {};
vi.mock('./BeatTransport', () => ({ BeatTransport: memo(() => null) }));
vi.mock('./PadGrid', () => ({ PadGrid: memo(() => null) }));
vi.mock('./PadEditor', () => ({ PadEditor: memo(() => null) }));
vi.mock('./StepSequencer', () => ({ StepSequencer: memo(() => null) }));
vi.mock('./KitMenu', () => ({
  KitMenu: (p: { onLoadFactoryKit: () => void; onLoadKit: (k: KitState) => void }) => {
    kitMenu.onLoadFactoryKit = p.onLoadFactoryKit;
    kitMenu.onLoadKit = p.onLoadKit;
    return null;
  },
}));

import { packService, type PendingLoadInfo } from '@/services/PackService';
import { arrangementEngine } from '@/services/ArrangementEngine';
import { createFactoryKit } from '@/audio/sampler/factoryKit';
import { BeatView } from './BeatView';
import { PendingLoadChipView, pendingLoadText } from './PendingLoadChip';

const info = (over: Partial<PendingLoadInfo> = {}): PendingLoadInfo => ({
  key: 'tr808-fischer/classic',
  packId: 'tr808-fischer',
  kitId: 'classic',
  packName: 'TR-808 Recordings',
  waitingForRecording: false,
  ...over,
});

/** Puts a pending "Get & load" in the service snapshot (as getAndLoad does). */
function setPending(i: PendingLoadInfo | null) {
  act(() => (packService as unknown as { set: (p: object) => void }).set({ pendingLoad: i?.key ?? null, pendingLoadInfo: i }));
}

beforeEach(() => {
  HTMLCanvasElement.prototype.getContext = (() => null) as typeof HTMLCanvasElement.prototype.getContext;
  packService.resetForTests();
  arrangementEngine.ensureDrumTrack();
});
afterEach(() => {
  vi.restoreAllMocks();
  packService.resetForTests();
});

describe('Pending "Get & load" chip (N2)', () => {
  it('texts: getting vs. waiting for the take', () => {
    expect(pendingLoadText(info()).text).toBe('Getting TR-808 Recordings…');
    expect(pendingLoadText(info({ waitingForRecording: true })).text).toBe('After take: TR-808 Recordings');
  });

  it('✕ cancels the download while it runs, else just the pending load', () => {
    const cancel = vi.spyOn(packService, 'cancel').mockResolvedValue();
    const cancelPending = vi.spyOn(packService, 'cancelPendingLoad');
    const { rerender } = render(
      <PendingLoadChipView info={info()} install={{ state: 'downloading', receivedBytes: 5, totalBytes: 10, fileIndex: 1, fileCount: 2 }} />,
    );
    expect(screen.getByRole('progressbar')).toHaveAttribute('aria-valuenow', '50');
    fireEvent.click(screen.getByRole('button', { name: 'Cancel loading TR-808 Recordings' }));
    expect(cancel).toHaveBeenCalledWith('tr808-fischer');
    rerender(<PendingLoadChipView info={info({ waitingForRecording: true })} />);
    fireEvent.click(screen.getByRole('button', { name: 'Cancel loading TR-808 Recordings' }));
    expect(cancelPending).toHaveBeenCalledTimes(1);
  });

  it('shows in the Beat pads header while a load is pending', () => {
    render(<BeatView onPadHit={vi.fn()} onPadRelease={vi.fn()} />);
    expect(document.querySelector('[data-pending-load]')).toBeNull();
    setPending(info());
    const chip = document.querySelector('section[aria-label="Pads"] header [data-pending-load]');
    expect(chip).toHaveTextContent('Getting TR-808 Recordings…');
    setPending(null);
    expect(document.querySelector('[data-pending-load]')).toBeNull();
  });

  it('an explicit kit choice in the Beat view cancels the pending load', () => {
    render(<BeatView onPadHit={vi.fn()} onPadRelease={vi.fn()} />);
    const cancelPending = vi.spyOn(packService, 'cancelPendingLoad');
    setPending(info());
    act(() => kitMenu.onLoadFactoryKit!());
    expect(cancelPending).toHaveBeenCalled();
    expect(packService.getSnapshot().pendingLoad).toBeNull();
    setPending(info());
    act(() => kitMenu.onLoadKit!({ ...createFactoryKit(), id: 'mine', name: 'Mine' }));
    expect(packService.getSnapshot().pendingLoad).toBeNull();
    expect(arrangementEngine.getKit(arrangementEngine.getActiveDrumTrackId()!).name).toBe('Mine');
  });
});
