import {
  PAD_COUNT,
  type CCMapping,
  type ControllerProfile,
  type KnobTarget,
  type PadIndex,
  type PadParam,
} from '@/types/sampler';

/**
 * App-level hardware mapping: which incoming MIDI notes trigger which pad and
 * which CCs drive which knob target. Independent of the kit (the kit says what
 * a pad *sounds* like, the profile says which hardware pad *is* that pad).
 *
 * Persisted in localStorage under `micro-daw:controller` as
 * `{ version, profile }`; anything unreadable falls back to the default
 * profile. Also owns the "learn" state (pad wizard / single pad / knob) —
 * while learning, useMidiNotes hands every note-on / CC here first and the
 * learner consumes it.
 */

export const CONTROLLER_STORAGE_KEY = 'micro-daw:controller';
export const CONTROLLER_STORAGE_VERSION = 1;
export const DEFAULT_PROFILE_ID = 'mpk-mini-default';

// --- Knob targets ------------------------------------------------------------

export interface KnobTargetOption {
  key: string;
  label: string;
  target: KnobTarget;
}

const PAD_PARAM_LABELS: Record<PadParam, string> = {
  pitch: 'Pad pitch',
  decay: 'Pad decay',
  filterCutoff: 'Pad filter',
  gainDb: 'Pad gain',
  pan: 'Pad pan',
  startSec: 'Pad start',
};

/** Every knob target, in menu order. */
export const KNOB_TARGET_OPTIONS: KnobTargetOption[] = [
  ...(['pitch', 'decay', 'filterCutoff', 'gainDb', 'pan', 'startSec'] as PadParam[]).map((param) => ({
    key: `pad:${param}`,
    label: PAD_PARAM_LABELS[param],
    target: { kind: 'pad', param } as KnobTarget,
  })),
  { key: 'swing', label: 'Swing', target: { kind: 'swing' } },
  { key: 'bpm', label: 'Tempo (BPM)', target: { kind: 'bpm' } },
  { key: 'trackVolume', label: 'Drum track volume', target: { kind: 'trackVolume' } },
  { key: 'masterVolume', label: 'Master volume', target: { kind: 'masterVolume' } },
];

/** Stable string id for a knob target (for selects / React keys). */
export function knobTargetKey(target: KnobTarget): string {
  return target.kind === 'pad' ? `pad:${target.param}` : target.kind;
}

export function knobTargetFromKey(key: string): KnobTarget | null {
  const option = KNOB_TARGET_OPTIONS.find((o) => o.key === key);
  return option ? cloneTarget(option.target) : null;
}

export function knobTargetLabel(target: KnobTarget): string {
  return KNOB_TARGET_OPTIONS.find((o) => o.key === knobTargetKey(target))?.label ?? knobTargetKey(target);
}

export function sameKnobTarget(a: KnobTarget, b: KnobTarget): boolean {
  return knobTargetKey(a) === knobTargetKey(b);
}

function cloneTarget(target: KnobTarget): KnobTarget {
  return target.kind === 'pad' ? { kind: 'pad', param: target.param } : { kind: target.kind };
}

// --- Default profile ----------------------------------------------------------

const DEFAULT_KNOB_TARGETS: KnobTarget[] = [
  { kind: 'pad', param: 'pitch' },
  { kind: 'pad', param: 'decay' },
  { kind: 'pad', param: 'filterCutoff' },
  { kind: 'pad', param: 'gainDb' },
  { kind: 'swing' },
  { kind: 'bpm' },
  { kind: 'trackVolume' },
  { kind: 'masterVolume' },
];

/**
 * "MPK Mini (default)": pads send notes 36..51 (bank A 36..43, bank B 44..51)
 * on any channel; knobs CC 70..77.
 */
export function createDefaultProfile(): ControllerProfile {
  return {
    id: DEFAULT_PROFILE_ID,
    name: 'MPK Mini (default)',
    padMap: {
      notes: Array.from({ length: PAD_COUNT }, (_, pad) => 36 + pad),
      channel: null,
    },
    ccMappings: DEFAULT_KNOB_TARGETS.map((target, i) => ({ cc: 70 + i, channel: null, target })),
  };
}

export function cloneProfile(p: ControllerProfile): ControllerProfile {
  return {
    id: p.id,
    name: p.name,
    padMap: { notes: [...p.padMap.notes], channel: p.padMap.channel },
    ccMappings: p.ccMappings.map((m) => ({ cc: m.cc, channel: m.channel, target: cloneTarget(m.target) })),
  };
}

// --- Validation / persistence (pure) -----------------------------------------

