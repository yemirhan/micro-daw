import * as Tone from 'tone';
import type {
  Arrangement,
  ArrangementTransportState,
  AutomationLane,
  AutomationParameter,
  AutomationPoint,
  LoopMarkers,
  Marker,
  Region,
  RegionNote,
  Track,
  TrackGroup,
  TrackInstrument,
} from '@/types/arrangement';
import type { TrackEffectState } from '@/types/effects';
import { DEFAULT_TRACK_EFFECTS } from '@/types/effects';
import { SYNTH_PRESETS, TRACK_COLORS, MARKER_COLORS, ARRANGEMENT_STORAGE_KEY, DEFAULT_ARRANGEMENT_LENGTH, DEFAULT_BPM } from '@/utils/constants';
import { midiToNoteName } from '@/utils/noteHelpers';
import { quantizeNotes } from '@/utils/pianoRollHelpers';
import { getAutomationValue, mapAutomationValue, setAutomationPoint as setAutoPoint, deleteAutomationPoint as deleteAutoPoint } from '@/utils/automationHelpers';
import { createTrackAudioNodes, disposeTrackAudioNodes, playDrumNote as playDrumNoteUtil, type TrackAudioNodes } from '@/utils/trackAudioFactory';
import { undoManager } from './UndoManager';
import { sampleManager } from './SampleManager';
import type { SampleRef } from '@/types/samples';

type StateCallback = () => void;
type PositionCallback = (beat: number) => void;

class ArrangementEngine {
  private arrangement: Arrangement;
  private transportState: ArrangementTransportState = 'stopped';
  private trackAudio = new Map<string, TrackAudioNodes>();
  private stateCallbacks: StateCallback[] = [];
  private positionCallbacks: PositionCallback[] = [];
  private positionRAF = 0;
  private metronomeSynth: Tone.Synth | null = null;
  private metronomeLoop: Tone.Loop | null = null;
  private metronomeEnabled = false;
  private armedTrackId: string | null = null;
  private recordingTrackId: string | null = null;
  private recordingStartBeat = 0;
  private recordingOpenNotes = new Map<number, { startBeat: number; velocity: number; isDrum: boolean }>();
  private capturedNotes: RegionNote[] = [];
  private colorIndex = 0;
  private loopEnabled = false;
  private masterMeter: Tone.Meter | null = null;
  private punchRecording = false;
  private punchLoopCallbackId: number | null = null;

  constructor() {
    this.arrangement = this.createDefaultArrangement();
  }

  private createDefaultArrangement(): Arrangement {
    return {
      id: crypto.randomUUID(),
      name: 'Untitled',
      bpm: DEFAULT_BPM,
      lengthBeats: DEFAULT_ARRANGEMENT_LENGTH,
      tracks: [],
    };
  }

  // --- Getters ---

  getArrangement(): Arrangement {
    return this.arrangement;
  }

  getTracks(): Track[] {
    return this.arrangement.tracks;
  }

  getState(): ArrangementTransportState {
    return this.transportState;
  }

  getBpm(): number {
    return this.arrangement.bpm;
  }

  getPosition(): number {
    if (this.transportState === 'stopped') return 0;
    const transport = Tone.getTransport();
    return transport.seconds * (this.arrangement.bpm / 60);
  }

  isMetronomeEnabled(): boolean {
    return this.metronomeEnabled;
  }

  isLoopEnabled(): boolean {
    return this.loopEnabled;
  }

  getLoopMarkers(): LoopMarkers | undefined {
    return this.arrangement.loopMarkers;
  }

  getArmedTrackId(): string | null {
    return this.armedTrackId;
  }

  getRecordingTrackId(): string | null {
    return this.recordingTrackId;
  }

  isPunchRecordingActive(): boolean {
    return this.punchRecording;
  }

  getLiveRegion(): Region | null {
    if (this.transportState !== 'recording' || !this.recordingTrackId) return null;

    const transport = Tone.getTransport();
    const currentBeat = transport.seconds * (this.arrangement.bpm / 60);
    const loopMarkers = this.arrangement.loopMarkers;

    let regionStart: number;
    let lengthBeats: number;

    if (this.punchRecording && loopMarkers) {
      regionStart = loopMarkers.startBeat;
      lengthBeats = loopMarkers.endBeat - loopMarkers.startBeat;
    } else {
      regionStart = this.recordingStartBeat;
      lengthBeats = Math.max(0.25, currentBeat - this.recordingStartBeat);
    }

    // Combine captured (closed) notes + currently-held open notes
    const notes: RegionNote[] = [...this.capturedNotes];
    for (const [noteNum, open] of this.recordingOpenNotes) {
      const elapsed = this.punchRecording && loopMarkers
        ? Math.max(0.125, (currentBeat - loopMarkers.startBeat) % (loopMarkers.endBeat - loopMarkers.startBeat) - open.startBeat)
        : Math.max(0.125, (currentBeat - this.recordingStartBeat) - open.startBeat);
      notes.push({
        note: noteNum,
        velocity: open.velocity,
        startBeat: open.startBeat,
        durationBeats: elapsed,
        isDrum: open.isDrum,
      });
    }

    const track = this.findTrack(this.recordingTrackId);
    return {
      id: '__live__',
      startBeat: regionStart,
      lengthBeats,
      notes,
      color: track?.color,
    };
  }

  // --- Track CRUD ---

  addTrack(type: 'synth' | 'drums' | 'audio' = 'synth', presetIndex = 0): Track {
    this.pushUndoSnapshot('Add Track');
    const color = TRACK_COLORS[this.colorIndex % TRACK_COLORS.length];
    this.colorIndex++;
    const name = type === 'drums' ? 'Drums' : type === 'audio' ? 'Audio' : SYNTH_PRESETS[presetIndex]?.name ?? 'Synth';
    const track: Track = {
      id: crypto.randomUUID(),
      name,
      instrument: { type, presetIndex: type === 'audio' ? 0 : presetIndex },
      regions: [],
      volume: -6,
      pan: 0,
      muted: false,
      solo: false,
      color,
      effects: structuredClone(DEFAULT_TRACK_EFFECTS),
    };
    this.arrangement.tracks.push(track);
    this.createTrackAudio(track);
    this.emitStateChange();
    return track;
  }

  removeTrack(trackId: string): void {
    this.pushUndoSnapshot('Remove Track');
    this.disposeTrackAudio(trackId);
    this.arrangement.tracks = this.arrangement.tracks.filter((t) => t.id !== trackId);
    if (this.armedTrackId === trackId) this.armedTrackId = null;
    // Clean up group references
    if (this.arrangement.groups) {
      for (const group of this.arrangement.groups) {
        group.trackIds = group.trackIds.filter((id) => id !== trackId);
      }
      this.arrangement.groups = this.arrangement.groups.filter((g) => g.trackIds.length > 0);
    }
    this.emitStateChange();
  }

  setTrackInstrument(trackId: string, instrument: TrackInstrument): void {
    this.pushUndoSnapshot('Change Instrument');
    const track = this.findTrack(trackId);
    if (!track) return;
    track.instrument = instrument;
    track.name = instrument.type === 'drums' ? 'Drums' : SYNTH_PRESETS[instrument.presetIndex]?.name ?? 'Synth';
    this.disposeTrackAudio(trackId);
    this.createTrackAudio(track);
    this.emitStateChange();
  }

