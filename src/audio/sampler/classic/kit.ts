import { PAD_COUNT, type ClassicMachineId, type KitState, type PadSample } from '@/types/sampler';
import { defaultPadSettings } from '@/audio/sampler/factoryKit';
import { classicKitId, classicSampleName, getClassicMachine, getClassicVoice } from './machines';

/** Pad sample for a classic voice ({ kind: 'classic' } source, name like "808 Kick"). */
export function classicSample(machine: ClassicMachineId, voice: string): PadSample {
  return { source: { kind: 'classic', machine, voice }, name: classicSampleName(machine, voice) };
}

/**
 * The built-in 16-pad kit of a classic machine: its default layout with
 * per-voice gain balance, hats in choke group 1. Pads are named by the bare
 * voice label ("Kick"; the kit name already says "808-style", and the prefixed
 * name wraps on the small pads). Pad colors are left to the default palette.
 */
export function createClassicKit(machine: ClassicMachineId): KitState {
  const m = getClassicMachine(machine);
  return {
    id: classicKitId(machine),
    name: m.name,
    pads: Array.from({ length: PAD_COUNT }, (_, i) => {
      const voice = m.pads[i];
      const info = voice ? getClassicVoice(machine, voice) : undefined;
      if (!voice || !info) return defaultPadSettings();
      return {
        ...defaultPadSettings(),
        sample: { ...classicSample(machine, voice), name: info.label },
        gainDb: info.gainDb ?? 0,
        chokeGroup: info.choke ?? 0,
        mode: info.mode ?? 'oneshot',
      };
    }),
  };
}
