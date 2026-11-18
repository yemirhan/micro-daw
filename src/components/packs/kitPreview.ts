/**
 * "Preview kit": a short built-in 2-bar groove played through a temporary
 * SamplerKit (installed kits load through the pad buffer cache; pre-install
 * previews inject a remote-buffer loader), scheduled on the AudioContext clock (not the Transport, so it
 * never touches the arrangement). One preview at a time.
 */
import * as Tone from 'tone';
import { SamplerKit } from '@/audio/sampler/SamplerKit';
import { PAD_COUNT, type KitState, type PadIndex } from '@/types/sampler';

export type DrumRole = 'kick' | 'snare' | 'clap' | 'closedHat' | 'openHat' | 'perc';

/** Abbreviation as its own token: "BD01", "808 bd", "sd-2" (letters may not touch it). */
const tok = (t: string) => new RegExp(`(^|[^a-z])(${t})([^a-z]|$)`, 'i');

const ROLE_PATTERNS: [DrumRole, RegExp[]][] = [
  // Order matters: open hat before closed hat (both say "hat").
  ['openHat', [/open/i, tok('oh|ohh')]],
  ['closedHat', [/hi\s*-?\s*hat|closed|pedal/i, tok('ch|chh|phh|hh|hat')]],
  ['kick', [/kick|bass\s*drum/i, tok('bd|kik|kk')]],
  ['snare', [/snare/i, tok('sd|snr|sn')]],
  ['clap', [/clap/i, tok('cp')]],
  ['perc', [/cowbell|clave|shaker|maraca|conga|bongo|tom|rim|tamb|perc|cabasa|agogo/i, tok('rs|cb|cl|ma|lt|mt|ht')]],
];

/** Role of a pad from its sample name (null = unknown). */
export function classifyPadName(name: string): DrumRole | null {
  const n = name.replace(/[_.]+/g, ' ');
  for (const [role, res] of ROLE_PATTERNS) if (res.some((re) => re.test(n))) return role;
  return null;
}

/** First pad for each role; falls back to the usual layout (0 kick, 1 snare, 2 closed hat, 3 open hat, 4 clap). */
export function kitRoles(kit: KitState): Partial<Record<DrumRole, PadIndex>> {
  const roles: Partial<Record<DrumRole, PadIndex>> = {};
  kit.pads.forEach((p, i) => {
    if (!p.sample) return;
    const role = classifyPadName(p.sample.name);
    if (role && roles[role] === undefined) roles[role] = i;
  });
  const has = (i: number) => !!kit.pads[i]?.sample;
  const fallback: [DrumRole, number][] = [['kick', 0], ['snare', 1], ['closedHat', 2], ['openHat', 3], ['clap', 4]];
  const used = new Set(Object.values(roles));
  for (const [role, pad] of fallback) {
    if (roles[role] === undefined && has(pad) && !used.has(pad)) {
      roles[role] = pad;
      used.add(pad);
    }
  }
  return roles;
}

export interface DemoHit {
  /** 16th-note step from the start, 0..31. */
  step: number;
  pad: PadIndex;
  velocity: number;
}

export const DEMO_STEPS = 32;
export const DEMO_BPM = 96;

/** A 2-bar groove for the kit (16th steps). Empty when the kit has no sounds. */
export function demoPattern(kit: KitState): DemoHit[] {
  const r = kitRoles(kit);
  const hits: DemoHit[] = [];
  const add = (pad: PadIndex | undefined, steps: number[], velocity: number) => {
    if (pad === undefined) return;
    for (const step of steps) hits.push({ step, pad, velocity });
  };
  add(r.kick, [0, 6, 10, 16, 22, 26, 27], 0.95);
  add(r.snare ?? r.clap, [4, 12, 20, 28], 0.9);
  if (r.snare !== undefined) add(r.clap, [28], 0.7);
  const hatSteps = Array.from({ length: 16 }, (_, i) => i * 2).filter((s) => s !== 14 && s !== 30);
  add(r.closedHat, hatSteps, 0.6);
  add(r.closedHat, [7, 23], 0.35);
  add(r.openHat, [14, 30], 0.7);
  add(r.perc, [11, 25], 0.55);
  if (hits.length === 0) {
    // No recognisable roles: walk the first sounding pads.
    const pads = kit.pads.map((p, i) => (p.sample ? i : -1)).filter((i) => i >= 0).slice(0, 8);
    pads.forEach((pad, k) => hits.push({ step: k * 4, pad, velocity: 0.85 }));
  }
  return hits.sort((a, b) => a.step - b.step || a.pad - b.pad);
}

/** Seconds per 16th step. */
export const stepSeconds = (bpm: number) => 60 / bpm / 4;

type Listener = (playingKitId: string | null) => void;

/** The running preview, for UIs that light pads in time with it. */
export interface PreviewSession {
  id: string;
  /** AudioContext time of step 0. */
  t0: number;
  stepDur: number;
  hits: DemoHit[];
}

/** Pads sounding at context time `now` (a hit lights its pad for one step). */
export function padsAt(session: PreviewSession, now: number): Set<PadIndex> {
  const step = Math.floor((now - session.t0) / session.stepDur);
  const out = new Set<PadIndex>();
  if (step < 0) return out;
  for (const h of session.hits) if (h.step === step) out.add(h.pad);
  return out;
}

