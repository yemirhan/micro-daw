import { describe, expect, it } from 'vitest';
import { SYNTH_PRESETS, FALLBACK_PIANO, LEGACY_PRESET_COUNT, getPresetIndexById, PRESET_CATEGORIES, PRESET_VIBES } from './presets';
import { createSynthFromPreset, resolvePreset, presetAt } from '@/utils/synthFactory';
import { mergePresetParams } from './presetParams';
import { advanceClock, getSoundingNotes } from '@/test/toneMock';
import type { SubtractivePreset, FMPreset } from '@/types/audio';
import { ALL_TEMPLATES } from '@/data/templates';

const releaseTail = (i: number) => {
  const p = SYNTH_PRESETS[i];
  if (p.engine === 'pluck' || p.engine === 'sampler') return p.release;
  return p.amp.release;
};

describe('preset bank integrity', () => {
  it('has 30-45 presets with unique ids and names', () => {
    expect(SYNTH_PRESETS.length).toBeGreaterThanOrEqual(30);
    expect(SYNTH_PRESETS.length).toBeLessThanOrEqual(45);
    expect(new Set(SYNTH_PRESETS.map((p) => p.id)).size).toBe(SYNTH_PRESETS.length);
    expect(new Set(SYNTH_PRESETS.map((p) => p.name)).size).toBe(SYNTH_PRESETS.length);
  });

  it('keeps every legacy index (0..35) loadable', () => {
    expect(SYNTH_PRESETS.length).toBeGreaterThan(LEGACY_PRESET_COUNT);
    for (let i = 0; i < LEGACY_PRESET_COUNT; i++) expect(SYNTH_PRESETS[i]).toBeDefined();
  });

  it('legacy slots keep their musical role', () => {
    // A few anchors from the previous bank: same category (role), new sound.
    expect(SYNTH_PRESETS[0].category).toBe('keys'); // Classic -> Grand Piano
    expect(SYNTH_PRESETS[0].engine).toBe('sampler');
    expect(SYNTH_PRESETS[7].category).toBe('bass'); // Bass
    expect(SYNTH_PRESETS[8].category).toBe('bass'); // Sub Bass
    expect(SYNTH_PRESETS[9].id).toBe('drawbar-organ'); // Organ
    expect(SYNTH_PRESETS[12].category).toBe('pad'); // Strings
    expect(SYNTH_PRESETS[13].id).toBe('rhodes'); // Electric Piano
    expect(SYNTH_PRESETS[14].category).toBe('brass'); // Brass
    expect(SYNTH_PRESETS[20].id).toBe('acid-bass'); // Acid Bass
    expect(SYNTH_PRESETS[28].id).toBe('nylon-guitar');
  });

  it('legacy slots that were polyphonic stay polyphonic (saved chords keep all notes)', () => {
    // Old bank: 24..27 were Tone.MonoSynth, everything else PolySynth/PluckSynth/MembraneSynth.
    const oldPoly = [...Array(24).keys(), 33, 34, 35];
    for (const i of oldPoly) {
      const p = SYNTH_PRESETS[i];
      expect(p.engine === 'subtractive' && p.mono, `${i} ${p.id}`).toBe(false);
      expect(p.polyphony, `${i} ${p.id}`).toBeGreaterThan(1);
    }
  });

  it('every template preset index resolves', () => {
    for (const t of ALL_TEMPLATES) {
      for (const track of t.arrangement.tracks) {
        if (track.instrument.type === 'synth') expect(SYNTH_PRESETS[track.instrument.presetIndex]).toBeDefined();
      }
    }
  });

  it('covers the required families', () => {
    const ids = new Set(SYNTH_PRESETS.map((p) => p.id));
    for (const id of [
      'grand-piano', 'bright-piano', 'rhodes', 'wurli', 'drawbar-organ', 'warm-pad', 'supersaw-pad', 'airy-pad',
      'string-ensemble', 'synth-brass', 'sub-bass', 'reese-bass', 'acid-bass', 'pluck-bass', 'saw-lead',
      'square-lead', 'sync-lead', 'synth-pluck', 'tubular-bell', 'marimba', 'vibraphone', 'laser-zap',
    ]) {
      expect(ids.has(id), id).toBe(true);
    }
  });

  it('uses only declared categories and vibes', () => {
    const cats = new Set(PRESET_CATEGORIES.map((c) => c.id));
    const vibes = new Set(PRESET_VIBES.map((v) => v.id));
    for (const p of SYNTH_PRESETS) {
      expect(cats.has(p.category), p.id).toBe(true);
      expect(vibes.has(p.vibe), p.id).toBe(true);
    }
  });

  it('has sane parameter ranges', () => {
    for (const p of [...SYNTH_PRESETS, FALLBACK_PIANO]) {
      expect(p.gain, p.id).toBeGreaterThanOrEqual(-15);
      expect(p.gain, p.id).toBeLessThanOrEqual(12);
      expect(p.polyphony, p.id).toBeGreaterThanOrEqual(1);
      if (p.engine === 'subtractive') {
        for (const env of [p.amp, p.filterEnv]) {
          expect(env.attack).toBeGreaterThan(0);
          expect(env.sustain).toBeGreaterThanOrEqual(0);
          expect(env.sustain).toBeLessThanOrEqual(1);
          expect(env.release).toBeGreaterThan(0);
        }
        expect(p.oscA.unison).toBeGreaterThanOrEqual(1);
        expect(p.oscA.unison).toBeLessThanOrEqual(7);
        expect(p.filter.cutoff).toBeGreaterThan(20);
        expect(p.filter.cutoff).toBeLessThanOrEqual(18000);
      }
      if (p.engine === 'fm') {
        expect(p.ratio).toBeGreaterThan(0);
        expect(p.index).toBeGreaterThanOrEqual(0);
        expect(p.amp.release).toBeGreaterThan(0);
      }
    }
  });

  it('looks presets up by id', () => {
    expect(getPresetIndexById('grand-piano')).toBe(0);
    expect(getPresetIndexById('nope')).toBe(-1);
  });
});

