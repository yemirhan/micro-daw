import * as Tone from 'tone';
import type { SampleProcessingParams, SignalGeneratorParams } from '@/types/samples';

function reverseBuffer(source: AudioBuffer): AudioBuffer {
  const ctx = Tone.getContext().rawContext as AudioContext;
  const reversed = ctx.createBuffer(
    source.numberOfChannels,
    source.length,
    source.sampleRate,
  );
  for (let ch = 0; ch < source.numberOfChannels; ch++) {
    const src = source.getChannelData(ch);
    const dst = reversed.getChannelData(ch);
    for (let i = 0; i < src.length; i++) {
      dst[i] = src[src.length - 1 - i];
    }
  }
  return reversed;
}

function normalizeBuffer(buffer: AudioBuffer): AudioBuffer {
  let peak = 0;
  for (let ch = 0; ch < buffer.numberOfChannels; ch++) {
    const data = buffer.getChannelData(ch);
    for (let i = 0; i < data.length; i++) {
      const abs = Math.abs(data[i]);
      if (abs > peak) peak = abs;
    }
  }
  if (peak > 0 && peak !== 1) {
    const scale = 1 / peak;
    for (let ch = 0; ch < buffer.numberOfChannels; ch++) {
      const data = buffer.getChannelData(ch);
      for (let i = 0; i < data.length; i++) {
        data[i] *= scale;
      }
    }
  }
  return buffer;
}

export async function processAudio(
  sourceBuffer: AudioBuffer,
  params: SampleProcessingParams,
): Promise<AudioBuffer> {
  let buffer = sourceBuffer;

  // Reverse (pure JS, before Tone.Offline)
  if (params.reverse.enabled) {
    buffer = reverseBuffer(buffer);
  }

  // Check if any Tone-based effect is enabled
  const needsTone =
    params.filter.enabled ||
    params.pitchShift.enabled ||
    params.reverb.enabled ||
    params.delay.enabled;

  if (needsTone) {
    // Calculate tail time for reverb/delay ring-out
    let tail = 0;
    if (params.reverb.enabled) tail += params.reverb.decay;
    if (params.delay.enabled) tail += params.delay.time * (1 / (1 - params.delay.feedback));
    tail = Math.min(tail, 3);

    const renderDuration = buffer.duration + tail;
    const channels = buffer.numberOfChannels;
    const sampleRate = buffer.sampleRate;

    const inputToneBuf = new Tone.ToneAudioBuffer(buffer);

    const offlineBuffer = await Tone.Offline(async ({ transport }) => {
      const player = new Tone.Player(inputToneBuf);

      // Build effect chain
      const chain: Tone.ToneAudioNode[] = [];

      if (params.filter.enabled) {
        const filter = new Tone.Filter({
          type: params.filter.type,
          frequency: params.filter.cutoff,
          Q: params.filter.resonance,
        });
        chain.push(filter);
      }

      if (params.pitchShift.enabled) {
        const pitch = new Tone.PitchShift({
          pitch: params.pitchShift.semitones,
        });
        chain.push(pitch);
      }

      if (params.delay.enabled) {
        const delay = new Tone.FeedbackDelay({
          delayTime: params.delay.time,
          feedback: params.delay.feedback,
          wet: params.delay.wet,
        });
        chain.push(delay);
      }

      if (params.reverb.enabled) {
        const reverb = new Tone.Reverb({
          decay: params.reverb.decay,
          wet: params.reverb.wet,
        });
        await reverb.ready;
        chain.push(reverb);
      }

      if (chain.length > 0) {
        player.chain(...chain, Tone.getDestination());
      } else {
        player.toDestination();
      }

      player.start(0);
      transport.start(0);
    }, renderDuration, channels, sampleRate);

    inputToneBuf.dispose();
    buffer = offlineBuffer.get() as unknown as AudioBuffer;
  }

  // Normalize (pure JS, after Tone.Offline)
  if (params.normalize.enabled) {
    buffer = normalizeBuffer(buffer);
  }

  return buffer;
}

export async function generateSignal(
  params: SignalGeneratorParams,
): Promise<AudioBuffer> {
  if (params.type === 'white-noise') {
    const sampleRate = Tone.getContext().sampleRate;
    const length = Math.ceil(sampleRate * params.duration);
    const ctx = Tone.getContext().rawContext as AudioContext;
    const buffer = ctx.createBuffer(1, length, sampleRate);
    const data = buffer.getChannelData(0);
    for (let i = 0; i < length; i++) {
      data[i] = (Math.random() * 2 - 1) * params.amplitude;
    }
    return buffer;
  }

  const offlineBuffer = await Tone.Offline(({ transport }) => {
    const osc = new Tone.Oscillator({
      type: params.type as Tone.ToneOscillatorType,
      frequency: params.frequency,
    });
    const vol = new Tone.Volume(Tone.gainToDb(params.amplitude));
    osc.chain(vol, Tone.getDestination());
    osc.start(0);
    osc.stop(params.duration);
    transport.start(0);
  }, params.duration, 1);

  return offlineBuffer.get() as unknown as AudioBuffer;
}
