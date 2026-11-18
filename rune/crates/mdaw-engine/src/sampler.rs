//! SamplerKit: the 16-pad sample player (port of `src/audio/sampler/SamplerKit.ts`).
//!
//! The TS class does two things that run on different threads here:
//! - **audio side** ([`SamplerKit`], this file): voices, envelopes, choke / steal / release /
//!   stopAll and the per-pad channel strips, rendered sample for sample like Chromium renders the
//!   Tone graph (see `param.rs` and `channel.rs`). It implements [`Instrument`]; every other
//!   method is meant to be called between render quanta (by the core, from its command rings) and
//!   none of them allocates or locks.
//! - **control side** ([`control::KitController`]): the kit state, the buffer keys, loading
//!   through `crate::buffers` (or an injected loader), the setKit diff and pack invalidation. It
//!   produces [`SamplerCommand`]s for the audio side.
//!
//! **Time.** `now` is the context time of the next quantum to render (`frame / sr` after the
//! last [`Instrument::process`], or what [`SamplerKit::set_now_frame`] says). It plays the role
//! of both `context.currentTime` (pruning, `cancelIfPending`) and `Tone.now()` (setKit,
//! `stopAll()` without a time): the Rust engine has no look-ahead.
//!
//! **Buffers and the audio thread.** Buffers arrive as [`PadBuffer`]s (`Arc`s) in commands.
//! A finished voice whose buffer is still its pad's current buffer just drops its clone (the
//! kit's own reference keeps it alive: a count decrement, never a free). Every other `Arc` the
//! kit lets go of (a replaced pad buffer, a voice still playing a replaced buffer) is moved to a
//! preallocated garbage list, so a last reference is never freed on the audio thread: the core
//! drains it (`Instrument::take_garbage`) after every command or pattern tick that touched the
//! kit and after each quantum, and sends the `Arc`s back to the control thread through its return
//! ring. Between two drains at most one kit change plus the voices playing replaced buffers can
//! retire, far below [`GARBAGE_CAP`]; should the list ever be full anyway, the `Arc` is leaked
//! (`mem::forget`, counted, reported in `Shared::leaked_garbage`), never freed.
//!
//! **Reverse.** The TS computes `getReversedBuffer(buffer)` on the first reversed hit. The audio
//! side cannot allocate, so [`PadBuffer::reversed`] must be present whenever the pad is reversed;
//! [`control::KitController`] takes care of that (from the shared reversed-buffer cache).
//!
//! **Voices.** A fixed pool of [`VOICE_POOL`] slots. A voice stays in its pad's list (max 8,
//! the oldest stolen) until pruned by the current time, exactly like the TS list; stolen and
//! choked voices leave the list and are freed by `process` once silent. If the pool is ever full
//! (hundreds of hits inside 5 ms), the unlisted voice that ends first is reused. `trigger`
//! returns the voice's [`VoiceHandle`] (what `collectVoices` hands out in the TS).
//!
//! **Parity.** `parity_tests.rs` replays the Chromium capture of the real SamplerKit
//! (`rune/parity/chromium/capture-sampler.steps.mjs`): 13 of 19 scripted cases are bit-exact,
//! the rest within −141 dB (see there).

mod channel;
pub mod control;
mod drum;
pub(crate) mod param;
mod slice;

use slice::db_to_gain;
pub use slice::{
    EnvelopePoint, Slice, clamp, compute_slice, norm_velocity, velocity_gain, voice_envelope,
};

use std::sync::Arc;

use mdaw_dsp::abs_node::{BufferPlayer, StartParams};
use mdaw_dsp::buffer::SharedBuffer;
use mdaw_model::types::{PAD_COUNT, PadPlayMode, PadSettings};

use crate::contract::{Instrument, RENDER_QUANTUM};
use channel::{Channel, Scratch};
use param::{AutoParam, Clock};

/// Voices per pad before the oldest is cut (keeps rolls / note repeat bounded).
pub const MAX_VOICES_PER_PAD: usize = 8;
/// Fade used for choke, voice stealing and stopAll.
pub const CHOKE_FADE: f64 = 0.005;
/// At or above this cutoff the pad's low-pass is bypassed.
pub const FILTER_BYPASS_HZ: f64 = 20000.0;
/// Fade-in when a slice starts mid-waveform (avoids a click).
pub const SLICE_FADE_IN: f64 = 0.002;
const VELOCITY_SENSITIVITY: f64 = 0.85;
/// Voice slots: 8 listed voices per pad plus room for stolen / choked voices still fading.
pub const VOICE_POOL: usize = 256;
/// Garbage `Arc`s held until [`SamplerKit::drain_garbage`].
pub const GARBAGE_CAP: usize = 1024;

