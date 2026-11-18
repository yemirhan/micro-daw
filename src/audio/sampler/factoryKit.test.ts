import { describe, expect, it } from 'vitest';
import { PAD_COUNT } from '@/types/sampler';
import { DrumKit, FACTORY_VOICE_RENDER, playFactoryVoice } from '@/audio/drumKit';
import {
  FACTORY_VOICE_GM_NOTE,
  FACTORY_VOICE_IDS,
  FACTORY_VOICE_LABELS,
  PAD_COLORS,
  cloneKit,
  createFactoryKit,
  defaultPadSettings,
  factoryPadForGmNote,
  normalizeKit,
} from './factoryKit';

describe('factory kit', () => {
  it('has 16 pads, each with a distinct factory voice', () => {
    const kit = createFactoryKit();
    expect(kit.pads).toHaveLength(PAD_COUNT);
    const voices = kit.pads.map((p) => (p.sample?.source.kind === 'factory' ? p.sample.source.voice : null));
    expect(new Set(voices).size).toBe(PAD_COUNT);
    expect(voices.slice(0, 4)).toEqual(['kick', 'snare', 'chh', 'ohh']);
    expect(kit.pads[0].sample?.name).toBe('Kick');
  });

  it('every factory voice has a label, a render spec and a GM note', () => {
    expect(FACTORY_VOICE_IDS).toHaveLength(16);
    for (const id of FACTORY_VOICE_IDS) {
      expect(FACTORY_VOICE_LABELS[id]).toBeTruthy();
      expect(FACTORY_VOICE_RENDER[id].seconds).toBeGreaterThan(0);
      expect(FACTORY_VOICE_RENDER[id].seconds).toBeLessThanOrEqual(3);
      expect(FACTORY_VOICE_GM_NOTE[id]).toBeGreaterThan(0);
    }
    expect(PAD_COLORS).toHaveLength(PAD_COUNT);
  });

  it('the synthesized kit can play every factory voice', () => {
    const kit = new DrumKit();
    FACTORY_VOICE_IDS.forEach((id, i) => {
      expect(() => playFactoryVoice(kit, id, 1, i * 0.1)).not.toThrow();
    });
    kit.dispose();
  });

  it('hi-hats share a choke group', () => {
    const kit = createFactoryKit();
    expect(kit.pads[2].chokeGroup).toBeGreaterThan(0);
    expect(kit.pads[3].chokeGroup).toBe(kit.pads[2].chokeGroup);
    expect(kit.pads[0].chokeGroup).toBe(0);
  });

  it('returns fresh objects every call', () => {
    const a = createFactoryKit();
    a.pads[0].gainDb = -12;
    expect(createFactoryKit().pads[0].gainDb).toBe(0);
  });

  it('maps GM drum notes to factory pads', () => {
    const kit = createFactoryKit();
    const voiceAt = (note: number) => {
      const pad = factoryPadForGmNote(note);
      const s = pad === null ? null : kit.pads[pad].sample?.source;
      return s?.kind === 'factory' ? s.voice : null;
    };
    expect(voiceAt(36)).toBe('kick');
    expect(voiceAt(38)).toBe('snare');
    expect(voiceAt(42)).toBe('chh');
    expect(voiceAt(46)).toBe('ohh');
    expect(voiceAt(41)).toBe('tom-lo');
    expect(voiceAt(50)).toBe('tom-hi');
    expect(factoryPadForGmNote(60)).toBeNull();
    // every voice's GM note maps back to the same voice
    for (const id of FACTORY_VOICE_IDS) {
      if (id === 'kick-sub') continue; // GM 35 is an acoustic kick: maps to 'kick'
      expect(voiceAt(FACTORY_VOICE_GM_NOTE[id])).toBe(id);
    }
  });
});

describe('normalizeKit / cloneKit', () => {
  it('fills missing pads and clamps values', () => {
    const kit = normalizeKit({
      id: 'k',
      name: 'Mine',
      pads: [
        { gainDb: 99, pan: -5, pitch: 40, decay: 0, filterCutoff: 1e6, mode: 'gate', chokeGroup: 2.4,
          sample: { name: 'x', source: { kind: 'file', path: '/a/b.wav' } } },
        { sample: { source: { kind: 'factory', voice: 'nope' } } },
      ],
    });
    expect(kit.pads).toHaveLength(PAD_COUNT);
    const p = kit.pads[0];
    expect([p.gainDb, p.pan, p.pitch, p.decay, p.filterCutoff, p.mode, p.chokeGroup]).toEqual([12, -1, 24, 0.005, 20000, 'gate', 2]);
    expect(p.sample?.source).toEqual({ kind: 'file', path: '/a/b.wav' });
    expect(kit.pads[1].sample).toBeNull();
    expect(kit.pads[5]).toEqual(defaultPadSettings());
  });

  it('drops an endSec that is not after startSec', () => {
    expect(normalizeKit({ pads: [{ startSec: 1, endSec: 0.5 }] }).pads[0].endSec).toBeNull();
  });

  it('clones deeply', () => {
    const a = createFactoryKit();
    const b = cloneKit(a);
    b.pads[0].sample!.name = 'changed';
    expect(a.pads[0].sample!.name).toBe('Kick');
  });
});
