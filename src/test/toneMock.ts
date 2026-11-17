/**
 * Tone.js test double.
 *
 * `src/test/setup.ts` replaces the real `tone` module with this mock for EVERY
 * test file (vi.mock in a setup file), so services/hooks that `import * as Tone
 * from 'tone'` can be unit tested in jsdom without a real AudioContext.
 * Opt out in a single test file with `vi.unmock('tone')`.
 *
 * How it works:
 * - Any `new Tone.Whatever(...)` returns a MockToneNode. Unknown properties are
 *   created on demand as a "callable signal": a vi.fn() (so `node.triggerAttack()`
 *   works and is spy-able) that ALSO carries signal fields (`value`, `rampTo`,
 *   `setValueAtTime`, ...) so `node.volume.value = -6` or
 *   `filter.frequency.rampTo(800, 0.1)` work too. Assigned properties are stored.
 * - Voice tracking: triggerAttack / triggerRelease / triggerAttackRelease /
 *   releaseAll are real implementations that maintain `node.activeNotes`
 *   (a Set of whatever note values were passed). Use `getSoundingNotes()` to
 *   assert on stuck notes across all synths.
 * - Every node is recorded in `toneMock.instances` (cleared before each test by
 *   setup.ts via `resetToneMock()`).
 * - `Tone.now()` returns `toneMock.clock.now` (settable / `advanceClock(sec)`).
 * - `Tone.getTransport()` is a stateful singleton (state/seconds/bpm/loop...);
 *   `schedule`/`scheduleRepeat`/`scheduleOnce` store callbacks you can fire with
 *   `toneMock.transport.runScheduled(time?)`.
 *
 * Example:
 *   import { toneMock, getSoundingNotes, instancesOf } from '@/test/toneMock';
 *   import { audioEngine } from '@/services/AudioEngine';
 *
 *   it('releases notes', () => {
 *     audioEngine.noteOn(60, 0.8);
 *     expect(getSoundingNotes()).toHaveLength(1);
 *     audioEngine.noteOff(60);
 *     expect(getSoundingNotes()).toHaveLength(0);
 *     expect(instancesOf('PolySynth')[0].triggerAttack).toHaveBeenCalled();
 *   });
 *
 * Note: services are module-level singletons, so their Tone nodes are created
 * once when the service module is first imported. resetToneMock() only clears
 * the instance registry, not nodes held by singletons; use `vi.resetModules()`
 * + dynamic import if a test needs a fresh singleton.
 */
import { vi, type Mock } from 'vitest';

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/** A vi.fn that also behaves like a Tone Signal/Param. */
export type CallableSignal = Mock & {
  value: unknown;
  [key: string]: unknown;
};

export interface MockToneNode {
  /** Name of the Tone class that created this node, e.g. 'PolySynth'. */
  readonly __className: string;
  /** Constructor arguments. */
  readonly __args: unknown[];
  /** Notes currently held (triggerAttack without matching triggerRelease). */
  readonly activeNotes: Set<unknown>;
  disposed: boolean;
  // Everything else is dynamic (callable signals / stored values).
  [key: string]: any;
}

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

interface ToneMockState {
  instances: MockToneNode[];
  clock: { now: number };
  started: boolean;
  transport: MockTransport;
  context: MockToneNode;
  destination: MockToneNode;
}

// Kept on globalThis so the state survives `vi.resetModules()`: a test that
// re-imports services (fresh singletons) still sees their nodes here.
const globalState = globalThis as typeof globalThis & { __toneMockState?: ToneMockState };
const isFirstLoad = !globalState.__toneMockState;

export const toneMock: ToneMockState = (globalState.__toneMockState ??= {
  instances: [],
  clock: { now: 0 },
  started: false,
  transport: null as unknown as MockTransport,
  context: null as unknown as MockToneNode,
  destination: null as unknown as MockToneNode,
});