const Q: usize = RENDER_QUANTUM;

/// The pad settings the audio side needs (`PadSettings` minus the sample and strings).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PadParams {
    pub gain_db: f64,
    pub pan: f64,
    pub pitch: f64,
    pub start_sec: f64,
    pub end_sec: Option<f64>,
    pub decay: f64,
    pub filter_cutoff: f64,
    pub reverse: bool,
    pub gate: bool,
    pub choke_group: i64,
}

impl Default for PadParams {
    fn default() -> Self {
        Self::from(&PadSettings::default())
    }
}

impl From<&PadSettings> for PadParams {
    fn from(s: &PadSettings) -> Self {
        Self {
            gain_db: s.gain_db,
            pan: s.pan,
            pitch: s.pitch,
            start_sec: s.start_sec,
            end_sec: s.end_sec,
            decay: s.decay,
            filter_cutoff: s.filter_cutoff,
            reverse: s.reverse,
            gate: s.mode == PadPlayMode::Gate,
            choke_group: s.choke_group,
        }
    }
}

/// A pad's loaded sample: the buffer at its own rate, and its reversed copy (required when the
/// pad plays reversed).
#[derive(Debug, Clone)]
pub struct PadBuffer {
    pub forward: SharedBuffer,
    pub reversed: Option<SharedBuffer>,
}

/// What `setKit` does to one pad's buffer.
#[derive(Debug, Clone, Default)]
pub enum PadChange {
    /// Same sample, buffer still current.
    #[default]
    Keep,
    /// The sample changed: stop the pad's voices (5 ms fade, future voices cancelled), then use
    /// this buffer (None: silent until a [`SamplerCommand::SetPadBuffer`]).
    Restart(Option<PadBuffer>),
    /// Same sample but a stale buffer (pack generation) or a missing reversed copy: swap the
    /// buffer without stopping anything.
    Reload(Option<PadBuffer>),
}

/// Control → audio commands produced by [`control::KitController`]. Not boxed on purpose: the
/// audio thread must not free a `Box` (it moves ~2 KB through the command ring instead).
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum SamplerCommand {
    /// `setKit`: new pad params and per-pad buffer changes (applied at `now`).
    SetKit {
        pads: [PadParams; PAD_COUNT],
        changes: [PadChange; PAD_COUNT],
    },
    /// A pad's buffer finished loading (or failed: None). Already checked for latest-wins.
    SetPadBuffer {
        pad: usize,
        buffer: Option<PadBuffer>,
    },
}

/// A voice started by [`SamplerKit::trigger`] (`VoiceHandle` of the TS `collectVoices`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoiceHandle {
    pub pad: usize,
    /// Context time the voice starts sounding.
    pub start: f64,
    slot: u16,
    serial: u64,
}

#[derive(Debug, Clone)]
struct Voice {
    /// 0 = free slot.
    serial: u64,
    pad: usize,
    gate: bool,
    /// In its pad's voice list (else stolen / choked and fading, or free).
    listed: bool,
    start: f64,
    stop_at: f64,
    end: f64,
    /// Tone's source gain is 1 on frames `[src_on, src_off)` (start / original slice end).
    src_on: u64,
    src_off: u64,
    channels: usize,
    player: Option<BufferPlayer>,
    env: AutoParam,
}

/// The audio side of a SamplerKit. See the module docs.
pub struct SamplerKit {
    sr: u32,
    srf: f64,
    now_frame: u64,
    pads: [PadParams; PAD_COUNT],
    buffers: [Option<PadBuffer>; PAD_COUNT],
    lists: [[u16; MAX_VOICES_PER_PAD]; PAD_COUNT],
    list_len: [usize; PAD_COUNT],
    channels: Vec<Channel>,
    voices: Vec<Voice>,
    serial: u64,
    garbage: Vec<SharedBuffer>,
    leaked: u64,
    scratch: Box<Scratch>,
    bus: Box<[[[f32; Q]; 2]; PAD_COUNT]>,
    vbuf: Box<[[f32; Q]; 2]>,
    env: Box<[f32; Q]>,
    out: Box<[[f32; Q]; 2]>,
}

