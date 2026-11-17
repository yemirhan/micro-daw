import * as Tone from 'tone';
import type { SampleRef } from '@/types/samples';
import { computePeaksFromBuffer } from '@/utils/waveformHelpers';

class SampleManager {
  private samples = new Map<string, SampleRef>();
  private buffers = new Map<string, AudioBuffer>();
  private toneBuffers = new Map<string, Tone.ToneAudioBuffer>();
  private peakCache = new Map<string, Float32Array>(); // key: `${sampleId}:${width}`
  /** In-place reloads running, by sample path. */
  private reloading = new Map<string, Promise<string[]>>();
  /** Samples whose file could not be re-read at the last reload (audio kept). */
  private unavailable = new Set<string>();
  private reloadListeners = new Set<(sampleIds: string[]) => void>();

  async loadFromPath(filePath: string): Promise<SampleRef> {
    const reload = this.whenReloaded(filePath);
    if (reload) await reload;
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
    const name = filePath.split(/[\\/]/).pop()?.replace(/\.\w+$/, '') || 'Sample';

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

  registerBuffer(name: string, audioBuffer: AudioBuffer): SampleRef {
    const id = crypto.randomUUID();
    const toneBuffer = new Tone.ToneAudioBuffer(audioBuffer);

    const sample: SampleRef = {
      id,
      name,
      path: '',
      durationSeconds: audioBuffer.duration,
      sampleRate: audioBuffer.sampleRate,
      channels: audioBuffer.numberOfChannels,
    };

    this.samples.set(id, sample);
    this.buffers.set(id, audioBuffer);
    this.toneBuffers.set(id, toneBuffer);

    return sample;
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
    this.unavailable.delete(sampleId);
    this.clearPeaks(sampleId);
  }

  /**
   * B3: re-reads, in place, every sample loaded from a path starting with
   * `prefix` (e.g. a pack folder after it was installed / updated), keeping
   * each sample's id, so audio regions and pads that reference the id keep
   * working. A sample whose file can no longer be read (pack removed) keeps
   * its current audio and is reported as unavailable instead of being dropped
   * (Song playback and export keep playing it). Listeners of
   * `onSamplesReloaded` get the ids whose audio changed (rebuild players).
   * Resolves with those ids; concurrent loads of a reloading path wait for it.
   */
  reloadByPathPrefix(prefix: string): Promise<string[]> {
    if (!prefix) return Promise.resolve([]);
    const targets = [...this.samples.values()].filter((s) => s.path && s.path.startsWith(prefix));
    if (targets.length === 0) return Promise.resolve([]);
    const run = (async () => {
      const reloaded: string[] = [];
      await Promise.all(
        targets.map(async (sample) => {
          const ok = await this.reloadOne(sample);
          if (ok) reloaded.push(sample.id);
        }),
      );
      if (reloaded.length) this.emitReloaded(reloaded);
      return reloaded;
    })();
    for (const t of targets) this.reloading.set(t.path, run);
    void run.finally(() => {
      for (const t of targets) if (this.reloading.get(t.path) === run) this.reloading.delete(t.path);
    });
    return run;
  }

  private async reloadOne(sample: SampleRef): Promise<boolean> {
    const api = typeof window !== 'undefined' ? window.electronAPI : undefined;
    try {
      if (!api) throw new Error('Electron API not available');
      const data = await api.sampleReadFile(sample.path);
      const audioBuffer = await (Tone.getContext().rawContext as BaseAudioContext).decodeAudioData(data);
      // Removed meanwhile (or replaced by another reload)? Leave it alone.
      if (this.samples.get(sample.id) !== sample) return false;
      const old = this.toneBuffers.get(sample.id);
      const next: SampleRef = {
        ...sample,
        durationSeconds: audioBuffer.duration,
        sampleRate: audioBuffer.sampleRate,
        channels: audioBuffer.numberOfChannels,
      };
      this.samples.set(sample.id, next);
      this.buffers.set(sample.id, audioBuffer);
      this.toneBuffers.set(sample.id, new Tone.ToneAudioBuffer(audioBuffer));
      this.clearPeaks(sample.id);
      this.unavailable.delete(sample.id);
      // Players copy the AudioBuffer out of a ToneAudioBuffer, so disposing the old wrapper is safe.
      old?.dispose();
      return true;
    } catch (err) {
      if (this.samples.has(sample.id)) {
        this.unavailable.add(sample.id);
        console.warn(`SampleManager: ${sample.path} could not be re-read; keeping the loaded audio`, err);
      }
      return false;
    }
  }

  /** The sample's file could not be re-read at the last reload (its pack was removed); its loaded audio is kept. */
  isUnavailable(sampleId: string): boolean {
    return this.unavailable.has(sampleId);
  }

  /** Resolves once the running in-place reload of `filePath` finished; null when none is running. */
  whenReloaded(filePath: string): Promise<void> | null {
    const p = this.reloading.get(filePath);
    return p ? p.then(() => undefined) : null;
  }

  /**
   * Called with the ids of samples whose audio was reloaded in place
   * (`reloadByPathPrefix`). ArrangementEngine should rebuild the audio-region
   * players of tracks that use them. Returns unsubscribe.
   */
  onSamplesReloaded(cb: (sampleIds: string[]) => void): () => void {
    this.reloadListeners.add(cb);
    return () => {
      this.reloadListeners.delete(cb);
    };
  }

  private emitReloaded(ids: string[]): void {
    for (const cb of [...this.reloadListeners]) {
      try {
        cb([...ids]);
      } catch (err) {
        console.warn('SampleManager: reload listener failed', err);
      }
    }
  }

  private clearPeaks(sampleId: string): void {
    for (const key of [...this.peakCache.keys()]) {
      if (key.startsWith(`${sampleId}:`)) this.peakCache.delete(key);
    }
  }

  /**
   * Forgets every sample loaded from a path starting with `prefix`, so the
   * next load re-reads the file. Returns the number of samples removed. Not
   * for pack changes: that would silence audio regions using those samples
   * (use `reloadByPathPrefix`).
   */
  removeByPathPrefix(prefix: string): number {
    if (!prefix) return 0;
    let removed = 0;
    for (const [id, sample] of [...this.samples]) {
      if (sample.path && sample.path.startsWith(prefix)) {
        this.removeSample(id);
        removed++;
      }
    }
    return removed;
  }

  dispose(): void {
    for (const buf of this.toneBuffers.values()) {
      buf.dispose();
    }
    this.samples.clear();
    this.buffers.clear();
    this.toneBuffers.clear();
    this.peakCache.clear();
    this.reloading.clear();
    this.unavailable.clear();
  }
}

export const sampleManager = new SampleManager();