describe('every preset constructs and releases cleanly (Tone double)', () => {
  SYNTH_PRESETS.forEach((preset, i) => {
    it(`${i} ${preset.name}`, () => {
      const player = createSynthFromPreset(preset);
      expect(() => {
        player.noteOn(60, 0.9, 0);
        player.noteOn(64, 0.5, 0.01);
        player.noteOn(67, 1, 0.02);
        player.noteOn(60, 0.7, 0.03); // re-strike while held
      }).not.toThrow();
      expect(player.getActiveVoiceCount(0.05)).toBeGreaterThan(0);

      player.noteOff(60, 0.5);
      player.noteOff(64, 0.5);
      player.noteOff(67, 0.5);
      expect(player.getHeldNotes()).toEqual([]);
      expect(player.getActiveVoiceCount(0.5 + releaseTail(i) + 0.1)).toBe(0);

      // Scheduled one-shots end on their own.
      player.triggerAttackRelease('C3', 0.25, 1, 0.8);
      expect(player.getActiveVoiceCount(1 + 0.25 + releaseTail(i) + 0.1)).toBe(0);

      // Live param change must not throw or recreate anything.
      expect(() => player.set({ gain: preset.gain - 1, character: { chorus: 0.2 } })).not.toThrow();

      player.noteOn(48, 0.8, 2);
      player.releaseAll(2.1);
      expect(player.getHeldNotes()).toEqual([]);
      expect(player.getActiveVoiceCount(2.2)).toBe(0);
      expect(getSoundingNotes()).toEqual([]);
      player.dispose();
    });
  });
});

