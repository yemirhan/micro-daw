import * as Tone from 'tone';
import { DRUM_NOTE_TO_PAD } from '@/utils/constants';
import type { DrumPadId } from '@/types/drums';
import { DrumKit } from '@/audio/drumKit';
import { ensureMasterBus } from '@/audio/masterBus';
import { normVelocity } from '@/audio/voices/common';

/** Live drum pads (on-screen pads + MIDI pads). Same kit as drum tracks. */
class DrumEngine {
  private volume: Tone.Volume | null = null;
  private kit: DrumKit | null = null;
  private started = false;

  async start(): Promise<void> {
    if (this.started) return;
    ensureMasterBus();
    this.volume = new Tone.Volume(-6).toDestination();
    this.kit = new DrumKit();
    this.kit.connect(this.volume);
    this.started = true;
  }

  /** Hit a pad now. velocity 0..1 (0..127 is normalised). */
  hit(padId: DrumPadId, velocity: number): void {
    if (!this.started || !this.kit) return;
    try {
      this.kit.trigger(padId, normVelocity(velocity), Tone.immediate());
    } catch (err) {
      console.warn('[drums] hit failed', err);
    }
  }

  /** Hit by GM drum note (35..59 supported). Returns false for unmapped notes. */
  hitByMidiNote(midiNote: number, velocity: number): boolean {
    const padId = DRUM_NOTE_TO_PAD.get(midiNote);
    if (!this.started || !this.kit) return padId !== undefined;
    if (padId !== undefined) {
      this.hit(padId, velocity);
      return true;
    }
    return this.kit.triggerMidi(midiNote, normVelocity(velocity), Tone.immediate());
  }

  mute(): void {
    if (this.volume) this.volume.mute = true;
  }

  unmute(): void {
    if (this.volume) this.volume.mute = false;
  }

  setVolume(db: number): void {
    if (this.volume) {
      this.volume.volume.value = db;
    }
  }

  isStarted(): boolean {
    return this.started;
  }

  dispose(): void {
    this.kit?.dispose();
    this.volume?.dispose();
    this.kit = null;
    this.volume = null;
    this.started = false;
  }
}

export const drumEngine = new DrumEngine();
