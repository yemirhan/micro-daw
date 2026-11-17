import { describe, expect, it, vi } from 'vitest';
import { advanceClock, getSoundingNotes, toneMock } from '@/test/toneMock';
import { SYNTH_PRESETS } from '@/utils/constants';
import type { SubtractivePreset } from '@/types/audio';

async function load() {
  vi.resetModules();
  const { audioEngine } = await import('@/services/AudioEngine');
  await audioEngine.start();
  return audioEngine;
}

describe('AudioEngine (live play contract)', () => {
  it('noteOn / noteOff / getActiveVoiceCount', async () => {
    const engine = await load();
    engine.setPreset(1);
    engine.noteOn(60, 0.8);
    engine.noteOn(64, 0.8);
    expect(engine.getHeldNotes()).toEqual([60, 64]);
    expect(engine.getActiveVoiceCount()).toBe(2);
    engine.noteOff(60);
    engine.noteOff(64);
    expect(engine.getHeldNotes()).toEqual([]);
    advanceClock(5);
    expect(engine.getActiveVoiceCount()).toBe(0);
    expect(getSoundingNotes()).toEqual([]);
  });

  it('re-triggering a sounding note does not leak a voice', async () => {
    const engine = await load();
    engine.setPreset(3);
    engine.noteOn(60, 0.8);
    engine.noteOn(60, 0.8);
    engine.noteOn(60, 0.8);
    expect(engine.getActiveVoiceCount()).toBe(1);
    engine.noteOff(60);
    advanceClock(10);
    expect(engine.getActiveVoiceCount()).toBe(0);
  });

  it('releaseAll silences voices whose noteOff was lost', async () => {
    const engine = await load();
    for (const idx of [0, 1, 7, 13, 20, 28]) {
      engine.setPreset(idx);
      engine.noteOn(48, 0.9);
      engine.noteOn(55, 0.9);
      engine.releaseAll();
      expect(engine.getHeldNotes()).toEqual([]);
      advanceClock(0.1);
      expect(engine.getActiveVoiceCount()).toBe(0);
    }
    expect(getSoundingNotes()).toEqual([]);
  });

  it('noteOff is honoured while muted (keyboard sound source)', async () => {
    const engine = await load();
    engine.setPreset(1);
    engine.noteOn(60, 0.8);
    engine.mute();
    engine.noteOff(60);
    engine.unmute();
    expect(engine.getHeldNotes()).toEqual([]);
  });

  it('switching presets with held keys leaves nothing held', async () => {
    const engine = await load();
    engine.setPreset(6);
    engine.noteOn(60, 0.8);
    engine.setPreset(9);
    engine.noteOff(60); // reaches the new instrument: harmless no-op
    expect(engine.getHeldNotes()).toEqual([]);
    expect(engine.getPresetIndex()).toBe(9);
  });

  it('every preset can be selected and played', async () => {
    const engine = await load();
    SYNTH_PRESETS.forEach((_, i) => {
      engine.setPreset(i);
      engine.noteOn(60, 0.9);
      engine.noteOff(60);
      expect(engine.getHeldNotes()).toEqual([]);
    });
    advanceClock(10);
    expect(engine.getActiveVoiceCount()).toBe(0);
  });

  it('rejects invalid preset indices', async () => {
    const engine = await load();
    engine.setPreset(2);
    engine.setPreset(-1);
    engine.setPreset(999);
    engine.setPreset(1.5);
    expect(engine.getPresetIndex()).toBe(2);
  });

  it('setSynthParam merges dotted paths and nested objects into the current preset', async () => {
    const engine = await load();
    engine.setPreset(1);
    engine.setSynthParam('filter.cutoff', 1234);
    engine.setSynthParam('amp', { release: 1.1 });
    const p = engine.getCurrentPreset() as SubtractivePreset;
    expect(p.filter.cutoff).toBe(1234);
    expect(p.amp.release).toBe(1.1);
    expect(engine.getSynthOverrides()).toEqual({ filter: { cutoff: 1234 }, amp: { release: 1.1 } });
    engine.setPreset(1); // reset
    expect(engine.getSynthOverrides()).toEqual({});
  });

  it('keeps the global effect setters working', async () => {
    const engine = await load();
    expect(() => {
      engine.setReverbWet(0.4);
      engine.setChorusDepth(0.5);
      engine.setFilterCutoff(1000);
      engine.setFilterResonance(4);
      engine.setFilterType('highpass');
      engine.setFilterLfoDepth(0.5);
      engine.setFilterLfoRate(3);
      engine.setFilterLfoWave('square');
      engine.setFilterLfoDepth(0);
      engine.setDelayTime(0.3);
      engine.setDelayFeedback(0.4);
      engine.setDelayWet(0.2);
      engine.setDistortionAmount(0.3);
      engine.setDistortionWet(0.5);
      engine.setEqLow(3);
      engine.setEqMid(-2);
      engine.setEqHigh(1);
      engine.setCompThreshold(-20);
      engine.setCompRatio(3);
      engine.setCompAttack(0.01);
      engine.setCompRelease(0.2);
      engine.setVolume(-6);
    }).not.toThrow();
    const fx = engine.getEffectParams();
    expect(fx.reverbWet).toBe(0.4);
    expect(fx.compRatio).toBe(3);
    expect(fx.filterType).toBe('highpass');
  });

  it('schedules live notes at the audio clock (no transport look-ahead) and installs the master bus', async () => {
    const engine = await load();
    const { PolyInstrument } = await import('@/audio/PolyInstrument');
    const spy = vi.spyOn(PolyInstrument.prototype, 'noteOn');
    engine.setPreset(1);
    advanceClock(2);
    engine.noteOn(60, 0.8);
    const time = spy.mock.calls[0][2] as number;
    expect(time).toBeGreaterThanOrEqual(2);
    expect(time).toBeLessThan(2.01);
    expect(toneMock.destination.chain).toHaveBeenCalled();
    spy.mockRestore();
  });
});
