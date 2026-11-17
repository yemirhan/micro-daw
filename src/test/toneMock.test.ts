import { describe, expect, it } from 'vitest';
import * as Tone from 'tone';
import { advanceClock, getSoundingNotes, instancesOf, toneMock } from './toneMock';
import { createSynthFromPreset } from '@/utils/synthFactory';
import { SYNTH_PRESETS } from '@/utils/constants';

describe('tone test double', () => {
  it('is installed globally for tests', () => {
    expect(Tone.getTransport()).toBe(toneMock.transport);
    expect(Tone.now()).toBe(0);
    advanceClock(1.5);
    expect(Tone.now()).toBe(1.5);
  });

  it('creates chainable nodes with signal-like params', () => {
    const filter = new Tone.Filter({ frequency: 1000, type: 'lowpass' }).toDestination();
    filter.frequency.value = 500;
    filter.frequency.rampTo(800, 0.1);
    expect(filter.frequency.value).toBe(800);
    expect(filter).toBeInstanceOf(Tone.ToneAudioNode);
    expect(instancesOf('Filter')).toHaveLength(1);
  });

  it('tracks held notes on synths built by synthFactory', () => {
    // Instruments are built from mock envelopes (held until triggerRelease).
    const player = createSynthFromPreset(SYNTH_PRESETS[1]);
    player.triggerAttack('C4', Tone.now(), 0.8);
    player.triggerAttack('E4', Tone.now(), 0.8);
    expect(player.getHeldNotes()).toEqual([60, 64]);
    expect(getSoundingNotes().length).toBeGreaterThan(0);
    player.triggerRelease('C4');
    expect(player.getHeldNotes()).toEqual([64]);
    player.releaseAll();
    expect(getSoundingNotes()).toHaveLength(0);
  });

  it('stores transport schedules', () => {
    const transport = Tone.getTransport();
    let fired = 0;
    transport.schedule(() => fired++, 0);
    transport.start();
    expect(transport.state).toBe('started');
    toneMock.transport.runScheduled();
    expect(fired).toBe(1);
  });
});
