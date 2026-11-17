import * as Tone from 'tone';
import { clamp, safeDispose } from './voices/common';

export interface ReverbOptions {
  /** RT60-ish decay time (s). */
  decay: number;
  /** Pre-delay (s) -- keeps the dry attack clear. */
  preDelay: number;
  /** 0..1 high-frequency damping over the tail (0 = bright plate, 1 = dark room). */
  damping: number;
  /** 0..1 wet/dry (equal power). */
  wet: number;
}

const DEFAULTS: ReverbOptions = { decay: 1.8, preDelay: 0.022, damping: 0.55, wet: 0 };

/** Small deterministic PRNG so every render (live / export) gets the same tail. */
function xorshift(seed: number) {
  let s = seed >>> 0 || 1;
  return () => {
    s ^= s << 13;
    s ^= s >>> 17;
    s ^= s << 5;
    return ((s >>> 0) / 4294967295) * 2 - 1;
  };
}

/**
 * Stereo plate/room impulse response: decorrelated noise with an exponential
 * decay (-60 dB at `decay`), progressive high-frequency damping, a handful of
 * early reflections and a pre-delay. Generated synchronously (no
 * OfflineContext), so it is ready immediately -- also inside Tone.Offline
 * exports, where Tone.Reverb's async IR generation used to be silently missing.
 */
export function createImpulseResponse(ctx: BaseAudioContext, opts: Partial<ReverbOptions> = {}): AudioBuffer {
  const o = { ...DEFAULTS, ...opts };
  const sr = ctx.sampleRate;
  const decay = clamp(o.decay, 0.2, 8);
  const pre = Math.floor(clamp(o.preDelay, 0, 0.2) * sr);
  const len = pre + Math.ceil(decay * sr);
  const buf = ctx.createBuffer(2, len, sr);
  const damping = clamp(o.damping, 0, 1);

  for (let ch = 0; ch < 2; ch++) {
    const data = buf.getChannelData(ch);
    const rnd = xorshift(ch === 0 ? 0x2545f491 : 0x9e3779b9);
    let lp = 0;
    for (let i = pre; i < len; i++) {
      const t = (i - pre) / sr;
      const env = Math.exp((-6.91 * t) / decay);
      // One-pole lowpass whose cutoff falls over the tail (air absorption).
      const a = Math.exp((-4.5 * damping * t) / decay) * 0.9 + 0.05;
      lp += a * (rnd() - lp);
      const fadeIn = Math.min(1, (i - pre) / (0.004 * sr));
      data[i] = lp * env * fadeIn;
    }
    // Early reflections (different per side for width).
    const taps = ch === 0 ? [0.007, 0.013, 0.021, 0.034, 0.047] : [0.009, 0.017, 0.026, 0.039, 0.053];
    taps.forEach((tap, k) => {
      const idx = pre + Math.floor(tap * sr);
      if (idx < len) data[idx] += (0.55 - k * 0.09) * (k % 2 ? -1 : 1);
    });
  }
  return buf;
}

/**
 * Convolution reverb with an equal-power wet/dry and a low-cut on the send
 * (no muddy low-end wash). API mirrors a Tone effect where the app needs it:
 * `wet` is a Signal (automation works), `connect`, `dispose`.
 */
export class RoomReverb {
  readonly input: Tone.Gain;
  readonly output: Tone.Gain;
  readonly wet: Tone.Signal<'normalRange'>;
  private readonly xfade: Tone.CrossFade;
  private readonly lowCut: Tone.Filter;
  private convolver: Tone.Convolver;
  private opts: ReverbOptions;

  constructor(opts: Partial<ReverbOptions> = {}) {
    this.opts = { ...DEFAULTS, ...opts };
    this.input = new Tone.Gain(1);
    this.output = new Tone.Gain(1);
    this.xfade = new Tone.CrossFade(clamp(this.opts.wet, 0, 1));
    this.wet = this.xfade.fade;
    this.lowCut = new Tone.Filter({ type: 'highpass', frequency: 200, rolloff: -12 });
    this.convolver = this.makeConvolver();

    this.input.connect(this.xfade.a);
    this.input.connect(this.lowCut);
    this.lowCut.connect(this.convolver);
    this.convolver.connect(this.xfade.b);
    this.xfade.connect(this.output);
  }

  connect(node: Tone.InputNode): this {
    this.output.connect(node);
    return this;
  }

  disconnect(): this {
    this.output.disconnect();
    return this;
  }

  /** Rebuild the tail with a new decay / damping. */
  setShape(opts: Partial<Omit<ReverbOptions, 'wet'>>): void {
    this.opts = { ...this.opts, ...opts };
    const next = this.makeConvolver();
    this.lowCut.disconnect(this.convolver);
    this.lowCut.connect(next);
    next.connect(this.xfade.b);
    safeDispose(this.convolver);
    this.convolver = next;
  }

  dispose(): void {
    safeDispose(this.convolver, this.lowCut, this.xfade, this.input, this.output);
  }

  private makeConvolver(): Tone.Convolver {
    const raw = Tone.getContext().rawContext as unknown as BaseAudioContext;
    const ir = createImpulseResponse(raw, this.opts);
    return new Tone.Convolver({ url: new Tone.ToneAudioBuffer(ir), normalize: true });
  }
}