impl std::fmt::Debug for SamplerKit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SamplerKit")
            .field("sr", &self.sr)
            .field("now_frame", &self.now_frame)
            .field("pads", &self.pads)
            .finish_non_exhaustive()
    }
}

impl SamplerKit {
    /// Build the kit (allocates: construct it off the audio thread). The channels are applied
    /// immediately, as at construction in the TS; buffers come with [`SamplerCommand`]s.
    pub fn new(sample_rate: u32, pads: [PadParams; PAD_COUNT], now_frame: u64) -> Self {
        let srf = sample_rate as f64;
        let cx = Clock {
            now: now_frame as f64 / srf,
            sr: srf,
        };
        let mut kit = Self {
            sr: sample_rate,
            srf,
            now_frame,
            pads,
            buffers: Default::default(),
            lists: [[0; MAX_VOICES_PER_PAD]; PAD_COUNT],
            list_len: [0; PAD_COUNT],
            channels: (0..PAD_COUNT).map(|_| Channel::new(cx)).collect(),
            voices: (0..VOICE_POOL)
                .map(|_| Voice {
                    serial: 0,
                    pad: 0,
                    gate: false,
                    listed: false,
                    start: 0.0,
                    stop_at: 0.0,
                    end: 0.0,
                    src_on: 0,
                    src_off: 0,
                    channels: 1,
                    player: None,
                    env: AutoParam::new(1.0, 0.0, f32::MIN, f32::MAX, cx),
                })
                .collect(),
            serial: 0,
            garbage: Vec::with_capacity(GARBAGE_CAP),
            leaked: 0,
            scratch: Box::default(),
            bus: Box::new([[[0.0; Q]; 2]; PAD_COUNT]),
            vbuf: Box::new([[0.0; Q]; 2]),
            env: Box::new([0.0; Q]),
            out: Box::new([[0.0; Q]; 2]),
        };
        for pad in 0..PAD_COUNT {
            kit.apply_channel(pad, true);
        }
        kit
    }

    pub fn sample_rate(&self) -> u32 {
        self.sr
    }

    /// Context time of the next quantum (see the module docs).
    pub fn now(&self) -> f64 {
        self.now_frame as f64 / self.srf
    }

    /// Tell the kit where the clock is (when it was not rendered for a while).
    pub fn set_now_frame(&mut self, frame: u64) {
        self.now_frame = frame;
    }

    fn cx(&self) -> Clock {
        Clock {
            now: self.now(),
            sr: self.srf,
        }
    }

    pub fn pads(&self) -> &[PadParams; PAD_COUNT] {
        &self.pads
    }

    /// True when the pad has a loaded buffer (`isPadReady`).
    pub fn is_pad_ready(&self, pad: usize) -> bool {
        self.buffers.get(pad).is_some_and(Option::is_some)
    }

    /// Apply a control-side command (between quanta).
    pub fn apply(&mut self, cmd: SamplerCommand) {
        match cmd {
            SamplerCommand::SetKit { pads, changes } => self.set_kit(pads, changes),
            SamplerCommand::SetPadBuffer { pad, buffer } => self.set_pad_buffer(pad, buffer),
        }
    }

    /// `setKit` (audio side): new params; pads whose sample changed are stopped (5 ms fade,
    /// future voices included) and get their new buffer; every channel is re-applied with ramps.
    pub fn set_kit(&mut self, pads: [PadParams; PAD_COUNT], changes: [PadChange; PAD_COUNT]) {
        self.pads = pads;
        let now = self.now();
        for (pad, change) in changes.into_iter().enumerate() {
            match change {
                PadChange::Keep => {}
                PadChange::Restart(buf) => {
                    self.stop_pad(pad, now, CHOKE_FADE, true);
                    self.set_pad_buffer(pad, buf);
                }
                PadChange::Reload(buf) => self.set_pad_buffer(pad, buf),
            }
            self.apply_channel(pad, false);
        }
    }

    /// Replace a pad's buffer (the old one goes to the garbage list).
    pub fn set_pad_buffer(&mut self, pad: usize, buffer: Option<PadBuffer>) {
        let Some(slot) = self.buffers.get_mut(pad) else {
            return;
        };
        if let Some(old) = std::mem::replace(slot, buffer) {
            self.trash(old.forward);
            if let Some(r) = old.reversed {
                self.trash(r);
            }
        }
    }

