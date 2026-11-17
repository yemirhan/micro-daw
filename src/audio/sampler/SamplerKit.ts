import * as Tone from 'tone';
import { PAD_COUNT, type KitState, type PadIndex, type PadSample, type PadSettings } from '@/types/sampler';
import { clamp, normVelocity, safeDispose, velocityGain, withContext } from '@/audio/voices/common';
import { defaultPadSettings } from './factoryKit';
import { getCachedPadBuffer, getReversedBuffer, loadPadBuffer, padSourceKey } from './bufferCache';

/** Voices per pad before the oldest is cut (keeps rolls / note repeat bounded). */
export const MAX_VOICES_PER_PAD = 8;
/** Fade used for choke, voice stealing and stopAll. */
export const CHOKE_FADE = 0.005;
/** At or above this cutoff the pad's low-pass is bypassed. */
export const FILTER_BYPASS_HZ = 20000;
/** Fade-in when a slice starts mid-waveform (avoids a click). */
const SLICE_FADE_IN = 0.002;
const PARAM_RAMP = 0.02;
const VELOCITY_SENSITIVITY = 0.85;

interface PadChannel {
  /** Voices connect here; gain = pad gainDb. */
  amp: Tone.Gain;
  filter: Tone.Filter | null;
  filtered: boolean;
  panner: Tone.Panner;
}

interface Voice {
  pad: PadIndex;
  source: Tone.ToneBufferSource;
  gate: boolean;
  /** When the fade-out to silence begins. */
  stopAt: number;
  /** When the voice is fully silent. */
  end: number;
}

/** Pure: the part of a buffer a pad plays, in buffer seconds and real seconds. */
export function computeSlice(
  pad: Pick<PadSettings, 'startSec' | 'endSec' | 'pitch' | 'reverse'>,
  bufferDuration: number,
): { offset: number; length: number; rate: number; realDuration: number } | null {
  const start = clamp(pad.startSec || 0, 0, bufferDuration);
  const end = pad.endSec == null ? bufferDuration : clamp(pad.endSec, 0, bufferDuration);
  const length = end - start;
  if (!(length > 0.0005)) return null;
  const rate = Math.pow(2, clamp(pad.pitch || 0, -24, 24) / 12);
  return {
    offset: pad.reverse ? bufferDuration - end : start,
    length,
    rate,
    realDuration: length / rate,
  };
}

const sampleKey = (s: PadSample | null) => (s ? padSourceKey(s.source) : null);
const dbToGain = (db: number) => Math.pow(10, db / 20);

/**
 * 16-pad sample player. One ToneBufferSource per hit, through a per-pad
 * gain -> (low-pass) -> panner chain into `output`. Every event is scheduled
 * at an explicit time, so the same class drives live pads, arrangement
 * playback and Tone.Offline export (await `ready` before rendering).
 */
export class SamplerKit {
  readonly output: Tone.Gain;

  private readonly context: Tone.BaseContext;
  private kit: KitState;
  private readonly channels: PadChannel[];
  private readonly buffers: (AudioBuffer | null)[] = new Array(PAD_COUNT).fill(null);
  private readonly bufferKeys: (string | null)[] = new Array(PAD_COUNT).fill(null);
  private readonly voices: Voice[][] = Array.from({ length: PAD_COUNT }, () => []);
  private readonly loads = new Set<Promise<void>>();
  private disposed = false;

  constructor(kit: KitState) {
    this.context = Tone.getContext();
    this.kit = copyKit(kit);
    this.output = new Tone.Gain(1);
    this.channels = Array.from({ length: PAD_COUNT }, () => {
      const amp = new Tone.Gain(1);
      const panner = new Tone.Panner(0);
      amp.connect(panner);
      panner.connect(this.output);
      return { amp, panner, filter: null, filtered: false };
    });
    for (let i = 0; i < PAD_COUNT; i++) {
      this.applyChannel(i, true);
      this.loadPad(i);
    }
  }