const isInt = (v: unknown, min: number, max: number): v is number =>
  typeof v === 'number' && Number.isInteger(v) && v >= min && v <= max;

const isChannel = (v: unknown): v is number | null => v === null || isInt(v, 1, 16);

function parseKnobTarget(v: unknown): KnobTarget | null {
  if (!v || typeof v !== 'object') return null;
  const t = v as { kind?: unknown; param?: unknown };
  if (t.kind === 'pad') return typeof t.param === 'string' ? knobTargetFromKey(`pad:${t.param}`) : null;
  return typeof t.kind === 'string' && t.kind !== 'pad' ? knobTargetFromKey(t.kind) : null;
}

/**
 * Validate an unknown value as a ControllerProfile. Returns a clean copy or
 * null. The pad map must be fully valid; individual malformed CC mappings
 * are dropped.
 */
export function validateProfile(value: unknown): ControllerProfile | null {
  if (!value || typeof value !== 'object') return null;
  const p = value as Record<string, unknown>;
  if (typeof p.id !== 'string' || typeof p.name !== 'string') return null;
  const padMap = p.padMap as Record<string, unknown> | undefined;
  if (!padMap || typeof padMap !== 'object') return null;
  const notes = padMap.notes;
  if (!Array.isArray(notes) || notes.length !== PAD_COUNT) return null;
  if (!notes.every((n) => n === null || isInt(n, 0, 127))) return null;
  if (!isChannel(padMap.channel)) return null;
  if (!Array.isArray(p.ccMappings)) return null;

  const ccMappings: CCMapping[] = [];
  for (const raw of p.ccMappings) {
    if (!raw || typeof raw !== 'object') continue;
    const m = raw as Record<string, unknown>;
    const target = parseKnobTarget(m.target);
    if (!isInt(m.cc, 0, 127) || !isChannel(m.channel) || !target) continue;
    ccMappings.push({ cc: m.cc, channel: m.channel, target });
  }
  return {
    id: p.id,
    name: p.name,
    padMap: { notes: [...(notes as (number | null)[])], channel: padMap.channel },
    ccMappings,
  };
}

export function serializeProfile(profile: ControllerProfile): string {
  return JSON.stringify({ version: CONTROLLER_STORAGE_VERSION, profile });
}

/** Parse stored JSON; anything missing, corrupt or from an unknown version → default profile. */
export function parseStoredProfile(raw: string | null): ControllerProfile {
  if (!raw) return createDefaultProfile();
  try {
    const data = JSON.parse(raw) as { version?: unknown; profile?: unknown } | null;
    if (!data || data.version !== CONTROLLER_STORAGE_VERSION) return createDefaultProfile();
    return validateProfile(data.profile) ?? createDefaultProfile();
  } catch {
    return createDefaultProfile();
  }
}

// --- Pure lookups / edits -------------------------------------------------------

/** Pad for an incoming note, honouring the pad map's channel filter. */
export function findPadForNote(profile: ControllerProfile, note: number, channel: number): PadIndex | null {
  const { notes, channel: padChannel } = profile.padMap;
  if (padChannel !== null && padChannel !== channel) return null;
  const pad = notes.indexOf(note);
  return pad >= 0 ? pad : null;
}

/** CC mappings that match an incoming CC. */
export function findCCMappings(profile: ControllerProfile, cc: number, channel: number): CCMapping[] {
  return profile.ccMappings.filter((m) => m.cc === cc && (m.channel === null || m.channel === channel));
}

/**
 * Assign `note` to `pad`. A note already on another pad moves (that pad
 * becomes unassigned), so one hardware pad never triggers two pads.
 */
export function assignPadNote(profile: ControllerProfile, pad: PadIndex, note: number): ControllerProfile {
  const next = cloneProfile(profile);
  next.padMap.notes = next.padMap.notes.map((n, i) => (i !== pad && n === note ? null : n));
  next.padMap.notes[pad] = note;
  return next;
}

/**
 * Bind `cc` to `target`. With `index` that mapping row is rebound (an
 * out-of-range index adds a row); without it the first mapping with that
 * target is rebound, or a new one added.
 * Any other mapping on the same CC is removed (a knob drives one target).
 */
export function assignKnobCC(
  profile: ControllerProfile,
  target: KnobTarget,
  cc: number,
  index?: number,
): ControllerProfile {
  const next = cloneProfile(profile);
  let at: number;
  if (index === undefined) at = next.ccMappings.findIndex((m) => sameKnobTarget(m.target, target));
  else at = index >= 0 && index < next.ccMappings.length ? index : -1; // out of range = add a row
  const binding: CCMapping = { cc, channel: null, target: cloneTarget(target) };
  if (at < 0) {
    next.ccMappings.push(binding);
    at = next.ccMappings.length - 1;
  } else {
    next.ccMappings[at] = binding;
  }
  const kept = next.ccMappings[at];
  next.ccMappings = next.ccMappings.filter((m) => m === kept || m.cc !== cc);
  return next;
}

