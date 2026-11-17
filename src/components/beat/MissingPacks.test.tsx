import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { createFactoryKit, defaultPadSettings } from '@/audio/sampler/factoryKit';
import { packService } from '@/services/PackService';
import type { PadSettings } from '@/types/sampler';
import { MissingPacksStrip } from './MissingPacks';
import { PadEditor } from './PadEditor';
import { PadGrid } from './PadGrid';
import { missingPacks, missingPackTitles, type MissingPack } from './usePackAvailability';

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
    expect(missingPacks(pads, avail)).toEqual([{ id: 'a', name: 'Pack A', pads: [0, 3], state: { state: 'idle' }, available: true }]);
    expect(missingPacks(pads, { ...avail, isMissing: () => false })).toEqual([]);
  });

  it('marks a pack the catalog no longer lists as unavailable (review L-c)', () => {
    const pads = [packPad('gone', 'k.wav', 'Kick')];
    const avail = { isMissing: () => true, nameOf: (id: string) => id, stateOf: () => ({ state: 'idle' as const }), isAvailable: () => false };
    expect(missingPacks(pads, avail)[0].available).toBe(false);
  });

  it('missingPackTitles: one tooltip per affected pad (review L-b)', () => {
    const titles = missingPackTitles(
      [
        { id: 'a', name: 'Pack A', pads: [0, 2], state: { state: 'idle' }, available: true },
        { id: 'b', name: 'Pack B', pads: [3], state: { state: 'idle' }, available: false },
      ],
      4,
    );
    expect(titles).toEqual(['Pack not installed: Pack A', null, 'Pack not installed: Pack A', 'Pack no longer available: Pack B']);
  });
});

const missingOf = (over: Partial<MissingPack> = {}): MissingPack => ({ id: 'tr808-fischer', name: 'tr808-fischer', pads: [0], state: { state: 'idle' }, available: true, ...over });

describe('missing pack UI (P1)', () => {
  it('the strip over the pads offers Get, progress and retry per pack', () => {
    const onGet = vi.fn();
    const { rerender } = render(<MissingPacksStrip missing={[missingOf({ name: 'TR-808 recordings', pads: [0, 1] })]} onGet={onGet} />);
    const strip = screen.getByRole('status', { name: 'Packs not installed' });
    expect(strip).toHaveTextContent('Not installed: TR-808 recordings · 2 pads');
    fireEvent.click(within(strip).getByRole('button', { name: 'Get TR-808 recordings' }));
    expect(onGet).toHaveBeenCalledWith('tr808-fischer');
    rerender(<MissingPacksStrip missing={[missingOf({ name: 'TR-808 recordings', state: { state: 'error', message: 'Download timed out: the server stopped responding' } })]} onGet={onGet} />);
    // The whole error is there (its own line, wrapping), not truncated (review L-d).
    expect(screen.getByRole('alert')).toHaveTextContent('Download timed out: the server stopped responding');
    fireEvent.click(screen.getByRole('button', { name: 'Retry downloading TR-808 recordings' }));
    expect(onGet).toHaveBeenCalledTimes(2);
    rerender(<MissingPacksStrip missing={[]} onGet={onGet} />);
    expect(screen.queryByRole('status', { name: 'Packs not installed' })).toBeNull();
  });

  it('an unavailable pack offers Clear pads / Replace with built-in instead of Get (review L-c)', () => {
    const onGet = vi.fn();
    const onFix = vi.fn();
    const pack = missingOf({ id: 'gone', name: 'Old Pack', pads: [0, 4], available: false, state: { state: 'error', message: 'not in catalog' } });
    render(<MissingPacksStrip missing={[pack]} onGet={onGet} onFix={onFix} />);
    const strip = screen.getByRole('status', { name: 'Packs not installed' });
    expect(strip).toHaveTextContent('No longer available: Old Pack · 2 pads');
    expect(within(strip).queryByRole('button', { name: /^Get / })).toBeNull();
    expect(within(strip).queryByRole('alert')).toBeNull();
    fireEvent.click(within(strip).getByRole('button', { name: 'Clear the pads that use Old Pack' }));
    expect(onFix).toHaveBeenLastCalledWith(pack, 'clear');
    fireEvent.click(within(strip).getByRole('button', { name: 'Replace the pads that use Old Pack with built-in sounds' }));
    expect(onFix).toHaveBeenLastCalledWith(pack, 'builtin');
    expect(onGet).not.toHaveBeenCalled();
  });

  it('the pad editor says "Pack not installed" with a Get button that installs the pack', async () => {
    const onGet = vi.fn();
    render(<PadEditor pad={0} settings={packPad('tr808-fischer', 'bd/BD0000.wav', 'Kick')} onChange={vi.fn()} missingPack={missingOf()} onGetPack={onGet} />);
    const header = await screen.findByText('Pack not installed');
    expect(screen.queryByText('File not found')).toBeNull();
    expect(screen.getByText(/This pad plays a sound from tr808-fischer\. Get the pack to hear it\./)).toBeInTheDocument();
    fireEvent.click(within(header).getByRole('button', { name: 'Get tr808-fischer' }));
    expect(onGet).toHaveBeenCalledWith('tr808-fischer');
  });

  it('the pad editor says "No longer available" with clear / replace for a withdrawn pack', async () => {
    const onFix = vi.fn();
    const pack = missingOf({ available: false });
    render(<PadEditor pad={0} settings={packPad('tr808-fischer', 'bd/BD0000.wav', 'Kick')} onChange={vi.fn()} missingPack={pack} onFixMissing={onFix} />);
    const header = await screen.findByText('No longer available');
    expect(screen.getByText(/no longer available to download/)).toBeInTheDocument();
    fireEvent.click(within(header).getByRole('button', { name: /Replace the pads/ }));
    expect(onFix).toHaveBeenCalledWith(pack, 'builtin');
  });

  it('a pad of a missing pack shows "no pack" instead of "missing"', async () => {
    const pads = createFactoryKit().pads;
    pads[0] = packPad('tr808-fischer', 'bd/BD0000.wav', 'Kick');
    const titles = missingPackTitles([missingOf()], 16);
    render(<PadGrid pads={pads} selectedPad={0} onHit={vi.fn()} onSelect={vi.fn()} missingPackTitles={titles} />);
    const thumb = await waitFor(() => {
      const el = document.querySelector('[data-pad="0"] [data-status="pack-missing"]');
      expect(el).not.toBeNull();
      return el!;
    });
    expect(thumb).toHaveTextContent('no pack');
    expect(thumb).toHaveAttribute('title', 'Pack not installed: tr808-fischer');
  });

  it('pads never subscribe to pack state themselves (review L-b)', () => {
    const subscribe = vi.spyOn(packService, 'subscribe');
    const pads = createFactoryKit().pads;
    pads[0] = packPad('tr808-fischer', 'bd/BD0000.wav', 'Kick');
    render(<PadGrid pads={pads} selectedPad={0} onHit={vi.fn()} onSelect={vi.fn()} missingPackTitles={missingPackTitles([missingOf()], 16)} />);
    expect(subscribe).not.toHaveBeenCalled();
  });
});