  /** Resolves when every pad buffer currently loading has settled (failures are logged). */
  get ready(): Promise<void> {
    return Promise.all([...this.loads]).then(() => undefined);
  }

  connect(node: Tone.InputNode): this {
    this.output.connect(node);
    return this;
  }

  getKit(): KitState {
    return this.kit;
  }

  /** Swap in a new kit state: only pads whose sample changed are reloaded. */
  setKit(kit: KitState): void {
    if (this.disposed) return;
    const prev = this.kit;
    this.kit = copyKit(kit);
    for (let i = 0; i < PAD_COUNT; i++) {
      if (sampleKey(prev.pads[i]?.sample ?? null) !== sampleKey(this.kit.pads[i]?.sample ?? null)) {
        this.stopPad(i, this.now(), CHOKE_FADE);
        this.loadPad(i);
      }
      this.applyChannel(i, false);
    }
  }

  /** True when the pad has a sample whose buffer is loaded. */
  isPadReady(pad: PadIndex): boolean {
    return !!this.buffers[pad];
  }

  trigger(pad: PadIndex, velocity: number, time: number): void {
    if (this.disposed || !isPad(pad)) return;
    const settings = this.kit.pads[pad];
    const buffer = this.buffers[pad];
    if (!settings?.sample || !buffer) return;
    const vel = normVelocity(velocity);
    if (vel <= 0) return;
    const slice = computeSlice(settings, buffer.duration);
    if (!slice) return;

    if (settings.chokeGroup > 0) {
      for (let p = 0; p < PAD_COUNT; p++) {
        if (p !== pad && this.kit.pads[p]?.chokeGroup === settings.chokeGroup) this.stopPad(p, time, CHOKE_FADE);
      }
    }

    const list = this.prune(pad, time);
    while (list.length >= MAX_VOICES_PER_PAD) {
      const oldest = list.shift();
      if (oldest) this.stopVoice(oldest, time, CHOKE_FADE);
    }

    const decay = clamp(settings.decay, 0.005, 4);
    const fadeOut = Math.min(decay, slice.realDuration);
    const gain = velocityGain(vel, VELOCITY_SENSITIVITY);
    const voice = withContext(this.context, () => {
      const v: Voice = {
        pad,
        gate: settings.mode === 'gate',
        stopAt: time + slice.realDuration - fadeOut,
        end: time + slice.realDuration,
        source: null as unknown as Tone.ToneBufferSource,
      };
      v.source = new Tone.ToneBufferSource({
        url: settings.reverse ? getReversedBuffer(buffer) : buffer,
        playbackRate: slice.rate,
        fadeIn: slice.offset > 0 ? SLICE_FADE_IN : 0,
        fadeOut,
        curve: 'linear',
        onended: () => this.forget(v),
      });
      return v;
    });
    try {
      voice.source.connect(this.channels[pad].amp);
      voice.source.start(time, slice.offset, undefined, gain);
      voice.source.stop(voice.stopAt);
      list.push(voice);
    } catch (err) {
      console.warn('[sampler] trigger failed', err);
      safeDispose(voice.source);
    }
  }

  /** Gate release: fades every held voice of the pad over its decay. */
  release(pad: PadIndex, time: number): void {
    if (this.disposed || !isPad(pad)) return;
    const decay = clamp(this.kit.pads[pad]?.decay ?? 0.05, 0.005, 4);
    for (const v of this.prune(pad, time)) {
      if (!v.gate || time >= v.stopAt) continue;
      // Never fade past the slice end (the buffer keeps playing beyond it).
      this.stopVoice(v, time, Math.min(decay, v.end - time));
    }
  }

  stopAll(time?: number): void {
    const t = time ?? this.now();
    for (let p = 0; p < PAD_COUNT; p++) this.stopPad(p, t, CHOKE_FADE);
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    for (const list of this.voices) {
      for (const v of list) safeDispose(v.source);
      list.length = 0;
    }
    for (const ch of this.channels) safeDispose(ch.amp, ch.filter, ch.panner);
    safeDispose(this.output);
    this.loads.clear();
  }