// --- Learn state ------------------------------------------------------------------

export type LearnState =
  | { kind: 'idle' }
  /** `sequence`: the "hit pads 1..16 in order" wizard; `pad` is the pad waiting for a hit. */
  | { kind: 'pad'; pad: PadIndex; sequence: boolean }
  /** `index`: the mapping row being re-learned, if any. */
  | { kind: 'knob'; target: KnobTarget; index?: number };

export type MidiActivity =
  | { kind: 'note'; note: number; velocity: number; channel: number; pad: PadIndex | null; at: number }
  | { kind: 'cc'; cc: number; value: number; channel: number; targets: KnobTarget[]; at: number };

type KnobHandler = (target: KnobTarget, value01: number) => void;

const now = () => (typeof performance !== 'undefined' ? performance.now() : Date.now());

function readStorage(): string | null {
  try {
    return typeof localStorage !== 'undefined' ? localStorage.getItem(CONTROLLER_STORAGE_KEY) : null;
  } catch {
    return null;
  }
}

class ControllerMapping {
  private profile: ControllerProfile = parseStoredProfile(readStorage());
  private learn: LearnState = { kind: 'idle' };
  /** Channels heard during the current sequence wizard (to narrow padMap.channel afterwards). */
  private sequenceChannels = new Set<number>();
  private lastActivity: MidiActivity | null = null;
  private listeners = new Set<() => void>();
  private activityListeners = new Set<() => void>();
  private knobHandlers = new Set<KnobHandler>();

  // --- Profile -----------------------------------------------------------------

  getProfile(): ControllerProfile {
    return this.profile;
  }

  setProfile(p: ControllerProfile): void {
    const valid = validateProfile(p);
    if (!valid) {
      console.warn('[ControllerMapping] ignoring invalid profile');
      return;
    }
    this.profile = valid;
    try {
      localStorage.setItem(CONTROLLER_STORAGE_KEY, serializeProfile(valid));
    } catch (err) {
      console.warn('[ControllerMapping] could not persist profile', err);
    }
    this.emit();
  }

  resetToDefault(): void {
    this.cancelLearn();
    this.setProfile(createDefaultProfile());
  }

  /** Re-read the stored profile (e.g. after another window changed it). */
  reload(): void {
    this.profile = parseStoredProfile(readStorage());
    this.emit();
  }

  padForNote(note: number, channel: number): PadIndex | null {
    return findPadForNote(this.profile, note, channel);
  }

  clearPad(pad: PadIndex): void {
    if (pad < 0 || pad >= PAD_COUNT) return;
    const next = cloneProfile(this.profile);
    next.padMap.notes[pad] = null;
    this.setProfile(next);
  }

  setPadChannel(channel: number | null): void {
    const next = cloneProfile(this.profile);
    next.padMap.channel = channel;
    this.setProfile(next);
  }

  setCCMapping(index: number, patch: Partial<CCMapping>): void {
    if (index < 0 || index >= this.profile.ccMappings.length) return;
    const next = cloneProfile(this.profile);
    next.ccMappings[index] = { ...next.ccMappings[index], ...patch };
    this.setProfile(next);
  }

  removeCCMapping(index: number): void {
    const next = cloneProfile(this.profile);
    next.ccMappings.splice(index, 1);
    this.setProfile(next);
  }

  // --- Learn ---------------------------------------------------------------------

  getLearnState(): LearnState {
    return this.learn;
  }

  isLearning(): boolean {
    return this.learn.kind !== 'idle';
  }

  /** Learn one pad, or `'sequence'` = wizard starting at pad 1 (index 0). */
  startPadLearn(pad: PadIndex | 'sequence'): void {
    if (pad === 'sequence') {
      this.sequenceChannels.clear();
      this.setLearn({ kind: 'pad', pad: 0, sequence: true });
      return;
    }
    if (pad < 0 || pad >= PAD_COUNT) return;
    this.setLearn({ kind: 'pad', pad, sequence: false });
  }

  /**
   * Bind the next CC received to `target`. `index`: rebind that mapping row
   * (pass `ccMappings.length` to add a new row); omitted = the target's first row.
   */
  startKnobLearn(target: KnobTarget, index?: number): void {
    this.setLearn({ kind: 'knob', target: cloneTarget(target), index });
  }

  cancelLearn(): void {
    if (this.learn.kind === 'idle') return;
    this.setLearn({ kind: 'idle' });
  }

