/**
 * Voice allocation (pure TS, no Tone.js) -- the single source of truth for
 * "which voice is playing which MIDI note".
 *
 * Guarantees relied on by the audio layer:
 *  - A note is held by at most one voice. Re-triggering a sounding note
 *    re-uses that voice (no leaked duplicate voices).
 *  - noteOff(note) always finds and releases the voice holding `note`.
 *  - releaseAll() returns every busy voice and clears all state.
 *  - When polyphony is exceeded the voice to steal is: a voice already in its
 *    release tail (oldest release first), otherwise the oldest held voice --
 *    sparing the lowest held note (the bass) when possible.
 *
 * Times are AudioContext seconds. Scheduled one-shots (sequencer playback)
 * use future times; comparisons are made against the event time, so the
 * allocator stays correct with Tone's lookahead scheduling.
 */

export interface VoiceSlot {
  readonly index: number;
  /** MIDI note this voice is assigned to (null = never used / hard-stopped). */
  note: number | null;
  velocity: number;
  startTime: number;
  /** When the key is/was released (Infinity while held). */
  releaseTime: number;
  /** When the release tail is over and the voice is silent. */
  endTime: number;
}

export interface Allocation {
  voice: number;
  /** The voice was still sounding a different note (cut off). */
  stolen: boolean;
  /** Same note re-struck on the voice that was holding/releasing it. */
  retrigger: boolean;
  /** Note the voice was playing before (stolen or retriggered). */
  previousNote: number | null;
}

const EPS = 1e-6;

export class PolyVoiceAllocator {
  private readonly slots: VoiceSlot[] = [];

  constructor(public maxVoices: number) {
    if (maxVoices < 1) this.maxVoices = 1;
  }

  /** Number of voice objects ever allocated (pool size). */
  get poolSize(): number {
    return this.slots.length;
  }

  getSlot(index: number): VoiceSlot | undefined {
    return this.slots[index];
  }

  /**
   * Assign a voice to `note` starting at `time`.
   * `releaseDuration` is not known yet; the slot stays held until noteOff.
   */
  noteOn(note: number, time: number, velocity = 1): Allocation {
    const sameNote = this.findVoiceForNote(note, time);
    if (sameNote) {
      return this.assign(sameNote, note, time, velocity, { stolen: false, retrigger: true });
    }

    const free = this.findFreeVoice(time);
    if (free) return this.assign(free, note, time, velocity, { stolen: false, retrigger: false });

    if (this.slots.length < this.maxVoices) {
      const slot: VoiceSlot = {
        index: this.slots.length,
        note: null,
        velocity: 0,
        startTime: -Infinity,
        releaseTime: -Infinity,
        endTime: -Infinity,
      };
      this.slots.push(slot);
      return this.assign(slot, note, time, velocity, { stolen: false, retrigger: false });
    }

    const victim = this.chooseVictim(time);
    return this.assign(victim, note, time, velocity, { stolen: true, retrigger: false });
  }

  /**
   * Release the held voice(s) playing `note`. Returns the released voice
   * indexes (normally 0 or 1 entries).
   */
  noteOff(note: number, time: number, releaseDuration: number): number[] {
    const released: number[] = [];
    for (const slot of this.slots) {
      if (slot.note === note && slot.releaseTime === Infinity) {
        slot.releaseTime = Math.max(time, slot.startTime);
        slot.endTime = slot.releaseTime + Math.max(0, releaseDuration);
        released.push(slot.index);
      }
    }
    return released;
  }

  /**
   * Book a self-releasing note (attack at `time`, release at `time + duration`).
   */
  oneShot(note: number, time: number, duration: number, releaseDuration: number, velocity = 1): Allocation {
    const alloc = this.noteOn(note, time, velocity);
    const slot = this.slots[alloc.voice];
    slot.releaseTime = time + Math.max(0, duration);
    slot.endTime = slot.releaseTime + Math.max(0, releaseDuration);
    return alloc;
  }

  /** Mark a voice silent from `time` (hard stop / fast fade). */
  silence(index: number, time: number, fadeDuration = 0): void {
    const slot = this.slots[index];
    if (!slot) return;
    slot.releaseTime = Math.min(slot.releaseTime, time);
    slot.endTime = time + fadeDuration;
  }

  /**
   * Release everything immediately: returns the indexes of every voice that
   * was held or still sounding at `time`, and resets all slots to idle.
   */
  releaseAll(time: number): number[] {
    const busy: number[] = [];
    for (const slot of this.slots) {
      if (slot.note !== null && (slot.releaseTime === Infinity || slot.endTime > time + EPS)) {
        busy.push(slot.index);
      }
      slot.note = null;
      slot.releaseTime = -Infinity;
      slot.endTime = -Infinity;
    }
    return busy;
  }

  /** Voices that are held or still in their release tail at `time`. */
  activeVoiceCount(time: number): number {
    let n = 0;
    for (const slot of this.slots) {
      if (slot.note !== null && (slot.releaseTime === Infinity || slot.endTime > time + EPS)) n++;
    }
    return n;
  }

