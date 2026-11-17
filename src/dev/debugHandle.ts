/**
 * Dev-only debug handle: `window.__microDaw`.
 *
 * Installed from renderer.ts only when `import.meta.env.DEV` (vite dev server,
 * `npm start`, scripts/ui-snap.mjs). Never present in production builds.
 *
 *   window.__microDaw.simulateNoteOn(60, 0.8)      // MIDI keyboard note on (velocity 0..1, channel default 1)
 *   window.__microDaw.simulateNoteOff(60)
 *   window.__microDaw.simulateControlChange(74, 64)
 *   window.__microDaw.setMode('learn')              // registered by App once mounted
 *   window.__microDaw.getMode()
 *   window.__microDaw.getActiveNotes()              // MIDI notes the UI thinks are held
 *   window.__microDaw.audioEngine / drumEngine / arrangementEngine / midiService
 *
 * simulateNote* go through MidiService's callback dispatch, i.e. exactly the
 * same path as real Web MIDI input (MidiService -> useMidi -> useMidiNotes).
 */
import { audioEngine } from '@/services/AudioEngine';
import { drumEngine } from '@/services/DrumEngine';
import { arrangementEngine } from '@/services/ArrangementEngine';
import { midiService } from '@/services/MidiService';
import type { AppMode } from '@/types/appMode';

/** Hooks the React tree registers (see App.tsx). */
export interface DebugAppHooks {
  setMode: (mode: AppMode) => void;
  getMode: () => AppMode;
  getActiveNotes: () => number[];
  /** Live-synth notes useMidiNotes believes are sounding (held or sustained). */
  getSoundingNotes: () => number[];
  /** Panic: release every live note (useMidiNotes().allNotesOff). */
  allNotesOff: () => void;
}

export interface MicroDawDebugHandle {
  audioEngine: typeof audioEngine;
  drumEngine: typeof drumEngine;
  arrangementEngine: typeof arrangementEngine;
  midiService: typeof midiService;
  simulateNoteOn: (note: number, velocity?: number, channel?: number) => void;
  simulateNoteOff: (note: number, channel?: number) => void;
  simulateControlChange: (cc: number, rawValue: number) => void;
  /** True once App has mounted past the StartScreen and registered its hooks. */
  isAppReady: () => boolean;
  setMode: (mode: AppMode) => void;
  getMode: () => AppMode | null;
  getActiveNotes: () => number[];
  getSoundingNotes: () => number[];
  allNotesOff: () => void;
}

declare global {
  interface Window {
    __microDaw?: MicroDawDebugHandle;
  }
}

let appHooks: Partial<DebugAppHooks> = {};

/** Called by App (dev only). Returns an unregister function for effect cleanup. */
export function registerDebugAppHooks(hooks: Partial<DebugAppHooks>): () => void {
  appHooks = { ...appHooks, ...hooks };
  return () => {
    for (const key of Object.keys(hooks) as (keyof DebugAppHooks)[]) {
      if (appHooks[key] === hooks[key]) delete appHooks[key];
    }
  };
}

export function installDebugHandle(): MicroDawDebugHandle {
  const handle: MicroDawDebugHandle = {
    audioEngine,
    drumEngine,
    arrangementEngine,
    midiService,
    simulateNoteOn: (note, velocity = 0.8, channel = 1) =>
      midiService.simulateNoteOn(note, Math.max(0, Math.min(1, velocity)), channel),
    simulateNoteOff: (note, channel = 1) => midiService.simulateNoteOff(note, channel),
    simulateControlChange: (cc, rawValue) => midiService.simulateControlChange(cc, rawValue),
    isAppReady: () => typeof appHooks.setMode === 'function',
    setMode: (mode) => {
      if (!appHooks.setMode) throw new Error('App not mounted yet (still on StartScreen?)');
      appHooks.setMode(mode);
    },
    getMode: () => appHooks.getMode?.() ?? null,
    getActiveNotes: () => appHooks.getActiveNotes?.() ?? [],
    getSoundingNotes: () => appHooks.getSoundingNotes?.() ?? [],
    allNotesOff: () => appHooks.allNotesOff?.(),
  };
  window.__microDaw = handle;
  return handle;
}
