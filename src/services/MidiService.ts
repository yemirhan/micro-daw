import { WebMidi, Input } from 'webmidi';
import type { MidiDeviceInfo, MidiCallbacks } from '@/types/midi';
import { MPK_MINI_PATTERN } from '@/utils/constants';
import { velocityToNormalized } from '@/utils/noteHelpers';

type DeviceChangeCallback = (devices: MidiDeviceInfo[]) => void;

class MidiService {
  private enabled = false;
  private activeInput: Input | null = null;
  private callbacks: MidiCallbacks | null = null;
  private deviceChangeCallbacks: DeviceChangeCallback[] = [];

  async enable(): Promise<void> {
    if (this.enabled) return;
    await WebMidi.enable();
    this.enabled = true;

    WebMidi.addListener('connected', () => this.handleDeviceChange());
    WebMidi.addListener('disconnected', () => this.handleDeviceChange());

    // Auto-select MPK Mini if connected
    this.autoSelectDevice();
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
    this.detachListeners();
    const input = WebMidi.inputs.find((i) => i.id === deviceId);
    if (!input) return;
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

  private autoSelectDevice(): void {
    const mpk = WebMidi.inputs.find((i) => MPK_MINI_PATTERN.test(i.name));
    if (mpk) {
      this.selectDevice(mpk.id);
    } else if (WebMidi.inputs.length > 0) {
      this.selectDevice(WebMidi.inputs[0].id);
    }
  }

  private handleDeviceChange(): void {
    const devices = this.getDevices();

    // If active device was disconnected, try to re-select
    if (this.activeInput && !WebMidi.inputs.find((i) => i.id === this.activeInput!.id)) {
      this.activeInput = null;
      this.autoSelectDevice();
    }

    this.deviceChangeCallbacks.forEach((cb) => cb(devices));
  }

  private attachListeners(): void {
    if (!this.activeInput) return;

    this.activeInput.addListener('noteon', (e) => {
      const velocity = velocityToNormalized(e.note.rawAttack);
      this.callbacks?.onNoteOn(e.note.number, velocity, e.message.channel);
    });

    this.activeInput.addListener('noteoff', (e) => {
      this.callbacks?.onNoteOff(e.note.number, e.message.channel);
    });

    this.activeInput.addListener('controlchange', (e) => {
      if (e.controller.number !== undefined && e.rawValue !== undefined) {
        this.callbacks?.onControlChange(e.controller.number, e.rawValue);
      }
    });
  }

  private detachListeners(): void {
    if (!this.activeInput) return;
    this.activeInput.removeListener('noteon');
    this.activeInput.removeListener('noteoff');
    this.activeInput.removeListener('controlchange');
  }

  isEnabled(): boolean {
    return this.enabled;
  }

  dispose(): void {
    this.detachListeners();
    this.activeInput = null;
    this.callbacks = null;
    this.deviceChangeCallbacks = [];
    if (this.enabled) {
      WebMidi.disable();
      this.enabled = false;
    }
  }
}

export const midiService = new MidiService();