  setTrackVolume(trackId: string, db: number): void {
    const track = this.findTrack(trackId);
    if (!track) return;
    track.volume = db;
    const audio = this.trackAudio.get(trackId);
    if (audio) audio.volume.volume.value = db;
    this.emitStateChange();
  }

  setTrackMute(trackId: string, muted: boolean): void {
    const track = this.findTrack(trackId);
    if (!track) return;
    track.muted = muted;
    const audio = this.trackAudio.get(trackId);
    if (audio) audio.volume.mute = muted;
    this.emitStateChange();
  }

  setTrackSolo(trackId: string, solo: boolean): void {
    const track = this.findTrack(trackId);
    if (!track) return;
    track.solo = solo;
    this.updateSoloState();
    this.emitStateChange();
  }

  setTrackPan(trackId: string, pan: number): void {
    const track = this.findTrack(trackId);
    if (!track) return;
    track.pan = pan;
    const audio = this.trackAudio.get(trackId);
    if (audio) audio.panner.pan.value = pan;
    this.emitStateChange();
  }

  setTrackEffect(trackId: string, effectName: keyof TrackEffectState, params: Partial<TrackEffectState[typeof effectName]>): void {
    this.pushUndoSnapshot('Change Track Effect');
    const track = this.findTrack(trackId);
    if (!track) return;
    if (!track.effects) track.effects = structuredClone(DEFAULT_TRACK_EFFECTS);

    const fx = track.effects[effectName] as Record<string, unknown>;
    Object.assign(fx, params);

    const audio = this.trackAudio.get(trackId);
    if (audio) {
      this.applyTrackEffect(audio, track.effects, effectName);
    }
    this.emitStateChange();
  }

  private applyTrackEffect(audio: TrackAudioNodes, effects: TrackEffectState, name: keyof TrackEffectState): void {
    switch (name) {
      case 'reverb':
        audio.reverb.wet.value = effects.reverb.enabled ? effects.reverb.wet : 0;
        break;
      case 'delay':
        audio.delay.wet.value = effects.delay.enabled ? effects.delay.wet : 0;
        audio.delay.delayTime.value = effects.delay.time;
        audio.delay.feedback.value = effects.delay.feedback;
        break;
      case 'chorus':
        audio.chorus.depth = effects.chorus.enabled ? effects.chorus.depth : 0;
        audio.chorus.wet.value = effects.chorus.enabled ? 0.5 : 0;
        break;
      case 'distortion':
        audio.distortion.distortion = effects.distortion.enabled ? effects.distortion.amount : 0;
        audio.distortion.wet.value = effects.distortion.enabled ? effects.distortion.wet : 0;
        break;
      case 'eq':
        audio.eq.low.value = effects.eq.enabled ? effects.eq.low : 0;
        audio.eq.mid.value = effects.eq.enabled ? effects.eq.mid : 0;
        audio.eq.high.value = effects.eq.enabled ? effects.eq.high : 0;
        break;
      case 'compressor':
        audio.compressor.threshold.value = effects.compressor.enabled ? effects.compressor.threshold : 0;
        audio.compressor.ratio.value = effects.compressor.enabled ? effects.compressor.ratio : 1;
        audio.compressor.attack.value = effects.compressor.attack;
        audio.compressor.release.value = effects.compressor.release;
        break;
      case 'filter':
        audio.filter.frequency.value = effects.filter.enabled ? effects.filter.cutoff : 18000;
        audio.filter.Q.value = effects.filter.enabled ? effects.filter.resonance : 1;
        break;
    }
  }

  // --- Region Operations ---

  addRegion(trackId: string, region: Region): void {
    this.pushUndoSnapshot('Add Region');
    const track = this.findTrack(trackId);
    if (!track) return;
    track.regions.push(region);
    this.autoExtendLength();
    this.emitStateChange();
  }

  removeRegion(trackId: string, regionId: string): void {
    this.pushUndoSnapshot('Remove Region');
    const track = this.findTrack(trackId);
    if (!track) return;
    track.regions = track.regions.filter((r) => r.id !== regionId);
    this.emitStateChange();
  }

  moveRegion(trackId: string, regionId: string, newStartBeat: number): void {
    this.pushUndoSnapshot('Move Region');
    const track = this.findTrack(trackId);
    if (!track) return;
    const region = track.regions.find((r) => r.id === regionId);
    if (!region) return;
    region.startBeat = Math.max(0, newStartBeat);
    this.autoExtendLength();
    this.emitStateChange();
  }

  moveRegionToTrack(fromTrackId: string, toTrackId: string, regionId: string): void {
    this.pushUndoSnapshot('Move Region to Track');
    const fromTrack = this.findTrack(fromTrackId);
    const toTrack = this.findTrack(toTrackId);
    if (!fromTrack || !toTrack) return;
    const regionIdx = fromTrack.regions.findIndex((r) => r.id === regionId);
    if (regionIdx === -1) return;
    const [region] = fromTrack.regions.splice(regionIdx, 1);
    toTrack.regions.push(region);
    this.emitStateChange();
  }

  splitRegion(trackId: string, regionId: string, splitBeat: number): void {
    this.pushUndoSnapshot('Split Region');
    const track = this.findTrack(trackId);
    if (!track) return;
    const region = track.regions.find((r) => r.id === regionId);
    if (!region) return;

    const localSplit = splitBeat - region.startBeat;
    if (localSplit <= 0 || localSplit >= region.lengthBeats) return;

    // Audio region split
    if (region.audio) {
      const localSplitSeconds = (localSplit / this.arrangement.bpm) * 60;

      const rightRegion: Region = {
        id: crypto.randomUUID(),
        startBeat: splitBeat,
        lengthBeats: region.lengthBeats - localSplit,
        notes: [],
        color: region.color,
        name: region.name ? `${region.name} (R)` : undefined,
        audio: {
          ...region.audio,
          offsetSeconds: region.audio.offsetSeconds + localSplitSeconds,
        },
      };

      region.lengthBeats = localSplit;
      if (region.name) region.name = `${region.name} (L)`;

      track.regions.push(rightRegion);
      this.emitStateChange();
      return;
    }

    // MIDI region split
    const leftNotes: RegionNote[] = [];
    const rightNotes: RegionNote[] = [];

    for (const note of region.notes) {
      if (note.startBeat + note.durationBeats <= localSplit) {
        leftNotes.push(note);
      } else if (note.startBeat >= localSplit) {
        rightNotes.push({
          ...note,
          startBeat: note.startBeat - localSplit,
        });
      } else {
        // Note spans the split — trim left, create right
        leftNotes.push({
          ...note,
          durationBeats: localSplit - note.startBeat,
        });
        rightNotes.push({
          ...note,
          startBeat: 0,
          durationBeats: note.startBeat + note.durationBeats - localSplit,
        });
      }
    }

    const rightRegion: Region = {
      id: crypto.randomUUID(),
      startBeat: splitBeat,
      lengthBeats: region.lengthBeats - localSplit,
      notes: rightNotes,
      color: region.color,
      name: region.name ? `${region.name} (R)` : undefined,
    };

    region.lengthBeats = localSplit;
    region.notes = leftNotes;
    if (region.name) region.name = `${region.name} (L)`;

    track.regions.push(rightRegion);
    this.emitStateChange();
  }