    /// Move the garbage `Arc`s out (call after each quantum, send them to the control thread).
    pub fn drain_garbage(&mut self, mut sink: impl FnMut(SharedBuffer)) {
        for b in self.garbage.drain(..) {
            sink(b);
        }
    }

    fn trash(&mut self, b: SharedBuffer) {
        if self.garbage.len() < self.garbage.capacity() {
            self.garbage.push(b);
        } else {
            std::mem::forget(b);
            self.leaked += 1;
        }
    }

    /// Buffers leaked since the last call (see the module docs).
    pub fn take_leaks(&mut self) -> u64 {
        std::mem::take(&mut self.leaked)
    }

    fn apply_channel(&mut self, pad: usize, immediate: bool) {
        let p = self.pads[pad];
        let gain = db_to_gain(clamp(p.gain_db, -60.0, 12.0));
        let pan = clamp(p.pan, -1.0, 1.0);
        let cutoff = clamp(p.filter_cutoff, 20.0, FILTER_BYPASS_HZ);
        let cx = self.cx();
        self.channels[pad].apply(gain, pan, cutoff, immediate, cx);
    }

    /// `trigger(pad, velocity, time)`. Returns the voice's handle (the TS `collectVoices`).
    pub fn trigger(&mut self, pad: usize, velocity: f64, time: f64) -> Option<VoiceHandle> {
        if pad >= PAD_COUNT {
            return None;
        }
        let p = self.pads[pad];
        let buf = self.buffers[pad].as_ref()?;
        let vel = norm_velocity(velocity);
        if vel <= 0.0 {
            return None;
        }
        let fwd = &buf.forward;
        let duration = fwd.frames() as f64 / fwd.sample_rate as f64;
        let slice = compute_slice(&p, duration)?;
        let played = if p.reverse {
            let r = buf.reversed.as_ref();
            debug_assert!(r.is_some(), "reversed pad without a reversed buffer");
            r?.clone()
        } else {
            fwd.clone()
        };

        if p.choke_group > 0 {
            for other in 0..PAD_COUNT {
                if other != pad && self.pads[other].choke_group == p.choke_group {
                    self.stop_pad(other, time, CHOKE_FADE, false);
                }
            }
        }

        self.prune(pad);
        while self.list_len[pad] >= MAX_VOICES_PER_PAD {
            let oldest = self.list_shift(pad);
            if self.voices[oldest].start > time {
                self.cancel_voice(oldest);
            } else {
                self.stop_voice(oldest, time, CHOKE_FADE);
            }
        }

        let decay = clamp(p.decay, 0.005, 4.0);
        let fade_out = decay.min(slice.real_duration);
        let gain = velocity_gain(vel, VELOCITY_SENSITIVITY);
        let end = time + slice.real_duration;
        let slot = self.alloc_voice();
        let cx = self.cx();
        self.serial += 1;
        let serial = self.serial;
        let channels = played.channel_count();
        let player = BufferPlayer::new(
            played,
            self.sr,
            StartParams::sampler(time, slice.offset, slice.rate, None),
        );
        let srf = self.srf;
        let v = &mut self.voices[slot];
        v.serial = serial;
        v.pad = pad;
        v.gate = p.gate;
        v.listed = true;
        v.start = time;
        v.stop_at = end - fade_out;
        v.end = end;
        v.src_on = (time * srf).ceil().max(0.0) as u64;
        v.src_off = (end * srf).ceil().max(0.0) as u64;
        v.channels = channels;
        v.player = Some(player);
        v.env.reset(1.0, 0.0, cx);
        let fade_in = if slice.offset > 0.0 {
            SLICE_FADE_IN
        } else {
            0.0
        };
        let (pts, n) = voice_envelope(time, slice.real_duration, gain, fade_in, fade_out);
        for pt in &pts[..n] {
            match *pt {
                EnvelopePoint::Set { value, time } => v.env.set_value_at_time(value, time, cx),
                EnvelopePoint::Ramp { value, time } => {
                    v.env.linear_ramp_to_value_at_time(value, time, cx)
                }
            }
        }
        if channels > 1 {
            self.channels[pad].stereo = true;
        }
        self.lists[pad][self.list_len[pad]] = slot as u16;
        self.list_len[pad] += 1;
        Some(VoiceHandle {
            pad,
            start: time,
            slot: slot as u16,
            serial,
        })
    }