const SIGNAL_DEFAULTS: Record<string, unknown> = {
  frequency: 440,
  detune: 0,
  volume: 0,
  gain: 1,
  Q: 1,
  pan: 0,
  wet: 1,
  bpm: 120,
  playbackRate: 1,
  harmonicity: 1,
  modulationIndex: 1,
  threshold: -24,
  ratio: 12,
  attack: 0.003,
  release: 0.25,
  delayTime: 0.25,
  feedback: 0.125,
  depth: 0.5,
  low: 0,
  mid: 0,
  high: 0,
};

/** Plain (non-signal) property defaults for every node. */
const NODE_DEFAULTS: Record<string, unknown> = {
  disposed: false,
  state: 'stopped',
  loaded: true,
  mute: false,
  numberOfInputs: 1,
  numberOfOutputs: 1,
  maxPolyphony: 32,
  sampleRate: 44100,
  channelCount: 2,
};

// Things that must NOT be auto-created (would confuse promise detection,
// React, vitest pretty-format, etc.).
const NEVER_AUTO = new Set<PropertyKey>([
  'then',
  'catch',
  'finally',
  'toJSON',
  'asymmetricMatch',
  '$$typeof',
  'nodeType',
  'tagName',
  '@@__IMMUTABLE_ITERABLE__@@',
  '@@__IMMUTABLE_RECORD__@@',
  '_isMockFunction',
  'constructor',
]);

// ---------------------------------------------------------------------------
// Callable signals
// ---------------------------------------------------------------------------

function createCallableSignal(initial: unknown, owner: MockToneNode): CallableSignal {
  const fn = vi.fn(() => owner) as unknown as CallableSignal;
  fn.value = initial;
  const setter = (v: unknown) => {
    fn.value = v;
    return fn;
  };
  Object.assign(fn, {
    rampTo: vi.fn(setter),
    linearRampTo: vi.fn(setter),
    exponentialRampTo: vi.fn(setter),
    targetRampTo: vi.fn(setter),
    setValueAtTime: vi.fn(setter),
    linearRampToValueAtTime: vi.fn(setter),
    exponentialRampToValueAtTime: vi.fn(setter),
    setTargetAtTime: vi.fn(setter),
    cancelScheduledValues: vi.fn(() => fn),
    cancelAndHoldAtTime: vi.fn(() => fn),
    getValueAtTime: vi.fn(() => fn.value),
    connect: vi.fn(() => fn),
    disconnect: vi.fn(() => fn),
    dispose: vi.fn(() => fn),
  });
  return fn;
}

// ---------------------------------------------------------------------------
// Voice tracking helpers
// ---------------------------------------------------------------------------

function toNoteList(notes: unknown): unknown[] {
  if (notes === undefined) return [];
  return Array.isArray(notes) ? notes : [notes];
}

function installVoiceMethods(node: MockToneNode) {
  const active = node.activeNotes;
  node.triggerAttack = vi.fn((notes?: unknown) => {
    for (const n of toNoteList(notes)) active.add(n);
    // Monophonic synths / noise synths: attack with no note
    if (notes === undefined) active.add('__mono__');
    return node;
  });
  node.triggerRelease = vi.fn((notes?: unknown) => {
    // Monophonic synths call triggerRelease(time) with no note -> release all
    if (notes === undefined || typeof notes === 'number' && node.__className !== 'PolySynth' && node.__className !== 'Sampler') {
      active.clear();
    } else {
      for (const n of toNoteList(notes)) active.delete(n);
    }
    return node;
  });
  node.triggerAttackRelease = vi.fn(() => node); // self-releasing: never "sounding"
  node.releaseAll = vi.fn(() => {
    active.clear();
    return node;
  });
}

// ---------------------------------------------------------------------------
// Node factory
// ---------------------------------------------------------------------------