  duplicateRegion(trackId: string, regionId: string): void {
    this.pushUndoSnapshot('Duplicate Region');
    const track = this.findTrack(trackId);
    if (!track) return;
    const region = track.regions.find((r) => r.id === regionId);
    if (!region) return;

    const duplicate: Region = {
      ...region,
      id: crypto.randomUUID(),
      startBeat: region.startBeat + region.lengthBeats,
      notes: region.notes.map((n) => ({ ...n })),
      name: region.name ? `${region.name} (copy)` : undefined,
    };
    track.regions.push(duplicate);
    this.autoExtendLength();
    this.emitStateChange();
  }

  // --- Audio Import ---

  addAudioRegion(trackId: string, sampleId: string, startBeat: number): Region | null {
    this.pushUndoSnapshot('Add Audio Region');
    const track = this.findTrack(trackId);
    if (!track || track.instrument.type !== 'audio') return null;

    const sample = sampleManager.getSample(sampleId);
    if (!sample) return null;

    const lengthBeats = (sample.durationSeconds / 60) * this.arrangement.bpm;

    const region: Region = {
      id: crypto.randomUUID(),
      startBeat,
      lengthBeats,
      notes: [],
      color: track.color,
      name: sample.name,
      audio: {
        sampleId: sample.id,
        samplePath: sample.path,
        offsetSeconds: 0,
        gainDb: 0,
        originalDuration: sample.durationSeconds,
      },
    };

    track.regions.push(region);
    this.autoExtendLength();
    this.emitStateChange();
    return region;
  }

  async importAudioFile(): Promise<void> {
    const api = window.electronAPI;
    if (!api) return;

    const filePaths = await api.sampleShowImportDialog();
    if (!filePaths || filePaths.length === 0) return;

    for (const filePath of filePaths) {
      const sample = await sampleManager.loadFromPath(filePath);
      const track = this.addTrack('audio');
      track.name = sample.name;
      this.addAudioRegion(track.id, sample.id, 0);
    }
  }

  // --- Recording into track ---

  armTrackForRecording(trackId: string | null): void {
    this.armedTrackId = trackId;
    this.emitStateChange();
  }

  startRecordingToTrack(trackId?: string): void {
    // Use provided trackId, or fall back to armed track
    const resolvedId = trackId ?? this.armedTrackId;

    // Auto-create a synth track if no tracks exist and no track specified
    if (!resolvedId && this.arrangement.tracks.length === 0) {
      const track = this.addTrack('synth');
      this.armedTrackId = track.id;
      return this.startRecordingToTrack(track.id);
    }

    if (!resolvedId) return;
    const track = this.findTrack(resolvedId);
    if (!track || this.transportState !== 'stopped') return;

    this.recordingTrackId = resolvedId;
    this.recordingOpenNotes.clear();
    this.capturedNotes = [];
    this.transportState = 'recording';

    const transport = Tone.getTransport();
    transport.stop();
    transport.cancel();
    transport.bpm.value = this.arrangement.bpm;

    // Punch recording: when loop is enabled with loop markers
    const loopMarkers = this.arrangement.loopMarkers;
    if (this.loopEnabled && loopMarkers) {
      this.punchRecording = true;
      this.recordingStartBeat = loopMarkers.startBeat;

      // Position transport at loop start
      transport.position = (loopMarkers.startBeat / this.arrangement.bpm) * 60;
      transport.loop = true;
      transport.loopStart = (loopMarkers.startBeat / this.arrangement.bpm) * 60;
      transport.loopEnd = (loopMarkers.endBeat / this.arrangement.bpm) * 60;

      // Schedule loop boundary callback to close open notes on each pass
      const loopEndSec = (loopMarkers.endBeat / this.arrangement.bpm) * 60;
      const safetyOffset = 0.05; // 50ms before loop end
      this.punchLoopCallbackId = transport.schedule((time) => {
        // Close all open notes at loop boundary
        for (const [noteNum, open] of this.recordingOpenNotes) {
          const loopLen = loopMarkers.endBeat - loopMarkers.startBeat;
          this.capturedNotes.push({
            note: noteNum,
            velocity: open.velocity,
            startBeat: open.startBeat,
            durationBeats: Math.max(0.125, loopLen - open.startBeat),
            isDrum: open.isDrum,
          });
        }
        // Replace captured notes (latest pass wins)
        this.recordingOpenNotes.clear();
      }, Math.max(0, loopEndSec - safetyOffset)) as unknown as number;
    } else {
      this.punchRecording = false;
      transport.position = 0;
      this.recordingStartBeat = 0;
    }

    if (this.metronomeEnabled) this.startMetronome();

    transport.start();
    this.startPositionUpdates();
    this.emitStateChange();
  }

  stopRecordingToTrack(): Region | null {
    if (this.transportState !== 'recording' || !this.recordingTrackId) return null;
    this.pushUndoSnapshot('Record');

    const transport = Tone.getTransport();
    const loopMarkers = this.arrangement.loopMarkers;
    const isPunch = this.punchRecording;

    // Capture end beat BEFORE stopping transport (stopTransport resets position to 0)
    const endBeat = transport.seconds * (this.arrangement.bpm / 60);

    if (isPunch && loopMarkers) {
      // Close open notes within punch range
      const loopLen = loopMarkers.endBeat - loopMarkers.startBeat;
      for (const [noteNum, open] of this.recordingOpenNotes) {
        this.capturedNotes.push({
          note: noteNum,
          velocity: open.velocity,
          startBeat: open.startBeat,
          durationBeats: Math.max(0.125, loopLen - open.startBeat),
          isDrum: open.isDrum,
        });
      }
    } else {
      // Close open notes at current position
      for (const [noteNum, open] of this.recordingOpenNotes) {
        this.capturedNotes.push({
          note: noteNum,
          velocity: open.velocity,
          startBeat: open.startBeat,
          durationBeats: Math.max(0.125, (endBeat - this.recordingStartBeat) - open.startBeat),
          isDrum: open.isDrum,
        });
      }
    }

    const track = this.findTrack(this.recordingTrackId);
    this.punchRecording = false;
    this.punchLoopCallbackId = null;
    this.stopTransport();

    if (!track || this.capturedNotes.length === 0) {
      this.capturedNotes = [];
      this.recordingTrackId = null;
      this.recordingOpenNotes.clear();
      this.emitStateChange();
      return null;
    }

    let regionStartBeat: number;
    let lengthBeats: number;

    if (isPunch && loopMarkers) {
      regionStartBeat = loopMarkers.startBeat;
      lengthBeats = loopMarkers.endBeat - loopMarkers.startBeat;

      // Remove existing regions that overlap the punch range (replace mode)
      track.regions = track.regions.filter((r) => {
        const rEnd = r.startBeat + r.lengthBeats;
        return rEnd <= loopMarkers.startBeat || r.startBeat >= loopMarkers.endBeat;
      });
    } else {
      regionStartBeat = this.recordingStartBeat;
      lengthBeats = Math.max(1, Math.ceil(endBeat));
    }

    const region: Region = {
      id: crypto.randomUUID(),
      startBeat: regionStartBeat,
      lengthBeats,
      notes: [...this.capturedNotes],
      color: track.color,
    };

    track.regions.push(region);
    this.autoExtendLength();
    this.capturedNotes = [];
    this.recordingTrackId = null;
    this.recordingOpenNotes.clear();
    this.emitStateChange();
    return region;
  }