    /// Gate release: fades every held gate voice of the pad over its decay; gate voices that
    /// start after `time` are cancelled.
    pub fn release(&mut self, pad: usize, time: f64) {
        if pad >= PAD_COUNT {
            return;
        }
        let decay = clamp(self.pads[pad].decay, 0.005, 4.0);
        self.prune(pad);
        let (list, len) = (self.lists[pad], self.list_len[pad]);
        for &slot in &list[..len] {
            let slot = slot as usize;
            let v = &self.voices[slot];
            if !v.gate || v.serial == 0 {
                continue;
            }
            if v.start > time {
                self.cancel_voice(slot);
                continue;
            }
            if time >= v.stop_at {
                continue;
            }
            let fade = decay.min(v.end - time);
            self.stop_voice(slot, time, fade);
        }
    }

    /// Fade out every voice by `time` (None = now); voices scheduled later are cancelled.
    pub fn stop_all(&mut self, time: Option<f64>) {
        let t = time.unwrap_or_else(|| self.now());
        for pad in 0..PAD_COUNT {
            self.stop_pad(pad, t, CHOKE_FADE, true);
        }
    }

    /// `VoiceHandle.cancelIfPending`: drop the voice if it has not started by now.
    pub fn cancel_if_pending(&mut self, h: &VoiceHandle) -> bool {
        let slot = h.slot as usize;
        let Some(v) = self.voices.get(slot) else {
            return false;
        };
        if v.serial != h.serial || v.serial == 0 || v.start <= self.now() {
            return false;
        }
        self.cancel_voice(slot);
        true
    }

    /// Drop the voices (of `pad`, or of every pad) that start after `time`, whatever the pad's
    /// mode (`drumEngine.release` → `VoiceHandle.cancelIfPending` on the hits handed out ahead).
    pub fn cancel_pending_after(&mut self, pad: Option<usize>, time: f64) {
        for p in 0..PAD_COUNT {
            if pad.is_some_and(|x| x != p) {
                continue;
            }
            let (list, len) = (self.lists[p], self.list_len[p]);
            for &slot in &list[..len] {
                if self.voices[slot as usize].start > time {
                    self.cancel_voice(slot as usize);
                }
            }
        }
    }

    /// Number of voices listed on a pad after pruning (tests / diagnostics).
    pub fn live_voices(&mut self, pad: usize) -> usize {
        self.prune(pad);
        self.list_len[pad]
    }

    // ------------------------------------------------------------------------

    fn list_shift(&mut self, pad: usize) -> usize {
        let first = self.lists[pad][0] as usize;
        self.list_remove(pad, first);
        first
    }

    fn list_remove(&mut self, pad: usize, slot: usize) {
        let len = self.list_len[pad];
        if let Some(i) = self.lists[pad][..len]
            .iter()
            .position(|&s| s as usize == slot)
        {
            self.lists[pad].copy_within(i + 1..len, i);
            self.list_len[pad] = len - 1;
        }
        self.voices[slot].listed = false;
    }

    /// Drop listed voices that are already silent by the current time.
    fn prune(&mut self, pad: usize) {
        let now = self.now();
        let mut i = self.list_len[pad];
        while i > 0 {
            i -= 1;
            let slot = self.lists[pad][i] as usize;
            if self.voices[slot].end <= now {
                self.list_remove(pad, slot);
                self.free_voice(slot);
            }
        }
    }

    fn stop_pad(&mut self, pad: usize, time: f64, fade: f64, include_future: bool) {
        self.prune(pad);
        let (list, len) = (self.lists[pad], self.list_len[pad]);
        for &slot in &list[..len] {
            let slot = slot as usize;
            if self.voices[slot].start > time {
                if include_future {
                    self.cancel_voice(slot);
                }
                continue;
            }
            self.stop_voice(slot, time, fade);
        }
    }

    /// Fade a sounding voice to silence over `fade` from `time` on its own envelope
    /// (cancelAndHold keeps the current level).
    fn stop_voice(&mut self, slot: usize, time: f64, fade: f64) {
        let cx = self.cx();
        let v = &mut self.voices[slot];
        if v.serial == 0 || v.start > time || v.end <= time {
            return;
        }
        let f = fade.max(0.001);
        if time >= v.stop_at && v.end <= time + f {
            return;
        }
        v.env.cancel_and_hold_at_time(time, cx);
        v.env.linear_ramp_to_value_at_time(0.0, time + f, cx);
        v.stop_at = time;
        v.end = time + f;
    }