  /** Wizard: leave the current pad as it is and go to the next one. */
  skipPad(): void {
    if (this.learn.kind !== 'pad') return;
    this.advancePadLearn();
  }

  /** Wizard (Backspace): go back one pad. */
  previousPad(): void {
    const learn = this.learn;
    if (learn.kind !== 'pad' || !learn.sequence) return;
    this.setLearn({ ...learn, pad: Math.max(0, learn.pad - 1) });
  }

  private advancePadLearn(): void {
    const learn = this.learn;
    if (learn.kind !== 'pad') return;
    if (!learn.sequence || learn.pad >= PAD_COUNT - 1) {
      this.finishPadLearn(learn.sequence);
      return;
    }
    this.setLearn({ ...learn, pad: learn.pad + 1 });
  }

  private finishPadLearn(sequence: boolean): void {
    // A completed wizard whose hits all came from one channel narrows the pad
    // map to that channel, so keyboard keys on another channel never hit pads.
    if (sequence && this.sequenceChannels.size === 1) {
      const [channel] = this.sequenceChannels;
      if (this.profile.padMap.channel !== channel) this.setPadChannel(channel);
    }
    this.sequenceChannels.clear();
    this.setLearn({ kind: 'idle' });
  }

  private setLearn(state: LearnState): void {
    this.learn = state;
    this.emit();
  }

  // --- Input (called by useMidiNotes) ------------------------------------------------

  /**
   * Every MIDI note-on goes through here first. Records activity; returns
   * true when the note was consumed by a pad learner (do not play it).
   */
  handleNoteOn(note: number, velocity: number, channel: number): boolean {
    const learn = this.learn;
    const consumed = learn.kind === 'pad';
    if (consumed) {
      const next = assignPadNote(this.profile, learn.pad, note);
      // A hit from a channel the pad map ignores widens it to "any" (a
      // finished wizard narrows it again, see finishPadLearn).
      if (next.padMap.channel !== null && next.padMap.channel !== channel) next.padMap.channel = null;
      if (learn.sequence) this.sequenceChannels.add(channel);
      this.setProfile(next);
    }
    this.recordActivity({
      kind: 'note',
      note,
      velocity,
      channel,
      pad: consumed ? learn.pad : this.padForNote(note, channel),
      at: now(),
    });
    if (consumed) this.advancePadLearn();
    return consumed;
  }

  /**
   * Every CC that isn't sustain/panic goes through here. While knob-learning
   * the CC is bound; otherwise it is dispatched to `onKnob` handlers.
   * Returns true when the CC was consumed (learned or mapped).
   */
  handleCC(cc: number, rawValue: number, channel: number): boolean {
    const learn = this.learn;
    if (learn.kind === 'knob') {
      this.setProfile(assignKnobCC(this.profile, learn.target, cc, learn.index));
      this.recordActivity({ kind: 'cc', cc, value: rawValue, channel, targets: [learn.target], at: now() });
      this.setLearn({ kind: 'idle' });
      return true;
    }
    const mappings = findCCMappings(this.profile, cc, channel);
    this.recordActivity({
      kind: 'cc',
      cc,
      value: rawValue,
      channel,
      targets: mappings.map((m) => m.target),
      at: now(),
    });
    if (mappings.length === 0) return false;
    const value01 = Math.min(1, Math.max(0, rawValue / 127));
    for (const m of mappings) {
      for (const handler of [...this.knobHandlers]) {
        try {
          handler(m.target, value01);
        } catch (err) {
          console.error('[ControllerMapping] knob handler failed', err);
        }
      }
    }
    return true;
  }

  // --- Subscriptions --------------------------------------------------------------

  onKnob(handler: KnobHandler): () => void {
    this.knobHandlers.add(handler);
    return () => {
      this.knobHandlers.delete(handler);
    };
  }

  /** Profile / learn-state changes. */
  subscribe(cb: () => void): () => void {
    this.listeners.add(cb);
    return () => {
      this.listeners.delete(cb);
    };
  }

  getLastActivity(): MidiActivity | null {
    return this.lastActivity;
  }

  /** Every received note-on / CC (high rate while a knob turns; subscribe only from visible UI). */
  onActivity(cb: () => void): () => void {
    this.activityListeners.add(cb);
    return () => {
      this.activityListeners.delete(cb);
    };
  }

  private recordActivity(activity: MidiActivity): void {
    this.lastActivity = activity;
    for (const cb of [...this.activityListeners]) cb();
  }

  private emit(): void {
    for (const cb of [...this.listeners]) cb();
  }
}

export type { ControllerMapping };
export const controllerMapping = new ControllerMapping();
