import * as Tone from 'tone';
import type { Track } from '@/types/arrangement';
import type { TrackEffectState } from '@/types/effects';
import { DEFAULT_TRACK_EFFECTS } from '@/types/effects';
import { createSynthFromPreset, resolvePreset, type SynthPlayer } from '@/utils/synthFactory';
import { RoomReverb } from '@/audio/RoomReverb';
import { SamplerKit } from '@/audio/sampler/SamplerKit';
import { createFactoryKit } from '@/audio/sampler/factoryKit';
import { noteToPad } from '@/types/sampler';

export interface TrackAudioNodes {
  synth: SynthPlayer | null;
  /** Drum tracks: the track's 16-pad sampler (built from `track.instrument.kit`). */
  sampler: SamplerKit | null;
  players: Map<string, Tone.Player> | null; // regionId → Player, for audio tracks
  filter: Tone.Filter;
  lfo: Tone.LFO | null;
  eq: Tone.EQ3;
  compressor: Tone.Compressor;
  /**
   * The compressor is wired into the chain (eq -> compressor -> chorus). A
   * disabled compressor is bypassed (eq -> chorus): Chromium's
   * DynamicsCompressor adds a fixed 6 ms look-ahead delay even at ratio 1.
   * Toggle with `setTrackCompressorEnabled`.
   */
  compressorInline: boolean;
  chorus: Tone.Chorus;
  delay: Tone.FeedbackDelay;
  distortion: Tone.Distortion;
  reverb: RoomReverb;
  panner: Tone.Panner;
  volume: Tone.Volume;
  /**
   * Mute / solo gate after the volume (gain 1 or 0). Kept apart from `volume` so a volume
   * write (fader, play-start reset, automation) never un-mutes the track: `Tone.Volume.mute`
   * stores -Infinity in the volume param itself, which any later volume write replaced.
   */
  mute: Tone.Gain;
  meter: Tone.Meter;
  part: Tone.Part | null;
}

/**
 * Per-track instrument + insert chain:
 *   instrument -> filter -> distortion -> EQ -> [compressor] -> chorus -> delay -> reverb -> pan -> volume -> mute -> meter -> destination
 * The compressor is only in the path while enabled (see `compressorInline`).
 */
export function createTrackAudioNodes(track: Track, destination: Tone.ToneAudioNode): TrackAudioNodes {
  const fx: TrackEffectState = track.effects ?? DEFAULT_TRACK_EFFECTS;

  const meter = new Tone.Meter();
  const volume = new Tone.Volume(track.volume);
  const mute = new Tone.Gain(track.muted ? 0 : 1);
  const panner = new Tone.Panner(track.pan ?? 0);
  const reverb = new RoomReverb({ wet: fx.reverb.enabled ? fx.reverb.wet : 0 });
  const distortion = new Tone.Distortion({
    distortion: fx.distortion.enabled ? fx.distortion.amount : 0,
    wet: fx.distortion.enabled ? fx.distortion.wet : 0,
    oversample: '2x',
  });
  const delay = new Tone.FeedbackDelay({
    delayTime: fx.delay.time,
    feedback: fx.delay.feedback,
    wet: fx.delay.enabled ? fx.delay.wet : 0,
  });
  const chorus = new Tone.Chorus({
    frequency: 0.7,
    delayTime: 3.5,
    spread: 180,
    depth: fx.chorus.enabled ? fx.chorus.depth : 0,
    wet: fx.chorus.enabled ? 0.5 : 0,
  }).start();
  const compressor = new Tone.Compressor({
    threshold: fx.compressor.enabled ? fx.compressor.threshold : 0,
    ratio: fx.compressor.enabled ? fx.compressor.ratio : 1,
    attack: fx.compressor.attack,
    release: fx.compressor.release,
  });
  const eq = new Tone.EQ3({
    low: fx.eq.enabled ? fx.eq.low : 0,
    mid: fx.eq.enabled ? fx.eq.mid : 0,
    high: fx.eq.enabled ? fx.eq.high : 0,
  });

  const filterType = fx.filter.type ?? 'lowpass';
  const filter = new Tone.Filter({
    frequency: fx.filter.enabled ? fx.filter.cutoff : 18000,
    type: filterType,
    Q: fx.filter.enabled ? fx.filter.resonance : 1,
  });

  // Create LFO if filter is enabled with non-zero depth
  let lfo: Tone.LFO | null = null;
  if (fx.filter.enabled && fx.filter.lfoDepth > 0) {
    const cutoff = fx.filter.cutoff;
    const depth = fx.filter.lfoDepth;
    const minFreq = Math.max(60, cutoff * (1 - 0.75 * depth));
    const maxFreq = Math.min(18000, cutoff * (1 + 0.75 * depth));
    lfo = new Tone.LFO({
      frequency: fx.filter.lfoRate ?? 1,
      type: fx.filter.lfoWave ?? 'sine',
      min: minFreq,
      max: maxFreq,
    }).start();
    lfo.connect(filter.frequency);
  }

  filter.connect(distortion);
  distortion.connect(eq);
  const compressorInline = compressorActive(fx.compressor.enabled, fx.compressor.ratio);
  eq.connect(compressorInline ? compressor : chorus);
  compressor.connect(chorus);
  chorus.connect(delay);
  delay.connect(reverb.input);
  reverb.connect(panner);
  panner.connect(volume);
  volume.connect(mute);
  mute.connect(meter);
  meter.connect(destination);

  const audio: TrackAudioNodes = {
    synth: null,
    sampler: null,
    players: null,
    filter,
    lfo,
    eq,
    compressor,
    compressorInline,
    chorus,
    delay,
    distortion,
    reverb,
    panner,
    volume,
    mute,
    meter,
    part: null,
  };

  if (track.instrument.type === 'audio') {
    // Audio tracks have no synth — playback via Tone.Player scheduled at play time
    audio.players = new Map();
    return audio;
  }

  if (track.instrument.type === 'synth') {
    const preset = resolvePreset(track.instrument.presetIndex, track.instrument.synthOverrides);
    audio.synth = createSynthFromPreset(preset);
    audio.synth.connect(filter);
  } else {
    audio.sampler = new SamplerKit(track.instrument.kit ?? createFactoryKit());
    audio.sampler.connect(filter);
  }

  return audio;
}