  /** Notes whose key is currently held (no noteOff yet). */
  heldNotes(): number[] {
    return this.slots
      .filter((s) => s.note !== null && s.releaseTime === Infinity)
      .map((s) => s.note as number)
      .sort((a, b) => a - b);
  }

  /** Voice currently assigned to `note` (held or releasing), if any. */
  voiceForNote(note: number, time: number): number | null {
    return this.findVoiceForNote(note, time)?.index ?? null;
  }

  // -------------------------------------------------------------------------

  private findVoiceForNote(note: number, time: number): VoiceSlot | undefined {
    return this.slots.find(
      (s) => s.note === note && (s.releaseTime === Infinity || s.endTime > time + EPS),
    );
  }

  private findFreeVoice(time: number): VoiceSlot | undefined {
    let best: VoiceSlot | undefined;
    for (const slot of this.slots) {
      const free = slot.note === null || (slot.releaseTime !== Infinity && slot.endTime <= time + EPS);
      if (!free) continue;
      // Least-recently-finished voice first (lets the newest tails ring out).
      if (!best || slot.endTime < best.endTime) best = slot;
    }
    return best;
  }

  private chooseVictim(time: number): VoiceSlot {
    // 1) Voices already released (in their tail): the one released longest ago.
    let releasing: VoiceSlot | undefined;
    for (const slot of this.slots) {
      if (slot.releaseTime !== Infinity && slot.releaseTime <= time + EPS) {
        if (!releasing || slot.releaseTime < releasing.releaseTime) releasing = slot;
      }
    }
    if (releasing) return releasing;

    // 2) Oldest held voice, sparing the lowest note (bass) if there is a choice.
    const held = this.slots.filter((s) => s.note !== null);
    let lowest: VoiceSlot | undefined;
    for (const s of held) if (!lowest || (s.note as number) < (lowest.note as number)) lowest = s;
    let oldest: VoiceSlot | undefined;
    for (const s of held) {
      if (held.length > 1 && s === lowest) continue;
      if (!oldest || s.startTime < oldest.startTime) oldest = s;
    }
    return oldest ?? this.slots[0];
  }

  private assign(
    slot: VoiceSlot,
    note: number,
    time: number,
    velocity: number,
    flags: { stolen: boolean; retrigger: boolean },
  ): Allocation {
    const previousNote = slot.note;
    const wasSounding =
      slot.note !== null && (slot.releaseTime === Infinity || slot.endTime > time + EPS);
    slot.note = note;
    slot.velocity = velocity;
    slot.startTime = time;
    slot.releaseTime = Infinity;
    slot.endTime = Infinity;
    return {
      voice: slot.index,
      stolen: flags.stolen || (!flags.retrigger && wasSounding),
      retrigger: flags.retrigger,
      previousNote,
    };
  }
}

// ---------------------------------------------------------------------------
// Monophonic note stack (last-note priority)
// ---------------------------------------------------------------------------

export type MonoAction =
  /** Start a new note from silence (or re-articulate when legato is off). */
  | { type: 'attack'; note: number; velocity: number; from: number | null }
  /** Glide to `note` without re-triggering envelopes. */
  | { type: 'legato'; note: number; velocity: number; from: number }
  /** All keys up: release the voice. */
  | { type: 'release'; note: number }
  /** Nothing audible changes (a non-sounding key was released). */
  | { type: 'none' };

export class MonoNoteStack {
  private stack: { note: number; velocity: number }[] = [];
  /** Note the voice is (or was last) sounding. */
  private sounding: number | null = null;

  constructor(public legato = true) {}

  noteOn(note: number, velocity: number): MonoAction {
    const wasEmpty = this.stack.length === 0;
    const from = this.sounding;
    this.stack = this.stack.filter((e) => e.note !== note);
    this.stack.push({ note, velocity });
    this.sounding = note;
    if (wasEmpty || !this.legato || from === null) {
      return { type: 'attack', note, velocity, from };
    }
    return { type: 'legato', note, velocity, from };
  }

  noteOff(note: number): MonoAction {
    const idx = this.stack.findIndex((e) => e.note === note);
    if (idx === -1) return { type: 'none' };
    const wasTop = idx === this.stack.length - 1;
    this.stack.splice(idx, 1);
    if (!wasTop) return { type: 'none' };
    if (this.stack.length === 0) {
      const released = this.sounding ?? note;
      return { type: 'release', note: released };
    }
    const top = this.stack[this.stack.length - 1];
    const from = this.sounding ?? note;
    this.sounding = top.note;
    // Falling back to a still-held key always glides (no re-attack), even when
    // legato is off -- re-attacking on release sounds like a double trigger.
    return { type: 'legato', note: top.note, velocity: top.velocity, from };
  }

  releaseAll(): void {
    this.stack = [];
  }

  /** Keys currently held, oldest first. */
  heldNotes(): number[] {
    return this.stack.map((e) => e.note);
  }

  current(): number | null {
    return this.stack.length ? this.stack[this.stack.length - 1].note : null;
  }

  lastSounding(): number | null {
    return this.sounding;
  }
}
