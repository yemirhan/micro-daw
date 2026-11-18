/**
 * Golden for MidiService's device handling (rune/crates/mdaw-midi/src/devices.rs + host.rs):
 * the real `MidiService` over a fake webmidi whose input list a script changes (plug, unplug,
 * select, notes held across a switch). After every step: the active and preferred device, the
 * devices listed, the note callbacks fired (held-note release), and the device-change
 * notifications.
 */
import { it, vi } from 'vitest';
import { writeFixture } from './fixture';

type Listener = (e: unknown) => void;

class FakeInput {
  listeners = new Map<string, Set<Listener>>();
  constructor(public id: string, public name: string) {}
  manufacturer = '';
  addListener(event: string, fn: Listener) {
    if (!this.listeners.has(event)) this.listeners.set(event, new Set());
    this.listeners.get(event)!.add(fn);
  }
  removeListener(event: string, fn?: Listener) {
    if (fn) this.listeners.get(event)?.delete(fn);
    else this.listeners.delete(event);
  }
  emit(event: string, e: unknown) {
    for (const fn of [...(this.listeners.get(event) ?? [])]) fn(e);
  }
}

const webmidi = vi.hoisted(() => {
  const listeners = new Map<string, Set<(e?: unknown) => void>>();
  return {
    inputs: [] as unknown[],
    listeners,
    enable: async () => undefined,
    disable: () => undefined,
    addListener(event: string, fn: (e?: unknown) => void) {
      if (!listeners.has(event)) listeners.set(event, new Set());
      listeners.get(event)!.add(fn);
    },
    removeListener(event: string, fn: (e?: unknown) => void) {
      listeners.get(event)?.delete(fn);
    },
    fire(event: string) {
      for (const fn of [...(listeners.get(event) ?? [])]) fn();
    },
  };
});
vi.mock('webmidi', () => ({ WebMidi: webmidi }));

type Step =
  | ['enable']
  | ['set', [string, string][]] // the input list changes (plug/unplug), then 'connected'/'disconnected'
  | ['select', string]
  | ['noteOn', string, number, number, number] // input id, note, raw velocity, channel
  | ['noteOff', string, number, number]
  | ['cc', string, number, number, number]
  | ['dispose'];

