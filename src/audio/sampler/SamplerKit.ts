import * as Tone from 'tone';
import { PAD_COUNT, type KitState, type PadIndex, type PadSample, type PadSettings } from '@/types/sampler';
import { clamp, normVelocity, safeDispose, velocityGain, withContext } from '@/audio/voices/common';
import { defaultPadSettings } from './factoryKit';
import { getCachedPadBuffer, getReversedBuffer, loadPadBuffer, onPadBuffersInvalidated, padSourceKey } from './bufferCache';

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
  /** Per-voice amplitude envelope (velocity, fades, choke/steal/release). */
  env: Tone.Gain;
  gate: boolean;
  /** When the voice starts sounding. */
  start: number;
  /** When the fade-out to silence begins. */
  stopAt: number;
  /** When the voice is fully silent. */
  end: number;
  /** Nodes released (after a choke/steal or the natural end). */
  disposed: boolean;
}

/** One automation step of a voice envelope: `set` = setValueAtTime, `ramp` = linearRampToValueAtTime. */
export interface EnvelopePoint {
  kind: 'set' | 'ramp';
  value: number;
  time: number;
}

/**
 * Pure: the gain automation of a voice, in time order. Optional fade-in
 * (slices that start mid-waveform), hold at `gain`, then a linear fade to
 * silence over the last `fadeOut` seconds (never before the fade-in is done).
 */
export function voiceEnvelope(start: number, duration: number, gain: number, fadeIn: number, fadeOut: number): EnvelopePoint[] {
  const end = start + Math.max(0, duration);
  const inEnd = Math.min(end, start + Math.max(0, fadeIn));
  const points: EnvelopePoint[] = [];
  if (inEnd > start) {
    points.push({ kind: 'set', value: 0, time: start }, { kind: 'ramp', value: gain, time: inEnd });
  } else {
    points.push({ kind: 'set', value: gain, time: start });
  }
  const fadeStart = Math.max(inEnd, end - Math.max(0, fadeOut));
  if (fadeStart > inEnd) points.push({ kind: 'set', value: gain, time: fadeStart });
  if (end > fadeStart) points.push({ kind: 'ramp', value: 0, time: end });
  else points.push({ kind: 'set', value: 0, time: end });
  return points;
}

/** Extra time after a voice is silent before its nodes are released. */
const CLEANUP_MARGIN = 0.05;

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

/**
 * A voice started by `trigger`, as handed to a `collectVoices` scope. Lets
 * the live path cancel hits it handed out ahead of time (note repeat) when
 * the pad comes up before they sound, whatever the pad's mode.
 */
export interface VoiceHandle {
  readonly pad: PadIndex;
  /** AudioContext time the voice starts sounding. */
  readonly start: number;
  /** Drop the voice if it has not started by the context's current time. Returns true when it was cancelled. */
  cancelIfPending(): boolean;
}

/** Collector of the innermost active `collectVoices` scope. */
let voiceCollector: ((h: VoiceHandle) => void) | null = null;

/**
 * Run `fn` and return the voices any SamplerKit `trigger` started during it
 * (synchronously). Used by the live pad path, which reaches the sampler
 * through a sink that hides it.
 */