    fn cancel_voice(&mut self, slot: usize) {
        let pad = self.voices[slot].pad;
        if self.voices[slot].listed {
            self.list_remove(pad, slot);
        }
        self.free_voice(slot);
    }

    fn free_voice(&mut self, slot: usize) {
        let v = &mut self.voices[slot];
        v.serial = 0;
        v.listed = false;
        let pad = v.pad;
        if let Some(p) = v.player.take() {
            let b = Arc::clone(p.buffer());
            drop(p);
            // Still the pad's buffer: the kit holds another reference, dropping is a decrement.
            let current = self
                .buffers
                .get(pad)
                .and_then(Option::as_ref)
                .is_some_and(|cur| {
                    Arc::ptr_eq(&cur.forward, &b)
                        || cur.reversed.as_ref().is_some_and(|r| Arc::ptr_eq(r, &b))
                });
            if current {
                drop(b);
            } else {
                self.trash(b);
            }
        }
    }

    fn alloc_voice(&mut self) -> usize {
        if let Some(i) = self.voices.iter().position(|v| v.serial == 0) {
            return i;
        }
        // Pool exhausted (only by many stolen voices fading at once): reuse the unlisted voice
        // that ends first.
        let i = self
            .voices
            .iter()
            .enumerate()
            .filter(|(_, v)| !v.listed)
            .min_by(|a, b| a.1.end.total_cmp(&b.1.end))
            .map(|(i, _)| i)
            .unwrap_or(0);
        if self.voices[i].listed {
            let pad = self.voices[i].pad;
            self.list_remove(pad, i);
        }
        self.free_voice(i);
        i
    }

    #[allow(clippy::needless_range_loop)]
    fn render_quantum(&mut self, frame: u64, n: usize) {
        let srf = self.srf;
        let mut fed = [false; PAD_COUNT];
        for b in self.bus.iter_mut() {
            b[0][..n].fill(0.0);
            b[1][..n].fill(0.0);
        }
        let end_frame = frame + n as u64;
        for slot in 0..self.voices.len() {
            let v = &mut self.voices[slot];
            if v.serial == 0 {
                continue;
            }
            let silent_from = v.src_off.min((v.end * srf).ceil().max(0.0) as u64);
            if frame >= silent_from {
                if !v.listed {
                    self.free_voice(slot);
                }
                continue;
            }
            if end_frame <= v.src_on {
                continue;
            }
            let Some(player) = v.player.as_mut() else {
                continue;
            };
            let nch = v.channels.min(2);
            {
                let [a, b] = &mut *self.vbuf;
                let mut outs: [&mut [f32]; 2] = [&mut a[..n], &mut b[..n]];
                player.process(&mut outs[..nch], frame);
            }
            v.env.render(frame, srf, &mut self.env[..n]);
            let stereo_bus = self.channels[v.pad].stereo;
            let bus = &mut self.bus[v.pad];
            fed[v.pad] = true;
            for i in 0..n {
                let k = frame + i as u64;
                if k < v.src_on || k >= v.src_off {
                    continue;
                }
                let g = self.env[i];
                let l = self.vbuf[0][i] * g;
                bus[0][i] += l;
                if nch == 2 {
                    bus[1][i] += self.vbuf[1][i] * g;
                } else if stereo_bus {
                    bus[1][i] += l;
                }
            }
        }
        let [ol, or] = &mut *self.out;
        ol[..n].fill(0.0);
        or[..n].fill(0.0);
        for pad in 0..PAD_COUNT {
            self.channels[pad].process(
                &mut self.bus[pad],
                !fed[pad],
                frame,
                srf,
                n,
                &mut self.scratch,
                &mut ol[..n],
                &mut or[..n],
            );
        }
    }
}

impl Instrument for SamplerKit {
    fn process(&mut self, frame: u64, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len()).min(Q);
        if n == 0 {
            return;
        }
        let now = self.now();
        for ch in &mut self.channels {
            ch.prune(now);
        }
        self.render_quantum(frame, n);
        for i in 0..n {
            left[i] += self.out[0][i];
            right[i] += self.out[1][i];
        }
        self.now_frame = frame + n as u64;
    }

    fn take_garbage(&mut self, sink: &mut dyn FnMut(SharedBuffer)) {
        drum::drain_into(self, sink);
    }

    fn take_leaks(&mut self) -> u64 {
        SamplerKit::take_leaks(self)
    }
}

#[cfg(test)]
mod parity_tests;
#[cfg(test)]
mod tests;