  captureNoteOn(note: number, velocity: number, isDrum: boolean): void {
    if (this.transportState !== 'recording') return;
    const transport = Tone.getTransport();
    const currentBeat = transport.seconds * (this.arrangement.bpm / 60);

    const relativeBeat = this.punchRecording && this.arrangement.loopMarkers
      ? (currentBeat - this.arrangement.loopMarkers.startBeat) % (this.arrangement.loopMarkers.endBeat - this.arrangement.loopMarkers.startBeat)
      : currentBeat - this.recordingStartBeat;

    if (isDrum) {
      this.capturedNotes.push({
        note,
        velocity,
        startBeat: relativeBeat,
        durationBeats: 0.25,
        isDrum: true,
      });
    } else {
      this.recordingOpenNotes.set(note, {
        startBeat: relativeBeat,
        velocity,
        isDrum: false,
      });
    }
  }

  captureNoteOff(note: number): void {
    if (this.transportState !== 'recording') return;
    const open = this.recordingOpenNotes.get(note);
    if (!open) return;

    const transport = Tone.getTransport();
    const currentBeat = transport.seconds * (this.arrangement.bpm / 60);

    const relativeBeat = this.punchRecording && this.arrangement.loopMarkers
      ? (currentBeat - this.arrangement.loopMarkers.startBeat) % (this.arrangement.loopMarkers.endBeat - this.arrangement.loopMarkers.startBeat)
      : currentBeat - this.recordingStartBeat;

    this.capturedNotes.push({
      note,
      velocity: open.velocity,
      startBeat: open.startBeat,
      durationBeats: Math.max(0.125, relativeBeat - open.startBeat),
      isDrum: false,
    });

    this.recordingOpenNotes.delete(note);
  }

  // --- Loop ---

  private getNoteRangeLoopMarkers(): LoopMarkers | null {
    let minBeat = Number.POSITIVE_INFINITY;
    let maxBeat = Number.NEGATIVE_INFINITY;

    for (const track of this.arrangement.tracks) {
      for (const region of track.regions) {
        if (region.notes.length === 0) continue;
        for (const note of region.notes) {
          const startBeat = region.startBeat + note.startBeat;
          const endBeat = startBeat + Math.max(0.125, note.durationBeats);
          minBeat = Math.min(minBeat, startBeat);
          maxBeat = Math.max(maxBeat, endBeat);
        }
      }
    }

    if (!Number.isFinite(minBeat) || !Number.isFinite(maxBeat)) {
      return null;
    }

    const maxStartBeat = Math.max(0, this.arrangement.lengthBeats - 1);
    const startBeat = Math.min(maxStartBeat, Math.max(0, minBeat));
    const endBeat = Math.min(this.arrangement.lengthBeats, Math.max(startBeat + 1, maxBeat));
    return { startBeat, endBeat };
  }

  private isFullArrangementLoop(markers: LoopMarkers): boolean {
    return markers.startBeat <= 0 && markers.endBeat >= this.arrangement.lengthBeats;
  }

  setLoopEnabled(enabled: boolean): void {
    this.loopEnabled = enabled;
    const transport = Tone.getTransport();
    if (enabled) {
      let markers = this.arrangement.loopMarkers;
      if (!markers || this.isFullArrangementLoop(markers)) {
        const noteRangeMarkers = this.getNoteRangeLoopMarkers();
        if (noteRangeMarkers) {
          markers = noteRangeMarkers;
          this.arrangement.loopMarkers = noteRangeMarkers;
        }
      }
      if (!markers) {
        markers = { startBeat: 0, endBeat: this.arrangement.lengthBeats };
        this.arrangement.loopMarkers = markers;
      }
      transport.loop = true;
      transport.loopStart = (markers.startBeat / this.arrangement.bpm) * 60;
      transport.loopEnd = (markers.endBeat / this.arrangement.bpm) * 60;
    } else {
      transport.loop = false;
    }
    this.emitStateChange();
  }

  setLoopMarkers(startBeat: number, endBeat: number): void {
    this.pushUndoSnapshot('Set Loop Markers');
    const clamped = {
      startBeat: Math.max(0, startBeat),
      endBeat: Math.min(this.arrangement.lengthBeats, Math.max(startBeat + 1, endBeat)),
    };
    this.arrangement.loopMarkers = clamped;
    if (this.loopEnabled) {
      const transport = Tone.getTransport();
      transport.loopStart = (clamped.startBeat / this.arrangement.bpm) * 60;
      transport.loopEnd = (clamped.endBeat / this.arrangement.bpm) * 60;
    }
    this.emitStateChange();
  }

  private applyLoopToTransport(): void {
    const transport = Tone.getTransport();
    if (this.loopEnabled && this.arrangement.loopMarkers) {
      transport.loop = true;
      transport.loopStart = (this.arrangement.loopMarkers.startBeat / this.arrangement.bpm) * 60;
      transport.loopEnd = (this.arrangement.loopMarkers.endBeat / this.arrangement.bpm) * 60;
    } else {
      transport.loop = false;
    }
  }

  // --- Transport ---

  play(): void {
    if (this.transportState !== 'stopped') return;
    this.transportState = 'playing';

    const transport = Tone.getTransport();
    transport.stop();
    transport.cancel();
    transport.position = 0;
    transport.bpm.value = this.arrangement.bpm;

    this.scheduleAllTracks();
    this.applyLoopToTransport();
    if (this.metronomeEnabled) this.startMetronome();

    transport.start();
    this.startPositionUpdates();
    this.emitStateChange();
  }

  stop(): void {
    if (this.transportState === 'recording') {
      this.stopRecordingToTrack();
      return;
    }
    this.stopTransport();
    this.emitStateChange();
  }

  private stopTransport(): void {
    const transport = Tone.getTransport();
    transport.stop();
    transport.cancel();
    transport.position = 0;

    // Dispose all parts and players
    for (const audio of this.trackAudio.values()) {
      if (audio.part) {
        audio.part.stop();
        audio.part.dispose();
        audio.part = null;
      }
      if (audio.players) {
        for (const p of audio.players.values()) {
          p.stop();
          p.dispose();
        }
        audio.players.clear();
      }
    }

    this.stopMetronome();
    this.stopPositionUpdates();
    this.transportState = 'stopped';
  }

  setPosition(beat: number): void {
    const transport = Tone.getTransport();
    transport.seconds = (beat / this.arrangement.bpm) * 60;
  }

  setBpm(bpm: number): void {
    this.arrangement.bpm = bpm;
    Tone.getTransport().bpm.value = bpm;
    if (this.loopEnabled) {
      this.applyLoopToTransport();
    }
    this.emitStateChange();
  }

  // --- Metronome ---

  setMetronomeEnabled(enabled: boolean): void {
    this.metronomeEnabled = enabled;
    if (!enabled) {
      this.stopMetronome();
    } else if (this.transportState !== 'stopped') {
      this.startMetronome();
    }
  }

