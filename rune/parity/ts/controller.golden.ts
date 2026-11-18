import { it, vi } from 'vitest';
import {
  KNOB_TARGET_OPTIONS,
  assignKnobCC,
  assignPadNote,
  CONTROLLER_STORAGE_KEY,
  createDefaultProfile,
  findCCMappings,
  findPadForNote,
  knobTargetFromKey,
  knobTargetKey,
  knobTargetLabel,
  parseStoredProfile,
  serializeProfile,
  validateProfile,
} from '@/services/ControllerMapping';
import type { ControllerProfile, KnobTarget } from '@/types/sampler';
import { writeFixture } from './fixture';

function mulberry32(seed: number) {
  return () => {
    seed |= 0;
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const ALL_TARGETS: KnobTarget[] = KNOB_TARGET_OPTIONS.map((o) => o.target);

it('dumps controller mapping vectors and learn sessions', async () => {
  const rnd = mulberry32(99);
  const def = createDefaultProfile();

  // --- profiles used across the vectors
  const profiles: ControllerProfile[] = [def];
  {
    const p = assignPadNote(def, 3, 60);
    p.padMap.channel = 10;
    profiles.push(p);
  }
  {
    const p = createDefaultProfile();
    p.padMap.notes = p.padMap.notes.map((n, i) => (i % 3 === 0 ? null : n));
    p.ccMappings[0].channel = 2;
    p.ccMappings.push({ cc: 70, channel: 3, target: { kind: 'swing' } }, { cc: 70, channel: null, target: { kind: 'pad', param: 'pan' } });
    p.name = 'Wëird "name" \\ \n\t\u0001\u001f\u007f  😀 </script>';
    p.id = 'x/y';
    profiles.push(p);
  }
  for (let k = 0; k < 6; k++) {
    let p = createDefaultProfile();
    for (let i = 0; i < 20; i++) p = assignPadNote(p, Math.floor(rnd() * 16), Math.floor(rnd() * 128));
    for (let i = 0; i < 6; i++) {
      const t = ALL_TARGETS[Math.floor(rnd() * ALL_TARGETS.length)];
      const idx = rnd() < 0.5 ? undefined : Math.floor(rnd() * 12) - 1;
      p = assignKnobCC(p, t, Math.floor(rnd() * 128), idx);
    }
    p.padMap.channel = rnd() < 0.5 ? null : 1 + Math.floor(rnd() * 16);
    for (const m of p.ccMappings) m.channel = rnd() < 0.6 ? null : 1 + Math.floor(rnd() * 16);
    profiles.push(p);
  }

  const serialize = profiles.map((p) => [p, serializeProfile(p)]);

  // --- lookups
  const lookups: unknown[] = [];
  profiles.forEach((p, pi) => {
    for (let ch = 1; ch <= 16; ch += pi % 2 === 0 ? 1 : 3)
      for (let note = 0; note < 128; note += pi < 3 ? 1 : 5) lookups.push(['pad', pi, note, ch, findPadForNote(p, note, ch)]);
    for (let ch = 1; ch <= 16; ch++)
      for (let cc = 0; cc < 128; cc += pi < 3 ? 1 : 3) {
        const found = findCCMappings(p, cc, ch);
        if (found.length || cc % 9 === 0) lookups.push(['cc', pi, cc, ch, found]);
      }
  });

  // --- edits
  const edits: unknown[] = [];
  profiles.forEach((p, pi) => {
    for (let i = 0; i < 15; i++) {
      const pad = Math.floor(rnd() * 16);
      const note = Math.floor(rnd() * 128);
      edits.push(['pad', pi, pad, note, assignPadNote(p, pad, note)]);
    }
    for (let i = 0; i < 15; i++) {
      const t = ALL_TARGETS[Math.floor(rnd() * ALL_TARGETS.length)];
      const cc = rnd() < 0.5 ? 70 + Math.floor(rnd() * 8) : Math.floor(rnd() * 128);
      const r = rnd();
      const idx = r < 0.4 ? undefined : r < 0.5 ? -1 : Math.floor(rnd() * 14);
      edits.push(['knob', pi, t, cc, idx ?? null, assignKnobCC(p, t, cc, idx)]);
    }
  });

  // --- target keys / labels
  const keys = ['pad:pitch', 'pad:decay', 'pad:filterCutoff', 'pad:gainDb', 'pad:pan', 'pad:startSec', 'swing', 'bpm', 'trackVolume', 'masterVolume', 'pad', 'pad:nope', '', 'Swing', 'master'];
  const targetKeys = keys.map((k) => [k, knobTargetFromKey(k)]);
  const targetLabels = ALL_TARGETS.map((t) => [t, knobTargetKey(t), knobTargetLabel(t)]);

  // --- validateProfile over JSON values
  const d = JSON.parse(JSON.stringify(def));
  const notes = d.padMap.notes as unknown[];
  const validateInputs: unknown[] = [
    d,
    null,
    0,
    1,
    '',
    'x',
    true,
    false,
    [],
    [d],
    {},
    { ...d, id: 3 },
    { ...d, id: null },
    { ...d, name: undefined },
    { ...d, name: '' },
    { ...d, id: '' },
    { ...d, padMap: null },
    { ...d, padMap: [] },
    { ...d, padMap: 'x' },
    { ...d, padMap: { notes } },
    { ...d, padMap: { notes, channel: 0 } },
    { ...d, padMap: { notes, channel: 1 } },
    { ...d, padMap: { notes, channel: 16 } },
    { ...d, padMap: { notes, channel: 17 } },
    { ...d, padMap: { notes, channel: 2.5 } },
    { ...d, padMap: { notes, channel: '2' } },
    { ...d, padMap: { notes: notes.slice(0, 15), channel: null } },
    { ...d, padMap: { notes: [...notes, 1], channel: null } },
    { ...d, padMap: { notes: notes.map((n, i) => (i === 2 ? null : n)), channel: null } },
    { ...d, padMap: { notes: notes.map((n, i) => (i === 2 ? 128 : n)), channel: null } },
    { ...d, padMap: { notes: notes.map((n, i) => (i === 2 ? -1 : n)), channel: null } },
    { ...d, padMap: { notes: notes.map((n, i) => (i === 2 ? 3.5 : n)), channel: null } },
    { ...d, padMap: { notes: notes.map((n, i) => (i === 2 ? '40' : n)), channel: null } },
    { ...d, padMap: { notes: notes.map(() => 0), channel: null } },
    { ...d, padMap: { notes: notes.map(() => 127), channel: null } },
    { ...d, padMap: { notes: notes.map(() => null), channel: null } },
    { ...d, padMap: { notes: { length: 16 }, channel: null } },
    { ...d, ccMappings: null },
    { ...d, ccMappings: {} },
    { ...d, ccMappings: [] },
    {
      ...d,
      ccMappings: [
        ...d.ccMappings,
        { cc: 300, channel: null, target: { kind: 'swing' } },
        { cc: 5, channel: null, target: { kind: 'pad', param: 'nope' } },
        { cc: 6, channel: null, target: { kind: 'unknown' } },
        null,
        0,
        'x',
        [],
        [1, 2],
        { cc: 7, target: { kind: 'swing' } },
        { cc: 8, channel: 0, target: { kind: 'swing' } },
        { cc: 9, channel: 16, target: { kind: 'bpm' } },
        { cc: 10, channel: 17, target: { kind: 'bpm' } },
        { cc: -1, channel: null, target: { kind: 'bpm' } },
        { cc: 127, channel: null, target: { kind: 'bpm', param: 'pitch' } },
        { cc: 0, channel: null, target: { kind: 'pad:pitch' } },
        { cc: 1, channel: null, target: { kind: 'pad:startSec', param: 'decay' } },
        { cc: 2, channel: null, target: { kind: 'pad' } },
        { cc: 3, channel: null, target: { kind: 'pad', param: 5 } },
        { cc: 4, channel: null, target: null },
        { cc: 4, channel: null, target: [] },
        { cc: 4, channel: null, target: 'swing' },
        { cc: 11.5, channel: null, target: { kind: 'swing' } },
        { cc: '12', channel: null, target: { kind: 'swing' } },
        { cc: 13, channel: null, target: { kind: 'masterVolume', extra: 1 }, extra: true },
        { cc: 14, channel: 3, target: { kind: 'trackVolume' } },
        { cc: 14, channel: 3, target: { kind: 'trackVolume' } },
      ],
    },
    { ...d, extra: { deep: [1, 2] } },
    ...profiles,
  ];
  const validate = validateInputs.map((v) => [v === undefined ? null : v, validateProfile(v)]);

  // --- parseStoredProfile over raw strings
  const goodBody = JSON.stringify(profiles[1]);
  const raws: (string | null)[] = [
    null,
    '',
    ' ',
    '{not json',
    'null',
    '0',
    '1',
    'false',
    'true',
    '"x"',
    '[]',
    '[1]',
    '{}',
    '{"version":1}',
    '{"version":1,"profile":null}',
    '{"version":1,"profile":{"id":3}}',
    '{"version":"1","profile":' + goodBody + '}',
    '{"version":2,"profile":' + goodBody + '}',
    '{"version":99,"profile":' + goodBody + '}',
    '{"version":1,"profile":' + goodBody + '}',
    '{"version":1.0,"profile":' + goodBody + '}',
    '{"version":1e0,"profile":' + goodBody + '}',
    '{"version":10e-1,"profile":' + goodBody + '}',
    '  {"version":1,"profile":' + goodBody + '}  \n',
    '{"version":1,"profile":' + goodBody + '} x',
    '{"profile":' + goodBody + ',"version":1}',
    '{"version":1,"version":2,"profile":' + goodBody + '}',
    '{"version":2,"version":1,"profile":' + goodBody + '}',
    '{"version":1,"profile":' + goodBody.replace('"notes":[36', '"notes":[36.0') + '}',
    '{"version":1,"profile":' + goodBody.replace('"notes":[36', '"notes":[3.6e1') + '}',
    '{"version":1,"profile":' + goodBody.replace('"notes":[36', '"notes":[36.5') + '}',
    '{"version":1,"profile":' + goodBody.replace('"channel":10', '"channel":1e1') + '}',
    '{"version":1,"profile":' + goodBody.replace('"name":"MPK Mini (default)"', '"name":"\\u00e9\\n\\"q\\""') + '}',
    '{"version":1,"profile":' + goodBody.replace('"id":"mpk-mini-default"', '"id":"a","id":"b"') + '}',
    ...profiles.map((p) => serializeProfile(p)),
    serializeProfile(def).replace('"cc":70', '"cc":700'),
    serializeProfile(def).replace('"cc":70', '"cc":-0'),
  ];
  const parse = raws.map((r) => [r, parseStoredProfile(r)]);

  // --- learn sessions on a fresh singleton (localStorage-backed)
  const sessions: unknown[] = [];
  for (let s = 0; s < 36; s++) {
    localStorage.clear();
    let initial: string | null = null;
    if (s % 5 === 1) initial = serializeProfile(profiles[1 + (s % (profiles.length - 1))]);
    if (s % 5 === 2) initial = '{"version":1,"profile":{"id":3}}';
    if (initial !== null) localStorage.setItem(CONTROLLER_STORAGE_KEY, initial);
    vi.resetModules();
    const cm = (await import('@/services/ControllerMapping')).controllerMapping;
    let changes = 0;
    let activity = 0;
    cm.subscribe(() => changes++);
    cm.onActivity(() => activity++);
    const knobs: unknown[] = [];
    cm.onKnob((target, value01) => knobs.push([target, value01]));
    const steps: unknown[] = [];
    // Profile / stored string only when they changed since the previous step (size).
    let lastProfile = '';
    let lastStored: string | null | undefined = undefined;
    const snapshot = () => {
      const a = cm.getLastActivity();
      const { at: _at, ...activityNoAt } = (a ?? { at: 0 }) as { at: number };
      const out: Record<string, unknown> = {
        learn: cm.getLearnState(),
        activity: a ? activityNoAt : null,
        changes,
        activityCount: activity,
      };
      const pj = JSON.stringify(cm.getProfile());
      if (pj !== lastProfile) out.profile = cm.getProfile();
      lastProfile = pj;
      const stored = localStorage.getItem(CONTROLLER_STORAGE_KEY);
      // `true` = exactly serializeProfile(current profile) (checked byte-for-byte in Rust too).
      if (stored !== lastStored) out.stored = stored !== null && stored === serializeProfile(cm.getProfile()) ? true : stored;
      lastStored = stored;
      return out;
    };
    steps.push([['init'], null, [], snapshot()]);
    const channelPool = s % 3 === 0 ? [10] : s % 3 === 1 ? [1, 10] : [1, 2, 3, 10, 16];
    for (let i = 0; i < 70; i++) {
      const r = rnd();
      let op: unknown[];
      let ret: unknown = null;
      knobs.length = 0;
      const ch = channelPool[Math.floor(rnd() * channelPool.length)];
      if (r < 0.06) {
        op = ['startPadLearn', 'sequence'];
        cm.startPadLearn('sequence');
      } else if (r < 0.1) {
        const pad = Math.floor(rnd() * 18) - 1;
        op = ['startPadLearn', pad];
        cm.startPadLearn(pad);
      } else if (r < 0.15) {
        const t = ALL_TARGETS[Math.floor(rnd() * ALL_TARGETS.length)];
        const q = rnd();
        const idx = q < 0.5 ? undefined : q < 0.6 ? -1 : Math.floor(rnd() * 12);
        op = ['startKnobLearn', t, idx ?? null];
        cm.startKnobLearn(t, idx);
      } else if (r < 0.18) {
        op = ['cancelLearn'];
        cm.cancelLearn();
      } else if (r < 0.21) {
        op = ['skipPad'];
        cm.skipPad();
      } else if (r < 0.24) {
        op = ['previousPad'];
        cm.previousPad();
      } else if (r < 0.5) {
        const note = rnd() < 0.9 ? 30 + Math.floor(rnd() * 40) : Math.floor(rnd() * 140) - 5;
        const vel = Math.round(rnd() * 127) / 127;
        op = ['noteOn', note, vel, ch];
        ret = cm.handleNoteOn(note, vel, ch);
      } else if (r < 0.8) {
        const cc = rnd() < 0.7 ? 68 + Math.floor(rnd() * 12) : Math.floor(rnd() * 132);
        const raw = rnd() < 0.95 ? Math.floor(rnd() * 128) : 130;
        op = ['cc', cc, raw, ch];
        ret = cm.handleCC(cc, raw, ch);
      } else if (r < 0.83) {
        const pad = Math.floor(rnd() * 18) - 1;
        op = ['clearPad', pad];
        cm.clearPad(pad);
      } else if (r < 0.86) {
        const c = rnd() < 0.3 ? null : Math.floor(rnd() * 18);
        op = ['setPadChannel', c];
        cm.setPadChannel(c);
      } else if (r < 0.89) {
        const idx = Math.floor(rnd() * 11) - 1;
        const q = rnd();
        const patch =
          q < 0.33 ? { cc: Math.floor(rnd() * 140) } : q < 0.66 ? { channel: rnd() < 0.4 ? null : Math.floor(rnd() * 18) } : { target: ALL_TARGETS[Math.floor(rnd() * ALL_TARGETS.length)] };
        op = ['setCCMapping', idx, patch];
        cm.setCCMapping(idx, patch);
      } else if (r < 0.92) {
        const idx = Math.floor(rnd() * 12) - 3;
        op = ['removeCCMapping', idx];
        cm.removeCCMapping(idx);
      } else if (r < 0.94) {
        op = ['resetToDefault'];
        cm.resetToDefault();
      } else if (r < 0.96) {
        const pi = Math.floor(rnd() * profiles.length);
        const p = profiles[pi];
        op = ['setProfileIndex', pi];
        cm.setProfile(p);
      } else if (r < 0.98) {
        const p = createDefaultProfile();
        p.padMap.notes[0] = 200; // invalid → ignored
        op = ['setInvalidProfile'];
        cm.setProfile(p);
      } else {
        const q = rnd();
        const pi = Math.floor(rnd() * profiles.length);
        const raw = q < 0.5 ? serializeProfile(profiles[pi]) : q < 0.75 ? null : '{bad';
        op = ['reload', q < 0.5 ? pi : q < 0.75 ? null : '{bad'];
        if (raw === null) localStorage.removeItem(CONTROLLER_STORAGE_KEY);
        else localStorage.setItem(CONTROLLER_STORAGE_KEY, raw);
        cm.reload();
      }
      steps.push([op, ret, knobs.map((k) => k), snapshot()]);
    }
    sessions.push({ initial, steps });
  }

  writeFixture('controller', { serialize, lookups, edits, targetKeys, targetLabels, validate, parse, sessions });
});
