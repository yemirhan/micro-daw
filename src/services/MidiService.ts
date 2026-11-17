import { WebMidi, Input, MessageEvent, NoteMessageEvent, ControlChangeMessageEvent } from 'webmidi';
import type { MidiDeviceInfo, MidiCallbacks } from '@/types/midi';
import { MPK_MINI_PATTERN } from '@/utils/constants';
import { velocityToNormalized } from '@/utils/noteHelpers';

type DeviceChangeCallback = (devices: MidiDeviceInfo[]) => void;
type RawMessageCallback = (event: MessageEvent) => void;

const MIDI_ENABLE_TIMEOUT_MS = 5000;

class MidiService {
  private enabled = false;
  private activeInput: Input | null = null;
  private callbacks: MidiCallbacks | null = null;
  private deviceChangeCallbacks: DeviceChangeCallback[] = [];
  private rawCallbacks: RawMessageCallback[] = [];
  /** Exactly the listener functions we attached to activeInput (so we only ever remove ours). */
  private attached: { input: Input; listeners: [string, (e: never) => void][] } | null = null;
  /** Notes held on the active input ("channel:note"), so a lost input can be released. */
  private heldNotes = new Map<string, { note: number; channel: number }>();
  private deviceListenersInstalled = false;
  /**
   * The device the user (or the initial auto-select) chose. Kept across
   * unplug/replug and sleep/wake so the keyboard is re-attached when it returns,
   * even if another input was picked as a fallback in the meantime.
   */
  private preferredDeviceId: string | null = null;

  /**
   * Enables Web MIDI. Never throws: resolves false when Web MIDI is unavailable,
   * denied, or doesn't answer in time (e.g. plain/headless browsers), so the
   * rest of the app keeps working without a keyboard.
   */
  async enable(): Promise<boolean> {
    if (this.enabled) return true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
      await Promise.race([
        WebMidi.enable(),
        new Promise<never>((_, reject) => {
          timer = setTimeout(() => reject(new Error('Timed out waiting for Web MIDI access')), MIDI_ENABLE_TIMEOUT_MS);
        }),
      ]);
    } catch (err) {
      console.warn('[MidiService] Web MIDI unavailable:', err instanceof Error ? err.message : err);
      return false;
    } finally {
      clearTimeout(timer);
    }
    this.enabled = true;

    if (!this.deviceListenersInstalled) {
      this.deviceListenersInstalled = true;
      WebMidi.addListener('connected', this.handleDeviceChange);
      WebMidi.addListener('disconnected', this.handleDeviceChange);
    }

