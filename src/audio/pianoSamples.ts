import * as Tone from 'tone';

/**
 * Salamander Grand Piano V3 (Alexander Holm, CC-BY 3.0) -- reduced set: one
 * sample every minor third from A0 to C8 (30 files, ~2 MB of MP3s), bundled in
 * src/audio/samples/piano. See the README there for attribution.
 *
 * The files are imported with Vite's `?inline` (base64 data URLs) and decoded
 * with atob + decodeAudioData, so loading never touches fetch()/file:// and
 * works identically in the dev server, packaged Electron builds and tests.
 * Each sample lives in its own lazily-imported chunk.
 */
const SAMPLE_MODULES = import.meta.glob<string>('./samples/piano/*.mp3', {
  query: '?inline',
  import: 'default',
});

const PITCH: Record<string, number> = { C: 0, Ds: 3, Fs: 6, A: 9 };

/** './samples/piano/Fs3.mp3' -> 54 */
export function sampleFileToMidi(path: string): number {
  const m = /([A-G]s?)(\d)\.mp3$/.exec(path);
  if (!m || !(m[1] in PITCH)) return -1;
  return (parseInt(m[2], 10) + 1) * 12 + PITCH[m[1]];
}

export type PianoBuffers = Map<number, Tone.ToneAudioBuffer>;

let buffers: PianoBuffers | null = null;
let loading: Promise<PianoBuffers> | null = null;
const listeners = new Set<(b: PianoBuffers) => void>();

export function getPianoBuffers(): PianoBuffers | null {
  return buffers;
}

export function isPianoLoaded(): boolean {
  return buffers !== null;
}

/** Called once the samples are decoded (immediately if they already are). Returns an unsubscribe fn. */
export function onPianoLoaded(cb: (b: PianoBuffers) => void): () => void {
  if (buffers) {
    cb(buffers);
    return () => undefined;
  }
  listeners.add(cb);
  return () => listeners.delete(cb);
}

function dataUrlToArrayBuffer(dataUrl: string): ArrayBuffer {
  const comma = dataUrl.indexOf(',');
  const b64 = comma >= 0 ? dataUrl.slice(comma + 1) : dataUrl;
  const bin = atob(b64);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return bytes.buffer;
}

/**
 * MP3 decoders leave ~25 ms of encoder delay/silence at the start; that would
 * be pure added latency on every key press. Trim to just before the onset.
 */
function trimLeadingSilence(buf: AudioBuffer, ctx: BaseAudioContext): AudioBuffer {
  const threshold = 0.002;
  let onset = buf.length;
  for (let ch = 0; ch < buf.numberOfChannels; ch++) {
    const data = buf.getChannelData(ch);
    for (let i = 0; i < Math.min(data.length, onset); i++) {
      if (Math.abs(data[i]) > threshold) {
        onset = i;
        break;
      }
    }
  }
  const start = Math.max(0, onset - Math.round(buf.sampleRate * 0.001));
  if (start < 16 || start >= buf.length - 1) return buf;
  const out = ctx.createBuffer(buf.numberOfChannels, buf.length - start, buf.sampleRate);
  for (let ch = 0; ch < buf.numberOfChannels; ch++) {
    out.getChannelData(ch).set(buf.getChannelData(ch).subarray(start));
  }
  return out;
}

async function decodeOne(path: string, ctx: BaseAudioContext): Promise<[number, Tone.ToneAudioBuffer] | null> {
  const midi = sampleFileToMidi(path);
  if (midi < 0) return null;
  const url = await SAMPLE_MODULES[path]();
  const decoded = await ctx.decodeAudioData(dataUrlToArrayBuffer(url));
  const trimmed = trimLeadingSilence(decoded, ctx);
  return [midi, new Tone.ToneAudioBuffer(trimmed)];
}

/** Decode all piano samples (memoised). Safe to call many times. */
export function loadPianoSamples(): Promise<PianoBuffers> {
  if (buffers) return Promise.resolve(buffers);
  if (loading) return loading;
  const ctx = Tone.getContext().rawContext as unknown as BaseAudioContext;
  loading = Promise.all(Object.keys(SAMPLE_MODULES).map((p) => decodeOne(p, ctx)))
    .then((entries) => {
      const map: PianoBuffers = new Map();
      for (const e of entries) if (e) map.set(e[0], e[1]);
      if (map.size === 0) throw new Error('No piano samples found');
      buffers = map;
      for (const cb of listeners) cb(map);
      listeners.clear();
      return map;
    })
    .catch((err) => {
      loading = null;
      console.warn('[audio] piano samples failed to load, using synthesized piano', err);
      throw err;
    });
  return loading;
}

/** Number of bundled samples (for tests/diagnostics). */
export const PIANO_SAMPLE_COUNT = Object.keys(SAMPLE_MODULES).length;
