import * as Tone from 'tone';

/**
 * Click level (dB on the click synth). A full-scale sine click at -6 dB peaked
 * at about -4 dBFS, ~6 dB over a full-velocity kick (review U5); -15 dB puts it
 * ~9 dB lower (≈ -13 dBFS): clearly audible, but under the drums.
 */
export const CLICK_VOLUME_DB = -15;

/** Accented (bar) and plain click pitches. */
const ACCENT = 'C6';
const BEAT = 'G5';

/** A sine click synth (straight to the destination: never through the master bus or a track). */
function createClickSynth(): Tone.Synth {
  return new Tone.Synth({
    oscillator: { type: 'sine' },
    envelope: { attack: 0.001, decay: 0.05, sustain: 0, release: 0.01 },
    volume: CLICK_VOLUME_DB,
  }).toDestination();
}

/**
 * The arrangement's metronome: a quarter-note click on the transport (accent on
 * bar starts, derived from the transport position so it is right across loop
 * wraps), plus the loop recorder's count-in clicks.
 *
 * Count-in clicks are pre-scheduled in audio time on their OWN synth, disposed
 * when the take stops: disposing silences clicks still scheduled, and the
 * metronome synth never sees out-of-order times.
 */
export class Metronome {
  private synth: Tone.Synth | null = null;
  private loop: Tone.Loop | null = null;
  /** Last click time on `synth` (a Synth asserts its events are scheduled in order). */
  private lastClickTime = -Infinity;
  private countInSynth: Tone.Synth | null = null;

  isRunning(): boolean {
    return this.loop !== null;
  }

  /** Start clicking on the transport (no-op when already running). */
  start(): void {
    if (this.loop) return;
    if (!this.synth) {
      this.synth = createClickSynth();
      this.lastClickTime = -Infinity;
    }
    const synth = this.synth;
    const transport = Tone.getTransport();

    let count = 0;
    this.loop = new Tone.Loop((time) => {
      // A Synth asserts its events are in time order: tempo changes and loop
      // wraps can hand us a time at/before the last click. Skip it, never throw
      // inside Tone's clock callback.
      if (!(time > this.lastClickTime)) return;
      // Accent bar starts from the transport position (stays right across loop wraps).
      let beat = count++;
      try {
        const ticks = transport.getTicksAtTime(time);
        if (typeof ticks === 'number' && Number.isFinite(ticks)) beat = Math.round(ticks / transport.PPQ);
      } catch {
        // keep the counter
      }
      try {
        synth.triggerAttackRelease(beat % 4 === 0 ? ACCENT : BEAT, '32n', time);
        this.lastClickTime = time;
      } catch (err) {
        console.warn('[metronome] click skipped', err);
      }
    }, '4n');
    this.loop.start(0);
  }

  stop(): void {
    if (!this.loop) return;
    this.loop.stop();
    this.loop.dispose();
    this.loop = null;
  }

  /**
   * Schedule `beats` count-in clicks from audio time `t0`, one per `secPerBeat`
   * (accent every 4). Replaces a previous count-in.
   */
  scheduleCountIn(t0: number, beats: number, secPerBeat: number): void {
    this.disposeCountIn();
    if (!(beats > 0)) return;
    const synth = (this.countInSynth = createClickSynth());
    for (let i = 0; i < beats; i++) {
      synth.triggerAttackRelease(i % 4 === 0 ? ACCENT : BEAT, '32n', t0 + i * secPerBeat);
    }
  }

  /** Silence and drop the count-in clicks (scheduled ones included). */
  disposeCountIn(): void {
    if (!this.countInSynth) return;
    try {
      this.countInSynth.dispose();
    } catch {
      // already disposed
    }
    this.countInSynth = null;
  }

  dispose(): void {
    this.stop();
    this.disposeCountIn();
    this.synth?.dispose();
    this.synth = null;
  }
}