    // Auto-select MPK Mini if connected
    this.autoSelectDevice();
    return true;
  }

  /**
   * Feed a note/CC into the app exactly as if it came from the active MIDI
   * input (same callbacks -> useMidiNotes path). Used by the dev debug handle
   * (window.__microDaw) and tests to emulate a keyboard.
   * @param velocity normalized 0..1
   */
  simulateNoteOn(note: number, velocity: number, channel = 1): void {
    this.dispatchNoteOn(note, velocity, channel);
  }

  simulateNoteOff(note: number, channel = 1): void {
    this.dispatchNoteOff(note, channel);
  }

  simulateControlChange(cc: number, rawValue: number, channel = 1): void {
    this.dispatchControlChange(cc, rawValue, channel);
  }

  private dispatchNoteOn(note: number, velocity: number, channel: number): void {
    if (velocity <= 0) {
      // Note-on with velocity 0 is a note-off (webmidi already maps this for
      // real input; simulated input gets the same treatment).
      this.dispatchNoteOff(note, channel);
      return;
    }
    this.heldNotes.set(`${channel}:${note}`, { note, channel });
    this.callbacks?.onNoteOn(note, velocity, channel);
  }

  private dispatchNoteOff(note: number, channel: number): void {
    this.heldNotes.delete(`${channel}:${note}`);
    this.callbacks?.onNoteOff(note, channel);
  }

  /**
   * Send note-offs for every note still held on the input we are about to
   * lose (device switch, unplug, dispose): its real note-offs will never come.
   */
  private releaseHeldNotes(): void {
    const held = [...this.heldNotes.values()];
    this.heldNotes.clear();
    for (const { note, channel } of held) this.callbacks?.onNoteOff(note, channel);
  }

  private dispatchControlChange(cc: number, rawValue: number, channel: number): void {
    this.callbacks?.onControlChange(cc, rawValue, channel);
  }

  getDevices(): MidiDeviceInfo[] {
    if (!this.enabled) return [];
    return WebMidi.inputs.map((input) => ({
      id: input.id,
      name: input.name,
      manufacturer: input.manufacturer,
      connected: true,
    }));
  }

  selectDevice(deviceId: string): void {
    if (!WebMidi.inputs.some((i) => i.id === deviceId)) return;
    this.preferredDeviceId = deviceId;
    this.attachTo(deviceId);
  }

  private attachTo(deviceId: string): void {
    const input = WebMidi.inputs.find((i) => i.id === deviceId);
    if (!input) return;
    // Re-selecting the device we already listen to must not double the listeners.
    if (this.attached?.input === input && this.activeInput === input) return;
    this.detachListeners();
    this.activeInput = input;
    this.attachListeners();
  }

  getActiveDeviceId(): string | null {
    return this.activeInput?.id ?? null;
  }

  setCallbacks(callbacks: MidiCallbacks): void {
    this.callbacks = callbacks;
  }

  onDeviceChange(callback: DeviceChangeCallback): () => void {
    this.deviceChangeCallbacks.push(callback);
    return () => {
      this.deviceChangeCallbacks = this.deviceChangeCallbacks.filter((cb) => cb !== callback);
    };
  }

  onRawMessage(callback: RawMessageCallback): () => void {
    this.rawCallbacks.push(callback);
    return () => {
      this.rawCallbacks = this.rawCallbacks.filter((cb) => cb !== callback);
    };
  }

  /** Picks the preferred device if present, else an MPK, else the first input. */
  private autoSelectDevice(): void {
    const preferred = this.preferredDeviceId
      ? WebMidi.inputs.find((i) => i.id === this.preferredDeviceId)
      : undefined;
    if (preferred) {
      this.attachTo(preferred.id);
      return;
    }
    const fallback = WebMidi.inputs.find((i) => MPK_MINI_PATTERN.test(i.name)) ?? WebMidi.inputs[0];
    if (!fallback) return;
    // The first automatic pick becomes the preferred device; later fallbacks
    // (while the preferred one is unplugged) must not replace it.
    if (!this.preferredDeviceId) this.preferredDeviceId = fallback.id;
    this.attachTo(fallback.id);
  }

  private handleDeviceChange = (): void => {
    // If the active device was disconnected, drop it (releasing its held
    // notes) and try to pick another one.
    if (this.activeInput && !WebMidi.inputs.find((i) => i.id === this.activeInput!.id)) {
      this.detachListeners();
      this.activeInput = null;
      this.autoSelectDevice();
    } else if (
      !this.activeInput ||
      (this.preferredDeviceId &&
        this.activeInput.id !== this.preferredDeviceId &&
        WebMidi.inputs.some((i) => i.id === this.preferredDeviceId))
    ) {
      // Nothing selected (all inputs were gone), or the preferred keyboard came
      // back (replug, sleep/wake): (re)attach to it.
      this.autoSelectDevice();
    }

    const devices = this.getDevices();
    this.deviceChangeCallbacks.forEach((cb) => cb(devices));
  };

  private attachListeners(): void {
    const input = this.activeInput;
    if (!input) return;
    this.detachListeners();

    const onNoteOn = (e: NoteMessageEvent) => {
      const velocity = velocityToNormalized(e.note.rawAttack);
      this.dispatchNoteOn(e.note.number, velocity, e.message.channel);
    };
    const onNoteOff = (e: NoteMessageEvent) => {
      this.dispatchNoteOff(e.note.number, e.message.channel);
    };
    const onControlChange = (e: ControlChangeMessageEvent) => {
      if (e.controller.number !== undefined && e.rawValue !== undefined) {
        this.dispatchControlChange(e.controller.number, e.rawValue, e.message.channel);
      }
    };
    const onMidiMessage = (e: MessageEvent) => {
      if (this.rawCallbacks.length === 0) return;
      for (const cb of this.rawCallbacks) cb(e);
    };

    input.addListener('noteon', onNoteOn);
    input.addListener('noteoff', onNoteOff);
    input.addListener('controlchange', onControlChange);
    input.addListener('midimessage', onMidiMessage);
    this.attached = {
      input,
      listeners: [
        ['noteon', onNoteOn],
        ['noteoff', onNoteOff],
        ['controlchange', onControlChange],
        ['midimessage', onMidiMessage],
      ] as [string, (e: never) => void][],
    };
  }

  private detachListeners(): void {
    const attached = this.attached;
    this.attached = null;
    if (attached) {
      for (const [event, listener] of attached.listeners) {
        try {
          (attached.input.removeListener as (event: string, listener: unknown) => void)(event, listener);
        } catch {
          // Port already gone; nothing to remove.
        }
      }
    }
    this.releaseHeldNotes();
  }

  /** Detach from inputs without disabling Web MIDI (used on dev hot reload). */
  detachForReload(): void {
    this.detachListeners();
    if (this.deviceListenersInstalled) {
      WebMidi.removeListener('connected', this.handleDeviceChange);
      WebMidi.removeListener('disconnected', this.handleDeviceChange);
      this.deviceListenersInstalled = false;
    }
  }

  isEnabled(): boolean {
    return this.enabled;
  }

  dispose(): void {
    this.detachListeners();
    this.activeInput = null;
    this.preferredDeviceId = null;
    this.callbacks = null;
    if (this.deviceListenersInstalled) {
      WebMidi.removeListener('connected', this.handleDeviceChange);
      WebMidi.removeListener('disconnected', this.handleDeviceChange);
      this.deviceListenersInstalled = false;
    }
    this.deviceChangeCallbacks = [];
    this.rawCallbacks = [];
    if (this.enabled) {
      WebMidi.disable();
      this.enabled = false;
    }
  }
}

export const midiService = new MidiService();

// Dev HMR: a reloaded MidiService module must not leave the old instance's
// listeners on the (shared, not reloaded) webmidi inputs, or every key would
// be dispatched twice.
if (import.meta.hot) {
  import.meta.hot.dispose(() => midiService.detachForReload());
}
