import * as Tone from 'tone';
import type { Track } from '@/types/arrangement';
import type { TrackEffectState } from '@/types/effects';
import { DEFAULT_TRACK_EFFECTS } from '@/types/effects';
import { SYNTH_PRESETS, MAX_POLYPHONY } from '@/utils/constants';

export interface TrackAudioNodes {
  synth: Tone.PolySynth | null;
  drumSynths: Map<number, Tone.Synth | Tone.NoiseSynth | Tone.MembraneSynth | Tone.MetalSynth> | null;
  drumFilter: Tone.Filter | null;
  filter: Tone.Filter;
  eq: Tone.EQ3;
  compressor: Tone.Compressor;
  chorus: Tone.Chorus;
  delay: Tone.FeedbackDelay;
  distortion: Tone.Distortion;
  reverb: Tone.Reverb;
  panner: Tone.Panner;
  volume: Tone.Volume;
  meter: Tone.Meter;
  part: Tone.Part | null;
}

export function createTrackAudioNodes(track: Track, destination: Tone.ToneAudioNode): TrackAudioNodes {
  const fx: TrackEffectState = track.effects ?? DEFAULT_TRACK_EFFECTS;

  const meter = new Tone.Meter();
  const volume = new Tone.Volume(track.volume);
  const panner = new Tone.Panner(track.pan ?? 0);
  const reverb = new Tone.Reverb({ decay: 2.5, wet: fx.reverb.enabled ? fx.reverb.wet : 0 });
  const distortion = new Tone.Distortion({ distortion: fx.distortion.enabled ? fx.distortion.amount : 0, wet: fx.distortion.enabled ? fx.distortion.wet : 0 });
  const delay = new Tone.FeedbackDelay({
    delayTime: fx.delay.time,
    feedback: fx.delay.feedback,
    wet: fx.delay.enabled ? fx.delay.wet : 0,
  });
  const chorus = new Tone.Chorus({
    frequency: 1.5, delayTime: 3.5,
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
  const filter = new Tone.Filter({
    frequency: fx.filter.enabled ? fx.filter.cutoff : 18000,
    type: 'lowpass',
    Q: fx.filter.enabled ? fx.filter.resonance : 1,
  });

  // Chain: Filter → EQ → Compressor → Chorus → Delay → Distortion → Reverb → Panner → Volume → Meter → Destination
  filter.connect(eq);
  eq.connect(compressor);
  compressor.connect(chorus);
  chorus.connect(delay);
  delay.connect(distortion);
  distortion.connect(reverb);
  reverb.connect(panner);
  panner.connect(volume);
  volume.connect(meter);
  meter.connect(destination);

  if (track.muted) volume.mute = true;

  const audio: TrackAudioNodes = {
    synth: null,
    drumSynths: null,
    drumFilter: null,
    filter,
    eq,
    compressor,
    chorus,
    delay,
    distortion,
    reverb,
    panner,
    volume,
    meter,
    part: null,
  };

  if (track.instrument.type === 'synth') {
    const preset = SYNTH_PRESETS[track.instrument.presetIndex] || SYNTH_PRESETS[0];
    audio.synth = new Tone.PolySynth(Tone.Synth, {
      maxPolyphony: MAX_POLYPHONY,
      oscillator: preset.oscillator as Tone.OmniOscillatorOptions,
      envelope: preset.envelope,
    });
    audio.synth.connect(filter);
  } else {
    audio.drumSynths = new Map();
    audio.drumFilter = new Tone.Filter({ frequency: 8000, type: 'highpass' });
    audio.drumFilter.connect(filter);

    const kick = new Tone.MembraneSynth({
      pitchDecay: 0.05, octaves: 6,
      oscillator: { type: 'sine' },
      envelope: { attack: 0.001, decay: 0.3, sustain: 0, release: 0.1 },
    });
    kick.connect(filter);
    audio.drumSynths.set(36, kick);

    const snare = new Tone.NoiseSynth({
      noise: { type: 'white' },
      envelope: { attack: 0.001, decay: 0.15, sustain: 0, release: 0.05 },
    });
    snare.connect(filter);
    audio.drumSynths.set(37, snare);

    const chh = new Tone.NoiseSynth({
      noise: { type: 'white' },
      envelope: { attack: 0.001, decay: 0.05, sustain: 0, release: 0.01 },
    });
    chh.connect(audio.drumFilter);
    audio.drumSynths.set(38, chh);

    const ohh = new Tone.NoiseSynth({
      noise: { type: 'white' },
      envelope: { attack: 0.001, decay: 0.3, sustain: 0, release: 0.1 },
    });
    ohh.connect(audio.drumFilter);
    audio.drumSynths.set(39, ohh);

    const clap = new Tone.NoiseSynth({
      noise: { type: 'pink' },
      envelope: { attack: 0.001, decay: 0.12, sustain: 0, release: 0.05 },
    });
    clap.connect(filter);
    audio.drumSynths.set(40, clap);

    const tom = new Tone.MembraneSynth({
      pitchDecay: 0.03, octaves: 4,
      oscillator: { type: 'sine' },
      envelope: { attack: 0.001, decay: 0.2, sustain: 0, release: 0.1 },
    });
    tom.connect(filter);
    audio.drumSynths.set(41, tom);

    const crash = new Tone.MetalSynth({
      frequency: 300,
      envelope: { attack: 0.001, decay: 1.2, release: 0.3 },
      harmonicity: 5.1, modulationIndex: 32, resonance: 4000, octaves: 1.5,
    });
    crash.connect(filter);
    audio.drumSynths.set(42, crash);

    const ride = new Tone.MetalSynth({
      frequency: 400,
      envelope: { attack: 0.001, decay: 0.4, release: 0.1 },
      harmonicity: 5.1, modulationIndex: 16, resonance: 5000, octaves: 1,
    });
    ride.connect(filter);
    audio.drumSynths.set(43, ride);
  }

  return audio;
}

export function disposeTrackAudioNodes(audio: TrackAudioNodes): void {
  if (audio.part) {
    audio.part.stop();
    audio.part.dispose();
  }
  audio.synth?.releaseAll();
  audio.synth?.dispose();
  if (audio.drumSynths) {
    for (const s of audio.drumSynths.values()) s.dispose();
  }
  audio.drumFilter?.dispose();
  audio.filter.dispose();
  audio.eq.dispose();
  audio.compressor.dispose();
  audio.chorus.dispose();
  audio.delay.dispose();
  audio.distortion.dispose();
  audio.reverb.dispose();
  audio.panner.dispose();
  audio.volume.dispose();
  audio.meter.dispose();
}

export function playDrumNote(
  drumSynths: Map<number, Tone.Synth | Tone.NoiseSynth | Tone.MembraneSynth | Tone.MetalSynth>,
  midiNote: number,
  velocity: number,
  time: number,
): void {
  const synth = drumSynths.get(midiNote);
  if (!synth) return;
  const vol = velocity * 0.8 + 0.2;

  if (synth instanceof Tone.MembraneSynth) {
    const pitch = midiNote === 36 ? 'C1' : 'G1';
    synth.triggerAttackRelease(pitch, '8n', time, vol);
  } else if (synth instanceof Tone.NoiseSynth) {
    const dur = midiNote === 38 ? '16n' : midiNote === 39 ? '8n' : '16n';
    synth.triggerAttackRelease(dur, time, vol);
  } else if (synth instanceof Tone.MetalSynth) {
    const dur = midiNote === 42 ? '16n' : '32n';
    synth.triggerAttackRelease(dur, time, vol);
  }
}
