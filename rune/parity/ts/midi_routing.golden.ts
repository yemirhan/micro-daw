/**
 * Golden for the MIDI routing core (rune/crates/mdaw-midi/src/router.rs): the real
 * `useMidiNotes` hook with the real `ControllerMapping` singleton and `NoteEventBus`, the audio
 * / drum / arrangement engines and note repeat replaced by loggers. Each scenario is a scripted
 * stream of MIDI / UI inputs; after every step the fixture holds the calls the hook made (in
 * order) and its state (held notes, sustain, sounding notes, learn state, profile, activity).
 */
import { it, vi } from 'vitest';
import { act, renderHook } from '@testing-library/react';
import type { EffectParams } from '@/types/effects';
import { DEFAULT_EFFECT_PARAMS } from '@/types/effects';
import type { KnobTarget } from '@/types/sampler';
import { writeFixture } from './fixture';

type Call = (string | number | boolean | null | object)[];
const log: Call[] = [];
const rec = { state: 'stopped' as 'stopped' | 'recording' };
const fx: { params: EffectParams } = { params: { ...DEFAULT_EFFECT_PARAMS } };

vi.mock('@/services/AudioEngine', () => ({
  audioEngine: {
    noteOn: (n: number, v: number) => log.push(['audio.noteOn', n, v]),
    noteOff: (n: number) => log.push(['audio.noteOff', n]),
    releaseAll: () => log.push(['audio.releaseAll']),
    setVolume: (db: number) => log.push(['audio.setVolume', db]),
    getEffectParams: () => ({ ...fx.params }),
    setReverbWet: (x: number) => {
      fx.params.reverbWet = x;
      log.push(['audio.setReverbWet', x]);
    },
    setChorusDepth: (x: number) => {
      fx.params.chorusDepth = x;
      log.push(['audio.setChorusDepth', x]);
    },
    setFilterCutoff: (x: number) => {
      fx.params.filterCutoff = x;
      log.push(['audio.setFilterCutoff', x]);
    },
    setFilterResonance: (x: number) => {
      fx.params.filterResonance = x;
      log.push(['audio.setFilterResonance', x]);
    },
  },
}));
vi.mock('@/services/DrumEngine', () => ({
  drumEngine: {
    hit: (pad: number, v: number) => log.push(['drum.hit', pad, v]),
    release: (pad: number) => log.push(['drum.release', pad]),
  },
}));
vi.mock('@/services/ArrangementEngine', () => ({
  arrangementEngine: {
    getState: () => rec.state,
    captureNoteOn: (n: number, v: number, drum: boolean) => log.push(['captureOn', n, v, drum]),
    captureNoteOff: (n: number) => log.push(['captureOff', n]),
  },
}));
vi.mock('@/services/NoteRepeat', () => ({
  noteRepeat: {
    press: (pad: number, v: number) => log.push(['repeat.press', pad, v]),
    release: (pad: number) => log.push(['repeat.release', pad]),
    releaseAll: () => log.push(['repeat.releaseAll']),
    setTrigger: () => undefined,
    getRate: () => '1/16',
  },
  repeatStepBeats: () => 0.25,
}));

const { useMidiNotes } = await import('@/hooks/useMidiNotes');
const { controllerMapping, createDefaultProfile, assignPadNote, assignKnobCC, knobTargetKey } = await import(
  '@/services/ControllerMapping'
);
const { noteEventBus } = await import('@/services/NoteEventBus');

/** One input. */
type Step =
  | ['on', number, number, number] // note, velocity 0..1, channel
  | ['off', number, number]
  | ['cc', number, number, number] // cc, value, channel
  | ['kon', number, number] // on-screen key
  | ['koff', number]
  | ['press', number, number] // on-screen pad
  | ['release', number]
  | ['hit', number, number]
  | ['panic']
  | ['sustain', boolean]
  | ['mode', 'auto' | 'keys' | 'split']
  | ['mute', boolean]
  | ['rec', boolean]
  | ['learnPad', number | 'sequence']
  | ['learnKnob', string, number | null]
  | ['cancelLearn']
  | ['skipPad']
  | ['prevPad'];

interface Scenario {
  name: string;
  profile: unknown;
  /** onVolumeChange provided (the app does). */
  volumeHandler: boolean;
  steps: Step[];
}

