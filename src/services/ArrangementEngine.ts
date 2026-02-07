import * as Tone from 'tone';
import type {
  Arrangement,
  ArrangementTransportState,
  Region,
  RegionNote,
  Track,
  TrackInstrument,
} from '@/types/arrangement';
import { SYNTH_PRESETS, TRACK_COLORS, ARRANGEMENT_STORAGE_KEY, DEFAULT_ARRANGEMENT_LENGTH, DEFAULT_BPM, MAX_POLYPHONY } from '@/utils/constants';
import { midiToNoteName } from '@/utils/noteHelpers';
import { undoManager } from './UndoManager';

type StateCallback = () => void;
type PositionCallback = (beat: number) => void;

interface TrackAudio {
  synth: Tone.PolySynth | null;
  drumSynths: Map<number, Tone.Synth | Tone.NoiseSynth | Tone.MembraneSynth | Tone.MetalSynth> | null;
  drumFilter: Tone.Filter | null;
  volume: Tone.Volume;
  part: Tone.Part | null;
}

class ArrangementEngine {
  private arrangement: Arrangement;
  private transportState: ArrangementTransportState = 'stopped';
  private trackAudio = new Map<string, TrackAudio>();
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

  addTrack(type: 'synth' | 'drums' = 'synth', presetIndex = 0): Track {
    this.pushUndoSnapshot('Add Track');
    const color = TRACK_COLORS[this.colorIndex % TRACK_COLORS.length];
    this.colorIndex++;
    const track: Track = {
      id: crypto.randomUUID(),
      name: type === 'drums' ? 'Drums' : SYNTH_PRESETS[presetIndex]?.name ?? 'Synth',
      instrument: { type, presetIndex },
      regions: [],
      volume: -6,
      muted: false,
      solo: false,
      color,
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

  // --- Transport ---

  play(): void {
    if (this.transportState !== 'stopped') return;
    this.transportState = 'playing';

    const transport = Tone.getTransport();
    transport.stop();
    transport.position = 0;
    transport.bpm.value = this.arrangement.bpm;

    this.scheduleAllTracks();
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

    // Dispose all parts
    for (const audio of this.trackAudio.values()) {
      if (audio.part) {
        audio.part.stop();
        audio.part.dispose();
        audio.part = null;
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
    const volume = new Tone.Volume(track.volume).toDestination();
    if (track.muted) volume.mute = true;

    const audio: TrackAudio = {
      synth: null,
      drumSynths: null,
      drumFilter: null,
      volume,
      part: null,
    };

    if (track.instrument.type === 'synth') {
      const preset = SYNTH_PRESETS[track.instrument.presetIndex] || SYNTH_PRESETS[0];
      audio.synth = new Tone.PolySynth(Tone.Synth, {
        maxPolyphony: MAX_POLYPHONY,
        oscillator: preset.oscillator as Tone.OmniOscillatorOptions,
        envelope: preset.envelope,
      });
      audio.synth.connect(volume);
    } else {
      audio.drumSynths = new Map();
      audio.drumFilter = new Tone.Filter({ frequency: 8000, type: 'highpass' });
      audio.drumFilter.connect(volume);

      // Kick
      const kick = new Tone.MembraneSynth({
        pitchDecay: 0.05, octaves: 6,
        oscillator: { type: 'sine' },
        envelope: { attack: 0.001, decay: 0.3, sustain: 0, release: 0.1 },
      });
      kick.connect(volume);
      audio.drumSynths.set(36, kick);

      // Snare
      const snare = new Tone.NoiseSynth({
        noise: { type: 'white' },
        envelope: { attack: 0.001, decay: 0.15, sustain: 0, release: 0.05 },
      });
      snare.connect(volume);
      audio.drumSynths.set(37, snare);

      // Closed HH
      const chh = new Tone.NoiseSynth({
        noise: { type: 'white' },
        envelope: { attack: 0.001, decay: 0.05, sustain: 0, release: 0.01 },
      });
      chh.connect(audio.drumFilter);
      audio.drumSynths.set(38, chh);

      // Open HH
      const ohh = new Tone.NoiseSynth({
        noise: { type: 'white' },
        envelope: { attack: 0.001, decay: 0.3, sustain: 0, release: 0.1 },
      });
      ohh.connect(audio.drumFilter);
      audio.drumSynths.set(39, ohh);

      // Clap
      const clap = new Tone.NoiseSynth({
        noise: { type: 'pink' },
        envelope: { attack: 0.001, decay: 0.12, sustain: 0, release: 0.05 },
      });
      clap.connect(volume);
      audio.drumSynths.set(40, clap);

      // Tom
      const tom = new Tone.MembraneSynth({
        pitchDecay: 0.03, octaves: 4,
        oscillator: { type: 'sine' },
        envelope: { attack: 0.001, decay: 0.2, sustain: 0, release: 0.1 },
      });
      tom.connect(volume);
      audio.drumSynths.set(41, tom);

      // Crash
      const crash = new Tone.MetalSynth({
        frequency: 300,
        envelope: { attack: 0.001, decay: 1.2, release: 0.3 },
        harmonicity: 5.1, modulationIndex: 32, resonance: 4000, octaves: 1.5,
      });
      crash.connect(volume);
      audio.drumSynths.set(42, crash);

      // Ride
      const ride = new Tone.MetalSynth({
        frequency: 400,
        envelope: { attack: 0.001, decay: 0.4, release: 0.1 },
        harmonicity: 5.1, modulationIndex: 16, resonance: 5000, octaves: 1,
      });
      ride.connect(volume);
      audio.drumSynths.set(43, ride);
    }

    this.trackAudio.set(track.id, audio);
  }

  private disposeTrackAudio(trackId: string): void {
    const audio = this.trackAudio.get(trackId);
    if (!audio) return;

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
    audio.volume.dispose();
    this.trackAudio.delete(trackId);
  }

  // --- Scheduling ---

  private scheduleAllTracks(): void {
    const hasSolo = this.arrangement.tracks.some((t) => t.solo);

    for (const track of this.arrangement.tracks) {
      const audio = this.trackAudio.get(track.id);
      if (!audio) continue;

      // Mute logic: if any track has solo, mute all non-solo tracks
      const shouldPlay = hasSolo ? track.solo : !track.muted;
      if (!shouldPlay) continue;

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
        if (track.instrument.type === 'drums') {
          this.playDrumNote(audio, note.note, note.velocity, time);
        } else if (audio.synth) {
          const noteName = midiToNoteName(note.note);
          const dur = (note.durationBeats * 60) / this.arrangement.bpm;
          audio.synth.triggerAttackRelease(noteName, dur, time, note.velocity);
        }
      }, events.map((e) => ({ time: e.time, note: e.note })));

      audio.part.start(0);
    }
  }

  private playDrumNote(audio: TrackAudio, midiNote: number, velocity: number, time: number): void {
    if (!audio.drumSynths) return;
    const synth = audio.drumSynths.get(midiNote);
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

      // Auto-stop at end (only during playback, not recording)
      if (this.transportState === 'playing' && beat >= this.arrangement.lengthBeats) {
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
      // Recreate audio for all tracks
      for (const track of this.arrangement.tracks) {
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

    // Recreate audio for all tracks
    for (const track of this.arrangement.tracks) {
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
