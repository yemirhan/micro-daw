import { describe, expect, it } from 'vitest';
import { PAD_COUNT, padToNote, type KitState } from '@/types/sampler';
import { normalizeKit } from '@/audio/sampler/factoryKit';
import { clearPadBufferCache, getCachedPadBuffer, loadPadBuffer, loadPreviewBuffer, padSourceKey } from '@/audio/sampler/bufferCache';
import { padName } from '@/components/beat/beatHelpers';
import { drumNoteToGm } from '@/utils/arrangementHelpers';
import {
  CLASSIC_MACHINES,
  CLASSIC_SAMPLE_RATE,
  analyzeVoice,
  classicKitId,
  classicSample,
  createClassicKit,
  getClassicVoice,
  hasClassicRecipe,
  isClassicVoice,
  renderClassicVoice,
  synthesizeClassicVoice,
} from './index';
import { CLASSIC_RECIPES } from './voices';
import { SR, TARGET_PEAK, env, finalize, lofi, lp, mul, osc, peakOf, samples } from './dsp';

const ALL_VOICES = CLASSIC_MACHINES.flatMap((m) => m.voices.map((v) => ({ machine: m.id, voice: v.id })));

describe('classic machine metadata', () => {
  it('has six "-style" machines with ≥ 12 unique voices, each backed by a recipe', () => {
    expect(CLASSIC_MACHINES.map((m) => m.id)).toEqual(['tr808', 'tr909', 'tr707', 'tr606', 'linn', 'cr78']);
    for (const m of CLASSIC_MACHINES) {
      expect(m.name).toMatch(/-style$/);
      expect(m.voices.length).toBeGreaterThanOrEqual(12);
      expect(new Set(m.voices.map((v) => v.id)).size).toBe(m.voices.length);
      for (const v of m.voices) {
        expect(hasClassicRecipe(m.id, v.id)).toBe(true);
        expect(v.label).toBeTruthy();
        expect(v.gm).toBeGreaterThanOrEqual(35);
        expect(v.gm).toBeLessThanOrEqual(87);
      }
      // No orphan recipes either.
      expect(Object.keys(CLASSIC_RECIPES[m.id]).sort()).toEqual(m.voices.map((v) => v.id).sort());
    }
  });

  it('never claims a trademark in names or taglines', () => {
    for (const m of CLASSIC_MACHINES) {
      expect(`${m.name} ${m.tagline} ${m.description}`).not.toMatch(/roland|linn electronics|sequential|oberheim/i);
    }
  });
});

describe('createClassicKit', () => {
  it.each(CLASSIC_MACHINES.map((m) => m.id))('%s: 16 distinct classic pads, kick+snare first, hats choke', (id) => {
    const kit = createClassicKit(id);
    expect(kit.id).toBe(classicKitId(id));
    expect(kit.name).toMatch(/-style$/);
    expect(kit.pads).toHaveLength(PAD_COUNT);
    const voices = kit.pads.map((p) => (p.sample?.source.kind === 'classic' ? p.sample.source.voice : null));
    expect(voices.every((v) => v !== null && isClassicVoice(id, v))).toBe(true);
    expect(new Set(voices).size).toBe(PAD_COUNT);
    expect(voices[0]).toBe('kick');
    expect(voices[1]).toBe('snare');
    expect(voices[2]).toBe('chh');
    expect(voices[3]).toBe('ohh');
    expect(kit.pads[2].chokeGroup).toBe(1);
    expect(kit.pads[3].chokeGroup).toBe(1);
    expect(kit.pads[0].chokeGroup).toBe(0);
    // Pads carry the bare voice label (the kit name already names the machine).
    expect(kit.pads[0].sample?.name).toBe(getClassicVoice(id, 'kick')?.label);
    expect(kit.pads[0].sample?.name).toMatch(/^Kick/);
    // Valid persisted state: normalizeKit keeps it unchanged.
    expect(normalizeKit(JSON.parse(JSON.stringify(kit)))).toEqual(kit);
  });

  it('names samples after the machine', () => {
    expect(classicSample('tr808', 'kick')).toEqual({ source: { kind: 'classic', machine: 'tr808', voice: 'kick' }, name: '808 Kick' });
    expect(classicSample('linn', 'sidestick').name).toBe('LinnDrum Sidestick');
  });
});

describe('normalizeKit with classic sources', () => {
  it('keeps valid classic sources and fills a missing name', () => {
    const kit = normalizeKit({ id: 'k', name: 'K', pads: [{ sample: { source: { kind: 'classic', machine: 'tr909', voice: 'clap' } } }] });
    expect(kit.pads[0].sample).toEqual({ source: { kind: 'classic', machine: 'tr909', voice: 'clap' }, name: '909 Clap' });
  });

  it('drops unknown machines / voices', () => {
    const kit = normalizeKit({
      pads: [
        { sample: { source: { kind: 'classic', machine: 'tr999', voice: 'kick' }, name: 'x' } },
        { sample: { source: { kind: 'classic', machine: 'tr808', voice: 'nope' }, name: 'x' } },
        { sample: { source: { kind: 'classic', machine: 'tr808', voice: 42 }, name: 'x' } },
      ],
    });
    expect(kit.pads.slice(0, 3).map((p) => p.sample)).toEqual([null, null, null]);
  });
});

