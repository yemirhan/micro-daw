import { beforeEach, describe, expect, it, vi } from 'vitest';

type Listener = (e: unknown) => void;

class FakeInput {
  listeners = new Map<string, Set<Listener>>();
  constructor(public id: string, public name: string, public manufacturer = 'Test') {}
  addListener(event: string, fn: Listener) {
    if (!this.listeners.has(event)) this.listeners.set(event, new Set());
    this.listeners.get(event)!.add(fn);
  }
  removeListener(event: string, fn?: Listener) {
    if (fn) this.listeners.get(event)?.delete(fn);
    else this.listeners.delete(event);
  }
  count(event: string) {
    return this.listeners.get(event)?.size ?? 0;
  }
  emit(event: string, e: unknown) {
    for (const fn of this.listeners.get(event) ?? []) fn(e);
  }
  noteOn(note: number, rawAttack = 100, channel = 1) {
    this.emit('noteon', { note: { number: note, rawAttack }, message: { channel } });
  }
  noteOff(note: number, channel = 1) {
    this.emit('noteoff', { note: { number: note }, message: { channel } });
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
      for (const fn of listeners.get(event) ?? []) fn();
    },
  };
});
vi.mock('webmidi', () => ({ WebMidi: webmidi }));

async function freshService() {
  vi.resetModules();
  webmidi.listeners.clear();
  const { midiService } = await import('@/services/MidiService');
  const calls: string[] = [];
  midiService.setCallbacks({
    onNoteOn: (n, v, ch) => calls.push(`on:${n}:${ch}:${v.toFixed(2)}`),
    onNoteOff: (n, ch) => calls.push(`off:${n}:${ch}`),
    onControlChange: (cc, v) => calls.push(`cc:${cc}:${v}`),
  });
  return { midiService, calls };
}

describe('MidiService', () => {
  let keyboard: FakeInput;
  let pads: FakeInput;
  beforeEach(() => {
    keyboard = new FakeInput('kbd', 'MPK mini 3');
    pads = new FakeInput('pads', 'Other Device');
    webmidi.inputs = [keyboard, pads];
  });

  it('auto-selects the MPK and dispatches its notes once', async () => {
    const { midiService, calls } = await freshService();
    expect(await midiService.enable()).toBe(true);
    expect(midiService.getActiveDeviceId()).toBe('kbd');
    keyboard.noteOn(60);
    keyboard.noteOff(60);
    expect(calls).toEqual(['on:60:1:0.79', 'off:60:1']);
  });

  it('re-selecting the same device or enabling twice never doubles listeners', async () => {
    const { midiService, calls } = await freshService();
    await midiService.enable();
    await midiService.enable();
    midiService.selectDevice('kbd');
    midiService.selectDevice('kbd');
    expect(keyboard.count('noteon')).toBe(1);
    keyboard.noteOn(62);
    expect(calls.filter((c) => c.startsWith('on:'))).toHaveLength(1);
  });

  it('only removes its own listeners (others on the same input survive)', async () => {
    const { midiService } = await freshService();
    const foreign = () => undefined;
    keyboard.addListener('noteon', foreign);
    await midiService.enable();
    midiService.selectDevice('pads');
    expect(keyboard.listeners.get('noteon')?.has(foreign)).toBe(true);
    expect(keyboard.count('noteon')).toBe(1);
    expect(pads.count('noteon')).toBe(1);
  });

  it('switching devices releases notes still held on the old one', async () => {
    const { midiService, calls } = await freshService();
    await midiService.enable();
    keyboard.noteOn(60);
    keyboard.noteOn(64, 100, 2);
    midiService.selectDevice('pads');
    expect(calls).toContain('off:60:1');
    expect(calls).toContain('off:64:2');
  });

  it('unplugging the active device releases its held notes and picks another', async () => {
    const { midiService, calls } = await freshService();
    await midiService.enable();
    keyboard.noteOn(67);
    webmidi.inputs = [pads];
    webmidi.fire('disconnected');
    expect(calls).toContain('off:67:1');
    expect(midiService.getActiveDeviceId()).toBe('pads');
    // Reconnecting the keyboard (same Input object) must not end up with stale listeners.
    expect(keyboard.count('noteon')).toBe(0);
  });

  it('re-attaches the keyboard when it is plugged back in (only input)', async () => {
    webmidi.inputs = [keyboard];
    const { midiService, calls } = await freshService();
    await midiService.enable();
    webmidi.inputs = [];
    webmidi.fire('disconnected');
    expect(midiService.getActiveDeviceId()).toBeNull();
    // webmidi reuses the same Input object on reconnect.
    webmidi.inputs = [keyboard];
    webmidi.fire('connected');
    expect(midiService.getActiveDeviceId()).toBe('kbd');
    expect(keyboard.count('noteon')).toBe(1);
    keyboard.noteOn(60);
    expect(calls).toContain('on:60:1:0.79');
  });

  it('switches back to the preferred keyboard when it returns after a fallback', async () => {
    const { midiService, calls } = await freshService();
    await midiService.enable();
    webmidi.inputs = [pads];
    webmidi.fire('disconnected');
    expect(midiService.getActiveDeviceId()).toBe('pads');
    webmidi.inputs = [keyboard, pads];
    webmidi.fire('connected');
    expect(midiService.getActiveDeviceId()).toBe('kbd');
    expect(pads.count('noteon')).toBe(0);
    expect(keyboard.count('noteon')).toBe(1);
    keyboard.noteOn(62);
    expect(calls).toContain('on:62:1:0.79');
  });

  it('a manual choice is remembered across reconnects; unrelated connects do not steal it', async () => {
    const { midiService } = await freshService();
    await midiService.enable();
    midiService.selectDevice('pads');
    const extra = new FakeInput('extra', 'Another Keyboard');
    webmidi.inputs = [keyboard, pads, extra];
    webmidi.fire('connected');
    expect(midiService.getActiveDeviceId()).toBe('pads');
    webmidi.inputs = [keyboard, extra];
    webmidi.fire('disconnected');
    expect(midiService.getActiveDeviceId()).toBe('kbd');
    webmidi.inputs = [keyboard, pads, extra];
    webmidi.fire('connected');
    expect(midiService.getActiveDeviceId()).toBe('pads');
    expect(keyboard.count('noteon')).toBe(0);
  });

  it('simulated note-on with velocity 0 is a note-off', async () => {
    const { midiService, calls } = await freshService();
    midiService.simulateNoteOn(60, 0.5);
    midiService.simulateNoteOn(60, 0);
    expect(calls).toEqual(['on:60:1:0.50', 'off:60:1']);
  });
});
