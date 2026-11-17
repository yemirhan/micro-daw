import * as Tone from 'tone';

/**
 * Master bus, inserted on a context's Destination so EVERYTHING (live synth,
 * drum pads, arrangement tracks, metronome, previews, sample audition) passes
 * through it:
 *
 *   Destination input -> bus compressor (soft-knee glue, catches dense chords)
 *                     -> soft clipper (zero-latency true-peak ceiling, ~ -0.2 dBFS)
 *                     -> speakers
 *
 * Only ONE DynamicsCompressor is used on purpose: in Chromium each one adds a
 * fixed 6 ms look-ahead delay, which you feel when playing live. The
 * waveshaper ceiling makes it impossible for the output to exceed 0 dBFS
 * (inputs up to +6 dBFS are rounded off smoothly instead of hard-clipping).
 */

const CLIP_KNEE = 0.85;
const CLIP_CEILING = 0.98;

/** Soft clipper transfer curve; domain is the shaper input u in [-1, 1] == signal x in [-2, 2]. */
export function softClip(x: number): number {
  const a = Math.abs(x);
  if (a <= CLIP_KNEE) return x;
  const room = CLIP_CEILING - CLIP_KNEE;
  return Math.sign(x) * (CLIP_KNEE + room * Math.tanh((a - CLIP_KNEE) / room));
}

export interface MasterBus {
  compressor: Tone.Compressor;
  dispose(): void;
}

const installed = new WeakMap<object, MasterBus>();

/** Create the master processing nodes and chain them onto `destination`. */
export function installMasterBus(destination: Tone.ToneAudioNode = Tone.getDestination()): MasterBus {
  const existing = installed.get(destination);
  if (existing) return existing;

  const compressor = new Tone.Compressor({
    threshold: -10,
    ratio: 3,
    knee: 10,
    attack: 0.003,
    release: 0.2,
  });
  const preClip = new Tone.Gain(0.5);
  // No oversampling on purpose: the up/down-sampling filters ring past the
  // curve's ceiling (measured +0.3 dBFS); without them the ceiling is exact.
  const clipper = new Tone.WaveShaper((u: number) => softClip(u * 2), 4096);

  (destination as unknown as { chain: (...nodes: Tone.ToneAudioNode[]) => void }).chain(compressor, preClip, clipper);

  const bus: MasterBus = {
    compressor,
    dispose: () => {
      for (const n of [compressor, preClip, clipper]) {
        try {
          n.dispose();
        } catch {
          /* already disposed */
        }
      }
      installed.delete(destination);
    },
  };
  installed.set(destination, bus);
  return bus;
}

/** Idempotent: install on the current (real-time) context's destination. */
export function ensureMasterBus(): MasterBus {
  return installMasterBus(Tone.getDestination());
}
