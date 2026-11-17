import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  assignKnobCC,
  assignPadNote,
  CONTROLLER_STORAGE_KEY,
  CONTROLLER_STORAGE_VERSION,
  createDefaultProfile,
  findCCMappings,
  findPadForNote,
  parseStoredProfile,
  serializeProfile,
  validateProfile,
} from '@/services/ControllerMapping';
import type { KnobTarget } from '@/types/sampler';

async function freshMapping() {
  vi.resetModules();
  return (await import('@/services/ControllerMapping')).controllerMapping;
}

describe('default profile', () => {
  it('maps pads to notes 36..51 on any channel and knobs to CC 70..77', () => {
    const p = createDefaultProfile();
    expect(p.padMap.notes).toEqual(Array.from({ length: 16 }, (_, i) => 36 + i));
    expect(p.padMap.channel).toBeNull();
    expect(p.ccMappings.map((m) => m.cc)).toEqual([70, 71, 72, 73, 74, 75, 76, 77]);
    expect(p.ccMappings[0].target).toEqual({ kind: 'pad', param: 'pitch' });
    expect(p.ccMappings[7].target).toEqual({ kind: 'masterVolume' });
    expect(findPadForNote(p, 36, 10)).toBe(0);
    expect(findPadForNote(p, 51, 1)).toBe(15);
    expect(findPadForNote(p, 60, 10)).toBeNull();
  });

  it('honours the pad map channel filter', () => {
    const p = createDefaultProfile();
    p.padMap.channel = 10;
    expect(findPadForNote(p, 36, 10)).toBe(0);
    expect(findPadForNote(p, 36, 1)).toBeNull();
  });

  it('matches CC mappings by cc and channel', () => {
    const p = createDefaultProfile();
    p.ccMappings[0].channel = 2;
    expect(findCCMappings(p, 70, 1)).toEqual([]);
    expect(findCCMappings(p, 70, 2)).toHaveLength(1);
    expect(findCCMappings(p, 71, 5)).toHaveLength(1);
  });
});

describe('profile edits', () => {
  it('assigning a note already used by another pad moves it', () => {
    const p = assignPadNote(createDefaultProfile(), 0, 40);
    expect(p.padMap.notes[0]).toBe(40);
    expect(p.padMap.notes[4]).toBeNull();
  });

  it('knob learn rebinds the target and removes the CC from other targets', () => {
    const swing: KnobTarget = { kind: 'swing' };
    const p = assignKnobCC(createDefaultProfile(), swing, 70);
    const on70 = findCCMappings(p, 70, 1);
    expect(on70).toHaveLength(1);
    expect(on70[0].target).toEqual(swing);
    expect(p.ccMappings.filter((m) => m.target.kind === 'swing')).toHaveLength(1);
    expect(p.ccMappings.some((m) => m.target.kind === 'pad' && m.target.param === 'pitch')).toBe(false);
  });

  it('knob learn with an out-of-range index adds a row', () => {
    const base = createDefaultProfile();
    const p = assignKnobCC(base, { kind: 'pad', param: 'pan' }, 20, base.ccMappings.length);
    expect(p.ccMappings).toHaveLength(base.ccMappings.length + 1);
    expect(p.ccMappings.at(-1)).toEqual({ cc: 20, channel: null, target: { kind: 'pad', param: 'pan' } });
  });
});

describe('persistence / validation', () => {
  it('round-trips through serialize/parse', () => {
    const p = assignPadNote(createDefaultProfile(), 3, 60);
    p.padMap.channel = 10;
    expect(parseStoredProfile(serializeProfile(p))).toEqual(p);
  });

  it('falls back to the default on missing, corrupt or wrong-version data', () => {
    const def = createDefaultProfile();
    expect(parseStoredProfile(null)).toEqual(def);
    expect(parseStoredProfile('{not json')).toEqual(def);
    expect(parseStoredProfile(JSON.stringify({ version: 99, profile: def }))).toEqual(def);
    const badNotes = { ...def, padMap: { notes: [1, 2, 3], channel: null } };
    expect(parseStoredProfile(JSON.stringify({ version: CONTROLLER_STORAGE_VERSION, profile: badNotes }))).toEqual(def);
    const badChannel = { ...def, padMap: { ...def.padMap, channel: 17 } };
    expect(validateProfile(badChannel)).toBeNull();
  });

  it('drops individual malformed CC mappings', () => {
    const def = createDefaultProfile();
    const raw = {
      ...def,
      ccMappings: [
        ...def.ccMappings,
        { cc: 300, channel: null, target: { kind: 'swing' } },
        { cc: 5, channel: null, target: { kind: 'pad', param: 'nope' } },
        { cc: 6, channel: null, target: { kind: 'unknown' } },
      ],
    };
    expect(validateProfile(raw)?.ccMappings).toEqual(def.ccMappings);
  });
});

