//! micro-daw offline DSP. Pure functions, no realtime engine, no UI.
//!
//! - `classic`: literal port of `src/audio/sampler/classic/**` (808/909/707/606/Linn/CR-78-style voices)
//! - `factory`: the 16 factory drum voices of `src/audio/drumKit.ts`, rendered offline
//! - `reverb_ir`: `src/audio/RoomReverb.ts` impulse response
//! - `shaping`: master-bus soft clip, drive curves
//! - `audio_io`: WAV encode (`src/utils/wavEncoder.ts`), decoding of wav/aiff/flac/mp3/ogg/m4a, resampling
//! - `buffer`: the shared multichannel buffer type
//! - `abs_node`: Chromium `AudioBufferSourceNode` playback (pad voices: rate conversion, pitch, offsets)

pub mod abs_node;
pub mod audio_io;
pub mod buffer;
pub mod classic;
pub mod factory;
pub mod pad_shape;
pub mod reverb_ir;
pub mod shaping;
pub mod sound_design;