/** Loads one pad's buffer from outside the kit's own sources (null = no sound for that pad). */
export type PadBufferLoader = (pad: PadIndex) => Promise<AudioBuffer | null>;

export interface KitPreviewOptions {
  /**
   * Pre-install preview: pad buffers come from here (e.g. a catalog pack's
   * remote files fetched through the preview IPC) instead of the kit's sources.
   * Played through the same SamplerKit as installed kits (choke, decay,
   * reverse, pan, filter, pitch-aware length: review M3).
   */
  loadBuffer?: PadBufferLoader;
}

class KitPreviewPlayer {
  private kit: SamplerKit | null = null;
  private volume: Tone.Volume | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private token = 0;
  private playingId: string | null = null;
  private current: PreviewSession | null = null;
  private listeners = new Set<Listener>();

  /** Id of the kit preview currently playing (the caller's id, e.g. "tr808" or "pack/kit"). */
  get playing(): string | null {
    return this.playingId;
  }

  get session(): PreviewSession | null {
    return this.current;
  }

  subscribe(cb: Listener): () => void {
    this.listeners.add(cb);
    return () => {
      this.listeners.delete(cb);
    };
  }

  /**
   * Play the demo groove once on `kit`. Stops any running preview first.
   * Resolves with an error message when the preview could not play, or played
   * with some sounds missing (null = it played fully, or was superseded by
   * another play / stop).
   */
  async play(id: string, kit: KitState, opts: KitPreviewOptions = {}): Promise<string | null> {
    this.stop();
    const token = ++this.token;
    this.setPlaying(id);
    let voices: SamplerKit | null = null;
    let volume: Tone.Volume | null = null;
    // Superseded (another play / stop ran while we awaited): release what we built.
    const superseded = () => {
      if (token === this.token) return false;
      if (voices && voices !== this.kit) disposeQuietly(voices, volume);
      return true;
    };
    try {
      await Tone.start();
      if (superseded()) return null;
      // Pre-install preview: count what the injected loader delivers, for the error / partial message.
      let failed = 0;
      let loadedCount = 0;
      let firstError: unknown = null;
      const custom = opts.loadBuffer;
      const loadBuffer = custom
        ? (pad: PadIndex) =>
            custom(pad).then(
              (buffer) => {
                if (buffer) loadedCount++;
                return buffer;
              },
              (err: unknown) => {
                failed++;
                firstError ??= err;
                throw err;
              },
            )
        : undefined;
      volume = new Tone.Volume(-6).toDestination();
      voices = new SamplerKit(kit, loadBuffer ? { loadBuffer } : {});
      voices.connect(volume);
      // Owned by the player from here on: stop() releases it.
      this.kit = voices;
      this.volume = volume;
      await voices.ready;
      if (token !== this.token) return null;
      /** Some (not all) sounds of a pre-install preview failed: it plays, and reports this. */
      let partial: string | null = null;
      if (custom) {
        const reason = firstError instanceof Error && firstError.message ? ` (${firstError.message})` : '';
        if (loadedCount === 0) throw new Error(`Couldn't load this kit's sounds${reason}`, { cause: firstError });
        if (failed > 0) partial = `${failed} of ${failed + loadedCount} sounds couldn't load${reason}`;
      }
      const hits = demoPattern(kit);
      const dur = stepSeconds(DEMO_BPM);
      const t0 = Tone.immediate() + 0.08;
      this.current = { id, t0, stepDur: dur, hits };
      for (const h of hits) {
        if (h.pad >= 0 && h.pad < PAD_COUNT) voices.trigger(h.pad, h.velocity, t0 + h.step * dur);
      }
      const totalMs = (DEMO_STEPS * dur + 1.2) * 1000;
      this.timer = setTimeout(() => {
        if (token === this.token) this.stop();
      }, totalMs);
      return partial;
    } catch (err) {
      console.warn('[packs] kit preview failed', err);
      if (token !== this.token) return null;
      if (voices && voices !== this.kit) disposeQuietly(voices, volume);
      else if (!voices) volume?.dispose();
      this.stop();
      return err instanceof Error && err.message ? err.message : 'Preview failed';
    }
  }

  stop(): void {
    this.token++;
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
    const kit = this.kit;
    const volume = this.volume;
    this.kit = null;
    this.volume = null;
    this.current = null;
    try {
      kit?.stopAll();
      kit?.dispose();
      volume?.dispose();
    } catch (err) {
      console.warn('[packs] kit preview cleanup failed', err);
    }
    this.setPlaying(null);
  }

  private setPlaying(id: string | null) {
    if (this.playingId === id) return;
    this.playingId = id;
    for (const cb of [...this.listeners]) cb(id);
  }
}

function disposeQuietly(voices: SamplerKit, volume: Tone.Volume | null): void {
  try {
    voices.stopAll();
    voices.dispose();
    volume?.dispose();
  } catch (err) {
    console.warn('[packs] kit preview cleanup failed', err);
  }
}

export const kitPreview = new KitPreviewPlayer();