function createNode(className: string, args: unknown[], proto: object): MockToneNode {
  const target = Object.create(proto) as MockToneNode;
  Object.defineProperties(target, {
    __className: { value: className, enumerable: true },
    __args: { value: args, enumerable: false },
    activeNotes: { value: new Set<unknown>(), enumerable: false },
  });
  Object.assign(target, NODE_DEFAULTS);

  // Constructor options object (Tone style) → expose as plain props
  const opts = args.find((a) => a && typeof a === 'object' && !Array.isArray(a)) as
    | Record<string, unknown>
    | undefined;

  const proxy: MockToneNode = new Proxy(target, {
    get(t, prop, receiver) {
      if (prop in t) return Reflect.get(t, prop, receiver);
      if (typeof prop === 'symbol' || NEVER_AUTO.has(prop)) return undefined;
      const initial = prop in SIGNAL_DEFAULTS ? SIGNAL_DEFAULTS[prop] : opts?.[prop];
      const signal = createCallableSignal(initial, proxy);
      (t as Record<string, unknown>)[prop] = signal;
      return signal;
    },
  });

  // Chainable / lifecycle methods with sensible behaviour
  Object.assign(target, {
    connect: vi.fn(() => proxy),
    disconnect: vi.fn(() => proxy),
    chain: vi.fn(() => proxy),
    fan: vi.fn(() => proxy),
    toDestination: vi.fn(() => proxy),
    start: vi.fn(() => {
      target.state = 'started';
      return proxy;
    }),
    stop: vi.fn(() => {
      target.state = 'stopped';
      return proxy;
    }),
    restart: vi.fn(() => proxy),
    sync: vi.fn(() => proxy),
    unsync: vi.fn(() => proxy),
    set: vi.fn((values: Record<string, unknown>) => {
      if (values && typeof values === 'object') {
        for (const [k, v] of Object.entries(values)) {
          const cur = (target as Record<string, unknown>)[k];
          if (typeof cur === 'function' && 'value' in (cur as object) && typeof v !== 'object') {
            (cur as CallableSignal).value = v;
          }
        }
      }
      return proxy;
    }),
    get: vi.fn(() => ({ ...(opts ?? {}) })),
    dispose: vi.fn(() => {
      target.disposed = true;
      target.activeNotes.clear();
      return proxy;
    }),
    generate: vi.fn(async () => proxy),
    load: vi.fn(async () => proxy),
    getValue: vi.fn(() => -Infinity), // Meter
    getLevel: vi.fn(() => -Infinity),
  });
  installVoiceMethods(target);

  toneMock.instances.push(proxy);
  return proxy;
}

/** Base class so `x instanceof Tone.ToneAudioNode` works for every mock node. */
class MockToneAudioNodeBase {}

const classCache = new Map<string, new (...args: unknown[]) => MockToneNode>();

function getMockClass(name: string): new (...args: unknown[]) => MockToneNode {
  let cls = classCache.get(name);
  if (!cls) {
    const Named = {
      [name]: class extends MockToneAudioNodeBase {
        constructor(...args: unknown[]) {
          super();
          return createNode(name, args, new.target.prototype);
        }
        static getDefaults() {
          return {};
        }
      },
    }[name];
    cls = Named as unknown as new (...args: unknown[]) => MockToneNode;
    classCache.set(name, cls);
  }
  return cls;
}

// ---------------------------------------------------------------------------
// Transport
// ---------------------------------------------------------------------------

interface ScheduledEvent {
  id: number;
  callback: (time: number) => void;
  time: unknown;
  repeat?: unknown;
}

export type MockTransport = MockToneNode & {
  scheduled: ScheduledEvent[];
  runScheduled: (time?: number) => void;
};