  private startMetronome(): void {
    if (this.metronomeLoop) return;

    if (!this.metronomeSynth) {
      this.metronomeSynth = new Tone.Synth({
        oscillator: { type: 'sine' },
        envelope: { attack: 0.001, decay: 0.05, sustain: 0, release: 0.01 },
        volume: -6,
      }).toDestination();
    }

    let beat = 0;
    this.metronomeLoop = new Tone.Loop((time) => {
      const note = beat % 4 === 0 ? 'C6' : 'G5';
      this.metronomeSynth!.triggerAttackRelease(note, '32n', time);
      beat++;
    }, '4n');
    this.metronomeLoop.start(0);
  }

  private stopMetronome(): void {
    if (this.metronomeLoop) {
      this.metronomeLoop.stop();
      this.metronomeLoop.dispose();
      this.metronomeLoop = null;
    }
  }

  // --- Per-Track Audio ---

  private createTrackAudio(track: Track): void {
    // Migrate old tracks without pan/effects
    track.pan ??= 0;
    track.effects ??= structuredClone(DEFAULT_TRACK_EFFECTS);

    const audio = createTrackAudioNodes(track, Tone.getDestination());
    this.trackAudio.set(track.id, audio);
  }

  private disposeTrackAudio(trackId: string): void {
    const audio = this.trackAudio.get(trackId);
    if (!audio) return;
    disposeTrackAudioNodes(audio);
    this.trackAudio.delete(trackId);
  }

  getTrackLevel(trackId: string): number {
    const audio = this.trackAudio.get(trackId);
    if (!audio) return -Infinity;
    const val = audio.meter.getValue();
    return typeof val === 'number' ? val : val[0] ?? -Infinity;
  }

  getMasterLevel(): number {
    if (!this.masterMeter) {
      this.masterMeter = new Tone.Meter();
      Tone.getDestination().connect(this.masterMeter);
    }
    const val = this.masterMeter.getValue();
    return typeof val === 'number' ? val : val[0] ?? -Infinity;
  }

  // --- Scheduling ---

  private scheduleAllTracks(): void {
    const hasSolo = this.arrangement.tracks.some((t) => t.solo);

    for (const track of this.arrangement.tracks) {
      const audio = this.trackAudio.get(track.id);
      if (!audio) continue;

      // Reset all effect parameters to their track config before automation overrides.
      // This ensures deleted automation lanes don't leave stale values on the nodes.
      this.resetTrackEffects(track, audio);

      // Mute logic: if any track has solo, mute all non-solo tracks
      const shouldPlay = hasSolo ? track.solo : !track.muted;
      if (!shouldPlay) continue;

      // Schedule automation for this track (even if no note events)
      this.scheduleAutomation(track, audio);

      // Audio track — schedule Tone.Players for each audio region
      if (track.instrument.type === 'audio' && audio.players) {
        // Dispose old players
        for (const p of audio.players.values()) {
          p.stop();
          p.dispose();
        }
        audio.players.clear();

        for (const region of track.regions) {
          if (!region.audio) continue;
          const toneBuffer = sampleManager.getToneBuffer(region.audio.sampleId);
          if (!toneBuffer) continue;

          const player = new Tone.Player(toneBuffer);
          if (region.audio.gainDb !== 0) {
            player.volume.value = region.audio.gainDb;
          }
          player.connect(audio.filter);
          audio.players.set(region.id, player);

          const startTimeSec = (region.startBeat / this.arrangement.bpm) * 60;
          const regionDurSec = (region.lengthBeats / this.arrangement.bpm) * 60;
          const offset = region.audio.offsetSeconds;

          const transport = Tone.getTransport();
          transport.schedule((time) => {
            player.start(time, offset, regionDurSec);
          }, startTimeSec);
        }
        continue;
      }

      const events: Array<{ time: number; note: RegionNote }> = [];
      for (const region of track.regions) {
        for (const note of region.notes) {
          const absoluteBeat = region.startBeat + note.startBeat;
          events.push({ time: absoluteBeat * (60 / this.arrangement.bpm), note });
        }
      }

      if (events.length === 0) continue;

      audio.part = new Tone.Part((time, value) => {
        const { note } = value;
        if (track.instrument.type === 'drums' && audio.drumSynths) {
          playDrumNoteUtil(audio.drumSynths, note.note, note.velocity, time);
        } else if (audio.synth) {
          const noteName = midiToNoteName(note.note);
          const dur = (note.durationBeats * 60) / this.arrangement.bpm;
          audio.synth.triggerAttackRelease(noteName, dur, time, note.velocity);
        }
      }, events.map((e) => ({ time: e.time, note: e.note })));

      audio.part.start(0);
    }
  }

  // --- Solo state ---

  private updateSoloState(): void {
    const hasSolo = this.arrangement.tracks.some((t) => t.solo);
    for (const track of this.arrangement.tracks) {
      const audio = this.trackAudio.get(track.id);
      if (!audio) continue;
      if (hasSolo) {
        audio.volume.mute = !track.solo;
      } else {
        audio.volume.mute = track.muted;
      }
    }
  }

  // --- Position updates ---

  private startPositionUpdates(): void {
    const update = () => {
      if (this.transportState === 'stopped') return;
      const beat = this.getPosition();
      for (const cb of this.positionCallbacks) cb(beat);

      // Auto-stop at end (only during playback, not recording, not looping)
      if (this.transportState === 'playing' && !this.loopEnabled && beat >= this.arrangement.lengthBeats) {
        this.stop();
        return;
      }

      this.positionRAF = requestAnimationFrame(update);
    };
    this.positionRAF = requestAnimationFrame(update);
  }

  private stopPositionUpdates(): void {
    cancelAnimationFrame(this.positionRAF);
    for (const cb of this.positionCallbacks) cb(0);
  }

  // --- Auto-extend ---

  private autoExtendLength(): void {
    let maxBeat = DEFAULT_ARRANGEMENT_LENGTH;
    for (const track of this.arrangement.tracks) {
      for (const region of track.regions) {
        maxBeat = Math.max(maxBeat, region.startBeat + region.lengthBeats + 4);
      }
    }
    this.arrangement.lengthBeats = Math.ceil(maxBeat / 4) * 4; // round up to bar
  }

  // --- Callbacks ---

  onStateChange(callback: StateCallback): () => void {
    this.stateCallbacks.push(callback);
    return () => {
      this.stateCallbacks = this.stateCallbacks.filter((cb) => cb !== callback);
    };
  }

  onPositionChange(callback: PositionCallback): () => void {
    this.positionCallbacks.push(callback);
    return () => {
      this.positionCallbacks = this.positionCallbacks.filter((cb) => cb !== callback);
    };
  }

  private emitStateChange(): void {
    for (const cb of this.stateCallbacks) cb();
  }

  // --- Persistence ---

  saveToLocalStorage(): void {
    try {
      localStorage.setItem(ARRANGEMENT_STORAGE_KEY, JSON.stringify(this.arrangement));
    } catch {
      // ignore quota errors
    }
  }

  loadFromLocalStorage(): boolean {
    try {
      const raw = localStorage.getItem(ARRANGEMENT_STORAGE_KEY);
      if (!raw) return false;
      const data = JSON.parse(raw) as Arrangement;
      this.arrangement = data;
      // Migrate + recreate audio for all tracks
      for (const track of this.arrangement.tracks) {
        track.pan ??= 0;
        track.effects ??= structuredClone(DEFAULT_TRACK_EFFECTS);
        this.createTrackAudio(track);
      }
      this.emitStateChange();
      return true;
    } catch {
      return false;
    }
  }