function mulberry32(seed: number) {
  return () => {
    seed |= 0;
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const vel = (raw: number) => Math.min(Math.max(raw / 127, 0), 1);
const TARGETS = ['pad:pitch', 'pad:decay', 'pad:gainDb', 'swing', 'bpm', 'trackVolume', 'masterVolume'];

function scenarios(): Scenario[] {
  const def = createDefaultProfile();
  const out: Scenario[] = [];
  const base = (name: string, steps: Step[], profile: unknown = def, volumeHandler = true) =>
    out.push({ name, profile, volumeHandler, steps });

  base('pads and keys', [
    ['on', 38, vel(115), 10], ['off', 38, 10], ['on', 60, vel(100), 1], ['on', 64, vel(90), 1],
    ['off', 60, 1], ['on', 64, vel(30), 1], ['off', 64, 1], ['on', 36, 0, 1], ['on', 70, vel(64), 10], ['off', 70, 10],
    ['on', 40, vel(1), 1], ['on', 40, vel(127), 1], ['off', 40, 1], ['off', 40, 1],
  ]);
  base('routing modes keep the note-on route', [
    ['on', 36, vel(100), 10], ['mode', 'keys'], ['off', 36, 10], ['on', 37, vel(100), 10],
    ['mode', 'split'], ['off', 37, 10], ['on', 37, vel(80), 10], ['mode', 'auto'], ['on', 37, vel(81), 10], ['off', 37, 10],
  ]);
  base('sustain and pedal up', [
    ['on', 60, vel(100), 1], ['cc', 64, 127, 1], ['off', 60, 1], ['on', 62, vel(90), 1], ['off', 62, 1],
    ['on', 60, vel(70), 1], ['cc', 64, 63, 1], ['off', 60, 1], ['cc', 64, 64, 1], ['on', 65, vel(10), 1],
    ['cc', 121, 0, 1], ['off', 65, 1], ['sustain', true], ['sustain', true], ['kon', 48, 0.5], ['koff', 48], ['sustain', false],
  ]);
  base('panic', [
    ['on', 60, vel(100), 1], ['on', 36, vel(100), 10], ['press', 5, 0.7], ['cc', 64, 100, 1], ['on', 62, vel(90), 1],
    ['off', 62, 1], ['cc', 123, 0, 1], ['off', 60, 1], ['off', 36, 10], ['release', 5], ['on', 61, vel(50), 2],
    ['cc', 120, 0, 3], ['panic'], ['panic'],
  ]);
  base('recording captures', [
    ['rec', true], ['on', 60, vel(100), 1], ['on', 60, vel(101), 1], ['on', 36, vel(99), 10], ['off', 36, 10],
    ['cc', 64, 127, 1], ['off', 60, 1], ['kon', 72, 0.25], ['panic'], ['on', 67, vel(5), 1], ['rec', false], ['off', 67, 1],
  ]);
  base('mute', [
    ['mute', true], ['on', 60, vel(100), 1], ['on', 36, vel(100), 10], ['mute', false], ['off', 60, 1], ['off', 36, 10],
    ['on', 61, vel(50), 1], ['mute', true], ['on', 61, vel(52), 1], ['off', 61, 1], ['press', 3, 1], ['release', 3],
    ['hit', 2, 0.5], ['hit', 16, 0.5], ['hit', -1, 0.5], ['press', 1.5, 0.5],
  ]);
  base('re-hit without note-off and channels', [
    ['on', 36, vel(100), 10], ['on', 36, vel(90), 10], ['on', 36, vel(80), 1], ['off', 36, 10], ['off', 36, 1],
    ['on', 51, vel(60), 16], ['on', 52, vel(60), 16], ['off', 51, 16], ['off', 52, 16],
  ]);
  {
    const p = assignPadNote(def, 0, 60);
    p.padMap.channel = 10;
    base('channel-filtered pad map', [
      ['on', 60, vel(100), 10], ['on', 60, vel(100), 1], ['off', 60, 10], ['off', 60, 1], ['on', 36, vel(10), 10],
      ['off', 36, 10], ['on', 37, vel(10), 10], ['off', 37, 10],
    ], p);
  }
  base('pad learn consumes notes', [
    ['learnPad', 3], ['on', 70, vel(100), 1], ['off', 70, 1], ['on', 70, vel(100), 1], ['off', 70, 1],
    ['learnPad', 'sequence'], ['on', 80, vel(100), 2], ['on', 81, vel(100), 2], ['off', 80, 2], ['skipPad'],
    ['on', 82, vel(10), 2], ['prevPad'], ['on', 83, vel(10), 2], ['cancelLearn'], ['off', 81, 2], ['off', 82, 2],
    ['off', 83, 2], ['on', 83, vel(64), 2], ['off', 83, 2], ['learnPad', 'sequence'], ['panic'], ['cancelLearn'],
  ]);
  base('knob mapping, learn and legacy CCs', [
    ['cc', 70, 0, 1], ['cc', 70, 127, 1], ['cc', 75, 64, 5], ['cc', 77, 100, 1], ['cc', 1, 0, 1], ['cc', 1, 64, 1],
    ['cc', 1, 70, 1], ['cc', 2, 0, 1], ['cc', 2, 127, 1], ['cc', 3, 0, 1], ['cc', 3, 2, 1], ['cc', 3, 100, 1],
    ['cc', 4, 0, 1], ['cc', 4, 127, 1], ['cc', 5, 0, 1], ['cc', 5, 127, 1], ['cc', 7, 50, 1], ['cc', 91, 50, 1],
    ['learnKnob', 'swing', null], ['cc', 1, 33, 1], ['cc', 1, 40, 1], ['learnKnob', 'bpm', 9], ['cc', 72, 10, 1],
    ['cc', 72, 0, 1], ['cc', 72, 12, 1], ['learnKnob', 'masterVolume', null], ['cc', 64, 0, 1], ['cc', 20, 1, 1],
    ['cc', 20, 2, 1], ['cc', 71, 0, 1], ['cc', 71, 127, 1], ['cc', 73, 64, 1], ['cc', 74, 127, 1],
  ]);
  {
    let p = createDefaultProfile();
    p.ccMappings = [];
    p = assignKnobCC(p, { kind: 'swing' }, 21);
    base('legacy CCs with an empty knob map', [
      ['cc', 70, 127, 1], ['cc', 70, 0, 1], ['cc', 70, 6, 1], ['cc', 71, 30, 1], ['cc', 72, 0, 1], ['cc', 72, 1, 1],
      ['cc', 73, 50, 1], ['cc', 74, 64, 1], ['cc', 74, 0, 1], ['cc', 21, 60, 1], ['cc', 1, 0, 1], ['cc', 1, 1, 1],
    ], p);
    base('volume CC without a handler', [['cc', 5, 64, 1], ['cc', 74, 3, 1], ['cc', 5, 127, 2]], p, false);
  }

  // Random streams: notes on pads and keys, CCs (sustain, panic, mapped, legacy), learns, modes.
  for (let seed = 1; seed <= 24; seed++) {
    const rnd = mulberry32(seed * 7919);
    const pick = <T,>(a: readonly T[]): T => a[Math.floor(rnd() * a.length)];
    let p = createDefaultProfile();
    if (seed % 3 === 0) p.padMap.channel = 10;
    if (seed % 4 === 1) {
      for (let i = 0; i < 4; i++) p = assignPadNote(p, Math.floor(rnd() * 16), 30 + Math.floor(rnd() * 50));
    }
    const steps: Step[] = [];
    const n = 60 + Math.floor(rnd() * 140);
    const notes = [36, 37, 38, 40, 44, 51, 52, 55, 60, 61, 64, 67, 72];
    for (let i = 0; i < n; i++) {
      const r = rnd();
      const ch = pick([1, 1, 1, 10, 10, 2, 16]);
      if (r < 0.32) steps.push(['on', pick(notes), rnd() < 0.05 ? 0 : vel(Math.floor(rnd() * 128)), ch]);
      else if (r < 0.58) steps.push(['off', pick(notes), ch]);
      else if (r < 0.66) steps.push(['cc', 64, Math.floor(rnd() * 128), ch]);
      else if (r < 0.69) steps.push(['cc', pick([120, 121, 123]), 0, ch]);
      else if (r < 0.77) steps.push(['cc', pick([1, 2, 3, 4, 5, 70, 71, 72, 73, 74, 75, 76, 77, 21]), Math.floor(rnd() * 128), ch]);
      else if (r < 0.8) steps.push(['kon', pick(notes), rnd()]);
      else if (r < 0.83) steps.push(['koff', pick(notes)]);
      else if (r < 0.85) steps.push(['press', Math.floor(rnd() * 16), rnd()]);
      else if (r < 0.87) steps.push(['release', Math.floor(rnd() * 16)]);
      else if (r < 0.88) steps.push(['hit', Math.floor(rnd() * 16), rnd()]);
      else if (r < 0.9) steps.push(['mode', pick(['auto', 'keys', 'split'] as const)]);
      else if (r < 0.92) steps.push(['mute', rnd() < 0.5]);
      else if (r < 0.94) steps.push(['rec', rnd() < 0.5]);
      else if (r < 0.95) steps.push(['panic']);
      else if (r < 0.96) steps.push(['learnPad', rnd() < 0.5 ? 'sequence' : Math.floor(rnd() * 16)]);
      else if (r < 0.97) steps.push(['learnKnob', pick(TARGETS), rnd() < 0.5 ? null : Math.floor(rnd() * 10)]);
      else if (r < 0.98) steps.push(['cancelLearn']);
      else if (r < 0.99) steps.push(['skipPad']);
      else steps.push(['sustain', rnd() < 0.5]);
    }
    base(`random ${seed}`, steps, p, seed % 5 !== 0);
  }
  return out;
}

function targetFromKey(key: string): KnobTarget {
  if (key.startsWith('pad:')) return { kind: 'pad', param: key.slice(4) } as KnobTarget;
  return { kind: key } as KnobTarget;
}

function stripAt(a: unknown): unknown {
  if (!a || typeof a !== 'object') return a;
  const { at: _at, ...rest } = a as Record<string, unknown>;
  return rest;
}

it('dumps routing-core call logs for scripted MIDI streams', () => {
  const offBus = noteEventBus.subscribe((e) => {
    const { time: _t, ...rest } = e as unknown as Record<string, unknown>;
    log.push(['bus', rest]);
  });
  const offKnob = controllerMapping.onKnob((target, value01) => log.push(['knob', knobTargetKey(target), value01]));
  const results = [];
  for (const sc of scenarios()) {
    controllerMapping.cancelLearn();
    controllerMapping.setProfile(sc.profile as ReturnType<typeof createDefaultProfile>);
    rec.state = 'stopped';
    fx.params = { ...DEFAULT_EFFECT_PARAMS };
    log.length = 0;
    const hook = renderHook(
      ({ muted }: { muted: boolean }) =>
        useMidiNotes({
          muted,
          onDrumFlash: (pad) => log.push(['flash', pad]),
          ...(sc.volumeHandler ? { onVolumeChange: (db: number) => log.push(['onVolumeChange', db]) } : {}),
          onEffectChange: (p) => log.push(['effects', p]),
        }),
      { initialProps: { muted: false } },
    );
    let muted = false;
    const steps = [];
    for (const step of sc.steps) {
      log.length = 0;
      act(() => {
        const r = hook.result.current;
        const cb = r.callbacks;
        switch (step[0]) {
          case 'on': cb.onNoteOn(step[1], step[2], step[3]); break;
          case 'off': cb.onNoteOff(step[1], step[2]); break;
          case 'cc': cb.onControlChange(step[1], step[2], step[3]); break;
          case 'kon': r.noteOn(step[1], step[2]); break;
          case 'koff': r.noteOff(step[1]); break;
          case 'press': r.drumPress(step[1], step[2]); break;
          case 'release': r.drumRelease(step[1]); break;
          case 'hit': r.drumHit(step[1], step[2]); break;
          case 'panic': r.allNotesOff(); break;
          case 'sustain': r.setSustain(step[1]); break;
          case 'mode': r.setRoutingMode(step[1]); break;
          case 'mute': muted = step[1]; hook.rerender({ muted }); break;
          case 'rec': rec.state = step[1] ? 'recording' : 'stopped'; break;
          case 'learnPad': controllerMapping.startPadLearn(step[1]); break;
          case 'learnKnob': controllerMapping.startKnobLearn(targetFromKey(step[1]), step[2] ?? undefined); break;
          case 'cancelLearn': controllerMapping.cancelLearn(); break;
          case 'skipPad': controllerMapping.skipPad(); break;
          case 'prevPad': controllerMapping.previousPad(); break;
        }
      });
      const r = hook.result.current;
      steps.push({
        calls: [...log],
        active: [...r.activeNotes.values()].map((a) => [a.note, a.velocity]),
        sustain: r.sustainOn,
        sounding: r.getSoundingNotes(),
        mode: r.routingMode,
        learn: controllerMapping.getLearnState(),
        profile: controllerMapping.getProfile(),
        activity: stripAt(controllerMapping.getLastActivity()),
        effects: r.effectParams,
      });
    }
    log.length = 0;
    hook.unmount(); // unmount = allNotesOff
    results.push({ name: sc.name, profile: sc.profile, volumeHandler: sc.volumeHandler, steps: sc.steps, results: steps, unmount: [...log] });
  }
  offBus();
  offKnob();
  writeFixture('midi_routing', { scenarios: results });
});
