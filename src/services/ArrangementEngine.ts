import * as Tone from 'tone';
import type {
  Arrangement,
  ArrangementTransportState,
  AutomationLane,
  AutomationParameter,
  AutomationPoint,
  LoopMarkers,
  Region,
  RegionNote,
  Track,
  TrackInstrument,
} from '@/types/arrangement';
import type { TrackEffectState } from '@/types/effects';
import { DEFAULT_TRACK_EFFECTS } from '@/types/effects';
import { SYNTH_PRESETS, TRACK_COLORS, ARRANGEMENT_STORAGE_KEY, DEFAULT_ARRANGEMENT_LENGTH, DEFAULT_BPM } from '@/utils/constants';
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

  getLiveRegion(): Region | null {
    if (this.transportState !== 'recording' || !this.recordingTrackId) return null;

    const transport = Tone.getTransport();
    const currentBeat = transport.seconds * (this.arrangement.bpm / 60);
    const lengthBeats = Math.max(0.25, currentBeat - this.recordingStartBeat);

    // Combine captured (closed) notes + currently-held open notes
    const notes: RegionNote[] = [...this.capturedNotes];
    for (const [noteNum, open] of this.recordingOpenNotes) {
      notes.push({
        note: noteNum,
        velocity: open.velocity,
        startBeat: open.startBeat,
        durationBeats: Math.max(0.125, (currentBeat - this.recordingStartBeat) - open.startBeat),
        isDrum: open.isDrum,
      });
    }

    const track = this.findTrack(this.recordingTrackId);
    return {
      id: '__live__',
      startBeat: this.recordingStartBeat,
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
    transport.position = 0;
    transport.bpm.value = this.arrangement.bpm;

    if (this.metronomeEnabled) this.startMetronome();

    this.recordingStartBeat = 0;
    transport.start();
    this.startPositionUpdates();
    this.emitStateChange();
  }

  stopRecordingToTrack(): Region | null {
    if (this.transportState !== 'recording' || !this.recordingTrackId) return null;
    this.pushUndoSnapshot('Record');

    const transport = Tone.getTransport();
    const endBeat = transport.seconds * (this.arrangement.bpm / 60);

    // Close open notes at current position
    for (const [noteNum, open] of this.recordingOpenNotes) {
      this.capturedNotes.push({
        note: noteNum,
        velocity: open.velocity,
        startBeat: open.startBeat,
        durationBeats: Math.max(0.125, endBeat - open.startBeat),
        isDrum: open.isDrum,
      });
    }

    const track = this.findTrack(this.recordingTrackId);
    this.stopTransport();

    if (!track || this.capturedNotes.length === 0) {
      this.capturedNotes = [];
      this.recordingTrackId = null;
      // Keep armedTrackId — track stays armed
      this.recordingOpenNotes.clear();
      this.emitStateChange();
      return null;
    }

    const lengthBeats = Math.max(1, Math.ceil(endBeat));

    const region: Region = {
      id: crypto.randomUUID(),
      startBeat: this.recordingStartBeat,
      lengthBeats,
      notes: [...this.capturedNotes],
      color: track.color,
    };

    track.regions.push(region);
    this.autoExtendLength();
    this.capturedNotes = [];
    this.recordingTrackId = null;
    // Keep armedTrackId — track stays armed for next recording
    this.recordingOpenNotes.clear();
    this.emitStateChange();
    return region;
  }

  captureNoteOn(note: number, velocity: number, isDrum: boolean): void {
    if (this.transportState !== 'recording') return;
    const transport = Tone.getTransport();
    const currentBeat = transport.seconds * (this.arrangement.bpm / 60);

    if (isDrum) {
      // Drums are one-shot — create note immediately
      this.capturedNotes.push({
        note,
        velocity,
        startBeat: currentBeat - this.recordingStartBeat,
        durationBeats: 0.25,
        isDrum: true,
      });
    } else {
      this.recordingOpenNotes.set(note, {
        startBeat: currentBeat - this.recordingStartBeat,
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

    this.capturedNotes.push({
      note,
      velocity: open.velocity,
      startBeat: open.startBeat,
      durationBeats: Math.max(0.125, (currentBeat - this.recordingStartBeat) - open.startBeat),
      isDrum: false,
    });

    this.recordingOpenNotes.delete(note);
  }

  // --- Loop ---

  setLoopEnabled(enabled: boolean): void {
    this.loopEnabled = enabled;
    const transport = Tone.getTransport();
    if (enabled) {
      const markers = this.arrangement.loopMarkers ?? { startBeat: 0, endBeat: this.arrangement.lengthBeats };
      if (!this.arrangement.loopMarkers) {
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
