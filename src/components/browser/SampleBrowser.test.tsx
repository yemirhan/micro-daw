import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { ElectronAPI, FsDirEntry } from '@/types/electron';

const preview = vi.fn<(src: unknown) => Promise<void>>(() => Promise.resolve());
const stopPreview = vi.fn();
vi.mock('@/services/DrumEngine', () => ({ drumEngine: { preview: (s: unknown) => preview(s), stopPreview: () => stopPreview() } }));

import { SampleBrowser } from './SampleBrowser';
import { SAMPLE_DRAG_MIME } from './browserHelpers';

const tree: Record<string, FsDirEntry[]> = {
  '/s': [
    { name: 'Kicks', path: '/s/Kicks', isDir: true, ext: '' },
    { name: 'Hat.wav', path: '/s/Hat.wav', isDir: false, ext: 'wav' },
    { name: 'Snare.wav', path: '/s/Snare.wav', isDir: false, ext: 'wav' },
  ],
  '/s/Kicks': [{ name: 'Kick 1.wav', path: '/s/Kicks/Kick 1.wav', isDir: false, ext: 'wav' }],
};

function installApi() {
  const api: Partial<ElectronAPI> = {
    fsListDir: vi.fn(async (p: string) => {
      const entries = tree[p];
      if (!entries) throw new Error('ENOENT');
      return entries;
    }),
    fsPickFolder: vi.fn(async () => '/s'),
    fsHomeDir: vi.fn(async () => '/home'),
    getPathForFile: vi.fn(() => ''),
  };
  window.electronAPI = api as ElectronAPI;
  return api;
}

beforeEach(() => {
  preview.mockClear();
  stopPreview.mockClear();
});

afterEach(() => {
  delete window.electronAPI;
});

describe('SampleBrowser', () => {
  it('shows an unavailable state outside Electron', () => {
    render(<SampleBrowser onAssign={vi.fn()} />);
    expect(screen.getByText('Browser unavailable')).toBeInTheDocument();
  });

  it('adds a root via the picker, previews on click and assigns on Enter', async () => {
    installApi();
    const onAssign = vi.fn();
    render(<SampleBrowser onAssign={onAssign} />);

    fireEvent.click(screen.getByRole('button', { name: /choose folder/i }));
    await screen.findByText('Hat');
    expect(JSON.parse(localStorage.getItem('micro-daw:browser-roots') ?? '[]')).toEqual(['/s']);

    fireEvent.click(screen.getByText('Hat'));
    expect(preview).toHaveBeenCalledWith({ kind: 'file', path: '/s/Hat.wav' });

    const list = screen.getByRole('listbox');
    fireEvent.keyDown(list, { key: 'ArrowDown' });
    expect(preview).toHaveBeenLastCalledWith({ kind: 'file', path: '/s/Snare.wav' });
    fireEvent.keyDown(list, { key: 'Enter' });
    expect(onAssign).toHaveBeenCalledWith('/s/Snare.wav', 'Snare');
  });

  it('enters and leaves folders with the arrow keys', async () => {
    installApi();
    localStorage.setItem('micro-daw:browser-roots', JSON.stringify(['/s']));
    localStorage.setItem('micro-daw:browser-last-path', '/s');
    render(<SampleBrowser onAssign={vi.fn()} />);
    await screen.findByText('Kicks');

    const list = screen.getByRole('listbox');
    // First row (Kicks) is selected after load.
    fireEvent.keyDown(list, { key: 'ArrowRight' });
    await screen.findByText('Kick 1');
    expect(screen.getByRole('navigation', { name: 'Folder path' })).toHaveTextContent('Kicks');

    fireEvent.keyDown(list, { key: 'ArrowLeft' });
    await screen.findByText('Hat');
    expect(screen.getByRole('option', { selected: true })).toHaveTextContent('Kicks');

    fireEvent.keyDown(list, { key: 'ArrowLeft' });
    await waitFor(() => expect(screen.getByRole('option', { selected: true })).toHaveTextContent('s'));
  });

  it('filters the current folder and drags files with the contract MIME type', async () => {
    installApi();
    localStorage.setItem('micro-daw:browser-roots', JSON.stringify(['/s']));
    localStorage.setItem('micro-daw:browser-last-path', '/s');
    render(<SampleBrowser onAssign={vi.fn()} />);
    await screen.findByText('Snare');

    fireEvent.change(screen.getByLabelText('Filter'), { target: { value: 'sna' } });
    expect(screen.queryByText('Hat')).not.toBeInTheDocument();

    const setData = vi.fn();
    fireEvent.dragStart(screen.getByText('Snare'), { dataTransfer: { setData, effectAllowed: '' } });
    expect(setData).toHaveBeenCalledWith(SAMPLE_DRAG_MIME, JSON.stringify({ path: '/s/Snare.wav', name: 'Snare' }));
  });

  it('type-to-filter: a letter on the list goes into the filter and never reaches global shortcuts', async () => {
    installApi();
    localStorage.setItem('micro-daw:browser-roots', JSON.stringify(['/s']));
    localStorage.setItem('micro-daw:browser-last-path', '/s');
    const windowKeys = vi.fn();
    window.addEventListener('keydown', windowKeys);
    render(<SampleBrowser onAssign={vi.fn()} />);
    await screen.findByText('Snare');
    expect(screen.getByRole('region', { name: 'Sample browser' })).toHaveAttribute('data-keys-scope', 'browser');

    const list = screen.getByRole('listbox');
    list.focus();
    fireEvent.keyDown(list, { key: 's' });
    const filter = screen.getByLabelText('Filter') as HTMLInputElement;
    expect(filter.value).toBe('s');
    expect(document.activeElement).toBe(filter);
    fireEvent.keyDown(filter, { key: 'n' }); // typed in the field (native input does the text)
    fireEvent.keyDown(list, { key: 'b' });
    expect(windowKeys).not.toHaveBeenCalled();
    // Command chords still reach the app (undo, palette…).
    fireEvent.keyDown(list, { key: 'z', metaKey: true });
    expect(windowKeys).toHaveBeenCalledTimes(1);
    window.removeEventListener('keydown', windowKeys);
  });
});
