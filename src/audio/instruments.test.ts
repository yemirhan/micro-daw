import { describe, expect, it, vi } from 'vitest';
import { PolyInstrument } from './PolyInstrument';
import { SampledPiano } from './SampledPiano';
import { SubtractiveVoice } from './voices/SubtractiveVoice';
import { SYNTH_PRESETS, FALLBACK_PIANO } from './presets';
import { mergePresetParams } from './presetParams';
import { DrumKit, GM_DRUM_MAP } from './drumKit';
import { softClip, installMasterBus } from './masterBus';
import { createImpulseResponse } from './RoomReverb';
import { sampleFileToMidi, PIANO_SAMPLE_COUNT } from './pianoSamples';
import { noteToMidi, normVelocity, velocityGain } from './voices/common';
import { createFakeAudioBuffer, getSoundingNotes, instancesOf, toneMock } from '@/test/toneMock';
import type { SamplerPreset, SubtractivePreset } from '@/types/audio';

const byId = <T,>(id: string) => SYNTH_PRESETS.find((p) => p.id === id) as T;

function poly(preset: SubtractivePreset) {
  return new PolyInstrument(preset, (p, mod) => new SubtractiveVoice(p, mod));
}

describe('PolyInstrument (poly)', () => {
  const pad = byId<SubtractivePreset>('warm-pad');

  it('re-striking a held note re-uses its voice and one noteOff silences it', () => {
    const inst = poly(pad);
    inst.noteOn(60, 0.8, 0);
    inst.noteOn(60, 0.8, 0.1);
    inst.noteOn(60, 0.8, 0.2);
    expect(inst.getPoolSize()).toBe(1);
    expect(inst.getHeldNotes()).toEqual([60]);
    inst.noteOff(60, 0.3);
    expect(inst.getHeldNotes()).toEqual([]);
    expect(inst.getActiveVoiceCount(0.3 + pad.amp.release + 0.05)).toBe(0);
    expect(getSoundingNotes()).toEqual([]);
  });

  it('two attacks in the same audio tick do not throw (monotonic start times)', () => {
    const inst = poly(pad);
    expect(() => {
      inst.noteOn(60, 0.8, 0);
      inst.noteOn(60, 0.8, 0);
      inst.noteOff(60, 0);
    }).not.toThrow();
    expect(inst.getHeldNotes()).toEqual([]);
  });

  it('caps voices at the preset polyphony and steals', () => {
    const inst = poly({ ...pad, polyphony: 3 });
    for (let n = 0; n < 6; n++) inst.noteOn(60 + n, 0.8, n * 0.01);
    expect(inst.getPoolSize()).toBe(3);
    expect(inst.getHeldNotes()).toHaveLength(3);
    // noteOffs for stolen notes are harmless; the rest still release.
    for (let n = 0; n < 6; n++) inst.noteOff(60 + n, 1);
    expect(inst.getHeldNotes()).toEqual([]);
  });

  it('normalises 0..127 velocities', () => {
    expect(normVelocity(127)).toBe(1);
    expect(normVelocity(64)).toBeCloseTo(0.504, 2);
    expect(normVelocity(0.5)).toBe(0.5);
    expect(normVelocity(undefined)).toBe(0.8);
    expect(velocityGain(1, 1)).toBe(1);
    expect(velocityGain(0.5, 1)).toBeLessThan(0.4);
    expect(velocityGain(0.2, 0)).toBe(1);
  });

  it('parses note names and MIDI numbers', () => {
    expect(noteToMidi('C4')).toBe(60);
    expect(noteToMidi('C#4')).toBe(61);
    expect(noteToMidi('Db4')).toBe(61);
    expect(noteToMidi('Fs3')).toBe(54);
    expect(noteToMidi('A0')).toBe(21);
    expect(noteToMidi(64)).toBe(64);
    expect(noteToMidi('nope')).toBe(-1);
  });

  it('ignores out-of-range notes', () => {
    const inst = poly(pad);
    inst.noteOn(-1, 1, 0);
    inst.noteOn(128, 1, 0);
    inst.triggerAttackRelease('nope', 1, 0);
    expect(inst.getPoolSize()).toBe(0);
  });

  it('live set() updates voices without creating new ones', () => {
    const inst = poly(pad);
    inst.noteOn(60, 0.8, 0);
    const voicesBefore = instancesOf('AmplitudeEnvelope').length;
    inst.set({ filter: { cutoff: 3000 }, oscA: { unison: 5 }, amp: { release: 2 } });
    expect(instancesOf('AmplitudeEnvelope').length).toBe(voicesBefore);
    expect((inst.getPreset() as SubtractivePreset).filter.cutoff).toBe(3000);
    inst.noteOff(60, 0.1);
    // New release time is honoured by the bookkeeping.
    expect(inst.getActiveVoiceCount(1.5)).toBe(1);
    expect(inst.getActiveVoiceCount(2.2)).toBe(0);
  });

  it('dispose releases and is idempotent', () => {
    const inst = poly(pad);
    inst.noteOn(60, 0.8, 0);
    inst.dispose();
    inst.dispose();
    inst.noteOn(62, 0.8, 0); // no-op after dispose
    expect(getSoundingNotes()).toEqual([]);
  });
});

