import { useState, useCallback, useEffect, useRef } from 'react';
import * as Tone from 'tone';
import { sampleManager } from '@/services/SampleManager';
import { arrangementEngine } from '@/services/ArrangementEngine';
import { SAMPLE_LIBRARY_STORAGE_KEY } from '@/utils/constants';
import { processAudio, generateSignal } from '@/services/SampleProcessor';
import type { SampleLibraryEntry, SampleTrim, SampleProcessingParams, SignalGeneratorParams } from '@/types/samples';

interface SerializedEntry {
  sampleId: string;
  samplePath: string;
  sampleName: string;
  trim?: SampleTrim;
  addedAt: number;
}

export function useSampleLibrary() {
  const [entries, setEntries] = useState<SampleLibraryEntry[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [isProcessing, setIsProcessing] = useState(false);
  const [isAuditioning, setIsAuditioning] = useState(false);
  const [auditionProgress, setAuditionProgress] = useState(0);
  const playerRef = useRef<Tone.Player | null>(null);
  const rafRef = useRef(0);
  const loadedRef = useRef(false);

  // Persist to localStorage
  const persist = useCallback((list: SampleLibraryEntry[]) => {
    const serialized: SerializedEntry[] = list
      .filter((e) => e.sample.path !== '')
      .map((e) => ({
        sampleId: e.sample.id,
        samplePath: e.sample.path,
        sampleName: e.sample.name,
        trim: e.trim,
        addedAt: e.addedAt,
      }));
    localStorage.setItem(SAMPLE_LIBRARY_STORAGE_KEY, JSON.stringify(serialized));
  }, []);

  // Load from localStorage on mount
  useEffect(() => {
    if (loadedRef.current) return;
    loadedRef.current = true;

    const raw = localStorage.getItem(SAMPLE_LIBRARY_STORAGE_KEY);
    if (!raw) return;

    try {
      const serialized: SerializedEntry[] = JSON.parse(raw);
      const loadAll = async () => {
        const loaded: SampleLibraryEntry[] = [];
        for (const entry of serialized) {
          const sample = await sampleManager.loadFromPathWithId(entry.sampleId, entry.samplePath);
          if (sample) {
            sample.name = entry.sampleName;
            loaded.push({
              sample,
              trim: entry.trim,
              addedAt: entry.addedAt,
            });
          }
        }
        setEntries(loaded);
      };
      loadAll();
    } catch {
      // Corrupt data — ignore
    }
  }, []);

  const stopAudition = useCallback(() => {
    if (playerRef.current) {
      playerRef.current.stop();
      playerRef.current.dispose();
      playerRef.current = null;
    }
    cancelAnimationFrame(rafRef.current);
    setIsAuditioning(false);
    setAuditionProgress(0);
  }, []);

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      if (playerRef.current) {
        playerRef.current.stop();
        playerRef.current.dispose();
      }
      cancelAnimationFrame(rafRef.current);
    };
  }, []);

  const importSamples = useCallback(async () => {
    const api = window.electronAPI;
    if (!api) return;

    const filePaths = await api.sampleShowImportDialog();
    if (!filePaths || filePaths.length === 0) return;

    const newEntries: SampleLibraryEntry[] = [];
    for (const filePath of filePaths) {
      // Check if already in library
      const existing = entries.find((e) => e.sample.path === filePath);
      if (existing) continue;

      const sample = await sampleManager.loadFromPath(filePath);
      newEntries.push({
        sample,
        addedAt: Date.now(),
      });
    }

    if (newEntries.length > 0) {
      const updated = [...entries, ...newEntries];
      setEntries(updated);
      persist(updated);
      // Select the first newly imported sample
      setSelectedId(newEntries[0].sample.id);
    }
  }, [entries, persist]);

  const removeSample = useCallback((id: string) => {
    stopAudition();
    sampleManager.removeSample(id);
    setEntries((prev) => {
      const updated = prev.filter((e) => e.sample.id !== id);
      persist(updated);
      return updated;
    });
    if (selectedId === id) setSelectedId(null);
  }, [selectedId, persist, stopAudition]);

  const selectSample = useCallback((id: string | null) => {
    stopAudition();
    setSelectedId(id);
  }, [stopAudition]);

  const renameSample = useCallback((id: string, name: string) => {
    setEntries((prev) => {
      const updated = prev.map((e) => {
        if (e.sample.id !== id) return e;
        return { ...e, sample: { ...e.sample, name } };
      });
      persist(updated);
      return updated;
    });
  }, [persist]);

  const startAudition = useCallback(() => {
    stopAudition();
    if (!selectedId) return;

    const entry = entries.find((e) => e.sample.id === selectedId);
    if (!entry) return;

    const buffer = sampleManager.getToneBuffer(selectedId);
    if (!buffer) return;

    const player = new Tone.Player(buffer).toDestination();
    playerRef.current = player;

    const trimStart = entry.trim?.startSeconds ?? 0;
    const trimEnd = entry.trim?.endSeconds ?? entry.sample.durationSeconds;
    const duration = trimEnd - trimStart;

    player.start(Tone.now(), trimStart, duration);
    setIsAuditioning(true);

    const startTime = Tone.now();
    const tick = () => {
      const elapsed = Tone.now() - startTime;
      const progress = Math.min(elapsed / duration, 1);
      setAuditionProgress(progress);
      if (progress >= 1) {
        setIsAuditioning(false);
        setAuditionProgress(0);
        playerRef.current?.dispose();
        playerRef.current = null;
        return;
      }
      rafRef.current = requestAnimationFrame(tick);
    };
    rafRef.current = requestAnimationFrame(tick);
  }, [selectedId, entries, stopAudition]);

  const toggleAudition = useCallback(() => {
    if (isAuditioning) {
      stopAudition();
    } else {
      startAudition();
    }
  }, [isAuditioning, startAudition, stopAudition]);

  const setTrimStart = useCallback((id: string, seconds: number) => {
    setEntries((prev) => {
      const updated = prev.map((e) => {
        if (e.sample.id !== id) return e;
        const endSec = e.trim?.endSeconds ?? e.sample.durationSeconds;
        return {
          ...e,
          trim: { startSeconds: Math.max(0, Math.min(seconds, endSec - 0.01)), endSeconds: endSec },
        };
      });
      persist(updated);
      return updated;
    });
  }, [persist]);

  const setTrimEnd = useCallback((id: string, seconds: number) => {
    setEntries((prev) => {
      const updated = prev.map((e) => {
        if (e.sample.id !== id) return e;
        const startSec = e.trim?.startSeconds ?? 0;
        return {
          ...e,
          trim: { startSeconds: startSec, endSeconds: Math.max(startSec + 0.01, Math.min(seconds, e.sample.durationSeconds)) },
        };
      });
      persist(updated);
      return updated;
    });
  }, [persist]);

  const resetTrim = useCallback((id: string) => {
    setEntries((prev) => {
      const updated = prev.map((e) => {
        if (e.sample.id !== id) return e;
        return { ...e, trim: undefined };
      });
      persist(updated);
      return updated;
    });
  }, [persist]);

  const sendToArrangement = useCallback(() => {
    if (!selectedId) return;
    const entry = entries.find((e) => e.sample.id === selectedId);
    if (!entry) return;

    const track = arrangementEngine.addTrack('audio');
    track.name = entry.sample.name;

    const region = arrangementEngine.addAudioRegion(track.id, entry.sample.id, 0);
    if (region && entry.trim) {
      // Apply trim offset
      region.audio!.offsetSeconds = entry.trim.startSeconds;
      const trimmedDuration = entry.trim.endSeconds - entry.trim.startSeconds;
      region.lengthBeats = (trimmedDuration / 60) * arrangementEngine.getArrangement().bpm;
    }
  }, [selectedId, entries]);

  const processSample = useCallback(async (params: SampleProcessingParams) => {
    if (!selectedId) return;
    const entry = entries.find((e) => e.sample.id === selectedId);
    if (!entry) return;

    const sourceBuffer = sampleManager.getBuffer(selectedId);
    if (!sourceBuffer) return;

    setIsProcessing(true);
    try {
      const result = await processAudio(sourceBuffer, params);
      const sample = sampleManager.registerBuffer(`${entry.sample.name} (processed)`, result);
      const newEntry: SampleLibraryEntry = { sample, addedAt: Date.now() };
      setEntries((prev) => {
        const updated = [...prev, newEntry];
        persist(updated);
        return updated;
      });
      setSelectedId(sample.id);
    } finally {
      setIsProcessing(false);
    }
  }, [selectedId, entries, persist]);

  const generateSample = useCallback(async (params: SignalGeneratorParams) => {
    setIsProcessing(true);
    try {
      const result = await generateSignal(params);
      const name = params.type === 'white-noise'
        ? `white-noise ${params.duration}s`
        : `${params.type} ${params.frequency}Hz ${params.duration}s`;
      const sample = sampleManager.registerBuffer(name, result);
      const newEntry: SampleLibraryEntry = { sample, addedAt: Date.now() };
      setEntries((prev) => {
        const updated = [...prev, newEntry];
        persist(updated);
        return updated;
      });
      setSelectedId(sample.id);
    } finally {
      setIsProcessing(false);
    }
  }, [persist]);

  const selectedEntry = entries.find((e) => e.sample.id === selectedId) ?? null;

  return {
    entries,
    selectedId,
    selectedEntry,
    isProcessing,
    isAuditioning,
    auditionProgress,
    importSamples,
    removeSample,
    selectSample,
    renameSample,
    startAudition,
    stopAudition,
    toggleAudition,
    setTrimStart,
    setTrimEnd,
    resetTrim,
    sendToArrangement,
    processSample,
    generateSample,
  };
}
