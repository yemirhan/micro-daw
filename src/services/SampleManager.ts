import * as Tone from 'tone';
import type { SampleRef } from '@/types/samples';
import { computePeaksFromBuffer } from '@/utils/waveformHelpers';

class SampleManager {
  private samples = new Map<string, SampleRef>();
  private buffers = new Map<string, AudioBuffer>();
  private toneBuffers = new Map<string, Tone.ToneAudioBuffer>();
  private peakCache = new Map<string, Float32Array>(); // key: `${sampleId}:${width}`

  async loadFromPath(filePath: string): Promise<SampleRef> {
    // Check if already loaded by path
    for (const [, sample] of this.samples) {
      if (sample.path === filePath) return sample;
    }

    const api = window.electronAPI;
    if (!api) throw new Error('Electron API not available');

    const arrayBuffer = await api.sampleReadFile(filePath);
    const audioCtx = Tone.getContext().rawContext as AudioContext;
    const audioBuffer = await audioCtx.decodeAudioData(arrayBuffer);

    const id = crypto.randomUUID();
    const name = filePath.split('/').pop()?.replace(/\.\w+$/, '') || 'Sample';

    const toneBuffer = new Tone.ToneAudioBuffer(audioBuffer);

    const sample: SampleRef = {
      id,
      name,
      path: filePath,
      durationSeconds: audioBuffer.duration,
      sampleRate: audioBuffer.sampleRate,
      channels: audioBuffer.numberOfChannels,
    };

    this.samples.set(id, sample);
    this.buffers.set(id, audioBuffer);
    this.toneBuffers.set(id, toneBuffer);

    return sample;
  }

  async loadFromPathWithId(sampleId: string, filePath: string): Promise<SampleRef | null> {
    // For project restore — loads with a specific ID
    if (this.samples.has(sampleId)) return this.samples.get(sampleId)!;

    const api = window.electronAPI;
    if (!api) return null;

    try {
      const arrayBuffer = await api.sampleReadFile(filePath);
      const audioCtx = Tone.getContext().rawContext as AudioContext;
      const audioBuffer = await audioCtx.decodeAudioData(arrayBuffer);

      const name = filePath.split('/').pop()?.replace(/\.\w+$/, '') || 'Sample';
      const toneBuffer = new Tone.ToneAudioBuffer(audioBuffer);

      const sample: SampleRef = {
        id: sampleId,
        name,
        path: filePath,
        durationSeconds: audioBuffer.duration,
        sampleRate: audioBuffer.sampleRate,
        channels: audioBuffer.numberOfChannels,
      };

      this.samples.set(sampleId, sample);
      this.buffers.set(sampleId, audioBuffer);
      this.toneBuffers.set(sampleId, toneBuffer);

      return sample;
    } catch (err) {
      console.warn(`SampleManager: failed to load sample ${filePath}:`, err);
      return null;
    }
  }

  getBuffer(sampleId: string): AudioBuffer | undefined {
    return this.buffers.get(sampleId);
  }

  getToneBuffer(sampleId: string): Tone.ToneAudioBuffer | undefined {
    return this.toneBuffers.get(sampleId);
  }

  getSample(id: string): SampleRef | undefined {
    return this.samples.get(id);
  }

  computePeaks(sampleId: string, targetWidth: number): Float32Array | undefined {
    const cacheKey = `${sampleId}:${targetWidth}`;
    const cached = this.peakCache.get(cacheKey);
    if (cached) return cached;

    const buffer = this.buffers.get(sampleId);
    if (!buffer) return undefined;

    const peaks = computePeaksFromBuffer(buffer, targetWidth);
    this.peakCache.set(cacheKey, peaks);
    return peaks;
  }

  getAllSamples(): SampleRef[] {
    return Array.from(this.samples.values());
  }

  removeSample(sampleId: string): void {
    const toneBuf = this.toneBuffers.get(sampleId);
    if (toneBuf) toneBuf.dispose();
    this.samples.delete(sampleId);
    this.buffers.delete(sampleId);
    this.toneBuffers.delete(sampleId);
    // Clear all peak cache entries for this sample
    for (const key of this.peakCache.keys()) {
      if (key.startsWith(`${sampleId}:`)) {
        this.peakCache.delete(key);
      }
    }
  }

  dispose(): void {
    for (const buf of this.toneBuffers.values()) {
      buf.dispose();
    }
    this.samples.clear();
    this.buffers.clear();
    this.toneBuffers.clear();
    this.peakCache.clear();
  }
}

export const sampleManager = new SampleManager();