describe('helpers outside the sampler', () => {
  it('padName and GM export understand classic voices', () => {
    const kit: KitState = createClassicKit('tr808');
    expect(padName({ sample: { ...kit.pads[0].sample!, name: '' } })).toBe('808 Kick');
    expect(drumNoteToGm(padToNote(0), kit)).toBe(36);
    expect(drumNoteToGm(padToNote(2), kit)).toBe(42); // closed hat
    expect(drumNoteToGm(padToNote(7), kit)).toBe(56); // cowbell
  });
});

describe('synthesis', () => {
  it('every voice renders finite mono audio at -1 dBFS within its length cap', () => {
    for (const { machine, voice } of ALL_VOICES) {
      const data = synthesizeClassicVoice(machine, voice);
      const label = `${machine}/${voice}`;
      expect(data.every(Number.isFinite), label).toBe(true);
      expect(peakOf(data), label).toBeCloseTo(TARGET_PEAK, 4);
      expect(data.length, label).toBeLessThanOrEqual(samples(CLASSIC_RECIPES[machine][voice].maxSec));
      expect(data.length / SR, label).toBeGreaterThan(0.02);
      expect(Math.abs(data[data.length - 1]), label).toBeLessThan(1e-3);
      // Starts without a hard click: the first sample is near zero.
      expect(Math.abs(data[0]), label).toBeLessThan(0.2);
    }
  });

  it('is deterministic', () => {
    expect(synthesizeClassicVoice('tr909', 'snare')).toEqual(synthesizeClassicVoice('tr909', 'snare'));
  });

  it('throws on unknown voices', () => {
    expect(() => synthesizeClassicVoice('tr808', 'nope')).toThrow(/Unknown classic voice/);
  });

  it('kicks live in the lows, hats/shakers in the highs', () => {
    for (const m of CLASSIC_MACHINES) {
      for (const v of m.voices) {
        const a = analyzeVoice(synthesizeClassicVoice(m.id, v.id), SR);
        const label = `${m.id}/${v.id}`;
        if (v.id.startsWith('kick')) {
          expect(a.lowFrac, label).toBeGreaterThan(0.8);
          expect(a.peakHz, label).toBeLessThan(90);
        }
        if (['chh', 'phh', 'ohh', 'maracas', 'shaker', 'cabasa'].includes(v.id)) {
          expect(a.highFrac, label).toBeGreaterThan(0.8);
        }
        if (['cymbal', 'crash'].includes(v.id)) expect(a.highFrac, label).toBeGreaterThan(0.6);
        if (v.id.startsWith('snare')) {
          expect(a.lowFrac, label).toBeLessThan(0.1);
          expect(a.centroidHz, label).toBeGreaterThan(600);
        }
      }
    }
  });

  it('machines sound different: kicks differ in length or pitch across machines', () => {
    const kicks = CLASSIC_MACHINES.map((m) => analyzeVoice(synthesizeClassicVoice(m.id, 'kick'), SR));
    for (let i = 0; i < kicks.length; i++) {
      for (let j = i + 1; j < kicks.length; j++) {
        const dLen = Math.abs(kicks[i].lengthSec - kicks[j].lengthSec);
        const dCentroid = Math.abs(kicks[i].centroidHz - kicks[j].centroidHz);
        expect(dLen > 0.02 || dCentroid > 4, `${i} vs ${j}`).toBe(true);
      }
    }
  });
});

describe('renderClassicVoice / buffer cache', () => {
  it('renders a mono 44.1 kHz AudioBuffer', async () => {
    const buf = await renderClassicVoice('tr606', 'chh');
    expect(buf.numberOfChannels).toBe(1);
    expect(buf.sampleRate).toBe(CLASSIC_SAMPLE_RATE);
    expect(peakOf(buf.getChannelData(0))).toBeCloseTo(TARGET_PEAK, 4);
  });

  it('caches classic pad buffers under a machine+voice key', async () => {
    clearPadBufferCache();
    const source = { kind: 'classic', machine: 'cr78', voice: 'guiro' } as const;
    expect(padSourceKey(source)).toBe('classic:cr78:guiro');
    expect(getCachedPadBuffer(source)).toBeNull();
    const [a, b] = await Promise.all([loadPadBuffer(source), loadPadBuffer({ ...source })]);
    expect(a).toBe(b);
    expect(getCachedPadBuffer(source)).toBe(a);
    expect(await loadPreviewBuffer(source)).toBe(a);
    expect(padSourceKey({ kind: 'classic', machine: 'tr808', voice: 'guiro' })).not.toBe(padSourceKey(source));
  });
});

describe('dsp', () => {
  it('finalize normalizes to -1 dBFS and trims trailing silence', () => {
    const raw = new Float32Array(SR);
    raw.set(mul(osc(samples(0.1), { freq: 440 }), env(samples(0.1), { t60: 0.1 })).map((x) => x * 0.25));
    const out = finalize(raw);
    expect(peakOf(out)).toBeCloseTo(TARGET_PEAK, 3);
    expect(out.length).toBeLessThan(samples(0.13));
    expect(out.length).toBeGreaterThan(samples(0.1));
  });

  it('lowpass attenuates highs, lofi quantizes to the given bit depth', () => {
    const hi = osc(SR / 4, { freq: 12000 });
    expect(peakOf(lp(hi.slice(), 1000).subarray(2000))).toBeLessThan(0.02);
    const ramp = Float32Array.from({ length: 1000 }, (_, i) => i / 999);
    const levels = new Set(lofi(ramp, { bits: 3 }));
    expect(levels.size).toBeLessThanOrEqual(4); // 0..3 of 3 positive steps
  });
});
