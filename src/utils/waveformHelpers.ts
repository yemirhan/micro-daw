/**
 * Compute peak values from an AudioBuffer for waveform rendering.
 * Takes the absolute max per bucket across all channels.
 */
export function computePeaksFromBuffer(audioBuffer: AudioBuffer, numPeaks: number): Float32Array {
  const peaks = new Float32Array(numPeaks);
  const channels = audioBuffer.numberOfChannels;
  const length = audioBuffer.length;
  const samplesPerPeak = length / numPeaks;

  for (let ch = 0; ch < channels; ch++) {
    const data = audioBuffer.getChannelData(ch);
    for (let i = 0; i < numPeaks; i++) {
      const start = Math.floor(i * samplesPerPeak);
      const end = Math.min(Math.floor((i + 1) * samplesPerPeak), length);
      let max = 0;
      for (let j = start; j < end; j++) {
        const abs = Math.abs(data[j]);
        if (abs > max) max = abs;
      }
      if (max > peaks[i]) peaks[i] = max;
    }
  }

  return peaks;
}