function createTransport(): MockTransport {
  const t = createNode('Transport', [], MockToneAudioNodeBase.prototype) as MockTransport;
  let nextId = 1;
  Object.assign(t, {
    seconds: 0,
    ticks: 0,
    position: '0:0:0',
    progress: 0,
    PPQ: 192,
    loop: false,
    loopStart: 0,
    loopEnd: 0,
    swing: 0,
    swingSubdivision: '8n',
    timeSignature: 4,
    scheduled: [] as ScheduledEvent[],
  });
  t.bpm.value = 120;
  const schedule = (callback: (time: number) => void, time: unknown, repeat?: unknown) => {
    const id = nextId++;
    t.scheduled.push({ id, callback, time, repeat });
    return id;
  };
  Object.assign(t, {
    schedule: vi.fn((cb: (time: number) => void, time: unknown) => schedule(cb, time)),
    scheduleOnce: vi.fn((cb: (time: number) => void, time: unknown) => schedule(cb, time)),
    scheduleRepeat: vi.fn((cb: (time: number) => void, interval: unknown, start?: unknown) =>
      schedule(cb, start ?? 0, interval),
    ),
    clear: vi.fn((id: number) => {
      t.scheduled = t.scheduled.filter((e) => e.id !== id);
      return t;
    }),
    cancel: vi.fn(() => {
      t.scheduled = [];
      return t;
    }),
    start: vi.fn(() => {
      t.state = 'started';
      return t;
    }),
    stop: vi.fn(() => {
      t.state = 'stopped';
      t.seconds = 0;
      t.ticks = 0;
      t.position = '0:0:0';
      return t;
    }),
    pause: vi.fn(() => {
      t.state = 'paused';
      return t;
    }),
    toggle: vi.fn(() => {
      t.state = t.state === 'started' ? 'stopped' : 'started';
      return t;
    }),
    setLoopPoints: vi.fn((start: number, end: number) => {
      t.loopStart = start;
      t.loopEnd = end;
      return t;
    }),
    on: vi.fn(() => t),
    off: vi.fn(() => t),
    once: vi.fn(() => t),
    emit: vi.fn(() => t),
    runScheduled: (time = toneMock.clock.now) => {
      for (const e of [...t.scheduled]) e.callback(time);
    },
  });
  return t;
}

function createContext(): MockToneNode {
  const ctx = createNode('Context', [], MockToneAudioNodeBase.prototype);
  const rawContext = {
    sampleRate: 44100,
    get currentTime() {
      return toneMock.clock.now;
    },
    state: 'running',
    resume: vi.fn(async () => undefined),
    suspend: vi.fn(async () => undefined),
    close: vi.fn(async () => undefined),
    decodeAudioData: vi.fn(async () => createFakeAudioBuffer(1, 44100)),
    createBuffer: vi.fn((channels: number, length: number, sampleRate: number) =>
      createFakeAudioBuffer(channels, length, sampleRate),
    ),
  };
  Object.assign(ctx, {
    rawContext,
    sampleRate: 44100,
    state: 'running',
    lookAhead: 0.1,
    latencyHint: 'interactive',
    resume: vi.fn(async () => undefined),
    now: () => toneMock.clock.now,
    immediate: () => toneMock.clock.now,
  });
  Object.defineProperty(ctx, 'currentTime', { get: () => toneMock.clock.now });
  return ctx;
}

export function createFakeAudioBuffer(channels = 1, length = 44100, sampleRate = 44100) {
  const data = Array.from({ length: channels }, () => new Float32Array(length));
  return {
    numberOfChannels: channels,
    length,
    sampleRate,
    duration: length / sampleRate,
    getChannelData: (c: number) => data[c],
    copyFromChannel: vi.fn(),
    copyToChannel: vi.fn(),
  };
}

// ---------------------------------------------------------------------------
// Frequency helper
// ---------------------------------------------------------------------------

const NOTE_NAMES = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'];