describe('PolyInstrument (mono / legato)', () => {
  const bass = byId<SubtractivePreset>('reese-bass');

  it('legato: overlapping notes glide instead of re-attacking; releasing returns to the held key', () => {
    const inst = poly(bass);
    const attack = vi.spyOn(SubtractiveVoice.prototype, 'attack');
    const glide = vi.spyOn(SubtractiveVoice.prototype, 'glide');
    const release = vi.spyOn(SubtractiveVoice.prototype, 'release');

    inst.noteOn(36, 0.8, 0);
    inst.noteOn(43, 0.8, 0.1);
    expect(attack).toHaveBeenCalledTimes(1);
    expect(glide).toHaveBeenLastCalledWith(43, 0.1);
    expect(inst.getHeldNotes()).toEqual([36, 43]);

    inst.noteOff(43, 0.2); // back to 36 (still held)
    expect(glide).toHaveBeenLastCalledWith(36, 0.2);
    expect(release).not.toHaveBeenCalled();

    inst.noteOff(36, 0.3);
    expect(release).toHaveBeenCalledTimes(1);
    expect(inst.getHeldNotes()).toEqual([]);
    expect(inst.getPoolSize()).toBe(1);
    expect(inst.getActiveVoiceCount(0.3 + bass.amp.release + 0.05)).toBe(0);
    vi.restoreAllMocks();
  });

  it('releasing a non-sounding key keeps the note playing; releaseAll silences', () => {
    const inst = poly(bass);
    inst.noteOn(36, 0.8, 0);
    inst.noteOn(38, 0.8, 0.1);
    inst.noteOff(36, 0.2);
    expect(inst.getActiveVoiceCount(5)).toBe(1); // 38 still held
    inst.releaseAll(5);
    expect(inst.getHeldNotes()).toEqual([]);
    expect(inst.getActiveVoiceCount(5.1)).toBe(0);
    expect(getSoundingNotes()).toEqual([]);
  });
});

describe('SampledPiano', () => {
  const piano = SYNTH_PRESETS[0] as SamplerPreset;

  it('plays the synthesized fallback until samples load, and routes noteOffs back to it', () => {
    const inst = new SampledPiano(piano, FALLBACK_PIANO);
    expect(inst.isSampled()).toBe(false);
    inst.noteOn(60, 0.8, 0);
    inst.noteOn(64, 0.8, 0);
    expect(inst.getHeldNotes()).toEqual([60, 64]);
    inst.noteOff(60, 0.5);
    inst.noteOff(64, 0.5);
    expect(inst.getHeldNotes()).toEqual([]);
    expect(inst.getActiveVoiceCount(0.5 + FALLBACK_PIANO.amp.release + 0.05)).toBe(0);
    inst.dispose();
  });

  it('with samples: re-strike releases the previous source, noteOff releases, releaseAll clears', () => {
    const piano2 = mergePresetParams(piano, { polyphony: 8 });
    const buffers = new Map([[60, {} as never]]);
    const inst = new SampledPiano(piano2, FALLBACK_PIANO, buffers);
    expect(inst.isSampled()).toBe(true);
    const sampler = instancesOf('Sampler')[0];
    inst.noteOn(60, 0.8, 0);
    inst.noteOn(60, 0.8, 0.1);
    expect(sampler.triggerRelease).toHaveBeenCalledWith('C4', 0.1);
    inst.noteOff(60, 0.2);
    expect(inst.getHeldNotes()).toEqual([]);
    inst.noteOn(62, 0.8, 0.3);
    inst.releaseAll(0.4);
    expect(sampler.releaseAll).toHaveBeenCalled();
    expect(inst.getHeldNotes()).toEqual([]);
    expect(inst.getActiveVoiceCount(5)).toBe(0);
  });
});