  // --- Undo/Redo ---

  private pushUndoSnapshot(label: string): void {
    undoManager.pushUndo(label, structuredClone(this.arrangement));
  }

  beginUndoGroup(label: string): void {
    undoManager.beginUndoGroup(label, structuredClone(this.arrangement));
  }

  endUndoGroup(): void {
    undoManager.endUndoGroup();
  }

  restoreFromSnapshot(snapshot: Arrangement): void {
    // Dispose all existing track audio
    for (const trackId of this.trackAudio.keys()) {
      this.disposeTrackAudio(trackId);
    }

    this.arrangement = structuredClone(snapshot);

    // Migrate + recreate audio for all tracks
    for (const track of this.arrangement.tracks) {
      track.pan ??= 0;
      track.effects ??= structuredClone(DEFAULT_TRACK_EFFECTS);
      this.createTrackAudio(track);
    }

    this.emitStateChange();
  }

  performUndo(): void {
    const snapshot = undoManager.undo();
    if (!snapshot) return;
    // Push current state to redo
    undoManager.pushRedo('Undo', structuredClone(this.arrangement));
    this.restoreFromSnapshot(snapshot);
  }

  performRedo(): void {
    const snapshot = undoManager.redo();
    if (!snapshot) return;
    // Push current state to undo (without clearing redo)
    undoManager.pushUndo('Redo', structuredClone(this.arrangement));
    this.restoreFromSnapshot(snapshot);
  }

  // --- Region Notes (for Piano Roll) ---

  updateRegionNotes(trackId: string, regionId: string, notes: RegionNote[]): void {
    this.pushUndoSnapshot('Edit Notes');
    const track = this.findTrack(trackId);
    if (!track) return;
    const region = track.regions.find((r) => r.id === regionId);
    if (!region) return;
    region.notes = notes;
    this.emitStateChange();
  }

  resizeRegion(trackId: string, regionId: string, newStartBeat?: number, newLengthBeats?: number): void {
    this.pushUndoSnapshot('Trim Region');
    const track = this.findTrack(trackId);
    if (!track) return;
    const region = track.regions.find((r) => r.id === regionId);
    if (!region) return;

    if (region.audio) {
      // Audio region resize
      if (newStartBeat !== undefined) {
        const clampedStart = Math.max(0, newStartBeat);
        const trimAmount = clampedStart - region.startBeat;
        if (trimAmount !== 0) {
          const trimSeconds = (trimAmount / this.arrangement.bpm) * 60;
          region.audio.offsetSeconds += trimSeconds;
          region.startBeat = clampedStart;
        }
      }
      if (newLengthBeats !== undefined) {
        region.lengthBeats = Math.max(0.25, newLengthBeats);
      }
      this.autoExtendLength();
      this.emitStateChange();
      return;
    }

    // MIDI region resize
    // Trim from left — shift notes and remove those that fall before the new start
    if (newStartBeat !== undefined) {
      const clampedStart = Math.max(0, newStartBeat);
      const trimAmount = clampedStart - region.startBeat; // positive = trimming inward
      if (trimAmount !== 0) {
        region.notes = region.notes
          .map((n) => ({ ...n, startBeat: n.startBeat - trimAmount }))
          .filter((n) => n.startBeat + n.durationBeats > 0)
          .map((n) => {
            // Clip notes that partially overlap the trim boundary
            if (n.startBeat < 0) {
              return { ...n, durationBeats: n.durationBeats + n.startBeat, startBeat: 0 };
            }
            return n;
          });
        region.startBeat = clampedStart;
      }
    }

    // Trim from right — clip/remove notes beyond new length
    if (newLengthBeats !== undefined) {
      const newLen = Math.max(0.25, newLengthBeats);
      region.notes = region.notes
        .filter((n) => n.startBeat < newLen)
        .map((n) => {
          if (n.startBeat + n.durationBeats > newLen) {
            return { ...n, durationBeats: newLen - n.startBeat };
          }
          return n;
        });
      region.lengthBeats = newLen;
    }

    this.autoExtendLength();
    this.emitStateChange();
  }

  // --- Automation ---

  addAutomationLane(trackId: string, parameter: AutomationParameter): void {
    this.pushUndoSnapshot('Add Automation Lane');
    const track = this.findTrack(trackId);
    if (!track) return;
    if (!track.automation) track.automation = [];
    // Don't add duplicate
    if (track.automation.some((l) => l.parameter === parameter)) return;
    track.automation.push({ parameter, points: [], visible: true });
    this.emitStateChange();
  }

  removeAutomationLane(trackId: string, parameter: AutomationParameter): void {
    this.pushUndoSnapshot('Remove Automation Lane');
    const track = this.findTrack(trackId);
    if (!track || !track.automation) return;
    track.automation = track.automation.filter((l) => l.parameter !== parameter);
    // Reset audio nodes immediately so stale automation values don't persist
    const audio = this.trackAudio.get(trackId);
    if (audio) this.resetTrackEffects(track, audio);
    this.emitStateChange();
  }

  setAutomationPoint(trackId: string, parameter: AutomationParameter, beat: number, value: number, snapValue: number): void {
    this.pushUndoSnapshot('Set Automation Point');
    const track = this.findTrack(trackId);
    if (!track || !track.automation) return;
    const lane = track.automation.find((l) => l.parameter === parameter);
    if (!lane) return;
    lane.points = setAutoPoint(lane.points, beat, value, snapValue);
    this.emitStateChange();
  }

  deleteAutomationPoint(trackId: string, parameter: AutomationParameter, pointIndex: number): void {
    this.pushUndoSnapshot('Delete Automation Point');
    const track = this.findTrack(trackId);
    if (!track || !track.automation) return;
    const lane = track.automation.find((l) => l.parameter === parameter);
    if (!lane) return;
    lane.points = deleteAutoPoint(lane.points, pointIndex);
    this.emitStateChange();
  }

  toggleAutomationLaneVisibility(trackId: string, parameter: AutomationParameter): void {
    const track = this.findTrack(trackId);
    if (!track || !track.automation) return;
    const lane = track.automation.find((l) => l.parameter === parameter);
    if (!lane) return;
    lane.visible = !lane.visible;
    this.emitStateChange();
  }

