import * as Tone from 'tone';
import type {
  Arrangement,
  ArrangementTransportState,
  BeatState,
  AutomationParameter,
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
import { mapAutomationValue, setAutomationPoint as setAutoPoint, deleteAutomationPoint as deleteAutoPoint } from '@/utils/automationHelpers';
import { createTrackAudioNodes, disposeTrackAudioNodes, playDrumNote as playDrumNoteUtil, compressorActive, setTrackCompressorEnabled, type TrackAudioNodes } from '@/utils/trackAudioFactory';
import {
  DEFAULT_STEP_VELOCITY,
  STEP_EPSILON,
  loopSwingGrid,
  migrateDrumTrack,
  normalizeBeatState,
  normalizeLoopSettings,
  patternStepAt,
  swingOffsetBeats,
} from '@/utils/arrangementHelpers';
import { PAD_COUNT, noteToPad, padToNote, type KitState, type LoopRecordSettings, type PadIndex, type PadSample, type PadSettings } from '@/types/sampler';
import { createFactoryKit, normalizeKit, normalizePad } from '@/audio/sampler/factoryKit';
import { padSourceKey } from '@/audio/sampler/bufferCache';
import type { SamplerKit } from '@/audio/sampler/SamplerKit';
import { undoManager } from './UndoManager';
import { sampleManager } from './SampleManager';
import { drumEngine, type DrumSink } from './DrumEngine';
import { Metronome } from './arrangement/Metronome';
import { LoopRecorder, type LoopRecordingSession } from './arrangement/LoopRecorder';

type StateCallback = () => void;

/**
 * Who chose a whole kit (setKit). 'user': an explicit choice in any view (kit
 * list, factory kit, Packs page Load); 'pack-load': a pending "Get & load"
 * applying its kit (review N2).
 */
export type KitSelectionSource = 'user' | 'pack-load';

export interface KitSelectedEvent {
  trackId: string;
  kitId: string;
  source: KitSelectionSource;
}

/** An audio region whose sample could not be re-read (its pack was removed); its loaded audio still plays (N5). */
export interface UnavailableAudioRegion {
  trackId: string;
  regionId: string;
  sampleId: string;
}
type PositionCallback = (beat: number) => void;

interface PartEvent {
  time: number;
  note: RegionNote;
  /** Absolute beat of the note (key for suppressing a just-played live hit). */
  beat: number;
}

const isPadIndex = (pad: number): pad is PadIndex => Number.isInteger(pad) && pad >= 0 && pad < PAD_COUNT;
const clamp01 = (v: number) => Math.min(1, Math.max(0, Number.isFinite(v) ? v : 0));
/** A live-sink event time: the given audio time (not before now), or now. */
const liveTime = (time?: number) => {
  const now = Tone.immediate();
  return time !== undefined && Number.isFinite(time) ? Math.max(time, now) : now;
};

/** A drum track's live pad sink: also releases every pad it still holds (on sink change). */
type LiveDrumSink = DrumSink & { releaseAllLive(): void };

function createLiveSink(sampler: SamplerKit): LiveDrumSink {
  /** Pads hit through this sink and not released yet. */
  const held = new Set<PadIndex>();
  return {
    // `time` (AudioContext seconds) from note repeat's look-ahead scheduler;
    // omitted = a live hit. A time already passed is clamped to now so the
    // attack isn't cut.
    hit: (pad, velocity, time) => {
      held.add(pad);
      sampler.trigger(pad, velocity, liveTime(time));
    },
    release: (pad, time) => {
      held.delete(pad);
      sampler.release(pad, liveTime(time));
    },
    releaseAllLive: () => {
      if (held.size === 0) return;
      const now = Tone.immediate();
      for (const pad of held) {
        try {
          sampler.release(pad, now);
        } catch {
          // sampler already disposed: its voices are gone
        }
      }
      held.clear();
    },
  };
}

/** Extra hints for `captureNoteOn`. */
export interface CaptureNoteOptions {
  /**
   * Grid (beats) the hit was generated on (note repeat). When finer than the
   * loop's record quantize, loop recording snaps the hit to this grid instead,
   * so a 1/32 roll under a 1/16 quantize keeps every hit.
   */
  gridBeats?: number;
}

class ArrangementEngine {
  private arrangement: Arrangement;
  private transportState: ArrangementTransportState = 'stopped';
  private trackAudio = new Map<string, TrackAudioNodes>();
  private stateCallbacks: StateCallback[] = [];
  private positionCallbacks: PositionCallback[] = [];
  private positionRAF = 0;
  private readonly metronome = new Metronome();
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
  private kitCallbacks: StateCallback[] = [];
  private kitSelectedCallbacks = new Set<(e: KitSelectedEvent) => void>();
  /**
   * Project session (review N2): bumped whenever a different project replaces
   * the arrangement (new, open, template, first load), never by edits or
   * undo/redo. Requests bound to "what the user was looking at" compare it.
   */
  private sessionId = 0;
  /** Live pad sink registered with drumEngine (bound to the active drum track's sampler). */
  private liveSink: { sampler: SamplerKit; sink: LiveDrumSink } | null = null;
  /** The beat view's loop recorder (src/services/arrangement/LoopRecorder.ts). */
  private readonly loopRecorder = new LoopRecorder({
    getBpm: () => this.arrangement.bpm,
    getLoopSettings: () => this.beat().loop,
    findTrack: (id) => this.findTrack(id),
    isGatePad: (trackId, pad) => this.getKit(trackId).pads[pad]?.mode === 'gate',
    ensurePatternRegion: (track) => this.ensurePatternRegionOn(track),
    patternNoteFilter: (region) => this.patternNoteFilter(region),
    transportBeatAt: (time) => this.transportBeatAt(time),
    addToPlayback: (track, region, hit) => this.addToRunningPart(track, region, hit),
    refreshPlayback: (trackId) => this.refreshPlaybackSchedule(trackId),
    emitStateChange: () => this.emitStateChange(),
  });
  /**
   * The transport plays the beat-view pattern (playPattern): it loops the
   * pattern range TRANSIENTLY (the stored loop markers and the Song loop
   * toggle are left alone) and the loop recorder's metronome setting clicks.
   */
  private patternPlayback = false;
  /**
   * The transport start this engine scheduled (audio time + the beat it starts
   * at). Tone answers tick lookups BEFORE a scheduled start from its previous
   * run / idle state, so transportBeatAt() returns this beat for earlier times
   * (review B1: hits right after Record, the first playhead frames).
   */
  private transportStart: { time: number; beat: number } | null = null;

  /** The running loop-recording take (null when not loop recording). */
  private get loopRec(): Readonly<LoopRecordingSession> | null {
    return this.loopRecorder.session;
  }

  constructor() {
    this.arrangement = this.createDefaultArrangement();
    // B3: a pack install/update re-reads its samples in place (same sampleId);
    // point the running audio-region players at the new audio.
    sampleManager.onSamplesReloaded?.((ids) => this.refreshAudioPlayers(ids));
  }

  // --- Project session (review N2) ---

  /** Current project session id; changes on new / open / template / first load, not on edits or undo. */
  getSessionId(): number {
    return this.sessionId;
  }

  private beginSession(): void {
    this.sessionId++;
  }

  /**
   * Open a project (replaces the arrangement, starts a new session). Unlike
   * restoreFromSnapshot (undo/redo), always a new session, even for a project
   * with the same id (re-opening it).
   */
  loadProject(arrangement: Arrangement): void {
    this.beginSession();
    this.restoreFromSnapshot(arrangement);
  }

  // --- Sample status for Song audio regions (review N5) ---

  /**
   * Audio regions whose sample is flagged unavailable by SampleManager (its
   * pack was removed; the loaded audio keeps playing). Song shows a warning
   * badge on them.
   */
  getUnavailableAudioRegions(): UnavailableAudioRegion[] {
    const out: UnavailableAudioRegion[] = [];
    for (const track of this.arrangement.tracks) {
      for (const region of track.regions) {
        const sampleId = region.audio?.sampleId;
        if (sampleId && this.isSampleUnavailable(sampleId)) out.push({ trackId: track.id, regionId: region.id, sampleId });
      }
    }
    return out;
  }

  /** The sample could not be re-read at its last in-place reload (its pack was removed). */
  isSampleUnavailable(sampleId: string): boolean {
    return !!sampleManager.isUnavailable?.(sampleId);
  }

  /** The audio region's sample could not be re-read (its pack was removed). */
  isAudioRegionUnavailable(region: Pick<Region, 'audio'>): boolean {
    const id = region.audio?.sampleId;
    return !!id && this.isSampleUnavailable(id);
  }

  /**
   * Waveform revision of a sample: changes when its audio was reloaded in
   * place (pack update) or its availability changed. Key waveform drawing on it.
   */
  getSampleRevision(sampleId: string): number {
    return sampleManager.getRevision?.(sampleId) ?? 0;
  }

  /**
   * Samples whose audio was reloaded in place or whose availability changed
   * (redraw Song waveforms, update warning badges). Returns unsubscribe.
   */
  onSamplesChanged(cb: (sampleIds: string[]) => void): () => void {
    return sampleManager.onSamplesChanged?.(cb) ?? (() => undefined);
  }

  /**
   * Swaps the reloaded audio into the scheduled audio-region players of the
   * samples in `sampleIds`, so the next start of each region (this pass or
   * the next loop pass) plays the new audio. A region already sounding keeps
   * its old buffer to the end (it is still valid audio). When stopped there
   * are no players: the next play reads the new buffers.
   */
  private refreshAudioPlayers(sampleIds: readonly string[]): void {
    const ids = new Set(sampleIds);
    for (const track of this.arrangement.tracks) {
      if (track.instrument.type !== 'audio') continue;
      const players = this.trackAudio.get(track.id)?.players;
      if (!players || players.size === 0) continue;
      for (const region of track.regions) {
        if (!region.audio || !ids.has(region.audio.sampleId)) continue;
        const player = players.get(region.id);
        const buffer = sampleManager.getToneBuffer(region.audio.sampleId);
        if (!player || !buffer) continue;
        try {
          player.buffer = buffer;
        } catch (err) {
          console.warn('[arrangement] could not swap reloaded audio into a player', err);
        }
      }
    }
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
    return this.transportBeatAt();
  }

  /**
   * Musical transport position (beats) at an audio-context time (default: now,
   * without look-ahead). Derived from transport TICKS, so it stays correct
   * after tempo changes (seconds × current BPM does not: the seconds already
   * elapsed were played at the old tempo).
   */
  private transportBeatAt(time?: number): number {
    const transport = Tone.getTransport();
    const at = time ?? Tone.immediate();
    // Before the scheduled start (look-ahead / count-in): the start beat, never the previous run.
    const start = this.transportStart;
    if (start && this.transportState !== 'stopped' && at < start.time) return start.beat;
    try {
      const ticks = transport.getTicksAtTime(at);
      const ppq = transport.PPQ;
      if (typeof ticks === 'number' && Number.isFinite(ticks) && ppq > 0) return ticks / ppq;
    } catch {
      // fall through
    }
    // Test doubles without a tick clock: seconds at the current tempo.
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

  /**
   * The range the transport loops right now (null = no loop): the pattern
   * range while loop recording or playing the pattern (Beat Play), else the
   * loop markers when the Song loop is on. The pattern range is transient: it
   * never rewrites the stored markers.
   */
  getActiveLoopRange(): LoopMarkers | null {
    const rec = this.loopRec;
    if (rec) return { startBeat: rec.patternStart, endBeat: rec.patternStart + rec.loopBeats };
    if (this.patternPlayback) {
      const start = this.getPatternStart();
      return { startBeat: start, endBeat: start + this.getPatternLength() };
    }
    const markers = this.arrangement.loopMarkers;
    return this.loopEnabled && markers ? markers : null;
  }

  /** The transport plays the beat-view pattern (playPattern), looping its range. */
  isPatternPlayback(): boolean {
    return this.patternPlayback;
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

    const currentBeat = this.transportBeatAt();
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
    const name =
      type === 'drums' ? this.uniqueTrackName('Drums') : type === 'audio' ? 'Audio' : SYNTH_PRESETS[presetIndex]?.name ?? 'Synth';
    const instrument: TrackInstrument = { type, presetIndex: type === 'audio' ? 0 : presetIndex };
    if (type === 'drums') instrument.kit = createFactoryKit();
    const track: Track = {
      id: crypto.randomUUID(),
      name,
      instrument,
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
    if (type === 'drums' && !this.getActiveDrumTrackId()) this.beat().activeDrumTrackId = track.id;
    this.syncDrumSink();
    this.emitStateChange();
    if (type === 'drums') this.emitKitChange();
    return track;
  }

  /** `base`, or "base 2", "base 3"... when another track (not `exceptId`) already has that name. */
  private uniqueTrackName(base: string, exceptId?: string): string {
    const taken = new Set(this.arrangement.tracks.filter((t) => t.id !== exceptId).map((t) => t.name));
    if (!taken.has(base)) return base;
    let n = 2;
    while (taken.has(`${base} ${n}`)) n++;
    return `${base} ${n}`;
  }

  removeTrack(trackId: string): void {
    if (this.loopRec?.trackId === trackId) this.stopLoopRecording();
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
    this.syncDrumSink();
    this.emitStateChange();
    this.emitKitChange();
  }

  setTrackInstrument(trackId: string, instrument: TrackInstrument): void {
    this.pushUndoSnapshot('Change Instrument');
    const track = this.findTrack(trackId);
    if (!track) return;
    const next: TrackInstrument = { ...instrument };
    if (next.type === 'drums') {
      // Keep the track's kit unless a new one is given.
      next.kit = normalizeKit(instrument.kit ?? track.instrument.kit ?? createFactoryKit());
    } else {
      delete next.kit;
    }
    const wasDrums = track.instrument.type === 'drums';
    track.instrument = next;
    if (instrument.type === 'drums') {
      if (!wasDrums) track.name = this.uniqueTrackName('Drums', track.id);
    } else {
      track.name = SYNTH_PRESETS[instrument.presetIndex]?.name ?? 'Synth';
    }
    this.withPlaybackRestart(() => {
      this.disposeTrackAudio(trackId);
      this.createTrackAudio(track);
    });
    this.syncDrumSink();
    this.emitStateChange();
    this.emitKitChange();
  }

  setTrackSynthOverrides(trackId: string, overrides: Record<string, unknown>): void {
    this.pushUndoSnapshot('Edit Synth Params');
    const track = this.findTrack(trackId);
    if (!track || track.instrument.type !== 'synth') return;
    track.instrument.synthOverrides = { ...(track.instrument.synthOverrides ?? {}), ...overrides };
    this.withPlaybackRestart(() => {
      this.disposeTrackAudio(trackId);
      this.createTrackAudio(track);
    });
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
        // A disabled compressor leaves the path (no 6 ms look-ahead delay).
        setTrackCompressorEnabled(audio, compressorActive(effects.compressor.enabled, effects.compressor.ratio));
        break;
      case 'filter': {
        audio.filter.frequency.value = effects.filter.enabled ? effects.filter.cutoff : 18000;
        audio.filter.Q.value = effects.filter.enabled ? effects.filter.resonance : 1;
        audio.filter.type = effects.filter.type ?? 'lowpass';

        // LFO management
        const depth = effects.filter.enabled ? (effects.filter.lfoDepth ?? 0) : 0;
        if (depth > 0) {
          if (!audio.lfo) {
            const cutoff = effects.filter.cutoff;
            const minFreq = Math.max(60, cutoff * (1 - 0.75 * depth));
            const maxFreq = Math.min(18000, cutoff * (1 + 0.75 * depth));
            audio.lfo = new Tone.LFO({
              frequency: effects.filter.lfoRate ?? 1,
              type: effects.filter.lfoWave ?? 'sine',
              min: minFreq,
              max: maxFreq,
            }).start();
            audio.lfo.connect(audio.filter.frequency);
          } else {
            audio.lfo.frequency.value = effects.filter.lfoRate ?? 1;
            audio.lfo.type = effects.filter.lfoWave ?? 'sine';
            const cutoff = effects.filter.cutoff;
            audio.lfo.min = Math.max(60, cutoff * (1 - 0.75 * depth));
            audio.lfo.max = Math.min(18000, cutoff * (1 + 0.75 * depth));
          }
        } else if (audio.lfo) {
          audio.lfo.stop();
          audio.lfo.dispose();
          audio.lfo = null;
          // Restore static cutoff
          audio.filter.frequency.value = effects.filter.enabled ? effects.filter.cutoff : 18000;
        }
        break;
      }
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

  /** Arm a track for recording; arming a drum track also makes it the active drum track (pads play it). */
  armTrackForRecording(trackId: string | null): void {
    this.armedTrackId = trackId;
    if (trackId && this.findDrumTrack(trackId)) this.setActiveDrumTrack(trackId);
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

    const transport = this.resetTransport();
    transport.bpm.value = this.arrangement.bpm;

    // Punch recording: when loop is enabled with loop markers
    const loopMarkers = this.arrangement.loopMarkers;
    if (this.loopEnabled && loopMarkers) {
      this.punchRecording = true;
      this.recordingStartBeat = loopMarkers.startBeat;

      // Position transport at loop start
      transport.seconds = (loopMarkers.startBeat / this.arrangement.bpm) * 60;
      transport.loop = true;
      transport.loopStart = (loopMarkers.startBeat / this.arrangement.bpm) * 60;
      transport.loopEnd = (loopMarkers.endBeat / this.arrangement.bpm) * 60;

      // Schedule loop boundary callback to close open notes on each pass
      const loopEndSec = (loopMarkers.endBeat / this.arrangement.bpm) * 60;
      const safetyOffset = 0.05; // 50ms before loop end
      this.punchLoopCallbackId = transport.schedule(() => {
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
      transport.seconds = 0;
      this.recordingStartBeat = 0;
    }

    if (this.metronomeEnabled) this.metronome.start();

    this.startTransportAt(this.recordingStartBeat);
    this.startPositionUpdates();
    this.emitStateChange();
  }

  stopRecordingToTrack(): Region | null {
    if (this.transportState !== 'recording' || !this.recordingTrackId) return null;
    this.pushUndoSnapshot('Record');

    const loopMarkers = this.arrangement.loopMarkers;
    const isPunch = this.punchRecording;

    // Capture end beat BEFORE stopping transport (stopTransport resets position to 0)
    const endBeat = this.transportBeatAt();

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

  /**
   * Record a note-on. `time` (audio-context seconds) is when the note sounds;
   * omitted = now (a live hit). Pass the scheduled time for generated notes
   * (note repeat) so they land where they are heard.
   */
  captureNoteOn(
    note: number,
    velocity: number,
    isDrum: boolean,
    time?: number,
    opts?: CaptureNoteOptions,
  ): void {
    if (this.loopRec) {
      this.loopRecorder.captureHit(note, velocity, isDrum, time, opts?.gridBeats);
      return;
    }
    if (this.transportState !== 'recording') return;
    const currentBeat = this.transportBeatAt(time);

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

  /** Record a note-off (`time`: audio-context seconds, default now; see captureNoteOn). */
  captureNoteOff(note: number, time?: number): void {
    if (this.loopRec) {
      this.loopRecorder.captureRelease(note, time);
      return;
    }
    if (this.transportState !== 'recording') return;
    const open = this.recordingOpenNotes.get(note);
    if (!open) return;

    const currentBeat = this.transportBeatAt(time);

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
    }
    this.applyLoopToTransport();
    this.emitStateChange();
  }

  setLoopMarkers(startBeat: number, endBeat: number): void {
    this.pushUndoSnapshot('Set Loop Markers');
    const clamped = {
      startBeat: Math.max(0, startBeat),
      endBeat: Math.min(this.arrangement.lengthBeats, Math.max(startBeat + 1, endBeat)),
    };
    this.arrangement.loopMarkers = clamped;
    this.applyLoopToTransport();
    this.emitStateChange();
  }

  /** Put the active loop range (getActiveLoopRange) on the transport, at the current tempo. */
  private applyLoopToTransport(): void {
    const transport = Tone.getTransport();
    const range = this.getActiveLoopRange();
    if (range) {
      const secPerBeat = 60 / this.arrangement.bpm;
      transport.loop = true;
      transport.loopStart = range.startBeat * secPerBeat;
      transport.loopEnd = range.endBeat * secPerBeat;
    } else {
      transport.loop = false;
    }
  }

  // --- Transport ---

  play(): void {
    this.startPlayback(0);
  }

  /** Start playback at `fromBeat` (positioned BEFORE the transport starts: no seek right after the start). */
  private startPlayback(fromBeat: number): void {
    if (this.transportState !== 'stopped') return;
    this.transportState = 'playing';

    const transport = this.resetTransport();
    transport.bpm.value = this.arrangement.bpm;
    transport.seconds = (fromBeat / this.arrangement.bpm) * 60;

    this.scheduleAllTracks();
    this.applyLoopToTransport();
    if (this.shouldClick()) this.metronome.start();

    this.startTransportAt(fromBeat);
    this.startPositionUpdates();
    this.emitStateChange();
  }

  /**
   * Stop + clear the transport, without stopping an already-stopped one: Tone's
   * TickSource.stop() on a stopped clock CANCELS the earlier stop event, so tick
   * lookups until the next start extrapolate the previous run across the idle
   * gap (review B1: a stale playhead for ~100 ms after Play, Song auto-stop
   * firing at once after a long idle, hits right after Record stored at the old
   * position).
   * A start still PENDING (a Record count-in, or a start inside the look-ahead)
   * reads as 'stopped' but must be cancelled: otherwise the transport starts on
   * its own after Stop (review N1: Record again starts the pattern early, Play
   * jumps back when the old count-in would have ended).
   */
  private resetTransport(): ReturnType<typeof Tone.getTransport> {
    const transport = Tone.getTransport();
    const pending = this.transportStart !== null && this.transportStart.time > Tone.immediate();
    if (transport.state !== 'stopped') {
      transport.stop();
    } else if (pending) {
      // Cancel the pending start WITHOUT stop() on a stopped clock (that would
      // cancel the previous stop event and extrapolate the earlier run for one
      // tick: a stray pattern hit on an abort → immediate restart, review R1).
      // A start+stop at the same instant replaces the pending start cleanly.
      const t = Tone.now();
      transport.start(t);
      transport.stop(t);
    }
    transport.cancel();
    return transport;
  }

  /** Start the transport (at `time`, default now + look-ahead) and remember the start for transportBeatAt(). */
  private startTransportAt(beat: number, time?: number): void {
    const at = time ?? Tone.now();
    this.transportStart = { time: at, beat };
    Tone.getTransport().start(at);
  }

  stop(): void {
    if (this.loopRec) {
      this.stopLoopRecording();
      return;
    }
    if (this.transportState === 'recording') {
      this.stopRecordingToTrack();
      return;
    }
    this.stopTransport();
    this.emitStateChange();
  }

  private stopTransport(): void {
    const transport = this.resetTransport();
    transport.seconds = 0;
    this.transportStart = null;

    // Dispose all parts and players
    for (const audio of this.trackAudio.values()) {
      if (audio.part) {
        // Explicit time: with no argument Tone derives the stop tick from "now",
        // which right after `position = 0` can be -3e-13 and throws (RangeError).
        audio.part.stop(0);
        audio.part.dispose();
        audio.part = null;
      }
      // Notes were scheduled with future releases; fade out what is still ringing.
      audio.synth?.releaseAll();
      // Drum voices (long one-shots, held gates, hits inside the look-ahead): short fade.
      if (audio.sampler) {
        try {
          audio.sampler.stopAll(Tone.immediate());
        } catch (err) {
          console.warn('[arrangement] sampler stopAll failed', err);
        }
      }
      if (audio.players) {
        for (const p of audio.players.values()) {
          p.stop();
          p.dispose();
        }
        audio.players.clear();
      }
    }

    this.metronome.stop();
    this.metronome.disposeCountIn();
    this.stopPositionUpdates();
    this.transportState = 'stopped';
    // The pattern's transient loop ends with the playback: back to the Song loop.
    this.patternPlayback = false;
    this.applyLoopToTransport();
  }

  setPosition(beat: number): void {
    const start = this.transportStart;
    const pending = start !== null && Tone.immediate() < start.time;
    // Review N4: moved during playback before the scheduled start took effect.
    // Tone applies `seconds` at now + look-ahead (AFTER the pending start), and
    // the steps from the old position may already be scheduled: restart from
    // here instead (stopTransport cancels voices that have not started yet).
    if (pending && this.transportState === 'playing' && !this.loopRec) {
      const pattern = this.patternPlayback;
      this.stopTransport();
      this.patternPlayback = pattern;
      this.startPlayback(Math.max(0, Number.isFinite(beat) ? beat : 0));
      return;
    }
    const transport = Tone.getTransport();
    transport.seconds = (beat / this.arrangement.bpm) * 60;
    // Moved before the scheduled start took effect: it starts from here.
    if (start && pending) start.beat = beat;
  }

  setBpm(bpm: number): void {
    this.arrangement.bpm = bpm;
    Tone.getTransport().bpm.value = bpm;
    // Loop points are set in seconds at the tempo: re-apply them (Song loop, pattern or take).
    this.applyLoopToTransport();
    this.emitStateChange();
  }

  // --- Metronome ---

  setMetronomeEnabled(enabled: boolean): void {
    if (this.metronomeEnabled === enabled) return;
    this.metronomeEnabled = enabled;
    this.updateMetronome();
    this.emitStateChange();
  }

  /**
   * Clicks are wanted: the global toggle, or the beat view's metronome setting
   * (loop settings) while loop recording or playing the pattern (playPattern).
   */
  private shouldClick(): boolean {
    if (this.metronomeEnabled) return true;
    return (this.loopRec !== null || this.patternPlayback) && this.beat().loop.metronome;
  }

  private updateMetronome(): void {
    if (!this.shouldClick()) this.metronome.stop();
    else if (this.transportState !== 'stopped') this.metronome.start();
  }

  // --- Per-Track Audio ---

  private createTrackAudio(track: Track): void {
    // Migrate old tracks without pan/effects
    track.pan ??= 0;
    track.effects ??= structuredClone(DEFAULT_TRACK_EFFECTS);
    if (track.instrument.type === 'drums') track.instrument.kit ??= createFactoryKit();

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

      audio.part = this.buildNotePart(track, audio);
    }
  }

  /** Part event of a region note (drum patterns are stored straight; swing is applied here, at playback). */
  private partEvent(isDrums: boolean, region: Region, note: RegionNote): PartEvent {
    const beat = region.startBeat + note.startBeat;
    const loop = this.beat().loop;
    const playBeat = isDrums ? beat + swingOffsetBeats(beat, loopSwingGrid(loop), loop.swing) : beat;
    // Seconds at the current tempo: Tone converts them to ticks when the event is added.
    return { time: playBeat * (60 / this.arrangement.bpm), note, beat };
  }

  /** Tone.Part playing every note of a MIDI/drum track (null when it has none). */
  private buildNotePart(track: Track, audio: TrackAudioNodes): Tone.Part<PartEvent> | null {
    const isDrums = track.instrument.type === 'drums';
    const events: PartEvent[] = [];
    for (const region of track.regions) {
      for (const note of region.notes) events.push(this.partEvent(isDrums, region, note));
    }
    if (events.length === 0) return null;
    const part = new Tone.Part<PartEvent>((time, ev) => {
      const { note } = ev;
      // Note lengths are musical: convert them at the tempo of NOW (the BPM may
      // have changed since the Part was built).
      const secPerBeat = this.secPerBeatAt(time);
      if (isDrums) {
        if (!audio.sampler || this.consumeSuppressedHit(ev.beat, note.note, time)) return;
        playDrumNoteUtil(audio.sampler, note.note, note.velocity, time, note.durationBeats * secPerBeat);
      } else if (audio.synth) {
        audio.synth.triggerAttackRelease(midiToNoteName(note.note), note.durationBeats * secPerBeat, time, note.velocity);
      }
    }, events);
    part.start(0);
    return part;
  }

  /** Seconds per beat of the transport tempo at an audio time (falls back to the arrangement BPM). */
  private secPerBeatAt(time: number): number {
    try {
      const bpm = Tone.getTransport().bpm.getValueAtTime(time);
      if (typeof bpm === 'number' && Number.isFinite(bpm) && bpm > 0) return 60 / bpm;
    } catch {
      // fall through
    }
    return 60 / this.arrangement.bpm;
  }

  private consumeSuppressedHit(beat: number, note: number, time: number): boolean {
    return this.loopRecorder.consumeSuppressedHit(beat, note, time);
  }

  /** Add one new note to a track's running Part (loop recording); false = rebuild it instead. */
  private addToRunningPart(track: Track, region: Region, note: RegionNote): boolean {
    const part = this.trackAudio.get(track.id)?.part;
    if (!part) return false;
    try {
      part.add(this.partEvent(track.instrument.type === 'drums', region, note));
      return true;
    } catch (err) {
      console.warn('[arrangement] incremental pattern update failed, rebuilding', err);
      return false;
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
      if (this.transportState === 'playing' && !this.getActiveLoopRange() && beat >= this.arrangement.lengthBeats) {
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
      this.beginSession();
      for (const trackId of [...this.trackAudio.keys()]) this.disposeTrackAudio(trackId);
      this.arrangement = migrateArrangement(data);
      for (const track of this.arrangement.tracks) this.createTrackAudio(track);
      this.syncDrumSink();
      this.emitStateChange();
      this.emitKitChange();
      return true;
    } catch {
      return false;
    }
  }

  // --- Undo/Redo ---

  private pushUndoSnapshot(label: string): void {
    // Inside an undo group the snapshot would be discarded: don't clone.
    if (undoManager.isGrouping()) {
      undoManager.markDirty();
      return;
    }
    undoManager.pushUndo(label, structuredClone(this.arrangement));
  }

  /** "Before" snapshot for an edit committed later with commitUndo (null inside an undo group: no clone). */
  private undoSnapshot(): Arrangement | null {
    return undoManager.isGrouping() ? null : structuredClone(this.arrangement);
  }

  private commitUndo(label: string, snapshot: Arrangement | null): void {
    if (snapshot) undoManager.pushUndo(label, snapshot);
    else undoManager.markDirty();
  }

  /**
   * Group edits into ONE undo step (knob drags, paste, chop...). Groups nest:
   * the outermost begin captures the state (and label), the last end commits.
   * Every begin must be paired with an end.
   */
  beginUndoGroup(label: string): void {
    undoManager.beginUndoGroup(label, () => structuredClone(this.arrangement));
  }

  endUndoGroup(): void {
    undoManager.endUndoGroup((before) => JSON.stringify(before) === JSON.stringify(this.arrangement));
  }

  restoreFromSnapshot(snapshot: Arrangement): void {
    // Another project (undo/redo snapshots keep the arrangement id): a new session (N2).
    if (snapshot.id !== this.arrangement.id) this.beginSession();
    // Restoring never writes history: a running take ends without its own undo entry.
    if (this.loopRec) this.endLoopTake(false);
    this.withPlaybackRestart(() => {
      // Dispose all existing track audio
      for (const trackId of [...this.trackAudio.keys()]) {
        this.disposeTrackAudio(trackId);
      }

      // Migrate (legacy GM drum notes, kitless drum tracks, beat state) + recreate audio
      this.arrangement = migrateArrangement(structuredClone(snapshot));
      for (const track of this.arrangement.tracks) {
        this.createTrackAudio(track);
      }
    });

    this.syncDrumSink();
    this.emitStateChange();
    this.emitKitChange();
  }

  /**
   * Run a change that disposes/recreates track audio nodes. The old nodes own the
   * scheduled Parts, Players and automation callbacks, so while playing we stop
   * the transport first and resume from the same beat afterwards; otherwise the
   * transport would keep running with nothing (or disposed nodes) scheduled.
   */
  private withPlaybackRestart(change: () => void): void {
    // A loop-recording take can't survive its track nodes being rebuilt: end it.
    if (this.loopRec) this.stopLoopRecording();
    if (this.transportState !== 'playing') {
      change();
      return;
    }
    const beat = this.getPosition();
    const pattern = this.patternPlayback;
    this.stopTransport();
    change();
    this.patternPlayback = pattern;
    this.playFrom(beat);
  }

  /**
   * Before an undo/redo pops history: end a running loop take (its hits are
   * already ONE undo step; a take without hits leaves no entry) and commit what
   * an open gesture group changed so far as its own step (the group stays open
   * and is rebased after the restore), so the pop undoes exactly the last step.
   */
  private settleHistory(): void {
    if (this.loopRec) this.endLoopTake(false);
    undoManager.flushGroup(
      () => structuredClone(this.arrangement),
      (before) => JSON.stringify(before) === JSON.stringify(this.arrangement),
    );
  }

  performUndo(): void {
    this.settleHistory();
    const snapshot = undoManager.undo();
    if (!snapshot) return;
    // Push current state to redo
    undoManager.pushRedo('Undo', structuredClone(this.arrangement));
    this.restoreFromSnapshot(snapshot);
    if (undoManager.isGrouping()) undoManager.rebaseGroup(structuredClone(this.arrangement));
  }

  performRedo(): void {
    this.settleHistory();
    const snapshot = undoManager.redo();
    if (!snapshot) return;
    // Push current state to undo WITHOUT clearing the rest of the redo stack.
    undoManager.pushUndoFromRedo('Redo', structuredClone(this.arrangement));
    this.restoreFromSnapshot(snapshot);
    if (undoManager.isGrouping()) undoManager.rebaseGroup(structuredClone(this.arrangement));
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
    audio.filter.type = fx.filter.type ?? 'lowpass';

    // LFO reset
    const lfoDepth = fx.filter.enabled ? (fx.filter.lfoDepth ?? 0) : 0;
    if (lfoDepth > 0) {
      if (!audio.lfo) {
        const cutoff = fx.filter.cutoff;
        const minFreq = Math.max(60, cutoff * (1 - 0.75 * lfoDepth));
        const maxFreq = Math.min(18000, cutoff * (1 + 0.75 * lfoDepth));
        audio.lfo = new Tone.LFO({
          frequency: fx.filter.lfoRate ?? 1,
          type: fx.filter.lfoWave ?? 'sine',
          min: minFreq,
          max: maxFreq,
        }).start();
        audio.lfo.connect(audio.filter.frequency);
      }
    } else if (audio.lfo) {
      audio.lfo.stop();
      audio.lfo.dispose();
      audio.lfo = null;
    }

    // EQ
    audio.eq.low.value = fx.eq.enabled ? fx.eq.low : 0;
    audio.eq.mid.value = fx.eq.enabled ? fx.eq.mid : 0;
    audio.eq.high.value = fx.eq.enabled ? fx.eq.high : 0;

    // Compressor
    audio.compressor.threshold.value = fx.compressor.enabled ? fx.compressor.threshold : 0;
    audio.compressor.ratio.value = fx.compressor.enabled ? fx.compressor.ratio : 1;
    setTrackCompressorEnabled(audio, compressorActive(fx.compressor.enabled, fx.compressor.ratio));

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

  /**
   * Fresh project, ready to play: an empty arrangement with one "Drums" track
   * (factory kit, active) and the default loop settings (2 bars, 1/16, metronome
   * while recording). Clears the undo history.
   */
  resetToStarter(): void {
    this.beginSession();
    this.restoreFromSnapshot(this.createDefaultArrangement());
    this.loopEnabled = false;
    this.colorIndex = 0;
    this.ensureDrumTrack();
    this.setLoopSettings({});
    this.applyPatternLoop(this.getPatternStart(), this.getPatternLength());
    undoManager.clear();
  }

  loadFromTemplate(arrangement: Arrangement): void {
    this.beginSession();
    // Deep clone and replace all IDs with fresh UUIDs
    const clone = structuredClone(arrangement);
    clone.id = crypto.randomUUID();
    const trackIds = new Map<string, string>();
    for (const track of clone.tracks) {
      const id = crypto.randomUUID();
      trackIds.set(track.id, id);
      track.id = id;
      for (const region of track.regions) {
        region.id = crypto.randomUUID();
      }
    }
    if (clone.beat?.activeDrumTrackId) {
      clone.beat.activeDrumTrackId = trackIds.get(clone.beat.activeDrumTrackId) ?? null;
    }
    if (clone.markers) {
      for (const marker of clone.markers) {
        marker.id = crypto.randomUUID();
      }
    }
    if (clone.groups) {
      for (const group of clone.groups) {
        group.id = crypto.randomUUID();
        group.trackIds = group.trackIds.flatMap((id) => trackIds.get(id) ?? []);
      }
    }
    this.restoreFromSnapshot(clone);
  }

  // --- Helpers ---

  private findTrack(id: string): Track | undefined {
    return this.arrangement.tracks.find((t) => t.id === id);
  }

  dispose(): void {
    this.loopRecorder.abort();
    this.stopTransport();
    for (const trackId of this.trackAudio.keys()) {
      this.disposeTrackAudio(trackId);
    }
    this.metronome.dispose();
    this.syncDrumSink();
    this.stateCallbacks = [];
    this.positionCallbacks = [];
    this.kitCallbacks = [];
    this.kitSelectedCallbacks.clear();
  }

  // --- Piano roll editor support ---

  /**
   * Replace a region's notes as ONE undoable step (the piano roll calls this
   * once per gesture). Optionally grows the region (never shrinks it here).
   * If the transport is playing, the new notes are rescheduled immediately.
   */
  editRegionNotes(
    trackId: string,
    regionId: string,
    notes: RegionNote[],
    label = 'Edit Notes',
    newLengthBeats?: number,
  ): void {
    const track = this.findTrack(trackId);
    const region = track?.regions.find((r) => r.id === regionId);
    if (!track || !region) return;
    this.pushUndoSnapshot(label);
    region.notes = notes;
    if (newLengthBeats !== undefined && newLengthBeats > region.lengthBeats) {
      region.lengthBeats = newLengthBeats;
      this.autoExtendLength();
    }
    this.emitStateChange();
    this.refreshPlaybackSchedule(trackId);
  }

  /**
   * Audition a note through a track's own instrument + effects chain.
   * Always a fixed-length one-shot (attack + scheduled release), so it can
   * never leave a voice hanging.
   */
  previewNote(trackId: string, midi: number, velocity = 0.8, durationSec = 0.35): void {
    const track = this.findTrack(trackId);
    const audio = this.trackAudio.get(trackId);
    if (!track || !audio) return;
    const vel = Math.max(0.05, Math.min(1, velocity));
    const dur = Math.max(0.03, Math.min(2, durationSec));
    try {
      const now = Tone.immediate(); // no lookAhead latency for auditioning
      if (track.instrument.type === 'drums') {
        if (audio.sampler) playDrumNoteUtil(audio.sampler, midi, vel, now, dur);
      } else if (audio.synth) {
        audio.synth.triggerAttackRelease(midiToNoteName(Math.max(0, Math.min(127, Math.round(midi)))), dur, now, vel);
      }
    } catch {
      // Audio not started / node disposed: previews are best-effort.
    }
  }

  /** Start playback from an absolute beat (the piano roll's cue position). */
  playFrom(beat: number): void {
    this.startPlayback(Math.max(0, Number.isFinite(beat) ? beat : 0));
  }

  /** Move the playhead while playing (no-op when stopped: playback always starts from a cue). */
  seek(beat: number): void {
    if (this.transportState !== 'playing') return;
    this.setPosition(Math.max(0, beat));
  }

  /**
   * Rebuild one track's note Part after an edit during playback, without
   * stopping the transport (other tracks, metronome and position are untouched).
   * Mirrors the note branch of scheduleAllTracks(). Notes already sounding were
   * started with triggerAttackRelease, so disposing the old Part can't strand them.
   */
  private refreshPlaybackSchedule(trackId: string): void {
    if (this.transportState !== 'playing' && !this.loopRec) return;
    const track = this.findTrack(trackId);
    const audio = this.trackAudio.get(trackId);
    if (!track || !audio || track.instrument.type === 'audio') return;
    if (audio.part) {
      // Disposing cancels the Part's scheduled events. A bare stop() derives its
      // tick from "now", which right after a (re)start can be -1e-12 and throws.
      try {
        audio.part.stop(Tone.immediate());
      } catch {
        // disposed below anyway
      }
      audio.part.dispose();
      audio.part = null;
    }
    const hasSolo = this.arrangement.tracks.some((t) => t.solo);
    if (hasSolo ? !track.solo : track.muted) return;
    audio.part = this.buildNotePart(track, audio);
  }

  // ===========================================================================
  // Beat mode: drum kits, active drum track, step pattern, loop recording
  // ===========================================================================

  /** Beat state of the arrangement (created on first use). */
  private beat(): BeatState {
    return (this.arrangement.beat ??= normalizeBeatState(undefined, this.arrangement.tracks));
  }

  private findDrumTrack(trackId: string): Track | undefined {
    const track = this.findTrack(trackId);
    return track?.instrument.type === 'drums' ? track : undefined;
  }

  // --- Active drum track + live pad sink ---

  getActiveDrumTrackId(): string | null {
    const beat = this.beat();
    if (beat.activeDrumTrackId && this.findDrumTrack(beat.activeDrumTrackId)) return beat.activeDrumTrackId;
    // Stale or unset: fall back to the first drum track.
    beat.activeDrumTrackId = this.arrangement.tracks.find((t) => t.instrument.type === 'drums')?.id ?? null;
    return beat.activeDrumTrackId;
  }

  setActiveDrumTrack(trackId: string): void {
    if (!this.findDrumTrack(trackId) || this.beat().activeDrumTrackId === trackId) return;
    if (this.loopRec && this.loopRec.trackId !== trackId) this.stopLoopRecording();
    this.beat().activeDrumTrackId = trackId;
    this.syncDrumSink();
    this.emitStateChange();
    this.emitKitChange();
  }

  /** Active drum track id; creates a "Drums" track (and makes it active) when there is none. */
  ensureDrumTrack(): string {
    return this.getActiveDrumTrackId() ?? this.addTrack('drums').id;
  }

  /**
   * Point drumEngine's live pad sink at the active drum track's sampler (so live
   * hits go through that track's FX, pan and volume). Called whenever the
   * active track or its audio nodes change; no drum track → drumEngine's fallback kit.
   */
  private syncDrumSink(): void {
    const id = this.getActiveDrumTrackId();
    const sampler = id ? this.trackAudio.get(id)?.sampler ?? null : null;
    if (this.liveSink && this.liveSink.sampler !== sampler) {
      // Pads still held on the old sink (gate voices) would ring on the old
      // track forever: their note-off may never reach it. Release them now.
      this.liveSink.sink.releaseAllLive();
    }
    if (!sampler) {
      this.liveSink = null;
      drumEngine.setSink(null);
      return;
    }
    if (this.liveSink?.sampler !== sampler) {
      this.liveSink = { sampler, sink: createLiveSink(sampler) };
    }
    drumEngine.setSink(this.liveSink.sink);
  }

  // --- Kits ---

  /** The drum track's kit (a fresh factory kit for unknown / non-drum tracks). Treat as read-only. */
  getKit(trackId: string): KitState {
    const track = this.findDrumTrack(trackId);
    if (!track) return createFactoryKit();
    return (track.instrument.kit ??= createFactoryKit());
  }

  /** Replace a drum track's kit (undoable). */
  /**
   * Choose a whole kit for a drum track (undoable). `source` tags who chose it
   * (onKitSelected): every explicit choice is 'user' (the default); a pending
   * "Get & load" applying its kit passes 'pack-load' (review N2).
   */
  setKit(trackId: string, kit: KitState, source: KitSelectionSource = 'user'): void {
    const track = this.findDrumTrack(trackId);
    if (!track) return;
    this.pushUndoSnapshot('Change Kit');
    const next = normalizeKit(kit);
    this.applyKit(track, next);
    const event: KitSelectedEvent = { trackId, kitId: next.id, source };
    for (const cb of [...this.kitSelectedCallbacks]) {
      try {
        cb(event);
      } catch (err) {
        console.warn('[arrangement] kit-selected listener failed', err);
      }
    }
  }

  /**
   * A whole kit was chosen for a drum track (setKit), with who chose it. Pad
   * edits, undo/redo and project loads do not fire it. PackService drops a
   * pending "Get & load" on any 'user' selection (N2). Returns unsubscribe.
   */
  onKitSelected(cb: (e: KitSelectedEvent) => void): () => void {
    this.kitSelectedCallbacks.add(cb);
    return () => {
      this.kitSelectedCallbacks.delete(cb);
    };
  }

  /**
   * Change pad settings (undoable). For continuous edits (knob drags) wrap the
   * calls in beginUndoGroup/endUndoGroup so the gesture is one undo step.
   */
  updatePad(trackId: string, pad: PadIndex, patch: Partial<PadSettings>): void {
    const track = this.findDrumTrack(trackId);
    if (!track || !isPadIndex(pad)) return;
    this.pushUndoSnapshot('Edit Pad');
    const kit = this.getKit(trackId);
    const pads = kit.pads.slice();
    pads[pad] = normalizePad({ ...kit.pads[pad], ...patch });
    this.applyKit(track, { ...kit, pads });
  }

  /** Assign (or clear) a pad's sample (undoable). A different sample resets trim and reverse. */
  setPadSample(trackId: string, pad: PadIndex, sample: PadSample | null): void {
    const track = this.findDrumTrack(trackId);
    if (!track || !isPadIndex(pad)) return;
    this.pushUndoSnapshot(sample ? 'Assign Sample' : 'Clear Pad');
    const kit = this.getKit(trackId);
    const prev = kit.pads[pad];
    const sameSource =
      !!prev.sample && !!sample && padSourceKey(prev.sample.source) === padSourceKey(sample.source);
    const next: PadSettings = {
      ...prev,
      sample: sample ? { name: sample.name, source: { ...sample.source } } : null,
      ...(sameSource ? {} : { startSec: 0, endSec: null, reverse: false }),
    };
    const pads = kit.pads.slice();
    pads[pad] = normalizePad(next);
    this.applyKit(track, { ...kit, pads });
  }

  private applyKit(track: Track, kit: KitState): void {
    track.instrument.kit = kit;
    this.trackAudio.get(track.id)?.sampler?.setKit(kit);
    this.emitStateChange();
    this.emitKitChange();
  }

  onKitChange(callback: StateCallback): () => void {
    this.kitCallbacks.push(callback);
    return () => {
      this.kitCallbacks = this.kitCallbacks.filter((cb) => cb !== callback);
    };
  }

  private emitKitChange(): void {
    for (const cb of this.kitCallbacks) cb();
  }

  // --- Selected pad + loop settings ---

  getSelectedPad(): PadIndex {
    return this.beat().selectedPad;
  }

  setSelectedPad(pad: PadIndex): void {
    if (!isPadIndex(pad) || this.beat().selectedPad === pad) return;
    this.beat().selectedPad = pad;
    this.emitStateChange();
  }

  getLoopSettings(): LoopRecordSettings {
    return this.beat().loop;
  }

  /**
   * Update loop-recorder settings. Only a change of `bars` moves the loop
   * markers (to [patternStart, patternStart + bars*4)); other settings leave
   * user-set markers alone. Swing / swing-grid changes reschedule drum playback
   * (swing is applied at playback, so it is heard on recorded loops); the
   * record quantize does not change playback.
   */
  setLoopSettings(patch: Partial<LoopRecordSettings>): void {
    const beat = this.beat();
    const prev = beat.loop;
    beat.loop = normalizeLoopSettings({ ...prev, ...patch }, prev);
    const length = beat.loop.bars * 4;
    if (beat.loop.bars !== prev.bars) {
      this.applyPatternLoop(this.getPatternStart(), length);
      const rec = this.loopRec;
      if (rec) {
        this.loopRecorder.setLoopBeats(length);
        const track = this.findTrack(rec.trackId);
        if (track && this.ensurePatternRegionOn(track).changed) {
          this.loopRecorder.markStructureChanged();
          this.refreshPlaybackSchedule(track.id);
        }
      }
      this.applyLoopToTransport();
    }
    if (beat.loop.swing !== prev.swing || beat.loop.swingGrid !== prev.swingGrid) {
      for (const t of this.arrangement.tracks) {
        if (t.instrument.type === 'drums') this.refreshPlaybackSchedule(t.id);
      }
    }
    this.updateMetronome();
    this.emitStateChange();
  }

  /**
   * Beat-view Play: loop the pattern range [patternStart, patternStart + bars*4)
   * and play it from its start. The pattern range is used TRANSIENTLY (review
   * U6): the stored loop markers and the Song loop toggle are not changed, and
   * the transport goes back to the Song loop when playback stops. The step
   * grid's "Loop pattern" action (setLoopMarkers) is the explicit way to make
   * the markers match the pattern. The beat view's metronome setting (loop
   * settings) clicks while it plays.
   */
  playPattern(): void {
    if (this.transportState !== 'stopped' || this.loopRec) return;
    const start = this.getPatternStart();
    const end = start + this.getPatternLength();
    if (this.arrangement.lengthBeats < end) this.arrangement.lengthBeats = Math.ceil(end / 4) * 4;
    this.patternPlayback = true;
    this.playFrom(start);
    if (this.transportState === 'stopped') {
      this.patternPlayback = false;
      this.applyLoopToTransport();
    }
  }

  /** Absolute beat the pattern starts at: the loop start, or 0. */
  private getPatternStart(): number {
    return this.arrangement.loopMarkers?.startBeat ?? 0;
  }

  private getPatternLength(): number {
    return this.beat().loop.bars * 4;
  }

  /** Set the loop markers to the pattern range (an explicit bars change / a new project). */
  private applyPatternLoop(start: number, length: number): void {
    const markers = { startBeat: Math.max(0, start), endBeat: Math.max(0, start) + length };
    this.arrangement.loopMarkers = markers;
    if (this.arrangement.lengthBeats < markers.endBeat) {
      this.arrangement.lengthBeats = Math.ceil(markers.endBeat / 4) * 4;
    }
    this.applyLoopToTransport();
  }

  // --- Pattern region + step editing ---

  /** The (MIDI) region of a track covering the pattern start, preferring one that starts exactly there. */
  getPatternRegion(trackId: string): Region | null {
    const track = this.findTrack(trackId);
    if (!track) return null;
    const start = this.getPatternStart();
    let best: Region | null = null;
    for (const r of track.regions) {
      if (r.audio) continue;
      if (r.startBeat > start + STEP_EPSILON || start >= r.startBeat + r.lengthBeats - STEP_EPSILON) continue;
      if (!best || r.startBeat > best.startBeat) best = r;
    }
    return best;
  }

  /** Pattern region of the track, created (or grown to cover the whole pattern) if needed. Undoable. */
  ensurePatternRegion(trackId: string): Region {
    const track = this.findTrack(trackId) ?? this.findTrack(this.ensureDrumTrack())!;
    const snapshot = this.undoSnapshot();
    const { region, changed } = this.ensurePatternRegionOn(track);
    if (changed) {
      this.commitUndo('Add Pattern', snapshot);
      this.emitStateChange();
    }
    return region;
  }

  private ensurePatternRegionOn(track: Track): { region: Region; changed: boolean } {
    const start = this.getPatternStart();
    const length = this.getPatternLength();
    const existing = this.getPatternRegion(track.id);
    if (existing) {
      const needed = start - existing.startBeat + length;
      if (existing.lengthBeats >= needed - STEP_EPSILON) return { region: existing, changed: false };
      existing.lengthBeats = needed;
      this.autoExtendLength();
      return { region: existing, changed: true };
    }
    const region: Region = {
      id: crypto.randomUUID(),
      startBeat: start,
      lengthBeats: length,
      notes: [],
      color: track.color,
      name: 'Pattern',
    };
    track.regions.push(region);
    this.autoExtendLength();
    return { region, changed: true };
  }

  /** Predicate: note of `pad` inside the pattern window of `region` (and on `step`, when given). */
  private patternNoteFilter(region: Region, pad?: PadIndex, step?: { index: number; stepBeats: number }) {
    const offset = this.getPatternStart() - region.startBeat;
    const length = this.getPatternLength();
    const padNote = pad === undefined ? null : padToNote(pad);
    const steps = step ? Math.max(1, Math.round(length / step.stepBeats)) : 0;
    return (n: RegionNote): boolean => {
      if (padNote !== null ? n.note !== padNote : noteToPad(n.note) === null) return false;
      const local = n.startBeat - offset;
      if (local < -STEP_EPSILON || local >= length - STEP_EPSILON) return false;
      // Same (straight, clamped) step mapping as the step grid's buildStepMatrix.
      return !step || patternStepAt(local, step.stepBeats, steps) === step.index;
    };
  }

  /**
   * Toggle a step of the pattern for a pad (undoable). The note is stored on
   * the straight grid (step × stepBeats); swing is applied at playback.
   * `stepBeats` is the grid step length in beats (0.25 = 1/16).
   */
  toggleStep(trackId: string, pad: PadIndex, step: number, stepBeats: number, velocity = DEFAULT_STEP_VELOCITY): void {
    const track = this.findDrumTrack(trackId);
    if (!track || !isPadIndex(pad) || !Number.isInteger(step) || step < 0 || !(stepBeats > 0)) return;
    const local = step * stepBeats;
    if (local >= this.getPatternLength() - STEP_EPSILON) return;

    const snapshot = this.undoSnapshot();
    const { region } = this.ensurePatternRegionOn(track);
    const onStep = this.patternNoteFilter(region, pad, { index: step, stepBeats });
    if (region.notes.some(onStep)) {
      region.notes = region.notes.filter((n) => !onStep(n));
    } else {
      region.notes = [
        ...region.notes,
        {
          note: padToNote(pad),
          velocity: clamp01(velocity),
          startBeat: this.getPatternStart() - region.startBeat + local,
          durationBeats: stepBeats,
          isDrum: true,
        },
      ];
    }
    this.commitUndo('Toggle Step', snapshot);
    this.emitStateChange();
    this.refreshPlaybackSchedule(track.id);
  }

  /** Set the velocity (0..1) of a pad's note on a step (undoable; no-op when the step is empty). */
  setStepVelocity(trackId: string, pad: PadIndex, step: number, stepBeats: number, velocity: number): void {
    const track = this.findDrumTrack(trackId);
    const region = track ? this.getPatternRegion(trackId) : null;
    if (!track || !region || !isPadIndex(pad) || !(stepBeats > 0)) return;
    const onStep = this.patternNoteFilter(region, pad, { index: step, stepBeats });
    if (!region.notes.some(onStep)) return;
    this.pushUndoSnapshot('Step Velocity');
    const v = clamp01(velocity);
    region.notes = region.notes.map((n) => (onStep(n) ? { ...n, velocity: v } : n));
    this.emitStateChange();
    this.refreshPlaybackSchedule(track.id);
  }

  /** Remove the pattern's notes (all pads, or one pad). Undoable. */
  clearPattern(trackId: string, pad?: PadIndex): void {
    const region = this.getPatternRegion(trackId);
    if (!region || (pad !== undefined && !isPadIndex(pad))) return;
    const inPattern = this.patternNoteFilter(region, pad);
    if (!region.notes.some(inPattern)) return;
    this.pushUndoSnapshot('Clear Pattern');
    region.notes = region.notes.filter((n) => !inPattern(n));
    this.emitStateChange();
    this.refreshPlaybackSchedule(trackId);
  }

  // --- Loop recording (src/services/arrangement/LoopRecorder.ts) ---

  isLoopRecording(): boolean {
    return this.loopRecorder.isActive();
  }

  /** Beats of count-in left (0 when not counting in). */
  getCountInRemaining(): number {
    return this.loopRecorder.getCountInRemaining();
  }

  /**
   * Loop-record into the active drum track's pattern: the transport loops the
   * pattern range (transiently: the stored loop markers are not changed), an
   * optional count-in clicks, then the pattern loops while pad hits are
   * quantized (to the straight grid; swing is applied at playback) and merged
   * into it live. The whole take is one undo step.
   * Transactional: if starting fails, state is rolled back and listeners are
   * notified (never left stuck in 'recording').
   */
  startLoopRecording(): void {
    if (this.loopRec) return;
    if (this.transportState === 'recording') this.stopRecordingToTrack();
    else if (this.transportState === 'playing') this.stopTransport();

    const trackId = this.ensureDrumTrack();
    const track = this.findTrack(trackId);
    if (!track) return;
    const undoSnapshot = structuredClone(this.arrangement);
    let structureChanged = false;

    try {
      const loop = this.beat().loop;
      const patternStart = this.getPatternStart();
      const loopBeats = loop.bars * 4;
      structureChanged = this.ensurePatternRegionOn(track).changed;

      const secPerBeat = 60 / this.arrangement.bpm;
      const countInBeats = loop.countInBars * 4;
      this.loopRecorder.begin({ trackId, patternStart, loopBeats, countInBeats, undoSnapshot, structureChanged });
      this.transportState = 'recording';

      const transport = this.resetTransport();
      transport.bpm.value = this.arrangement.bpm;
      transport.seconds = patternStart * secPerBeat;
      this.applyLoopToTransport();

      this.scheduleAllTracks();
      if (this.shouldClick()) this.metronome.start();

      if (countInBeats > 0) {
        // Small margin so the first click is never in the past.
        const t0 = Tone.immediate() + 0.05;
        this.metronome.scheduleCountIn(t0, countInBeats, secPerBeat);
        const countInEnd = t0 + countInBeats * secPerBeat;
        this.startTransportAt(patternStart, countInEnd);
        this.loopRecorder.setCountInEnd(countInEnd);
      } else {
        this.startTransportAt(patternStart);
      }

      this.startPositionUpdates();
    } catch (err) {
      console.error('[arrangement] could not start loop recording', err);
      this.loopRecorder.abort();
      try {
        this.stopTransport();
      } catch {
        this.transportState = 'stopped';
      }
      if (structureChanged) this.restoreFromSnapshot(undoSnapshot);
      else this.applyLoopToTransport();
    }
    this.emitStateChange();
  }

  stopLoopRecording(): void {
    this.endLoopTake(true);
  }

  /**
   * End the take. `recordStructure`: a take without hits that created/grew the
   * pattern region still gets its undo step (false when ending it for an
   * undo/redo or a restore, which must not write history).
   */
  private endLoopTake(recordStructure: boolean): void {
    // Closes gate notes still held when the take ends.
    const take = this.loopRecorder.end();
    if (!take) return;
    if (recordStructure && !take.undoPushed && take.structureChanged) undoManager.pushUndo('Loop Record', take.undoSnapshot);
    this.stopTransport();
    this.emitStateChange();
  }
}

/**
 * Bring a loaded/restored track up to the current schema: default pan/effects,
 * and note velocities on the 0..1 scale (older projects and templates stored
 * MIDI 0..127 values, which the piano roll, playback and MIDI export misread).
 */
function migrateTrack(track: Track): void {
  track.pan ??= 0;
  track.effects ??= structuredClone(DEFAULT_TRACK_EFFECTS);
  for (const region of track.regions) {
    for (const note of region.notes) {
      if (note.velocity > 1) note.velocity = Math.min(1, note.velocity / 127);
    }
  }
  const dropped = migrateDrumTrack(track);
  if (dropped > 0) {
    console.warn(`[arrangement] "${track.name}": ${dropped} legacy drum note(s) have no pad and were dropped`);
  }
}

/** Migrate every track and validate the beat state (mutates and returns `arrangement`). */
function migrateArrangement(arrangement: Arrangement): Arrangement {
  for (const track of arrangement.tracks) migrateTrack(track);
  arrangement.beat = normalizeBeatState(arrangement.beat, arrangement.tracks);
  // Projects/templates without loop markers: the pattern (and the loop) is the
  // first `bars` bars, not whatever range the notes happen to cover.
  const m = arrangement.loopMarkers;
  if (!m || !Number.isFinite(m.startBeat) || !Number.isFinite(m.endBeat) || m.endBeat <= m.startBeat) {
    const endBeat = arrangement.beat.loop.bars * 4;
    arrangement.loopMarkers = { startBeat: 0, endBeat };
    if (!(arrangement.lengthBeats >= endBeat)) arrangement.lengthBeats = Math.ceil(endBeat / 4) * 4;
  }
  return arrangement;
}

export const arrangementEngine = new ArrangementEngine();
