/**
 * B3 (engine side): when a pack update reloads a sample in place, the Song's
 * scheduled audio-region players switch to the new audio (same sampleId), and
 * the next play / export reads it too. Nothing is dropped.
 */
import * as Tone from 'tone';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ElectronAPI } from '@/types/electron';
import { createFakeAudioBuffer, instancesOf, type MockToneNode } from '@/test/toneMock';

vi.mock('@/services/DrumEngine', async () => (await import('@/test/fakeAudioEngine')).drumEngineModule());

const FILE = '/data/packs/uzu-drumkit/kick/10_bd.wav';
let decode: ReturnType<typeof vi.fn>;

async function load() {
  vi.resetModules();
  const { arrangementEngine } = await import('@/services/ArrangementEngine');
  const { sampleManager } = await import('@/services/SampleManager');
  return { engine: arrangementEngine, sampleManager };
}

beforeEach(() => {
  vi.spyOn(console, 'warn').mockImplementation(() => undefined);
  window.electronAPI = { sampleReadFile: vi.fn(async () => new ArrayBuffer(8)) } as unknown as ElectronAPI;
  const raw = Tone.getContext().rawContext as unknown as { decodeAudioData: ReturnType<typeof vi.fn> };
  decode = raw.decodeAudioData;
  decode.mockImplementation(async () => createFakeAudioBuffer(1, 44100, 44100));
});

afterEach(() => {
  delete window.electronAPI;
});

const livePlayers = () => instancesOf('Player').filter((p: MockToneNode) => !p.disposed);

describe('B3: reloaded samples reach the Song audio players', () => {
  it('a reload while playing swaps the new buffer into the scheduled player (same region, same sampleId)', async () => {
    const { engine, sampleManager } = await load();
    const ref = await sampleManager.loadFromPath(FILE);
    const track = engine.addTrack('audio');
    const region = engine.addAudioRegion(track.id, ref.id, 0)!;
    expect(region).toBeTruthy();

    engine.play();
    const [player] = livePlayers();
    expect(player).toBeTruthy();
    const oldBuffer = sampleManager.getToneBuffer(ref.id);
    expect(player.__args[0]).toBe(oldBuffer);

    const fresh = createFakeAudioBuffer(2, 88200, 44100);
    decode.mockImplementationOnce(async () => fresh);
    const ids = await sampleManager.reloadByPathPrefix('/data/packs/uzu-drumkit');
    expect(ids).toEqual([ref.id]);

    const newBuffer = sampleManager.getToneBuffer(ref.id);
    expect(newBuffer).toBeDefined();
    expect(newBuffer).not.toBe(oldBuffer);
    expect(player.buffer).toBe(newBuffer);
    // Region still references the same sample.
    expect(engine.getTracks().find((t) => t.id === track.id)?.regions[0].audio?.sampleId).toBe(ref.id);
    engine.stop();
  });

  it('a reload while stopped is picked up by the next play', async () => {
    const { engine, sampleManager } = await load();
    const ref = await sampleManager.loadFromPath(FILE);
    const track = engine.addTrack('audio');
    engine.addAudioRegion(track.id, ref.id, 0);

    await sampleManager.reloadByPathPrefix('/data/packs/uzu-drumkit');
    const newBuffer = sampleManager.getToneBuffer(ref.id);
    engine.play();
    const [player] = livePlayers();
    expect(player.__args[0]).toBe(newBuffer);
    engine.stop();
  });

  it('a removed pack keeps the region playing its loaded audio', async () => {
    const { engine, sampleManager } = await load();
    const ref = await sampleManager.loadFromPath(FILE);
    const track = engine.addTrack('audio');
    engine.addAudioRegion(track.id, ref.id, 0);
    const kept = sampleManager.getToneBuffer(ref.id);
    (window.electronAPI!.sampleReadFile as ReturnType<typeof vi.fn>).mockRejectedValueOnce(new Error('ENOENT'));

    await sampleManager.reloadByPathPrefix('/data/packs/uzu-drumkit');
    expect(sampleManager.isUnavailable(ref.id)).toBe(true);
    engine.play();
    const [player] = livePlayers();
    expect(player.__args[0]).toBe(kept);
    engine.stop();
  });
});