describe('resolvePreset / override merging', () => {
  const lead = SYNTH_PRESETS[1] as SubtractivePreset;

  it('returns the base preset when there are no overrides', () => {
    expect(resolvePreset(1)).toBe(SYNTH_PRESETS[1]);
    expect(resolvePreset(1, {})).toBe(SYNTH_PRESETS[1]);
  });

  it('falls back to preset 0 for bad indices', () => {
    expect(presetAt(999)).toBe(SYNTH_PRESETS[0]);
    expect(presetAt(-1)).toBe(SYNTH_PRESETS[0]);
    expect(presetAt(Number.NaN)).toBe(SYNTH_PRESETS[0]);
    expect(resolvePreset(1.7)).toBe(SYNTH_PRESETS[1]);
  });

  it('deep-merges nested overrides without mutating the bank', () => {
    const before = structuredClone(lead);
    const p = resolvePreset(1, { filter: { cutoff: 900 }, amp: { release: 1.5 } }) as SubtractivePreset;
    expect(p.filter.cutoff).toBe(900);
    expect(p.filter.resonance).toBe(lead.filter.resonance);
    expect(p.amp.release).toBe(1.5);
    expect(p.amp.attack).toBe(lead.amp.attack);
    expect(SYNTH_PRESETS[1]).toEqual(before);
  });

  it('ignores unknown keys, wrong types, bad enums and locked identity fields', () => {
    const p = resolvePreset(1, {
      bogus: 1,
      filter: { cutoff: 'loud', type: 'wobbly', resonance: Number.NaN, rolloff: -48, nope: 3 },
      oscA: { wave: 'square', level: '1' },
      id: 'hacked',
      engine: 'fm',
      name: 'X',
    }) as SubtractivePreset & Record<string, unknown>;
    expect(p.bogus).toBeUndefined();
    expect(p.filter.cutoff).toBe(lead.filter.cutoff);
    expect(p.filter.type).toBe(lead.filter.type);
    expect(p.filter.resonance).toBe(lead.filter.resonance);
    expect(p.filter.rolloff).toBe(lead.filter.rolloff);
    expect((p.filter as unknown as Record<string, unknown>).nope).toBeUndefined();
    expect(p.oscA.wave).toBe('square');
    expect(p.oscA.level).toBe(lead.oscA.level);
    expect(p.id).toBe('saw-lead');
    expect(p.engine).toBe('subtractive');
    expect(p.name).toBe('Saw Lead');
  });

  it('translates overrides saved by the previous engine', () => {
    const sub = resolvePreset(1, {
      envelope: { attack: 0.2, release: 2 },
      filterEnvelope: { attack: 0.3, baseFrequency: 500, octaves: 3 },
      oscillator: { type: 'fatsawtooth' },
    }) as SubtractivePreset;
    expect(sub.amp.attack).toBe(0.2);
    expect(sub.amp.release).toBe(2);
    expect(sub.filterEnv.attack).toBe(0.3);
    expect(sub.filter.cutoff).toBe(500);
    expect(sub.filter.envAmount).toBe(3);

    const fmIndex = SYNTH_PRESETS.findIndex((p) => p.id === 'rhodes');
    const fm = resolvePreset(fmIndex, { modulationIndex: 5, harmonicity: 2, envelope: { decay: 1 } }) as FMPreset;
    expect(fm.index).toBe(5);
    expect(fm.ratio).toBe(2);
    expect(fm.amp.decay).toBe(1);

    const pluckIndex = SYNTH_PRESETS.findIndex((p) => p.id === 'nylon-guitar');
    const pl = resolvePreset(pluckIndex, { dampening: 1500, resonance: 0.9, attackNoise: 2 });
    expect(pl).toMatchObject({ dampening: 1500, resonance: 0.9, attackNoise: 2 });
  });

  it('every preset survives a garbage override object', () => {
    const garbage = { amp: null, filter: 5, oscA: [], character: { drive: 'x' }, release: {}, ratio: 'y' };
    SYNTH_PRESETS.forEach((p, i) => {
      const merged = resolvePreset(i, garbage as unknown as Record<string, unknown>);
      expect(merged).toEqual(p);
      expect(() => createSynthFromPreset(merged).dispose()).not.toThrow();
    });
  });

  it('mergePresetParams accepts partials arrays only with numbers', () => {
    const organ = SYNTH_PRESETS[9] as SubtractivePreset;
    const ok = mergePresetParams(organ, { oscA: { partials: [1, 0.5] } });
    expect(ok.oscA.partials).toEqual([1, 0.5]);
    const bad = mergePresetParams(organ, { oscA: { partials: [1, 'x'] } });
    expect(bad.oscA.partials).toEqual(organ.oscA.partials);
  });
});

describe('time advances', () => {
  it('advanceClock helper works with instruments using Tone.immediate()', () => {
    const p = createSynthFromPreset(SYNTH_PRESETS[1]);
    p.noteOn(60, 0.8, 0);
    p.noteOff(60, 0.1);
    advanceClock(5);
    expect(p.getActiveVoiceCount()).toBe(0);
  });
});
