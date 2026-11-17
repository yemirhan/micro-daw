import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { createFactoryKit, defaultPadSettings } from '@/audio/sampler/factoryKit';
import { packService } from '@/services/PackService';
import type { PadSettings } from '@/types/sampler';
import { MissingPacksStrip } from './MissingPacks';
import { PadEditor } from './PadEditor';
import { PadGrid } from './PadGrid';
import { missingPacks } from './usePackAvailability';

beforeEach(() => {
  // jsdom has no canvas.
  HTMLCanvasElement.prototype.getContext = (() => null) as typeof HTMLCanvasElement.prototype.getContext;
});
afterEach(() => vi.restoreAllMocks());

const packPad = (packId: string, file: string, name: string): PadSettings => ({
  ...defaultPadSettings(),
  sample: { source: { kind: 'pack', packId, file }, name },
});

describe('missingPacks', () => {
  it('groups pads by pack and skips installed packs and other sources', () => {
    const pads = [packPad('a', 'k.wav', 'Kick'), defaultPadSettings(), packPad('b', 's.wav', 'Snare'), packPad('a', 'h.wav', 'Hat'), ...createFactoryKit().pads.slice(0, 2)];
    const avail = { isMissing: (id: string) => id === 'a', nameOf: (id: string) => (id === 'a' ? 'Pack A' : id), stateOf: () => ({ state: 'idle' as const }) };
    expect(missingPacks(pads, avail)).toEqual([{ id: 'a', name: 'Pack A', pads: [0, 3], state: { state: 'idle' } }]);
    expect(missingPacks(pads, { ...avail, isMissing: () => false })).toEqual([]);
  });
});

describe('missing pack UI (P1)', () => {
  it('the strip over the pads offers Get, progress and retry per pack', () => {
    const onGet = vi.fn();
    const { rerender } = render(<MissingPacksStrip missing={[{ id: 'tr808-fischer', name: 'TR-808 recordings', pads: [0, 1], state: { state: 'idle' } }]} onGet={onGet} />);
    const strip = screen.getByRole('status', { name: 'Packs not installed' });
    expect(strip).toHaveTextContent('Pack not installed: TR-808 recordings · 2 pads');
    fireEvent.click(within(strip).getByRole('button', { name: 'Get TR-808 recordings' }));
    expect(onGet).toHaveBeenCalledWith('tr808-fischer');
    rerender(<MissingPacksStrip missing={[{ id: 'tr808-fischer', name: 'TR-808 recordings', pads: [0], state: { state: 'error', message: 'HTTP 500' } }]} onGet={onGet} />);
    fireEvent.click(screen.getByRole('button', { name: 'Retry downloading TR-808 recordings' }));
    expect(onGet).toHaveBeenCalledTimes(2);
    rerender(<MissingPacksStrip missing={[]} onGet={onGet} />);
    expect(screen.queryByRole('status', { name: 'Packs not installed' })).toBeNull();
  });

  it('the pad editor says "Pack not installed" with a Get button that installs the pack', async () => {
    const install = vi.spyOn(packService, 'install').mockResolvedValue(null);
    render(<PadEditor pad={0} settings={packPad('tr808-fischer', 'bd/BD0000.wav', 'Kick')} onChange={vi.fn()} />);
    const header = await screen.findByText('Pack not installed');
    expect(screen.queryByText('File not found')).toBeNull();
    expect(screen.getByText(/This pad plays a sound from tr808-fischer\. Get the pack to hear it\./)).toBeInTheDocument();
    fireEvent.click(within(header).getByRole('button', { name: 'Get tr808-fischer' }));
    await waitFor(() => expect(install).toHaveBeenCalledWith('tr808-fischer'));
  });

  it('a pad of a missing pack shows "no pack" instead of "missing"', async () => {
    const pads = createFactoryKit().pads;
    pads[0] = packPad('tr808-fischer', 'bd/BD0000.wav', 'Kick');
    render(<PadGrid pads={pads} selectedPad={0} onHit={vi.fn()} onSelect={vi.fn()} />);
    const thumb = await waitFor(() => {
      const el = document.querySelector('[data-pad="0"] [data-status="pack-missing"]');
      expect(el).not.toBeNull();
      return el!;
    });
    expect(thumb).toHaveTextContent('no pack');
    expect(thumb).toHaveAttribute('title', 'Pack not installed: tr808-fischer');
  });
});