describe('controllerMapping singleton', () => {
  beforeEach(() => localStorage.clear());

  it('loads the stored profile, and persists setProfile with a version', async () => {
    const stored = assignPadNote(createDefaultProfile(), 0, 60);
    localStorage.setItem(CONTROLLER_STORAGE_KEY, serializeProfile(stored));
    const cm = await freshMapping();
    expect(cm.padForNote(60, 1)).toBe(0);

    cm.setProfile(createDefaultProfile());
    const saved = JSON.parse(localStorage.getItem(CONTROLLER_STORAGE_KEY)!);
    expect(saved.version).toBe(CONTROLLER_STORAGE_VERSION);
    expect(saved.profile.padMap.notes[0]).toBe(36);
  });

  it('corrupt storage falls back to the default profile', async () => {
    localStorage.setItem(CONTROLLER_STORAGE_KEY, '{"version":1,"profile":{"id":3}}');
    const cm = await freshMapping();
    expect(cm.getProfile()).toEqual(createDefaultProfile());
  });

  it('sequence wizard: each hit maps the next pad, Backspace goes back, duplicates move', async () => {
    const cm = await freshMapping();
    const changes = vi.fn();
    cm.subscribe(changes);
    cm.startPadLearn('sequence');
    expect(cm.getLearnState()).toEqual({ kind: 'pad', pad: 0, sequence: true });

    expect(cm.handleNoteOn(60, 0.8, 10)).toBe(true);
    expect(cm.handleNoteOn(61, 0.8, 10)).toBe(true);
    expect(cm.getLearnState()).toMatchObject({ pad: 2 });
    cm.previousPad();
    expect(cm.getLearnState()).toMatchObject({ pad: 1 });
    cm.handleNoteOn(60, 0.8, 10); // pad 1 takes note 60 from pad 0
    expect(cm.getProfile().padMap.notes.slice(0, 2)).toEqual([null, 60]);
    cm.skipPad();
    expect(cm.getLearnState()).toMatchObject({ pad: 3 });
    for (let n = 0; n < 13; n++) cm.handleNoteOn(70 + n, 0.8, 10);
    expect(cm.getLearnState()).toEqual({ kind: 'idle' });
    expect(cm.getProfile().padMap.notes[15]).toBe(82);
    // All wizard hits came from channel 10 → pads narrowed to channel 10.
    expect(cm.getProfile().padMap.channel).toBe(10);
    expect(changes).toHaveBeenCalled();
  });

  it('per-pad learn consumes one note then stops; cancel leaves the profile untouched', async () => {
    const cm = await freshMapping();
    cm.startPadLearn(5);
    expect(cm.handleNoteOn(72, 1, 1)).toBe(true);
    expect(cm.padForNote(72, 1)).toBe(5);
    expect(cm.isLearning()).toBe(false);
    expect(cm.handleNoteOn(72, 1, 1)).toBe(false);

    cm.startPadLearn(6);
    cm.cancelLearn();
    expect(cm.handleNoteOn(90, 1, 1)).toBe(false);
    expect(cm.padForNote(90, 1)).toBeNull();
  });

  it('knob learn binds the next CC; mapped CCs dispatch normalized values to onKnob', async () => {
    const cm = await freshMapping();
    const knob = vi.fn();
    const off = cm.onKnob(knob);

    expect(cm.handleCC(70, 127, 1)).toBe(true);
    expect(knob).toHaveBeenLastCalledWith({ kind: 'pad', param: 'pitch' }, 1);
    expect(cm.handleCC(20, 64, 1)).toBe(false);

    cm.startKnobLearn({ kind: 'bpm' });
    expect(cm.handleCC(20, 10, 1)).toBe(true); // consumed by the learner, not dispatched
    expect(knob).toHaveBeenCalledTimes(1);
    expect(cm.isLearning()).toBe(false);
    cm.handleCC(20, 0, 1);
    expect(knob).toHaveBeenLastCalledWith({ kind: 'bpm' }, 0);
    expect(findCCMappings(cm.getProfile(), 75, 1)).toEqual([]); // bpm moved off CC 75

    off();
    cm.handleCC(20, 127, 1);
    expect(knob).toHaveBeenCalledTimes(2);
  });

  it('records the last activity', async () => {
    const cm = await freshMapping();
    const cb = vi.fn();
    cm.onActivity(cb);
    cm.handleNoteOn(37, 0.5, 10);
    expect(cm.getLastActivity()).toMatchObject({ kind: 'note', note: 37, channel: 10, pad: 1 });
    cm.handleCC(71, 30, 1);
    expect(cm.getLastActivity()).toMatchObject({ kind: 'cc', cc: 71, value: 30, targets: [{ kind: 'pad', param: 'decay' }] });
    expect(cb).toHaveBeenCalledTimes(2);
  });
});
