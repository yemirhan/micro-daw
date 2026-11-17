/**
 * N5: Song audio regions warn when their sample file is gone and redraw their
 * waveform when the sample's audio is reloaded in place.
 */
import * as Tone from 'tone';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, render } from '@testing-library/react';
import type { ElectronAPI } from '@/types/electron';
import type { Region } from '@/types/arrangement';
import { createFakeAudioBuffer } from '@/test/toneMock';
import { sampleManager } from '@/services/SampleManager';
import { RegionBlock, SAMPLE_PROBLEM_TITLE } from './RegionBlock';

const FILE = '/data/packs/uzu-drumkit/kick/10_bd.wav';
let read: ReturnType<typeof vi.fn>;
let getContext: ReturnType<typeof vi.fn>;

beforeEach(() => {
  read = vi.fn(async (_p: string) => new ArrayBuffer(8));
  window.electronAPI = { sampleReadFile: read } as unknown as ElectronAPI;
  const raw = Tone.getContext().rawContext as unknown as { decodeAudioData: ReturnType<typeof vi.fn> };
  raw.decodeAudioData.mockImplementation(async () => createFakeAudioBuffer(1, 44100, 44100));
  // A 2D context stub: counts redraws (clearRect runs once per draw).
  const ctx = { scale: vi.fn(), clearRect: vi.fn(), beginPath: vi.fn(), moveTo: vi.fn(), lineTo: vi.fn(), closePath: vi.fn(), fill: vi.fn(), fillStyle: '', globalAlpha: 1 };
  getContext = vi.fn(() => ctx);
  HTMLCanvasElement.prototype.getContext = getContext as unknown as typeof HTMLCanvasElement.prototype.getContext;
});

afterEach(() => {
  sampleManager.dispose();
  delete window.electronAPI;
  vi.restoreAllMocks();
});

function audioRegion(sampleId: string): Region {
  return {
    id: 'r1',
    trackId: 't1',
    startBeat: 0,
    lengthBeats: 4,
    notes: [],
    name: 'Loop',
    audio: { sampleId, samplePath: FILE, offsetSeconds: 0, gainDb: 0, originalDuration: 1 },
  } as Region;
}

function renderRegion(region: Region) {
  return render(
    <RegionBlock region={region} trackColor="#f00" pxPerBeat={40} snapValue={1} tool="pointer" bpm={120} onMove={vi.fn()} onContextMenu={vi.fn()} />,
  );
}

const block = () => document.querySelector<HTMLElement>('[data-region-id="r1"]')!;

describe('RegionBlock sample status (N5)', () => {
  it('shows a warning badge once its pack is removed, and redraws on in-place reloads', async () => {
    const ref = await sampleManager.loadFromPath(FILE);
    renderRegion(audioRegion(ref.id));
    expect(block()).not.toHaveAttribute('data-sample-state');
    const draws = getContext.mock.calls.length;

    // Pack updated: same id, new audio -> the waveform redraws.
    await act(async () => {
      await sampleManager.reloadByPathPrefix('/data/packs/uzu-drumkit/');
    });
    expect(getContext.mock.calls.length).toBeGreaterThan(draws);
    expect(block()).not.toHaveAttribute('data-sample-state');

    // Pack removed: the file can't be re-read -> badge (audio kept).
    read.mockRejectedValue(new Error('ENOENT'));
    await act(async () => {
      await sampleManager.reloadByPathPrefix('/data/packs/uzu-drumkit/');
    });
    expect(block()).toHaveAttribute('data-sample-state', 'unavailable');
    const badge = block().querySelector('[data-sample-badge="unavailable"]');
    expect(badge).toHaveAttribute('title', SAMPLE_PROBLEM_TITLE.unavailable);
    expect(badge).toHaveAccessibleName('Sample file unavailable');

    // Reinstalled: the badge goes away.
    read.mockResolvedValue(new ArrayBuffer(8));
    await act(async () => {
      await sampleManager.reloadByPathPrefix('/data/packs/uzu-drumkit/');
    });
    expect(block()).not.toHaveAttribute('data-sample-state');
    expect(block().querySelector('[data-sample-badge]')).toBeNull();
  });

  it('a region whose sample never loaded (file gone at project open) says it is silent', () => {
    renderRegion(audioRegion('not-loaded'));
    expect(block()).toHaveAttribute('data-sample-state', 'missing');
    expect(block().querySelector('[data-sample-badge="missing"]')).toHaveAttribute('title', SAMPLE_PROBLEM_TITLE.missing);
  });

  it('MIDI regions never show a sample badge', () => {
    renderRegion({ ...audioRegion('x'), audio: undefined });
    expect(block()).not.toHaveAttribute('data-sample-state');
  });
});
