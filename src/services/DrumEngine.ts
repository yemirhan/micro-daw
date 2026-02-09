import * as Tone from 'tone';
import { DRUM_NOTE_TO_PAD } from '@/utils/constants';
import type { DrumPadId } from '@/types/drums';

class DrumEngine {
  private volume: Tone.Volume | null = null;
  private started = false;

  // Synths for each drum sound
  private kick: Tone.MembraneSynth | null = null;
  private snare: Tone.NoiseSynth | null = null;
  private closedHH: Tone.NoiseSynth | null = null;
  private openHH: Tone.NoiseSynth | null = null;
  private clap: Tone.NoiseSynth | null = null;
  private tom: Tone.MembraneSynth | null = null;
  private crash: Tone.MetalSynth | null = null;
  private ride: Tone.MetalSynth | null = null;

  // Filters for hi-hats
  private hhFilter: Tone.Filter | null = null;

  async start(): Promise<void> {
    if (this.started) return;

    this.volume = new Tone.Volume(-6).toDestination();
    this.hhFilter = new Tone.Filter({ frequency: 8000, type: 'highpass' });
    this.hhFilter.connect(this.volume);

    // Kick — low membrane hit
    this.kick = new Tone.MembraneSynth({
      pitchDecay: 0.05,
      octaves: 6,
      oscillator: { type: 'sine' },
      envelope: { attack: 0.001, decay: 0.3, sustain: 0, release: 0.1 },
    });
    this.kick.connect(this.volume);

    // Snare — short white noise burst
    this.snare = new Tone.NoiseSynth({
      noise: { type: 'white' },
      envelope: { attack: 0.001, decay: 0.15, sustain: 0, release: 0.05 },
    });
    this.snare.connect(this.volume);

    // Closed HH — very short noise through highpass
    this.closedHH = new Tone.NoiseSynth({
      noise: { type: 'white' },
      envelope: { attack: 0.001, decay: 0.05, sustain: 0, release: 0.01 },
    });
    this.closedHH.connect(this.hhFilter);

    // Open HH — longer noise through highpass
    this.openHH = new Tone.NoiseSynth({
      noise: { type: 'white' },
      envelope: { attack: 0.001, decay: 0.3, sustain: 0, release: 0.1 },
    });
    this.openHH.connect(this.hhFilter);

    // Clap — short pink noise
    this.clap = new Tone.NoiseSynth({
      noise: { type: 'pink' },
      envelope: { attack: 0.001, decay: 0.12, sustain: 0, release: 0.05 },
    });
    this.clap.connect(this.volume);

    // Tom — mid-pitched membrane
    this.tom = new Tone.MembraneSynth({
      pitchDecay: 0.03,
      octaves: 4,
      oscillator: { type: 'sine' },
      envelope: { attack: 0.001, decay: 0.2, sustain: 0, release: 0.1 },
    });
    this.tom.connect(this.volume);

    // Crash — long metallic wash
    this.crash = new Tone.MetalSynth({
      frequency: 300,
      envelope: { attack: 0.001, decay: 1.4, release: 0.4 },
      harmonicity: 5.1,
      modulationIndex: 20,
      resonance: 800,
      octaves: 1.5,
    } as any);
    this.crash.connect(this.volume);

    // Ride — short metallic tap
    this.ride = new Tone.MetalSynth({
      frequency: 400,
      envelope: { attack: 0.001, decay: 0.5, release: 0.15 },
      harmonicity: 5.1,
      modulationIndex: 14,
      resonance: 1200,
      octaves: 1,
    } as any);
    this.ride.connect(this.volume);

    this.started = true;
  }

  hit(padId: DrumPadId, velocity: number): void {
    if (!this.started) return;
    const vol = velocity * 0.8 + 0.2; // minimum 20% volume

    switch (padId) {
      case 0: // Kick
        this.kick?.triggerAttackRelease('C1', '8n', Tone.now(), vol);
        break;
      case 1: // Snare
        this.snare?.triggerAttackRelease('8n', Tone.now(), vol);
        break;
      case 2: // Closed HH
        this.closedHH?.triggerAttackRelease('16n', Tone.now(), vol);
        break;
      case 3: // Open HH
        this.openHH?.triggerAttackRelease('8n', Tone.now(), vol);
        break;
      case 4: // Clap
        this.clap?.triggerAttackRelease('16n', Tone.now(), vol);
        break;
      case 5: // Tom
        this.tom?.triggerAttackRelease('G1', '8n', Tone.now(), vol);
        break;
      case 6: // Crash
        this.crash?.triggerAttackRelease(300, '16n', Tone.now(), vol);
        break;
      case 7: // Ride
        this.ride?.triggerAttackRelease(400, '32n', Tone.now(), vol);
        break;
    }
  }

  hitByMidiNote(midiNote: number, velocity: number): boolean {
    const padId = DRUM_NOTE_TO_PAD.get(midiNote);
    if (padId === undefined) return false;
    this.hit(padId, velocity);
    return true;
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
    this.kick?.dispose();
    this.snare?.dispose();
    this.closedHH?.dispose();
    this.openHH?.dispose();
    this.clap?.dispose();
    this.tom?.dispose();
    this.crash?.dispose();
    this.ride?.dispose();
    this.hhFilter?.dispose();
    this.volume?.dispose();
    this.kick = null;
    this.snare = null;
    this.closedHH = null;
    this.openHH = null;
    this.clap = null;
    this.tom = null;
    this.crash = null;
    this.ride = null;
    this.hhFilter = null;
    this.volume = null;
    this.started = false;
  }
}

export const drumEngine = new DrumEngine();
