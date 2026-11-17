import type { SynthPreset } from '@/types/audio';

/**
 * Merging of (possibly legacy / untrusted) parameter overrides onto presets.
 *
 * - Only keys that exist on the base preset are applied, and only with a
 *   value of the same type (numbers must be finite). Unknown keys are dropped,
 *   so old saved projects or hand-edited files can never crash the engine.
 * - Enumerated string fields are validated against their allowed values.
 * - Overrides written by the previous engine (Tone.js synth options such as
 *   `envelope`, `filterEnvelope`, `modulationIndex`...) are translated.
 * - Identity fields (id, name, engine, category, vibe) are never overridden.
 */

const LOCKED_KEYS = new Set(['id', 'name', 'engine', 'category', 'vibe', 'description', 'sample']);

const ENUMS: Record<string, readonly (string | number)[]> = {
  'oscA.wave': ['sine', 'triangle', 'sawtooth', 'square', 'pulse'],
  'oscB.wave': ['sine', 'triangle', 'sawtooth', 'square', 'pulse'],
  'sub.wave': ['sine', 'triangle', 'square'],
  'noise.type': ['white', 'pink', 'brown'],
  'filter.type': ['lowpass', 'highpass', 'bandpass', 'notch'],
  'filter.rolloff': [-12, -24],
  'carrier.wave': ['sine', 'triangle', 'square', 'sawtooth'],
};

type Obj = Record<string, unknown>;

const isObj = (v: unknown): v is Obj => v !== null && typeof v === 'object' && !Array.isArray(v);

function mergeInto(base: Obj, over: Obj, path: string): Obj {
  const out: Obj = { ...base };
  for (const [key, value] of Object.entries(over)) {
    if (!(key in base)) continue;
    const full = path ? `${path}.${key}` : key;
    if (!path && LOCKED_KEYS.has(key)) continue;
    const cur = base[key];
    if (isObj(cur)) {
      if (isObj(value)) out[key] = mergeInto(cur, value, full);
      continue;
    }
    if (Array.isArray(cur) || (cur === undefined && Array.isArray(value))) {
      if (Array.isArray(value) && value.every((x) => typeof x === 'number' && Number.isFinite(x))) {
        out[key] = value.slice(0, 32);
      }
      continue;
    }
    const allowed = ENUMS[full];
    if (allowed) {
      if (allowed.includes(value as string | number)) out[key] = value;
      continue;
    }
    if (typeof cur === 'number') {
      if (typeof value === 'number' && Number.isFinite(value)) out[key] = value;
    } else if (typeof cur === 'boolean') {
      if (typeof value === 'boolean') out[key] = value;
    } else if (typeof cur === 'string') {
      if (typeof value === 'string') out[key] = value;
    }
  }
  return out;
}

/** Translate overrides written by the old Tone.js-option based engine. */
export function translateLegacyOverrides(preset: SynthPreset, overrides: Obj): Obj {
  const out: Obj = {};
  for (const [key, value] of Object.entries(overrides)) {
    switch (key) {
      case 'envelope': {
        if (!isObj(value)) break;
        if (preset.engine === 'subtractive' || preset.engine === 'fm') {
          out.amp = { ...(isObj(out.amp) ? out.amp : {}), ...value };
        } else if ('release' in value) {
          out.release = value.release;
        }
        break;
      }
      case 'filterEnvelope': {
        if (!isObj(value) || preset.engine !== 'subtractive') break;
        const { baseFrequency, octaves, ...adsr } = value;
        out.filterEnv = adsr;
        const filter: Obj = {};
        if (typeof baseFrequency === 'number') filter.cutoff = baseFrequency;
        if (typeof octaves === 'number') filter.envAmount = octaves;
        out.filter = { ...(isObj(out.filter) ? out.filter : {}), ...filter };
        break;
      }
      case 'modulationIndex':
        if (preset.engine === 'fm') out.index = value;
        break;
      case 'harmonicity':
        if (preset.engine === 'fm') out.ratio = value;
        break;
      case 'modulationEnvelope':
        if (preset.engine === 'fm' && isObj(value)) out.modEnv = value;
        break;
      case 'pitchDecay':
        if (preset.engine === 'subtractive') out.pitchEnv = { decay: value };
        break;
      default:
        out[key] = value;
    }
  }
  return out;
}

/**
 * Deep-merge `overrides` onto `base`, returning a new, always-valid preset.
 * Never mutates its inputs.
 */
export function mergePresetParams<P extends SynthPreset>(base: P, overrides?: Record<string, unknown> | null): P {
  if (!overrides || !isObj(overrides) || Object.keys(overrides).length === 0) return base;
  const translated = translateLegacyOverrides(base, overrides);
  return mergeInto(base as unknown as Obj, translated, '') as unknown as P;
}

/**
 * Turn a dotted path + value into a nested object: ('filter.cutoff', 800)
 * -> { filter: { cutoff: 800 } }. A plain key is returned as { key: value }.
 */
export function pathToObject(path: string, value: unknown): Obj {
  const parts = path.split('.').filter(Boolean);
  const root: Obj = {};
  let cur = root;
  parts.forEach((part, i) => {
    if (i === parts.length - 1) cur[part] = value;
    else {
      const next: Obj = {};
      cur[part] = next;
      cur = next;
    }
  });
  return root;
}

/** Deep-merge two plain override objects (later wins). */
export function mergeOverrides(a: Obj | undefined, b: Obj): Obj {
  const out: Obj = { ...(a ?? {}) };
  for (const [k, v] of Object.entries(b)) {
    out[k] = isObj(v) && isObj(out[k]) ? mergeOverrides(out[k] as Obj, v) : v;
  }
  return out;
}

/** Read a dotted path from an object (for UI). */
export function getPath(obj: unknown, path: string): unknown {
  let cur: unknown = obj;
  for (const part of path.split('.')) {
    if (!isObj(cur)) return undefined;
    cur = cur[part];
  }
  return cur;
}
