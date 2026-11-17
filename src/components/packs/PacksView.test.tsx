import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, within } from '@testing-library/react';
import type { PackInstallState } from '@/types/packs';
import { createFactoryKit } from '@/audio/sampler/factoryKit';
import { FAKE_MACHINE, fakeInstalled, fakeManifest } from './fixtures';
import type { PacksController } from './usePacksController';
import { soundKey } from './packsModel';

// The real controller binds to the audio + pack services; the browser is presentational.
vi.mock('./usePacksController', () => ({ usePacksController: vi.fn() }));
const { PacksBrowser } = await import('./PacksView');

const m808 = fakeManifest();
const m909 = fakeManifest({ id: 'tr909-x', name: '909 Hits', tagline: 'Punchy techno', tags: ['909', 'techno'], license: 'CC-BY-4.0', attribution: 'Sounds by Jane, CC BY 4.0' });

function controller(over: Partial<PacksController> = {}): PacksController {
  return {
    builtins: [FAKE_MACHINE],
    catalog: { version: 1, updatedAt: '2026-11-01', packs: [m808, m909] },
    catalogStatus: 'ready',
    catalogError: null,
    installed: [],
    stateOf: () => ({ state: 'idle' }),
    retryCatalog: vi.fn(),
    install: vi.fn(),
    cancel: vi.fn(),
    uninstall: vi.fn(),
    uninstallError: () => null,
    dismissUninstallError: vi.fn(),
    hasUpdate: () => false,
    kitFor: vi.fn(() => createFactoryKit()),
    previewing: null,
    previewKit: vi.fn(),
    stopKitPreview: vi.fn(),
    playSound: vi.fn(),
    soundPreviewing: null,
    loadKit: vi.fn(),
    ...over,
  };
}

const card = (id: string) => document.querySelector(`[data-pack-id="${id}"]`) as HTMLElement;

