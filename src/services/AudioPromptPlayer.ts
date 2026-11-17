import { audioEngine } from './AudioEngine';
import type { AudioPrompt } from '@/types/appMode';

/** Velocity for prompt notes (audioEngine expects 0..1). */
export const PROMPT_VELOCITY = 0.8;

type PlaybackListener = (playing: boolean) => void;

/**
 * Plays ear-training prompts (interval / chord / note sequence) on the live
 * synth.
 *
 * Invariant: every note this player starts is released by this player, even
 * when playback is interrupted. `stop()` cancels pending note-ons AND releases
 * every note that has already started, so leaving a step / lesson / mode in
 * the middle of a prompt can never leave a note ringing.
 */
class AudioPromptPlayer {
  private timeouts = new Set<ReturnType<typeof setTimeout>>();
  /** Notes this player has attacked and not yet released. */
  private sounding = new Set<number>();
  private listeners = new Set<PlaybackListener>();
  private playing = false;

  play(prompt: AudioPrompt): void {
    this.stop();
    const notes = prompt.notes.filter((n) => Number.isFinite(n));
    if (notes.length === 0) return;
    const duration = Math.max(0.05, prompt.noteDuration ?? 0.5);
    const gap = Math.max(0, prompt.gap ?? 0.3);

    this.setPlaying(true);
    if (prompt.type === 'chord') {
      for (const note of new Set(notes)) this.attack(note);
      this.later(duration * 1000, () => {
        for (const note of new Set(notes)) this.release(note);
        this.setPlaying(false);
      });
      return;
    }

    // Sequential: interval or note-sequence
    notes.forEach((note, i) => {
      const onTime = i * (duration + gap) * 1000;
      const offTime = onTime + duration * 1000;
      if (onTime === 0) this.attack(note);
      else this.later(onTime, () => this.attack(note));
      this.later(offTime, () => {
        this.release(note);
        if (i === notes.length - 1) this.setPlaying(false);
      });
    });
  }

  /** Cancel pending notes and release everything already sounding. */
  stop(): void {
    for (const t of this.timeouts) clearTimeout(t);
    this.timeouts.clear();
    for (const note of [...this.sounding]) this.release(note);
    this.setPlaying(false);
  }

  isPlaying(): boolean {
    return this.playing;
  }

  /** Notes currently held by the prompt player (for tests / debugging). */
  getSoundingNotes(): number[] {
    return [...this.sounding].sort((a, b) => a - b);
  }

  onPlaybackChange(listener: PlaybackListener): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  private attack(note: number): void {
    // Repeated pitch (e.g. a melody with repeated notes): release first so
    // the synth never holds two voices for one pitch.
    if (this.sounding.has(note)) audioEngine.noteOff(note);
    this.sounding.add(note);
    audioEngine.noteOn(note, PROMPT_VELOCITY);
  }

  private release(note: number): void {
    if (!this.sounding.delete(note)) return;
    audioEngine.noteOff(note);
  }

  private later(ms: number, fn: () => void): void {
    const t = setTimeout(() => {
      this.timeouts.delete(t);
      fn();
    }, ms);
    this.timeouts.add(t);
  }

  private setPlaying(playing: boolean): void {
    if (this.playing === playing) return;
    this.playing = playing;
    for (const l of [...this.listeners]) l(playing);
  }
}

export const audioPromptPlayer = new AudioPromptPlayer();