  // -------------------------------------------------------------------------

  private now(): number {
    return this.context.now();
  }

  private loadPad(pad: PadIndex): void {
    const sample = this.kit.pads[pad]?.sample ?? null;
    const key = sampleKey(sample);
    this.bufferKeys[pad] = key;
    this.buffers[pad] = null;
    if (!sample || !key) return;
    const cached = getCachedPadBuffer(sample.source);
    if (cached) {
      this.buffers[pad] = cached;
      return;
    }
    const load = loadPadBuffer(sample.source)
      .then((buffer) => {
        if (!this.disposed && this.bufferKeys[pad] === key) this.buffers[pad] = buffer;
      })
      .catch((err) => {
        console.warn(`[sampler] could not load pad ${pad + 1} (${sample.name})`, err);
      })
      .finally(() => this.loads.delete(load));
    this.loads.add(load);
  }

  /** Sync the pad's live chain (gain, pan, filter) with its settings. */
  private applyChannel(pad: PadIndex, immediate: boolean): void {
    const s = this.kit.pads[pad];
    const ch = this.channels[pad];
    if (!s || !ch) return;
    const gain = dbToGain(clamp(s.gainDb, -60, 12));
    const pan = clamp(s.pan, -1, 1);
    if (immediate) {
      ch.amp.gain.value = gain;
      ch.panner.pan.value = pan;
    } else {
      ch.amp.gain.rampTo(gain, PARAM_RAMP);
      ch.panner.pan.rampTo(pan, PARAM_RAMP);
    }

    const cutoff = clamp(s.filterCutoff, 20, FILTER_BYPASS_HZ);
    const wantFilter = cutoff < FILTER_BYPASS_HZ;
    if (wantFilter && !ch.filter) {
      ch.filter = withContext(this.context, () =>
        new Tone.Filter({ type: 'lowpass', frequency: cutoff, rolloff: -12, Q: 0.707 }),
      );
      ch.filter.connect(ch.panner);
    }
    if (wantFilter && ch.filter) {
      if (ch.filtered && !immediate) ch.filter.frequency.rampTo(cutoff, PARAM_RAMP);
      else ch.filter.frequency.value = cutoff;
    }
    if (wantFilter !== ch.filtered) {
      ch.amp.disconnect();
      ch.amp.connect(wantFilter && ch.filter ? ch.filter : ch.panner);
      ch.filtered = wantFilter;
    }
  }

  /** Drop voices that are silent by `time`; returns the pad's live list. */
  private prune(pad: PadIndex, time: number): Voice[] {
    const list = this.voices[pad];
    for (let i = list.length - 1; i >= 0; i--) {
      if (list[i].end <= time) list.splice(i, 1);
    }
    return list;
  }

  private stopPad(pad: PadIndex, time: number, fade: number): void {
    for (const v of this.prune(pad, time)) this.stopVoice(v, time, fade);
  }

  private stopVoice(v: Voice, time: number, fade: number): void {
    if (v.end <= time || (time >= v.stopAt && v.end <= time + fade)) return;
    try {
      v.source.fadeOut = Math.max(0.001, fade);
      v.source.stop(time);
      v.stopAt = time;
      v.end = time + fade;
    } catch {
      /* already ended / disposed */
    }
  }

  private forget(v: Voice): void {
    const list = this.voices[v.pad];
    const i = list.indexOf(v);
    if (i >= 0) list.splice(i, 1);
  }
}

function isPad(pad: number): boolean {
  return Number.isInteger(pad) && pad >= 0 && pad < PAD_COUNT;
}

/** Snapshot of the pads the kit was given (so later in-place edits still diff). */
function copyKit(kit: KitState): KitState {
  return {
    ...kit,
    pads: Array.from({ length: PAD_COUNT }, (_, i) => {
      const p = kit.pads[i];
      return p ? { ...p, sample: p.sample ? { ...p.sample, source: { ...p.sample.source } } : null } : defaultPadSettings();
    }),
  };
}

