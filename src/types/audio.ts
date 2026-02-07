import type { RecursivePartial } from 'tone/build/esm/core/util/Interface';
import type { OmniOscillatorOptions } from 'tone/build/esm/source/oscillator/OscillatorInterface';

export interface SynthPreset {
  name: string;
  oscillator: RecursivePartial<OmniOscillatorOptions>;
  envelope: {
    attack: number;
    decay: number;
    sustain: number;
    release: number;
  };
}