describe('DrumKit', () => {
  it('maps the 8 pads and GM drum notes', () => {
    const kit = new DrumKit();
    for (let pad = 0; pad < 8; pad++) {
      expect(() => kit.trigger(pad as 0, 0.9, pad * 0.1)).not.toThrow();
    }
    for (const note of [36, 38, 42, 46, 39, 45, 49, 51, 35, 37, 40, 41, 43, 44, 47, 48, 50, 57, 59]) {
      expect(kit.triggerMidi(note, 100, 1)).toBe(true); // 0..127 velocity accepted
    }
    expect(kit.triggerMidi(60, 0.8, 1)).toBe(false);
    expect(GM_DRUM_MAP[38]).toBe('snare');
    expect(GM_DRUM_MAP[42]).toBe('chh');
    // Two hits on the same pad in the same tick are fine.
    expect(() => {
      kit.trigger(0, 1, 2);
      kit.trigger(0, 1, 2);
    }).not.toThrow();
    kit.dispose();
  });

  it('starts its continuous sources lazily (no CPU for unused kits)', () => {
    const kit = new DrumKit();
    const noise = instancesOf('Noise')[0];
    expect(noise.start).not.toHaveBeenCalled();
    kit.trigger(2, 0.8, 0);
    expect(noise.start).toHaveBeenCalledTimes(1);
    kit.trigger(2, 0.8, 0.1);
    expect(noise.start).toHaveBeenCalledTimes(1);
  });
});

describe('master bus', () => {
  it('soft clipper is transparent below the knee and never exceeds the ceiling', () => {
    for (const x of [0, 0.1, -0.5, 0.8, -0.85]) expect(softClip(x)).toBeCloseTo(x, 6);
    for (let x = -4; x <= 4; x += 0.01) expect(Math.abs(softClip(x))).toBeLessThanOrEqual(0.98);
    // monotonic
    let prev = -Infinity;
    for (let x = -2; x <= 2; x += 0.01) {
      const y = softClip(x);
      expect(y).toBeGreaterThanOrEqual(prev);
      prev = y;
    }
  });

  it('installs once per destination', () => {
    const dest = toneMock.destination;
    const a = installMasterBus(dest as never);
    const b = installMasterBus(dest as never);
    expect(a).toBe(b);
    expect(dest.chain).toHaveBeenCalledTimes(1);
    a.dispose();
  });
});

describe('reverb impulse response', () => {
  it('is stereo, pre-delayed, decaying and deterministic', () => {
    const ctx = {
      sampleRate: 8000,
      createBuffer: (ch: number, len: number, sr: number) => createFakeAudioBuffer(ch, len, sr),
    } as unknown as BaseAudioContext;
    const ir = createImpulseResponse(ctx, { decay: 1, preDelay: 0.02 });
    expect(ir.numberOfChannels).toBe(2);
    const l = ir.getChannelData(0);
    expect(Array.from(l.slice(0, 150)).every((v) => v === 0)).toBe(true); // 20 ms pre-delay @ 8 kHz
    const rms = (a: Float32Array) => Math.sqrt(a.reduce((s, v) => s + v * v, 0) / a.length);
    expect(rms(l.slice(160, 1000))).toBeGreaterThan(rms(l.slice(6000, 8000)) * 10);
    const again = createImpulseResponse(ctx, { decay: 1, preDelay: 0.02 });
    expect(Array.from(again.getChannelData(1).slice(200, 220))).toEqual(Array.from(ir.getChannelData(1).slice(200, 220)));
  });
});

describe('piano samples', () => {
  it('bundles one sample per minor third A0..C8', () => {
    expect(PIANO_SAMPLE_COUNT).toBe(30);
    expect(sampleFileToMidi('./samples/piano/A0.mp3')).toBe(21);
    expect(sampleFileToMidi('./samples/piano/C4.mp3')).toBe(60);
    expect(sampleFileToMidi('./samples/piano/Ds4.mp3')).toBe(63);
    expect(sampleFileToMidi('./samples/piano/Fs4.mp3')).toBe(66);
    expect(sampleFileToMidi('./samples/piano/C8.mp3')).toBe(108);
    expect(sampleFileToMidi('./samples/piano/readme.mp3')).toBe(-1);
  });
});

describe('SubtractiveVoice tuning (Electron bug fixed in both apps, 2026-11-18)', () => {
  it('applies oscA / oscB semi + fine at build, for every voice built later too', () => {
    const lead = byId<SubtractivePreset>('saw-lead');
    const pad = byId<SubtractivePreset>('warm-pad');
    for (const preset of [pad, lead]) {
      const before = instancesOf('OmniOscillator').length;
      new SubtractiveVoice(preset, null);
      const [a, b] = instancesOf('OmniOscillator').slice(before);
      expect(a.detune.value).toBe((preset.oscA.semi ?? 0) * 100 + (preset.oscA.fine ?? 0));
      expect(b.detune.value).toBe((preset.oscB.semi ?? 0) * 100 + (preset.oscB.fine ?? 0));
    }
    expect(pad.oscB.semi).toBe(-12);
    // a live edit and a voice allocated after it agree
    const inst = poly(lead);
    inst.set({ oscB: { ...lead.oscB, semi: 5, fine: -3 } });
    const before = instancesOf('OmniOscillator').length;
    inst.noteOn(60, 0.8, 0);
    const [, b] = instancesOf('OmniOscillator').slice(before);
    expect(b.detune.value).toBe(497);
  });
});