function mulberry32(seed: number) {
  return () => {
    seed |= 0;
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const MPK: [string, string] = ['mpk', 'MPK mini Play mk3'];
const IAC: [string, string] = ['iac', 'IAC Driver Bus 1'];
const KEY: [string, string] = ['key', 'Keystation 49'];
const MPK2: [string, string] = ['mpk2', 'Akai MPKmini2'];

function scenarios(): { name: string; steps: Step[] }[] {
  const out: { name: string; steps: Step[] }[] = [
    { name: 'auto-select MPK', steps: [['set', [IAC, MPK]], ['enable'], ['noteOn', 'mpk', 60, 100, 1], ['noteOn', 'iac', 61, 100, 1]] },
    { name: 'first input when no MPK', steps: [['set', [IAC, KEY]], ['enable'], ['set', [KEY]], ['set', []], ['set', [IAC]]] },
    {
      name: 'unplug releases held notes, replug re-attaches the preferred',
      steps: [
        ['set', [MPK, IAC]], ['enable'], ['noteOn', 'mpk', 60, 100, 1], ['noteOn', 'mpk', 38, 90, 10], ['noteOn', 'mpk', 60, 20, 1],
        ['set', [IAC]], ['noteOn', 'iac', 70, 1, 2], ['set', [IAC, MPK]], ['noteOn', 'mpk', 64, 127, 3], ['noteOff', 'mpk', 64, 3],
      ],
    },
    {
      name: 'select switches and becomes preferred',
      steps: [
        ['set', [MPK, IAC, KEY]], ['enable'], ['noteOn', 'mpk', 40, 64, 1], ['select', 'key'], ['select', 'key'], ['select', 'nope'],
        ['noteOn', 'key', 41, 64, 2], ['set', [MPK, IAC]], ['set', [MPK, IAC, KEY]], ['select', 'mpk'], ['cc', 'mpk', 64, 127, 1],
      ],
    },
    { name: 'enable with no inputs then plug', steps: [['set', []], ['enable'], ['set', [IAC]], ['set', [IAC, MPK2]], ['set', [MPK2]]] },
    {
      // Electron bug fixed in both apps (user decision 2026-11-18): a lost input's pedal is lifted
      name: 'sustain pedal lifted on switch, unplug and dispose',
      steps: [
        ['set', [MPK, IAC, KEY]], ['enable'], ['cc', 'mpk', 64, 127, 1], ['cc', 'mpk', 64, 90, 2], ['noteOn', 'mpk', 60, 100, 1],
        ['select', 'key'], ['cc', 'key', 64, 127, 3], ['cc', 'key', 64, 10, 3], ['cc', 'key', 64, 64, 4], ['set', [MPK, IAC]],
        ['cc', 'mpk', 64, 127, 5], ['noteOn', 'mpk', 61, 10, 5], ['dispose'],
      ],
    },
    { name: 'note-on velocity 0 and dispose', steps: [['set', [MPK]], ['enable'], ['noteOn', 'mpk', 50, 100, 1], ['noteOn', 'mpk', 50, 0, 1], ['noteOn', 'mpk', 51, 1, 16], ['dispose']] },
  ];
  const pool = [MPK, IAC, KEY, MPK2];
  for (let seed = 1; seed <= 20; seed++) {
    const rnd = mulberry32(seed * 104729);
    const steps: Step[] = [];
    const subset = () => pool.filter(() => rnd() < 0.55);
    steps.push(['set', subset()]);
    steps.push(['enable']);
    for (let i = 0; i < 40; i++) {
      const r = rnd();
      const dev = pool[Math.floor(rnd() * pool.length)][0];
      if (r < 0.25) steps.push(['set', subset()]);
      else if (r < 0.4) steps.push(['select', rnd() < 0.9 ? dev : 'ghost']);
      else if (r < 0.7) steps.push(['noteOn', dev, 36 + Math.floor(rnd() * 8), Math.floor(rnd() * 128), 1 + Math.floor(rnd() * 3)]);
      else if (r < 0.95) steps.push(['noteOff', dev, 36 + Math.floor(rnd() * 8), 1 + Math.floor(rnd() * 3)]);
      else steps.push(['cc', dev, Math.floor(rnd() * 128), Math.floor(rnd() * 128), 1]);
    }
    out.push({ name: `random ${seed}`, steps });
  }
  return out;
}

it('dumps MidiService device-selection sessions', async () => {
  const results = [];
  for (const sc of scenarios()) {
    vi.resetModules();
    webmidi.listeners.clear();
    webmidi.inputs = [];
    const inputs = new Map<string, FakeInput>();
    const { midiService } = await import('@/services/MidiService');
    const calls: unknown[] = [];
    midiService.setCallbacks({
      onNoteOn: (n, v, ch) => calls.push(['on', n, v, ch]),
      onNoteOff: (n, ch) => calls.push(['off', n, ch]),
      onControlChange: (cc, v, ch) => calls.push(['cc', cc, v, ch]),
    });
    let notified = 0;
    let subscribed = false;
    const steps = [];
    for (const step of sc.steps) {
      calls.length = 0;
      notified = 0;
      switch (step[0]) {
        case 'enable':
          await midiService.enable();
          if (!subscribed) {
            midiService.onDeviceChange(() => notified++);
            subscribed = true;
          }
          break;
        case 'set': {
          const before = new Set((webmidi.inputs as FakeInput[]).map((i) => i.id));
          webmidi.inputs = step[1].map(([id, name]) => {
            // a replugged device is a new Input object with the same id
            const fresh = !before.has(id) || !inputs.has(id) ? new FakeInput(id, name) : inputs.get(id)!;
            inputs.set(id, fresh);
            return fresh;
          });
          const after = new Set(step[1].map(([id]) => id));
          const added = [...after].some((id) => !before.has(id));
          const removed = [...before].some((id) => !after.has(id));
          if (removed) webmidi.fire('disconnected');
          if (added) webmidi.fire('connected');
          break;
        }
        case 'select':
          midiService.selectDevice(step[1]);
          break;
        case 'noteOn': {
          const inp = (webmidi.inputs as FakeInput[]).find((i) => i.id === step[1]);
          // webmidi: a note-on with velocity 0 arrives as a noteoff event
          if (step[3] === 0) inp?.emit('noteoff', { note: { number: step[2] }, message: { channel: step[4] } });
          else inp?.emit('noteon', { note: { number: step[2], rawAttack: step[3] }, message: { channel: step[4] } });
          break;
        }
        case 'noteOff': {
          const inp = (webmidi.inputs as FakeInput[]).find((i) => i.id === step[1]);
          inp?.emit('noteoff', { note: { number: step[2] }, message: { channel: step[3] } });
          break;
        }
        case 'cc': {
          const inp = (webmidi.inputs as FakeInput[]).find((i) => i.id === step[1]);
          inp?.emit('controlchange', { controller: { number: step[2] }, rawValue: step[3], message: { channel: step[4] } });
          break;
        }
        case 'dispose':
          midiService.dispose();
          break;
      }
      steps.push({
        calls: [...calls],
        notified,
        active: midiService.getActiveDeviceId(),
        preferred: (midiService as unknown as { preferredDeviceId: string | null }).preferredDeviceId,
        devices: midiService.getDevices().map((d) => d.id),
      });
    }
    midiService.dispose();
    results.push({ name: sc.name, steps: sc.steps, results: steps });
  }
  writeFixture('midi_devices', { scenarios: results });
});
