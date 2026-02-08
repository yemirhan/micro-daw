import { audioEngine } from './AudioEngine';
import type { AudioPrompt } from '@/types/appMode';

class AudioPromptPlayer {
  private timeouts: ReturnType<typeof setTimeout>[] = [];

  play(prompt: AudioPrompt): void {
    this.stop();
    const duration = prompt.noteDuration ?? 0.5;
    const gap = prompt.gap ?? 0.3;

    if (prompt.type === 'chord') {
      // Play all notes simultaneously
      for (const note of prompt.notes) {
        audioEngine.noteOn(note, 80);
      }
      const off = setTimeout(() => {
        for (const note of prompt.notes) {
          audioEngine.noteOff(note);
        }
      }, duration * 1000);
      this.timeouts.push(off);
    } else {
      // Sequential: interval or note-sequence
      prompt.notes.forEach((note, i) => {
        const onTime = i * (duration + gap) * 1000;
        const offTime = onTime + duration * 1000;

        const onTimer = setTimeout(() => {
          audioEngine.noteOn(note, 80);
        }, onTime);

        const offTimer = setTimeout(() => {
          audioEngine.noteOff(note);
        }, offTime);

        this.timeouts.push(onTimer, offTimer);
      });
    }
  }

  stop(): void {
    for (const t of this.timeouts) clearTimeout(t);
    this.timeouts = [];
  }
}

export const audioPromptPlayer = new AudioPromptPlayer();