export function collectVoices(fn: () => void): VoiceHandle[] {
  const out: VoiceHandle[] = [];
  const prev = voiceCollector;
  voiceCollector = (h) => out.push(h);
  try {
    fn();
  } finally {
    voiceCollector = prev;
  }
  return out;
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
  /** Unsubscribes from pack invalidations (bufferCache.onPadBuffersInvalidated). */
  private readonly stopInvalidationWatch: () => void;
  /**
   * Rendering in a Tone.OfflineContext (WAV export). Tone runs the whole
   * offline clock (context timeouts and sources' onended included) BEFORE
   * any audio is rendered, so voice nodes must never be disposed there: that
   * would cut every voice out of the render. The graph goes away with the
   * offline context.
   */
  private readonly offline: boolean;

  constructor(kit: KitState) {
    this.context = Tone.getContext();
    this.offline = isOfflineContext(this.context);
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
    // A pack was installed / updated / removed (review P2): its sounds' cache
    // keys changed, so the pads that play them reload (a removed pack leaves
    // them silent until it is installed again).
    this.stopInvalidationWatch = onPadBuffersInvalidated(() => this.reloadStalePads());
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

  /**
   * Swap in a new kit state: only pads whose sample changed are reloaded (or
   * whose loaded buffer is stale: a pack sound's key includes the pack's
   * generation, which changes on install / update / uninstall).
   */
  setKit(kit: KitState): void {
    if (this.disposed) return;
    const prev = this.kit;
    this.kit = copyKit(kit);
    for (let i = 0; i < PAD_COUNT; i++) {
      const next = sampleKey(this.kit.pads[i]?.sample ?? null);
      if (sampleKey(prev.pads[i]?.sample ?? null) !== next) {
        this.stopPad(i, this.now(), CHOKE_FADE, true);
        this.loadPad(i);
      } else if (this.bufferKeys[i] !== next) {
        this.loadPad(i);
      }
      this.applyChannel(i, false);
    }
  }

  /** Reload every pad whose loaded buffer key no longer matches its sample (pack invalidated). */
  private reloadStalePads(): void {
    if (this.disposed) return;
    for (let i = 0; i < PAD_COUNT; i++) {
      if (this.bufferKeys[i] !== sampleKey(this.kit.pads[i]?.sample ?? null)) this.loadPad(i);
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

    const list = this.prune(pad);
    while (list.length >= MAX_VOICES_PER_PAD) {
      const oldest = list.shift();
      if (!oldest) break;
      if (oldest.start > time) this.cancelVoice(oldest);
      else this.stopVoice(oldest, time, CHOKE_FADE);
    }

    const decay = clamp(settings.decay, 0.005, 4);
    const fadeOut = Math.min(decay, slice.realDuration);
    const gain = velocityGain(vel, VELOCITY_SENSITIVITY);
    const end = time + slice.realDuration;
    const voice = withContext(this.context, (): Voice => ({
      pad,
      gate: settings.mode === 'gate',
      start: time,
      stopAt: end - fadeOut,
      end,
      disposed: false,
      env: new Tone.Gain(0),
      // No Tone fades on the source: its own stop() re-schedules them (a
      // second stop() cancels a running fade). All shaping is on `env`.
      source: new Tone.ToneBufferSource({
        url: settings.reverse ? getReversedBuffer(buffer) : buffer,
        playbackRate: slice.rate,
        fadeIn: 0,
        fadeOut: 0,
        curve: 'linear',
      }),
    }));
    // Tone disposes the source itself after onended (real-time context).
    voice.source.onended = () => this.releaseVoice(voice, true);
    try {
      const g = voice.env.gain;
      for (const p of voiceEnvelope(time, slice.realDuration, gain, slice.offset > 0 ? SLICE_FADE_IN : 0, fadeOut)) {
        if (p.kind === 'set') g.setValueAtTime(p.value, p.time);
        else g.linearRampToValueAtTime(p.value, p.time);
      }
      voice.env.connect(this.channels[pad].amp);
      voice.source.connect(voice.env);
      voice.source.start(time, slice.offset, undefined, 1);
      // The only stop() call a voice ever gets: chokes fade `env` instead.
      voice.source.stop(end);
      list.push(voice);
      voiceCollector?.({
        pad,
        start: time,
        cancelIfPending: () => this.cancelIfPending(voice),
      });
    } catch (err) {
      console.warn('[sampler] trigger failed', err);
      this.releaseVoice(voice);
    }
  }

  /**
   * Gate release: fades every held gate voice of the pad over its decay.
   * Gate voices scheduled after `time` (e.g. note-repeat hits inside the
   * look-ahead) are cancelled: the pad is already up when they would start.
   */
  release(pad: PadIndex, time: number): void {
    if (this.disposed || !isPad(pad)) return;
    const decay = clamp(this.kit.pads[pad]?.decay ?? 0.05, 0.005, 4);
    for (const v of [...this.prune(pad)]) {
      if (!v.gate) continue;
      if (v.start > time) {
        this.cancelVoice(v);
        continue;
      }
      if (time >= v.stopAt) continue;
      // Never fade past the slice end (the buffer keeps playing beyond it).
      this.stopVoice(v, time, Math.min(decay, v.end - time));
    }
  }

  /** Fade out every voice by `time` (default now); voices scheduled later are cancelled. */
  stopAll(time?: number): void {
    const t = time ?? this.now();
    for (let p = 0; p < PAD_COUNT; p++) this.stopPad(p, t, CHOKE_FADE, true);
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.stopInvalidationWatch();
    for (const list of this.voices) {
      for (const v of [...list]) this.releaseVoice(v);
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

  /**
   * Drop voices that are already silent (by the context's CURRENT time, never
   * an event time: a gate release scheduled in the future must still find a
   * voice that ends before it, or Stop / choke could no longer silence it).
   * Returns the pad's live list. Offline renders never prune (the offline
   * clock sits at 0 while scheduling): voice stealing bounds the list.
   */
  private prune(pad: PadIndex): Voice[] {
    const list = this.voices[pad];
    const now = this.currentTime();
    for (let i = list.length - 1; i >= 0; i--) {
      if (list[i].end <= now) list.splice(i, 1);
    }
    return list;
  }

  /** What is being heard now (no look-ahead, unlike `now()`). */
  private currentTime(): number {
    const t = (this.context as unknown as { currentTime?: number }).currentTime;
    return typeof t === 'number' && Number.isFinite(t) ? t : this.now();
  }

  /** Cancel a voice that has not started sounding yet (see VoiceHandle). */
  private cancelIfPending(v: Voice): boolean {
    if (v.disposed || v.start <= this.currentTime()) return false;
    this.cancelVoice(v);
    return true;
  }

  /**
   * Fade out the pad's voices at `time`. Voices that start after `time` are
   * left alone (a choke only silences what was already sounding when the
   * choking hit plays), unless `includeFuture` cancels them too.
   */
  private stopPad(pad: PadIndex, time: number, fade: number, includeFuture = false): void {
    for (const v of [...this.prune(pad)]) {
      if (v.start > time) {
        if (includeFuture) this.cancelVoice(v);
        continue;
      }
      this.stopVoice(v, time, fade);
    }
  }

  /**
   * Fade a sounding voice to silence over `fade` seconds from `time`, on its
   * own envelope: cancelAndHoldAtTime keeps the current level (mid-fade
   * included), so a choke can never jump back to full level. The source is
   * never stopped a second time; its nodes are released once silent.
   */
  private stopVoice(v: Voice, time: number, fade: number): void {
    if (v.disposed || v.start > time || v.end <= time) return;
    const f = Math.max(0.001, fade);
    // Already fading out to silence sooner than this stop would.
    if (time >= v.stopAt && v.end <= time + f) return;
    try {
      const g = v.env.gain;
      g.cancelAndHoldAtTime(time);
      g.linearRampToValueAtTime(0, time + f);
      v.stopAt = time;
      v.end = time + f;
      this.scheduleCleanup(v);
    } catch {
      /* already disposed */
    }
  }

  /** Drop a voice that has not started yet. */
  private cancelVoice(v: Voice): void {
    if (this.offline && !v.disposed) {
      // Its nodes are kept (see `offline`): mute it for good instead.
      try {
        v.env.gain.cancelScheduledValues(0);
        v.env.gain.setValueAtTime(0, 0);
      } catch {
        /* already gone */
      }
    }
    this.forget(v);
    this.releaseVoice(v);
  }

  /** Release a voice's nodes once it is silent (context clock, so Offline renders stay intact). */
  private scheduleCleanup(v: Voice): void {
    const ctx = this.context as unknown as { setTimeout?: (fn: () => void, sec: number) => number; currentTime?: number };
    if (typeof ctx.setTimeout !== 'function') return; // the source's own stop() still cleans up at the slice end
    const delay = Math.max(0, v.end - (ctx.currentTime ?? this.now())) + CLEANUP_MARGIN;
    try {
      ctx.setTimeout(() => {
        if (this.context.currentTime >= v.end) this.releaseVoice(v);
      }, delay);
    } catch {
      /* context closed */
    }
  }

  private releaseVoice(v: Voice, sourceEnded = false): void {
    this.forget(v);
    if (v.disposed) return;
    v.disposed = true;
    if (this.offline) return;
    if (sourceEnded) safeDispose(v.env);
    else safeDispose(v.source, v.env);
  }

  private forget(v: Voice): void {
    const list = this.voices[v.pad];
    const i = list.indexOf(v);
    if (i >= 0) list.splice(i, 1);
  }
}

/** True for a Tone.OfflineContext (or any context wrapping an OfflineAudioContext). */
export function isOfflineContext(ctx: Tone.BaseContext): boolean {
  const OfflineCtx = (Tone as unknown as { OfflineContext?: new (...args: never[]) => unknown }).OfflineContext;
  if (typeof OfflineCtx === 'function' && ctx instanceof OfflineCtx) return true;
  const raw = (ctx as unknown as { rawContext?: unknown }).rawContext;
  return typeof OfflineAudioContext !== 'undefined' && raw instanceof OfflineAudioContext;
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

