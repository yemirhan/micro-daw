import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import type { ElectronAPI, FsDirEntry } from '@/types/electron';
import { manifest } from '@/main/packs/testUtils';

vi.mock('@/services/DrumEngine', () => ({ drumEngine: { preview: vi.fn(() => Promise.resolve()), stopPreview: vi.fn() } }));

import { packService } from '@/services/PackService';
import { SampleBrowser } from './SampleBrowser';

const tree: Record<string, FsDirEntry[]> = {
  '/data/packs': [{ name: 'test-pack', path: '/data/packs/test-pack', isDir: true, ext: '' }],
  '/data/packs/test-pack': [{ name: 'sd.wav', path: '/data/packs/test-pack/sd.wav', isDir: false, ext: 'wav' }],
};

function installApi(installedCount: number) {
  const m = manifest();
  const api: Partial<ElectronAPI> = {
    fsListDir: vi.fn(async (p: string) => tree[p] ?? []),
    fsPickFolder: vi.fn(async () => null),
    fsHomeDir: vi.fn(async () => '/home'),
    getPathForFile: vi.fn(() => ''),
    packsDir: vi.fn(async () => '/data/packs'),
    packsActiveStates: vi.fn(async () => ({})),
    packsListInstalled: vi.fn(async () =>
      installedCount ? [{ id: m.id, version: 1, dir: '/data/packs/test-pack', installedAt: 'x', manifest: m }] : [],
    ),
    onPacksProgress: vi.fn(() => () => undefined),
  };
  window.electronAPI = api as ElectronAPI;
}

afterEach(() => {
  packService.resetForTests();
  delete window.electronAPI;
});

describe('SampleBrowser installed packs root', () => {
  it('shows a pinned, non-removable "Installed packs" root once packs are installed', async () => {
    installApi(1);
    render(<SampleBrowser onAssign={vi.fn()} />);
    const row = await screen.findByText('Installed packs');
    expect(screen.queryByRole('button', { name: /remove installed packs/i })).toBeNull();
    fireEvent.click(row);
    await screen.findByText('test-pack');
    expect(screen.getByRole('navigation', { name: 'Folder path' }).textContent ?? '').toContain('Installed packs');
  });

  it('keeps the add-folder empty state when no packs are installed', async () => {
    installApi(0);
    render(<SampleBrowser onAssign={vi.fn()} />);
    await screen.findByRole('button', { name: /choose folder/i });
    expect(screen.queryByText('Installed packs')).toBeNull();
  });
});