describe('PacksBrowser', () => {
  it('shows built-in kits and downloadable packs', () => {
    render(<PacksBrowser controller={controller()} />);
    expect(screen.getByRole('region', { name: 'Built-in kits' })).toHaveTextContent('808-style');
    const remote = screen.getByRole('region', { name: 'Downloadable packs' });
    expect(remote).toHaveTextContent('TR-808 recordings');
    expect(remote).toHaveTextContent('909 Hits');
    expect(within(card('tr909-x')).getByText('CC BY 4.0')).toBeInTheDocument();
  });

  it('filters by search and tag', () => {
    render(<PacksBrowser controller={controller()} />);
    fireEvent.change(screen.getByRole('searchbox', { name: 'Search packs' }), { target: { value: 'techno' } });
    expect(card('tr909-x')).toBeTruthy();
    expect(card('tr808-fischer')).toBeNull();
    expect(screen.queryByRole('region', { name: 'Built-in kits' })).toBeNull();

    fireEvent.change(screen.getByRole('searchbox', { name: 'Search packs' }), { target: { value: 'zzz' } });
    fireEvent.click(screen.getByRole('button', { name: 'Show all packs' }));
    expect(card('tr808-fischer')).toBeTruthy();

    fireEvent.click(screen.getByRole('radio', { name: /^808/ }));
    expect(card('builtin:tr808')).toBeTruthy();
    expect(card('tr808-fischer')).toBeTruthy();
    expect(card('tr909-x')).toBeNull();
  });

  it('installs, shows progress with cancel, and retry on error', () => {
    const states: Record<string, PackInstallState> = {
      'tr808-fischer': { state: 'downloading', receivedBytes: 1, totalBytes: 4, fileIndex: 0, fileCount: 3 },
      'tr909-x': { state: 'error', message: 'Network down' },
    };
    const c = controller({ stateOf: (id) => states[id] ?? { state: 'idle' } });
    render(<PacksBrowser controller={c} />);
    expect(within(card('tr808-fischer')).getByText('25%')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Cancel downloading TR-808 recordings' }));
    expect(c.cancel).toHaveBeenCalledWith('tr808-fischer');
    fireEvent.click(screen.getByRole('button', { name: 'Retry downloading 909 Hits' }));
    expect(c.install).toHaveBeenCalledWith('tr909-x');
    // Install clicks don't open the detail panel.
    expect(screen.queryByRole('complementary')).toBeNull();
  });

  it('opens details; kits need the pack installed', () => {
    const c = controller();
    render(<PacksBrowser controller={c} />);
    fireEvent.click(screen.getByRole('button', { name: /^909 Hits:/ }));
    const panel = screen.getByRole('complementary', { name: '909 Hits details' });
    expect(within(panel).getByRole('button', { name: 'Load on pads' })).toBeDisabled();
    expect(panel).toHaveTextContent('Sounds by Jane, CC BY 4.0');
    fireEvent.click(within(panel).getByRole('button', { name: 'Get 909 Hits' }));
    expect(c.install).toHaveBeenCalledWith('tr909-x');
    // Sounds play (remote preview before install).
    fireEvent.click(within(panel).getByRole('button', { name: 'BD0000' }));
    expect(c.playSound).toHaveBeenCalledWith(expect.objectContaining({ id: 'tr909-x' }), { kind: 'file', file: m909.files[0] });
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(screen.queryByRole('complementary')).toBeNull();
    // Esc also stops a running kit preview.
    expect(c.stopKitPreview).toHaveBeenCalled();
  });

  it('offers an update when the catalog has a newer version', () => {
    const c = controller({ installed: [fakeInstalled(m808)], hasUpdate: (id) => id === 'tr808-fischer' });
    render(<PacksBrowser controller={c} />);
    fireEvent.click(screen.getByRole('button', { name: /^TR-808 recordings:/ }));
    fireEvent.click(within(screen.getByRole('complementary')).getByRole('button', { name: 'Update' }));
    expect(c.install).toHaveBeenCalledWith('tr808-fischer');
  });

  it('lists only audio files as sounds (licence / readme files are not sounds)', () => {
    const m = fakeManifest({
      files: [...m808.files, { path: 'LICENSE.txt', url: 'https://raw.githubusercontent.com/x/y/sha/LICENSE' }, { path: 'README.md', url: 'https://raw.githubusercontent.com/x/y/sha/README.md' }],
    });
    render(<PacksBrowser controller={controller({ catalog: { version: 1, updatedAt: '2026-11-01', packs: [m] } })} />);
    expect(within(card('tr808-fischer')).getByText(/3 sounds/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: /^TR-808 recordings:/ }));
    const panel = screen.getByRole('complementary');
    expect(within(panel).queryByRole('button', { name: 'LICENSE' })).toBeNull();
    expect(within(panel).queryByRole('button', { name: 'README' })).toBeNull();
  });

  it('loads and previews kits of an installed pack, and uninstalls', () => {
    const c = controller({ installed: [fakeInstalled(m808)] });
    render(<PacksBrowser controller={c} />);
    fireEvent.click(screen.getByRole('button', { name: /^TR-808 recordings:/ }));
    const panel = screen.getByRole('complementary');
    fireEvent.click(within(panel).getByRole('button', { name: 'Preview 808 Basic' }));
    expect(c.previewKit).toHaveBeenCalledWith('tr808-fischer/main', expect.any(Object));
    fireEvent.click(within(panel).getByRole('button', { name: 'Load on pads' }));
    expect(c.kitFor).toHaveBeenCalledWith(expect.objectContaining({ id: 'tr808-fischer' }), 'main');
    expect(c.loadKit).toHaveBeenCalled();
    // Uninstall asks for confirmation first.
    fireEvent.click(within(panel).getByRole('button', { name: 'Uninstall' }));
    expect(c.uninstall).not.toHaveBeenCalled();
    fireEvent.click(within(panel).getByRole('button', { name: 'Confirm uninstall' }));
    expect(c.uninstall).toHaveBeenCalledWith('tr808-fischer');
    expect(screen.getByRole('radio', { name: /Installed/ })).toBeInTheDocument();
  });

  it('shows an uninstall failure inline; trying again clears it', () => {
    const c = controller({ installed: [fakeInstalled(m808)], uninstallError: (id) => (id === 'tr808-fischer' ? 'EBUSY: resource busy' : null) });
    render(<PacksBrowser controller={c} />);
    fireEvent.click(screen.getByRole('button', { name: /^TR-808 recordings:/ }));
    const panel = screen.getByRole('complementary');
    expect(within(panel).getByRole('alert')).toHaveTextContent("Couldn't uninstall: EBUSY: resource busy");
    fireEvent.click(within(panel).getByRole('button', { name: 'Uninstall' }));
    expect(c.dismissUninstallError).toHaveBeenCalledWith('tr808-fischer');
  });

  it('marks only the sound that is actually playing (follows the preview, not the last click)', () => {
    const snare = soundKey('builtin:tr808', { kind: 'voice', voice: 'snare' });
    const { rerender } = render(<PacksBrowser controller={controller({ soundPreviewing: snare })} />);
    fireEvent.click(screen.getByRole('button', { name: /^808-style:/ }));
    const panel = screen.getByRole('complementary');
    expect(within(panel).getByRole('button', { name: 'Snare' })).toHaveAttribute('data-playing', 'true');
    expect(panel.querySelectorAll('[data-playing]')).toHaveLength(1);
    // Preview ended: the marker goes away.
    rerender(<PacksBrowser controller={controller({ soundPreviewing: null })} />);
    expect(screen.getByRole('complementary').querySelectorAll('[data-playing]')).toHaveLength(0);
  });

  it('plays built-in voices and loads the built-in kit', () => {
    const c = controller();
    render(<PacksBrowser controller={c} />);
    fireEvent.click(screen.getByRole('button', { name: /^808-style:/ }));
    const panel = screen.getByRole('complementary');
    expect(within(panel).queryByRole('button', { name: /^Get/ })).toBeNull();
    fireEvent.click(within(panel).getByRole('button', { name: 'Snare' }));
    expect(c.playSound).toHaveBeenCalledWith(expect.objectContaining({ kind: 'builtin' }), { kind: 'voice', voice: 'snare' });
    fireEvent.click(within(panel).getByRole('button', { name: 'Load on pads' }));
    expect(c.kitFor).toHaveBeenCalledWith(expect.objectContaining({ id: 'builtin:tr808' }), 'tr808');
  });

  it('shows the offline state with retry; built-in kits stay', () => {
    const c = controller({ catalog: null, catalogStatus: 'error', catalogError: 'getaddrinfo ENOTFOUND' });
    render(<PacksBrowser controller={c} />);
    expect(screen.getByText("Can't reach the pack catalog")).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Try again' }));
    expect(c.retryCatalog).toHaveBeenCalled();
    expect(card('builtin:tr808')).toBeTruthy();
  });

  it('shows loading placeholders while the catalog loads', () => {
    render(<PacksBrowser controller={controller({ catalog: null, catalogStatus: 'loading' })} />);
    expect(screen.getByLabelText('Loading packs')).toHaveAttribute('aria-busy', 'true');
  });
});