/** Mute or un-mute a track's strip (the volume is left alone). */
export function setTrackMuted(audio: TrackAudioNodes, muted: boolean): void {
  audio.mute.gain.value = muted ? 0 : 1;
}

/**
 * Stop and drop the filter LFO and put the filter back on its static `cutoff`.
 * Connecting the LFO marked `filter.frequency` overridden (Tone's `connectSignal`), which
 * makes every later value write store 0 Hz; Tone never clears the flag, so a lowpass /
 * bandpass track went silent once its LFO was switched off. Clear it before the write.
 */
export function removeFilterLfo(audio: { filter: Tone.Filter; lfo: Tone.LFO | null }, cutoff: number): void {
  if (!audio.lfo) return;
  audio.lfo.stop();
  audio.lfo.dispose();
  audio.lfo = null;
  audio.filter.frequency.overridden = false;
  audio.filter.frequency.value = cutoff;
}

/** A compressor at ratio 1 does nothing: only then is it worth its look-ahead delay. */
export function compressorActive(enabled: boolean, ratio: number): boolean {
  return enabled && ratio > 1.001;
}

/**
 * Put the track compressor in the signal path (enabled) or bypass it
 * (disabled: eq feeds chorus directly, no look-ahead delay). Idempotent; call
 * whenever the compressor's `enabled` flag may have changed.
 */
export function setTrackCompressorEnabled(audio: TrackAudioNodes, enabled: boolean): void {
  if (audio.compressorInline === enabled) return;
  try {
    audio.eq.disconnect();
    audio.eq.connect(enabled ? audio.compressor : audio.chorus);
    audio.compressorInline = enabled;
  } catch (err) {
    console.warn('[track] compressor rewire failed', err);
  }
}

export function disposeTrackAudioNodes(audio: TrackAudioNodes): void {
  if (audio.part) {
    audio.part.stop();
    audio.part.dispose();
  }
  audio.synth?.releaseAll();
  audio.synth?.dispose();
  audio.sampler?.dispose();
  if (audio.players) {
    for (const p of audio.players.values()) {
      p.stop();
      p.dispose();
    }
    audio.players.clear();
  }
  if (audio.lfo) {
    audio.lfo.stop();
    audio.lfo.dispose();
  }
  audio.filter.dispose();
  audio.eq.dispose();
  audio.compressor.dispose();
  audio.chorus.dispose();
  audio.delay.dispose();
  audio.distortion.dispose();
  audio.reverb.dispose();
  audio.panner.dispose();
  audio.volume.dispose();
  audio.mute.dispose();
  audio.meter.dispose();
}

/**
 * Play a drum-region note on a track's sampler. Notes are pad notes
 * (`padToNote(pad)`, 36..51); anything else is ignored. `durationSec` releases
 * pads in 'gate' mode (one-shot pads ignore the release).
 */
export function playDrumNote(
  sampler: SamplerKit,
  note: number,
  velocity: number,
  time: number,
  durationSec?: number,
): void {
  const pad = noteToPad(Math.round(note));
  if (pad === null) return;
  sampler.trigger(pad, velocity, time);
  if (durationSec !== undefined && sampler.getKit().pads[pad]?.mode === 'gate') {
    sampler.release(pad, time + Math.max(0.01, durationSec));
  }
}