  private resetTrackEffects(track: Track, audio: TrackAudioNodes): void {
    const fx = track.effects ?? DEFAULT_TRACK_EFFECTS;

    // Cancel any scheduled AudioParam automation curves before setting values.
    // Without this, stale setValueAtTime/linearRampToValueAtTime curves from
    // previous playback take priority over .value assignments.
    audio.distortion.wet.cancelScheduledValues(0);
    audio.reverb.wet.cancelScheduledValues(0);
    audio.delay.wet.cancelScheduledValues(0);
    audio.chorus.wet.cancelScheduledValues(0);
    audio.filter.frequency.cancelScheduledValues(0);
    audio.filter.Q.cancelScheduledValues(0);
    audio.eq.low.cancelScheduledValues(0);
    audio.eq.mid.cancelScheduledValues(0);
    audio.eq.high.cancelScheduledValues(0);
    audio.panner.pan.cancelScheduledValues(0);
    audio.volume.volume.cancelScheduledValues(0);
    audio.compressor.threshold.cancelScheduledValues(0);
    audio.compressor.ratio.cancelScheduledValues(0);

    // Distortion
    audio.distortion.distortion = fx.distortion.enabled ? fx.distortion.amount : 0;
    audio.distortion.wet.value = fx.distortion.enabled ? fx.distortion.wet : 0;

    // Reverb
    audio.reverb.wet.value = fx.reverb.enabled ? fx.reverb.wet : 0;

    // Delay
    audio.delay.delayTime.value = fx.delay.time || 0.25;
    audio.delay.feedback.value = fx.delay.feedback || 0.3;
    audio.delay.wet.value = fx.delay.enabled ? fx.delay.wet : 0;

    // Chorus
    audio.chorus.depth = fx.chorus.enabled ? fx.chorus.depth : 0;
    audio.chorus.wet.value = fx.chorus.enabled ? 0.5 : 0;

    // Filter
    audio.filter.frequency.value = fx.filter.enabled ? fx.filter.cutoff : 18000;
    audio.filter.Q.value = fx.filter.enabled ? fx.filter.resonance : 1;

    // EQ
    audio.eq.low.value = fx.eq.enabled ? fx.eq.low : 0;
    audio.eq.mid.value = fx.eq.enabled ? fx.eq.mid : 0;
    audio.eq.high.value = fx.eq.enabled ? fx.eq.high : 0;

    // Compressor
    audio.compressor.threshold.value = fx.compressor.enabled ? fx.compressor.threshold : 0;
    audio.compressor.ratio.value = fx.compressor.enabled ? fx.compressor.ratio : 1;

    // Pan & volume
    audio.panner.pan.value = track.pan ?? 0;
    audio.volume.volume.value = track.volume;
  }

  private scheduleAutomation(track: Track, audio: TrackAudioNodes): void {
    if (!track.automation) return;
    const bpm = this.arrangement.bpm;
    const transport = Tone.getTransport();
    const fx = track.effects ?? DEFAULT_TRACK_EFFECTS;

    // Ensure effects have audible core settings when their wet/depth is automated.
    // Without this, automating wet on a disabled effect (amount=0) produces no change.
    for (const lane of track.automation) {
      if (lane.points.length === 0 || !lane.visible) continue;
      switch (lane.parameter) {
        case 'distortionWet':
          audio.distortion.distortion = Math.max(fx.distortion.amount, 0.4);
          break;
        case 'delayWet':
          audio.delay.delayTime.value = fx.delay.time || 0.25;
          audio.delay.feedback.value = fx.delay.feedback || 0.3;
          break;
      }
    }

    for (const lane of track.automation) {
      if (lane.points.length === 0 || !lane.visible) continue;

      for (let i = 0; i < lane.points.length; i++) {
        const pt = lane.points[i];
        const transportTime = (pt.beat / bpm) * 60;
        const realValue = mapAutomationValue(lane.parameter, pt.value);

        // Calculate ramp duration to next point (if any)
        let rampToValue: number | undefined;
        let rampDuration = 0;
        if (i < lane.points.length - 1) {
          const next = lane.points[i + 1];
          rampToValue = mapAutomationValue(lane.parameter, next.value);
          rampDuration = ((next.beat - pt.beat) / bpm) * 60;
        }

        // Use Transport.schedule so we get the correct AudioContext time
        // (setValueAtTime needs absolute AudioContext time, not transport-relative)
        const param = lane.parameter;
        const rv = realValue;
        const rtv = rampToValue;
        const rd = rampDuration;

        transport.schedule((time) => {
          switch (param) {
            case 'volume':
              audio.volume.volume.setValueAtTime(rv, time);
              if (rtv !== undefined) audio.volume.volume.linearRampToValueAtTime(rtv, time + rd);
              break;
            case 'pan':
              audio.panner.pan.setValueAtTime(rv, time);
              if (rtv !== undefined) audio.panner.pan.linearRampToValueAtTime(rtv, time + rd);
              break;
            case 'reverbWet':
              audio.reverb.wet.setValueAtTime(rv, time);
              if (rtv !== undefined) audio.reverb.wet.linearRampToValueAtTime(rtv, time + rd);
              break;
            case 'delayWet':
              audio.delay.wet.setValueAtTime(rv, time);
              if (rtv !== undefined) audio.delay.wet.linearRampToValueAtTime(rtv, time + rd);
              break;
            case 'chorusDepth':
              audio.chorus.depth = rv;
              audio.chorus.wet.value = rv > 0 ? 0.5 : 0;
              break;
            case 'distortionWet':
              audio.distortion.wet.setValueAtTime(rv, time);
              if (rtv !== undefined) audio.distortion.wet.linearRampToValueAtTime(rtv, time + rd);
              break;
            case 'filterCutoff':
              audio.filter.frequency.setValueAtTime(rv, time);
              if (rtv !== undefined) audio.filter.frequency.linearRampToValueAtTime(rtv, time + rd);
              break;
            case 'eqLow':
              audio.eq.low.setValueAtTime(rv, time);
              if (rtv !== undefined) audio.eq.low.linearRampToValueAtTime(rtv, time + rd);
              break;
            case 'eqMid':
              audio.eq.mid.setValueAtTime(rv, time);
              if (rtv !== undefined) audio.eq.mid.linearRampToValueAtTime(rtv, time + rd);
              break;
            case 'eqHigh':
              audio.eq.high.setValueAtTime(rv, time);
              if (rtv !== undefined) audio.eq.high.linearRampToValueAtTime(rtv, time + rd);
              break;
          }
        }, transportTime);
      }
    }
  }

  // --- Quantize ---

  quantizeRegionNotes(trackId: string, regionId: string, snapValue: number): void {
    this.pushUndoSnapshot('Quantize Notes');
    const track = this.findTrack(trackId);
    if (!track) return;
    const region = track.regions.find((r) => r.id === regionId);
    if (!region) return;
    region.notes = quantizeNotes(region.notes, snapValue);
    this.emitStateChange();
  }

  quantizeSelectedNotes(trackId: string, regionId: string, noteIndices: Set<number>, snapValue: number): void {
    this.pushUndoSnapshot('Quantize Selected Notes');
    const track = this.findTrack(trackId);
    if (!track) return;
    const region = track.regions.find((r) => r.id === regionId);
    if (!region) return;
    region.notes = region.notes.map((note, i) => {
      if (!noteIndices.has(i)) return note;
      return { ...note, startBeat: Math.round(note.startBeat / snapValue) * snapValue };
    });
    this.emitStateChange();
  }

  // --- Clipboard ---

  private clipboard: { region: Region; trackId: string } | null = null;

  copyRegion(trackId: string, regionId: string): void {
    const track = this.findTrack(trackId);
    if (!track) return;
    const region = track.regions.find((r) => r.id === regionId);
    if (!region) return;
    this.clipboard = { region: structuredClone(region), trackId };
  }

  pasteRegion(trackId: string, atBeat: number): void {
    if (!this.clipboard) return;
    this.pushUndoSnapshot('Paste Region');
    const track = this.findTrack(trackId);
    if (!track) return;
    const pasted: Region = {
      ...structuredClone(this.clipboard.region),
      id: crypto.randomUUID(),
      startBeat: atBeat,
    };
    track.regions.push(pasted);
    this.autoExtendLength();
    this.emitStateChange();
  }