function toMidiNumber(value: unknown, units?: string): number {
  if (typeof value === 'number') {
    if (units === 'midi') return value;
    return 69 + 12 * Math.log2(value / 440);
  }
  const m = String(value).match(/^([A-Ga-g])([#b]?)(-?\d+)$/);
  if (!m) return 69;
  const base = NOTE_NAMES.indexOf(m[1].toUpperCase());
  const acc = m[2] === '#' ? 1 : m[2] === 'b' ? -1 : 0;
  return (parseInt(m[3], 10) + 1) * 12 + base + acc;
}

function Frequency(value: unknown, units?: string) {
  const midi = toMidiNumber(value, units);
  const api = {
    toMidi: () => Math.round(midi),
    toFrequency: () => 440 * Math.pow(2, (midi - 69) / 12),
    toNote: () => {
      const r = Math.round(midi);
      return `${NOTE_NAMES[((r % 12) + 12) % 12]}${Math.floor(r / 12) - 1}`;
    },
    transpose: (semitones: number) => Frequency(midi + semitones, 'midi'),
    valueOf: () => api.toFrequency(),
    toSeconds: () => 1 / api.toFrequency(),
  };
  return api;
}

// ---------------------------------------------------------------------------
// Module factory
// ---------------------------------------------------------------------------

/** Clears registries and resets clock/transport state. Called before each test by setup.ts. */
export function resetToneMock(): void {
  toneMock.instances.length = 0;
  toneMock.clock.now = 0;
  toneMock.started = false;
  toneMock.transport = createTransport();
  toneMock.context = createContext();
  toneMock.destination = createNode('Destination', [], MockToneAudioNodeBase.prototype);
  // the helpers above register themselves; keep them out of user-facing lists
  toneMock.instances.length = 0;
}
if (isFirstLoad) resetToneMock();

export function advanceClock(seconds: number): number {
  toneMock.clock.now += seconds;
  return toneMock.clock.now;
}

/** All mock nodes created from `new Tone.<className>()` since the last reset. */
export function instancesOf(className: string): MockToneNode[] {
  return toneMock.instances.filter((n) => n.__className === className);
}

/** Synths whose sound decays on its own; attacks on them are never "held". */
const SELF_DECAYING = new Set(['MembraneSynth', 'MetalSynth', 'NoiseSynth', 'PluckSynth']);

/**
 * Notes still held on any non-disposed mock synth (i.e. potential stuck notes).
 * Self-decaying percussive synths are ignored unless `includePercussive` is set.
 */
export function getSoundingNotes(
  { includePercussive = false }: { includePercussive?: boolean } = {},
): { className: string; note: unknown }[] {
  const out: { className: string; note: unknown }[] = [];
  for (const node of toneMock.instances) {
    if (node.disposed) continue;
    if (!includePercussive && SELF_DECAYING.has(node.__className)) continue;
    for (const note of node.activeNotes) out.push({ className: node.__className, note });
  }
  return out;
}

/**
 * Builds the fake `tone` module. Used by setup.ts:
 *   vi.mock('tone', async () => (await import('@/test/toneMock')).createToneModule());
 */
export function createToneModule(): Record<string, unknown> {
  const explicit: Record<string, unknown> = {
    now: () => toneMock.clock.now,
    immediate: () => toneMock.clock.now,
    start: vi.fn(async () => {
      toneMock.started = true;
    }),
    getTransport: () => toneMock.transport,
    getContext: () => toneMock.context,
    setContext: vi.fn(),
    getDestination: () => toneMock.destination,
    getDraw: () => ({ schedule: (cb: () => void) => cb(), cancel: vi.fn() }),
    get Transport() {
      return toneMock.transport;
    },
    get Destination() {
      return toneMock.destination;
    },
    get context() {
      return toneMock.context;
    },
    loaded: vi.fn(async () => undefined),
    gainToDb: (gain: number) => 20 * Math.log10(gain),
    dbToGain: (db: number) => Math.pow(10, db / 20),
    Frequency,
    Midi: (v: unknown) => Frequency(v, 'midi'),
    Time: (v: unknown) => ({ toSeconds: () => (typeof v === 'number' ? v : 0), valueOf: () => v }),
    Offline: vi.fn(async (cb: (ctx: unknown) => unknown, duration = 1) => {
      await cb({
        destination: toneMock.destination,
        transport: toneMock.transport,
        rawContext: toneMock.context.rawContext,
      });
      const raw = createFakeAudioBuffer(2, Math.max(1, Math.round(duration * 44100)));
      return { ...raw, get: () => raw, toArray: () => [], duration };
    }),
    ToneAudioNode: MockToneAudioNodeBase,
    version: 'mock',
  };
  return new Proxy(explicit, {
    get(t, prop) {
      if (prop in t) return Reflect.get(t, prop);
      if (typeof prop === 'symbol' || prop === 'then' || prop === 'default' || prop === '__esModule') {
        return prop === 'default' ? t : undefined;
      }
      // Any capitalised export is treated as a node class (Synth, PolySynth, Filter, ...)
      if (/^[A-Z]/.test(prop)) return getMockClass(prop);
      return undefined;
    },
    has(t, prop) {
      return prop in t || (typeof prop === 'string' && /^[A-Z]/.test(prop));
    },
  });
}