  hasClipboard(): boolean {
    return this.clipboard !== null;
  }

  // --- Markers ---

  getMarkers(): Marker[] {
    return this.arrangement.markers ?? [];
  }

  addMarker(name: string, beat: number, color?: string): Marker {
    this.pushUndoSnapshot('Add Marker');
    if (!this.arrangement.markers) this.arrangement.markers = [];
    const markerColor = color ?? MARKER_COLORS[this.arrangement.markers.length % MARKER_COLORS.length];
    const marker: Marker = {
      id: crypto.randomUUID(),
      name,
      beat: Math.max(0, beat),
      color: markerColor,
    };
    this.arrangement.markers.push(marker);
    this.arrangement.markers.sort((a, b) => a.beat - b.beat);
    this.emitStateChange();
    return marker;
  }

  removeMarker(id: string): void {
    this.pushUndoSnapshot('Remove Marker');
    if (!this.arrangement.markers) return;
    this.arrangement.markers = this.arrangement.markers.filter((m) => m.id !== id);
    this.emitStateChange();
  }

  updateMarker(id: string, updates: Partial<Pick<Marker, 'name' | 'beat' | 'color'>>): void {
    this.pushUndoSnapshot('Update Marker');
    if (!this.arrangement.markers) return;
    const marker = this.arrangement.markers.find((m) => m.id === id);
    if (!marker) return;
    if (updates.name !== undefined) marker.name = updates.name;
    if (updates.beat !== undefined) marker.beat = Math.max(0, updates.beat);
    if (updates.color !== undefined) marker.color = updates.color;
    this.arrangement.markers.sort((a, b) => a.beat - b.beat);
    this.emitStateChange();
  }

  seekToMarker(id: string): void {
    const markers = this.arrangement.markers;
    if (!markers) return;
    const marker = markers.find((m) => m.id === id);
    if (!marker) return;
    this.setPosition(marker.beat);
  }

  seekToNextMarker(): void {
    const markers = this.arrangement.markers;
    if (!markers || markers.length === 0) return;
    const currentBeat = this.getPosition();
    const next = markers.find((m) => m.beat > currentBeat + 0.01);
    if (next) this.setPosition(next.beat);
  }

  seekToPreviousMarker(): void {
    const markers = this.arrangement.markers;
    if (!markers || markers.length === 0) return;
    const currentBeat = this.getPosition();
    // Find the last marker before current position
    let prev: Marker | undefined;
    for (let i = markers.length - 1; i >= 0; i--) {
      if (markers[i].beat < currentBeat - 0.01) {
        prev = markers[i];
        break;
      }
    }
    if (prev) this.setPosition(prev.beat);
  }

  // --- Track Groups ---

  getGroups(): TrackGroup[] {
    return this.arrangement.groups ?? [];
  }

  createGroup(name: string, trackIds: string[]): TrackGroup {
    this.pushUndoSnapshot('Create Group');
    if (!this.arrangement.groups) this.arrangement.groups = [];
    // Remove trackIds from existing groups
    for (const group of this.arrangement.groups) {
      group.trackIds = group.trackIds.filter((id) => !trackIds.includes(id));
    }
    // Remove empty groups
    this.arrangement.groups = this.arrangement.groups.filter((g) => g.trackIds.length > 0);
    const color = TRACK_COLORS[this.arrangement.groups.length % TRACK_COLORS.length];
    const group: TrackGroup = {
      id: crypto.randomUUID(),
      name,
      color,
      trackIds,
      collapsed: false,
    };
    this.arrangement.groups.push(group);
    this.emitStateChange();
    return group;
  }

  removeGroup(id: string): void {
    this.pushUndoSnapshot('Remove Group');
    if (!this.arrangement.groups) return;
    this.arrangement.groups = this.arrangement.groups.filter((g) => g.id !== id);
    this.emitStateChange();
  }

  renameGroup(id: string, name: string): void {
    this.pushUndoSnapshot('Rename Group');
    if (!this.arrangement.groups) return;
    const group = this.arrangement.groups.find((g) => g.id === id);
    if (group) group.name = name;
    this.emitStateChange();
  }

  toggleGroupCollapsed(id: string): void {
    if (!this.arrangement.groups) return;
    const group = this.arrangement.groups.find((g) => g.id === id);
    if (group) group.collapsed = !group.collapsed;
    this.emitStateChange();
  }

  addTrackToGroup(groupId: string, trackId: string): void {
    this.pushUndoSnapshot('Add Track to Group');
    if (!this.arrangement.groups) return;
    // Remove from any existing group
    for (const group of this.arrangement.groups) {
      group.trackIds = group.trackIds.filter((id) => id !== trackId);
    }
    const group = this.arrangement.groups.find((g) => g.id === groupId);
    if (group) group.trackIds.push(trackId);
    // Clean empty groups
    this.arrangement.groups = this.arrangement.groups.filter((g) => g.trackIds.length > 0);
    this.emitStateChange();
  }

  removeTrackFromGroup(groupId: string, trackId: string): void {
    this.pushUndoSnapshot('Remove Track from Group');
    if (!this.arrangement.groups) return;
    const group = this.arrangement.groups.find((g) => g.id === groupId);
    if (group) {
      group.trackIds = group.trackIds.filter((id) => id !== trackId);
    }
    // Clean empty groups
    this.arrangement.groups = this.arrangement.groups.filter((g) => g.trackIds.length > 0);
    this.emitStateChange();
  }

  setGroupMute(groupId: string, muted: boolean): void {
    if (!this.arrangement.groups) return;
    const group = this.arrangement.groups.find((g) => g.id === groupId);
    if (!group) return;
    for (const trackId of group.trackIds) {
      this.setTrackMute(trackId, muted);
    }
  }

  setGroupSolo(groupId: string, solo: boolean): void {
    if (!this.arrangement.groups) return;
    const group = this.arrangement.groups.find((g) => g.id === groupId);
    if (!group) return;
    for (const trackId of group.trackIds) {
      this.setTrackSolo(trackId, solo);
    }
  }

  // --- Templates ---

  loadFromTemplate(arrangement: Arrangement): void {
    // Deep clone and replace all IDs with fresh UUIDs
    const clone = structuredClone(arrangement);
    clone.id = crypto.randomUUID();
    for (const track of clone.tracks) {
      track.id = crypto.randomUUID();
      for (const region of track.regions) {
        region.id = crypto.randomUUID();
      }
    }
    if (clone.markers) {
      for (const marker of clone.markers) {
        marker.id = crypto.randomUUID();
      }
    }
    if (clone.groups) {
      for (const group of clone.groups) {
        group.id = crypto.randomUUID();
      }
    }
    this.restoreFromSnapshot(clone);
  }

  // --- Helpers ---

  private findTrack(id: string): Track | undefined {
    return this.arrangement.tracks.find((t) => t.id === id);
  }

  dispose(): void {
    this.stopTransport();
    for (const trackId of this.trackAudio.keys()) {
      this.disposeTrackAudio(trackId);
    }
    this.metronomeSynth?.dispose();
    this.metronomeSynth = null;
    this.stateCallbacks = [];
    this.positionCallbacks = [];
  }
}

export const arrangementEngine = new ArrangementEngine();
