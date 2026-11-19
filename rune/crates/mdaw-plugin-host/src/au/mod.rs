//! Audio Unit v2 hosting through AudioToolbox's C API.
//!
//! **Threads.** [`Instance`] methods run on the main thread (opening, state, the editor). The
//! plugin's `AudioUnitRender` is called by exactly one [`Renderer`] at a time, on the audio
//! thread; Audio Units are built for that split (the system's own hosts do the same), so
//! [`Inner`] is `Send + Sync`. The instance is disposed when its last handle goes: the engine
//! returns retired renderers to the control side, so that never happens on the audio thread.
//!
//! **Host side of a render** allocates nothing: the scratch buffers live in the [`Renderer`],
//! and the input callback hands the plugin pointers into them.

mod editor;
pub mod ffi;

use std::cell::Cell;
use std::ffi::c_void;
use std::ptr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use core_foundation::base::{CFGetTypeID, CFRelease, TCFType};
use core_foundation::data::CFData;
use core_foundation::dictionary::CFDictionaryGetTypeID;
use core_foundation::propertylist;
use core_foundation::string::{CFString, CFStringRef};

pub use editor::EditorWindow;
use ffi::*;

use crate::{HostTransport, MAX_BLOCK, PluginError, PluginId, PluginInfo, PluginKind};

/// `kAudioComponentFlag_RequiresAsyncInstantiation`: a v3 unit that cannot be opened with
/// `AudioComponentInstanceNew`.
const FLAG_REQUIRES_ASYNC: u32 = 2;
/// `kCFPropertyListBinaryFormat_v1_0`.
const PLIST_BINARY: isize = 200;
/// `kCFPropertyListImmutable`.
const PLIST_IMMUTABLE: usize = 0;

#[allow(non_upper_case_globals)]
fn kind_of(component_type: u32) -> Option<PluginKind> {
    match component_type {
        kAudioUnitType_Effect | kAudioUnitType_MusicEffect => Some(PluginKind::Effect),
        kAudioUnitType_MusicDevice => Some(PluginKind::Instrument),
        _ => None,
    }
}

fn describe(component: AudioComponent) -> Option<PluginInfo> {
    let mut desc = AudioComponentDescription::default();
    // SAFETY: `component` comes from AudioComponentFindNext; `desc` is a valid out pointer.
    if unsafe { AudioComponentGetDescription(component, &mut desc) } != 0 {
        return None;
    }
    let kind = kind_of(desc.componentType)?;
    if desc.componentFlags & FLAG_REQUIRES_ASYNC != 0 {
        return None;
    }
    let mut name: CFStringRef = ptr::null();
    // SAFETY: a valid out pointer; the returned string is owned by the caller (Copy rule).
    let full = if unsafe { AudioComponentCopyName(component, &mut name) } == 0 && !name.is_null() {
        // SAFETY: `name` is a +1 CFString.
        unsafe { CFString::wrap_under_create_rule(name) }.to_string()
    } else {
        String::new()
    };
    // The registry names components "Vendor: Name".
    let (vendor, name) = match full.split_once(": ") {
        Some((v, n)) => (v.trim().to_owned(), n.trim().to_owned()),
        None => (String::new(), full.trim().to_owned()),
    };
    let mut version = 0u32;
    // SAFETY: a valid out pointer.
    unsafe { AudioComponentGetVersion(component, &mut version) };
    Some(PluginInfo {
        id: PluginId {
            kind: desc.componentType,
            subtype: desc.componentSubType,
            manufacturer: desc.componentManufacturer,
        },
        kind,
        name: if name.is_empty() {
            crate::fourcc_string(desc.componentSubType)
        } else {
            name
        },
        vendor,
        version,
    })
}

/// Every installed effect and instrument, sorted by vendor then name. Reads the system's
/// component registry only (no plugin is loaded).
pub fn scan() -> Vec<PluginInfo> {
    let any = AudioComponentDescription::default();
    let mut out = Vec::new();
    let mut c: AudioComponent = ptr::null_mut();
    loop {
        // SAFETY: `c` is null or the previous result; `any` is a valid description.
        c = unsafe { AudioComponentFindNext(c, &any) };
        if c.is_null() {
            break;
        }
        if let Some(info) = describe(c) {
            out.push(info);
        }
    }
    out.sort_by(|a, b| {
        (a.vendor.to_lowercase(), a.name.to_lowercase())
            .cmp(&(b.vendor.to_lowercase(), b.name.to_lowercase()))
    });
    out.dedup_by(|a, b| a.id == b.id);
    out
}

/// What the render callbacks read. Lives in a `Box` owned by [`Inner`] (a stable address: the
/// plugin holds it as the callbacks' user data).
struct RenderCtx {
    /// The block being rendered (audio thread only).
    in_l: Cell<*const f32>,
    in_r: Cell<*const f32>,
    frames: Cell<u32>,
    sample_time: Cell<f64>,
    bpm: AtomicU64,
    beat: AtomicU64,
    playing: AtomicBool,
}

struct Inner {
    unit: AudioUnit,
    info: PluginInfo,
    sample_rate: f64,
    /// Effects: channels of the input bus (1: the plugin takes mono).
    input_channels: u32,
    output_channels: u32,
    ctx: Box<RenderCtx>,
}

// SAFETY: see the module docs: the unit's control API is used from the main thread and its
// render from the one audio thread, which is the threading contract of an Audio Unit; the
// `Cell`s of `RenderCtx` are touched by the audio thread only (the renderer and the callbacks
// it triggers synchronously).
unsafe impl Send for Inner {}
unsafe impl Sync for Inner {}

impl Drop for Inner {
    fn drop(&mut self) {
        // SAFETY: the unit is valid until here and nothing renders it any more (the last handle).
        unsafe {
            AudioUnitUninitialize(self.unit);
            AudioComponentInstanceDispose(self.unit);
        }
    }
}

unsafe extern "C" fn input_proc(
    refcon: *mut c_void,
    _flags: *mut u32,
    _ts: *const AudioTimeStamp,
    _bus: u32,
    frames: u32,
    io: *mut AudioBufferList2,
) -> OSStatus {
    if refcon.is_null() || io.is_null() {
        return -50; // paramErr
    }
    // SAFETY: `refcon` is the `RenderCtx` of the unit being rendered (set in `open`, alive as
    // long as the unit); `io` is the plugin's buffer list for this pull.
    let (ctx, io) = unsafe { (&*(refcon as *const RenderCtx), &mut *io) };
    let n = frames as usize;
    let available = n <= ctx.frames.get() as usize;
    let sources = [ctx.in_l.get(), ctx.in_r.get()];
    for (i, b) in io
        .mBuffers
        .iter_mut()
        .take((io.mNumberBuffers as usize).min(2))
        .enumerate()
    {
        let src = sources[i];
        b.mDataByteSize = (n * 4) as u32;
        if src.is_null() || !available {
            if !b.mData.is_null() {
                // SAFETY: the plugin provided a buffer of at least `frames` floats.
                unsafe { ptr::write_bytes(b.mData as *mut f32, 0, n) };
            }
        } else if b.mData.is_null() {
            b.mData = src as *mut c_void;
        } else {
            // SAFETY: both hold at least `n` floats and never overlap (ours is the renderer's).
            unsafe { ptr::copy_nonoverlapping(src, b.mData as *mut f32, n) };
        }
    }
    0
}

unsafe extern "C" fn beat_and_tempo(
    user: *mut c_void,
    out_beat: *mut f64,
    out_tempo: *mut f64,
) -> OSStatus {
    if user.is_null() {
        return -50;
    }
    // SAFETY: `user` is the unit's `RenderCtx`; the out pointers are the plugin's (may be null).
    unsafe {
        let ctx = &*(user as *const RenderCtx);
        if !out_beat.is_null() {
            *out_beat = f64::from_bits(ctx.beat.load(Ordering::Relaxed));
        }
        if !out_tempo.is_null() {
            *out_tempo = f64::from_bits(ctx.bpm.load(Ordering::Relaxed));
        }
    }
    0
}

unsafe extern "C" fn musical_time(
    user: *mut c_void,
    out_offset: *mut u32,
    out_num: *mut f32,
    out_den: *mut u32,
    out_downbeat: *mut f64,
) -> OSStatus {
    if user.is_null() {
        return -50;
    }
    // SAFETY: as `beat_and_tempo`.
    unsafe {
        let ctx = &*(user as *const RenderCtx);
        let beat = f64::from_bits(ctx.beat.load(Ordering::Relaxed));
        if !out_offset.is_null() {
            *out_offset = 0;
        }
        if !out_num.is_null() {
            *out_num = 4.0;
        }
        if !out_den.is_null() {
            *out_den = 4;
        }
        if !out_downbeat.is_null() {
            *out_downbeat = (beat / 4.0).floor() * 4.0;
        }
    }
    0
}

unsafe extern "C" fn transport_state(
    user: *mut c_void,
    out_playing: *mut u8,
    out_changed: *mut u8,
    out_sample: *mut f64,
    out_cycling: *mut u8,
    out_cycle_start: *mut f64,
    out_cycle_end: *mut f64,
) -> OSStatus {
    if user.is_null() {
        return -50;
    }
    // SAFETY: as `beat_and_tempo`.
    unsafe {
        let ctx = &*(user as *const RenderCtx);
        if !out_playing.is_null() {
            *out_playing = ctx.playing.load(Ordering::Relaxed) as u8;
        }
        if !out_changed.is_null() {
            *out_changed = 0;
        }
        if !out_sample.is_null() {
            *out_sample = ctx.sample_time.get();
        }
        if !out_cycling.is_null() {
            *out_cycling = 0;
        }
        if !out_cycle_start.is_null() {
            *out_cycle_start = 0.0;
        }
        if !out_cycle_end.is_null() {
            *out_cycle_end = 0.0;
        }
    }
    0
}

unsafe extern "C" fn transport_state2(
    user: *mut c_void,
    out_playing: *mut u8,
    out_recording: *mut u8,
    out_changed: *mut u8,
    out_sample: *mut f64,
    out_cycling: *mut u8,
    out_cycle_start: *mut f64,
    out_cycle_end: *mut f64,
) -> OSStatus {
    if !out_recording.is_null() {
        // SAFETY: the plugin's out pointer.
        unsafe { *out_recording = 0 };
    }
    // SAFETY: forwarded unchanged.
    unsafe {
        transport_state(
            user,
            out_playing,
            out_changed,
            out_sample,
            out_cycling,
            out_cycle_start,
            out_cycle_end,
        )
    }
}

fn check(what: &'static str, status: OSStatus) -> Result<(), PluginError> {
    if status == 0 {
        Ok(())
    } else {
        Err(PluginError::Failed { what, status })
    }
}

fn set_property<T>(unit: AudioUnit, id: u32, scope: u32, element: u32, value: &T) -> OSStatus {
    // SAFETY: `value` is a valid `T` for the duration of the call; the unit copies it.
    unsafe {
        AudioUnitSetProperty(
            unit,
            id,
            scope,
            element,
            value as *const T as *const c_void,
            std::mem::size_of::<T>() as u32,
        )
    }
}

/// A loaded plugin (a cheap handle: clones share the instance). See the module docs.
#[derive(Clone)]
pub struct Instance {
    inner: Arc<Inner>,
}

impl Instance {
    /// Load and initialise the plugin `id` for stereo rendering at `sample_rate`. Main thread
    /// only; may take a moment (the plugin loads its code and data).
    pub fn open(id: PluginId, sample_rate: f64) -> Result<Self, PluginError> {
        if objc2::MainThreadMarker::new().is_none() {
            return Err(PluginError::NotMainThread);
        }
        let desc = AudioComponentDescription {
            componentType: id.kind,
            componentSubType: id.subtype,
            componentManufacturer: id.manufacturer,
            componentFlags: 0,
            componentFlagsMask: 0,
        };
        // SAFETY: a valid description; null starts the search.
        let component = unsafe { AudioComponentFindNext(ptr::null_mut(), &desc) };
        if component.is_null() {
            return Err(PluginError::NotFound);
        }
        let info = describe(component).ok_or(PluginError::WrongKind)?;
        let mut unit: AudioUnit = ptr::null_mut();
        // SAFETY: a valid component and out pointer.
        check("load", unsafe {
            AudioComponentInstanceNew(component, &mut unit)
        })?;
        if unit.is_null() {
            return Err(PluginError::Failed {
                what: "load",
                status: -1,
            });
        }
        let ctx = Box::new(RenderCtx {
            in_l: Cell::new(ptr::null()),
            in_r: Cell::new(ptr::null()),
            frames: Cell::new(0),
            sample_time: Cell::new(0.0),
            bpm: AtomicU64::new(120f64.to_bits()),
            beat: AtomicU64::new(0f64.to_bits()),
            playing: AtomicBool::new(false),
        });
        // From here the unit is owned by `inner` (disposed on every error path by its Drop).
        let mut inner = Inner {
            unit,
            info,
            sample_rate,
            input_channels: 0,
            output_channels: 2,
            ctx,
        };
        inner.configure()?;
        // SAFETY: a configured, uninitialised unit.
        check("initialize", unsafe { AudioUnitInitialize(unit) })?;
        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    pub fn info(&self) -> &PluginInfo {
        &self.inner.info
    }

    pub fn kind(&self) -> PluginKind {
        self.inner.info.kind
    }

    /// The plugin's whole state (its preset blob: a binary property list of the unit's
    /// `ClassInfo`), None when the plugin gives none.
    pub fn state(&self) -> Option<Vec<u8>> {
        let mut plist: *const c_void = ptr::null();
        let mut size = std::mem::size_of::<*const c_void>() as u32;
        // SAFETY: ClassInfo's value is one CFPropertyListRef, returned +1.
        let status = unsafe {
            AudioUnitGetProperty(
                self.inner.unit,
                kAudioUnitProperty_ClassInfo,
                kAudioUnitScope_Global,
                0,
                &mut plist as *mut _ as *mut c_void,
                &mut size,
            )
        };
        if status != 0 || plist.is_null() {
            return None;
        }
        let data = propertylist::create_data(plist, PLIST_BINARY as _);
        // SAFETY: balances the +1 from the property read.
        unsafe { CFRelease(plist) };
        data.ok().map(|d| d.bytes().to_vec())
    }

    /// Restore a state made by [`Self::state`] (of the same plugin).
    pub fn set_state(&self, state: &[u8]) -> Result<(), PluginError> {
        let (plist, _) =
            propertylist::create_with_data(CFData::from_buffer(state), PLIST_IMMUTABLE as _)
                .map_err(|_| PluginError::BadState)?;
        if plist.is_null() {
            return Err(PluginError::BadState);
        }
        // SAFETY: `plist` is a valid +1 property list until the release below.
        let result = unsafe {
            if CFGetTypeID(plist) != CFDictionaryGetTypeID() {
                Err(PluginError::BadState)
            } else {
                check(
                    "restore its state",
                    set_property(
                        self.inner.unit,
                        kAudioUnitProperty_ClassInfo,
                        kAudioUnitScope_Global,
                        0,
                        &plist,
                    ),
                )
            }
        };
        // SAFETY: balances the +1 from `create_with_data`.
        unsafe { CFRelease(plist) };
        result
    }

    /// The plugin's processing delay (seconds).
    pub fn latency_seconds(&self) -> f64 {
        let mut v = 0f64;
        let mut size = std::mem::size_of::<f64>() as u32;
        // SAFETY: Latency's value is one Float64.
        let status = unsafe {
            AudioUnitGetProperty(
                self.inner.unit,
                kAudioUnitProperty_Latency,
                kAudioUnitScope_Global,
                0,
                &mut v as *mut f64 as *mut c_void,
                &mut size,
            )
        };
        if status == 0 && v.is_finite() && v > 0.0 {
            v
        } else {
            0.0
        }
    }

    /// Tell the plugin it renders faster or slower than real time (an export).
    pub fn set_offline(&self, offline: bool) {
        let v: u32 = offline as u32;
        set_property(
            self.inner.unit,
            kAudioUnitProperty_OfflineRender,
            kAudioUnitScope_Global,
            0,
            &v,
        );
    }

    /// The audio thread's half. Only one renderer of an instance may render at a time.
    pub fn renderer(&self) -> Renderer {
        Renderer {
            inner: self.inner.clone(),
            in_l: vec![0.0; MAX_BLOCK].into_boxed_slice(),
            in_r: vec![0.0; MAX_BLOCK].into_boxed_slice(),
            out_l: vec![0.0; MAX_BLOCK].into_boxed_slice(),
            out_r: vec![0.0; MAX_BLOCK].into_boxed_slice(),
            errors: 0,
        }
    }

    /// Whether the plugin brings its own editor view.
    pub fn has_editor(&self) -> bool {
        editor::has_cocoa_view(self.inner.unit)
    }

    /// Show the plugin's editor in its own window titled `title` (main thread). The window
    /// keeps the instance alive while it is open.
    pub fn open_editor(&self, title: &str) -> Result<EditorWindow, PluginError> {
        editor::open(self.clone(), title)
    }

    pub(crate) fn unit(&self) -> AudioUnit {
        self.inner.unit
    }
}

impl Inner {
    fn configure(&mut self) -> Result<(), PluginError> {
        let unit = self.unit;
        let max: u32 = MAX_BLOCK as u32;
        check(
            "set its block size",
            set_property(
                unit,
                kAudioUnitProperty_MaximumFramesPerSlice,
                kAudioUnitScope_Global,
                0,
                &max,
            ),
        )?;
        let stereo = AudioStreamBasicDescription::float_planar(self.sample_rate, 2);
        let mono = AudioStreamBasicDescription::float_planar(self.sample_rate, 1);
        let format = |scope: u32, f: &AudioStreamBasicDescription| {
            set_property(unit, kAudioUnitProperty_StreamFormat, scope, 0, f)
        };
        if self.info.kind == PluginKind::Effect {
            // Stereo in → stereo out; else mono in → stereo out; else mono → mono.
            if format(kAudioUnitScope_Input, &stereo) == 0 {
                self.input_channels = 2;
            } else {
                check("accept a mono input", format(kAudioUnitScope_Input, &mono))?;
                self.input_channels = 1;
            }
        }
        if format(kAudioUnitScope_Output, &stereo) == 0 {
            self.output_channels = 2;
        } else {
            check("produce an output", format(kAudioUnitScope_Output, &mono))?;
            self.output_channels = 1;
        }
        let refcon = &*self.ctx as *const RenderCtx as *mut c_void;
        if self.info.kind == PluginKind::Effect {
            let cb = AURenderCallbackStruct {
                inputProc: input_proc,
                inputProcRefCon: refcon,
            };
            check(
                "take an input",
                set_property(
                    unit,
                    kAudioUnitProperty_SetRenderCallback,
                    kAudioUnitScope_Input,
                    0,
                    &cb,
                ),
            )?;
        }
        // Optional: a plugin that ignores the host transport refuses this.
        let host = HostCallbackInfo {
            hostUserData: refcon,
            beatAndTempoProc: Some(beat_and_tempo),
            musicalTimeLocationProc: Some(musical_time),
            transportStateProc: Some(transport_state),
            transportStateProc2: Some(transport_state2),
        };
        set_property(
            unit,
            kAudioUnitProperty_HostCallbacks,
            kAudioUnitScope_Global,
            0,
            &host,
        );
        Ok(())
    }
}

/// The audio thread's half of an [`Instance`]: renders blocks. Dropping it never frees the
/// plugin while another handle exists; the engine drops retired renderers on the control side.
pub struct Renderer {
    inner: Arc<Inner>,
    in_l: Box<[f32]>,
    in_r: Box<[f32]>,
    out_l: Box<[f32]>,
    out_r: Box<[f32]>,
    errors: u64,
}

impl Renderer {
    /// The transport the plugin sees during the next blocks.
    pub fn set_transport(&mut self, t: HostTransport) {
        let ctx = &self.inner.ctx;
        ctx.bpm.store(t.bpm.to_bits(), Ordering::Relaxed);
        ctx.beat.store(t.beat.to_bits(), Ordering::Relaxed);
        ctx.playing.store(t.playing, Ordering::Relaxed);
    }

    /// Blocks the plugin failed to render so far (they pass through dry / stay silent).
    pub fn render_errors(&self) -> u64 {
        self.errors
    }

    /// One `AudioUnitRender` of `n` ≤ [`MAX_BLOCK`] frames into the plugin's output buffers.
    /// Returns pointers to the rendered channels (the plugin may substitute its own buffers).
    fn render(&mut self, n: usize) -> Option<(*const f32, *const f32)> {
        let ctx = &self.inner.ctx;
        ctx.frames.set(n as u32);
        let ts = AudioTimeStamp {
            mSampleTime: ctx.sample_time.get(),
            mFlags: kAudioTimeStampSampleTimeValid,
            ..AudioTimeStamp::default()
        };
        let bytes = (n * 4) as u32;
        let channels = self.inner.output_channels.clamp(1, 2);
        let mut abl = AudioBufferList2 {
            mNumberBuffers: channels,
            mBuffers: [
                AudioBuffer {
                    mNumberChannels: 1,
                    mDataByteSize: bytes,
                    mData: self.out_l.as_mut_ptr() as *mut c_void,
                },
                AudioBuffer {
                    mNumberChannels: 1,
                    mDataByteSize: bytes,
                    mData: self.out_r.as_mut_ptr() as *mut c_void,
                },
            ],
        };
        let mut flags = 0u32;
        // SAFETY: the unit is initialised; the buffer list holds `channels` buffers of `n`
        // floats; the input callback reads `ctx`, which this renderer just set for `n` frames.
        let status =
            unsafe { AudioUnitRender(self.inner.unit, &mut flags, &ts, 0, n as u32, &mut abl) };
        ctx.sample_time.set(ctx.sample_time.get() + n as f64);
        ctx.frames.set(0);
        if status != 0 {
            self.errors += 1;
            return None;
        }
        let l = abl.mBuffers[0].mData as *const f32;
        let r = if channels == 2 {
            abl.mBuffers[1].mData as *const f32
        } else {
            l
        };
        (!l.is_null() && !r.is_null()).then_some((l, r))
    }

    /// Run `left` / `right` (same length) through the effect, in place. A block the plugin
    /// fails to render is left dry.
    pub fn process_effect(&mut self, left: &mut [f32], right: &mut [f32]) {
        let total = left.len().min(right.len());
        let mut done = 0;
        while done < total {
            let n = (total - done).min(MAX_BLOCK);
            let (l, r) = (&mut left[done..done + n], &mut right[done..done + n]);
            if self.inner.input_channels == 1 {
                for i in 0..n {
                    self.in_l[i] = 0.5 * (l[i] + r[i]);
                }
            } else {
                self.in_l[..n].copy_from_slice(l);
                self.in_r[..n].copy_from_slice(r);
            }
            let ctx = &self.inner.ctx;
            ctx.in_l.set(self.in_l.as_ptr());
            ctx.in_r.set(if self.inner.input_channels == 1 {
                ptr::null()
            } else {
                self.in_r.as_ptr()
            });
            if let Some((ol, or)) = self.render(n) {
                // SAFETY: the plugin rendered `n` floats per channel into these buffers (ours,
                // or its own, valid until its next render); `l` / `r` are distinct from them.
                unsafe {
                    ptr::copy(ol, l.as_mut_ptr(), n);
                    ptr::copy(or, r.as_mut_ptr(), n);
                }
            }
            self.inner.ctx.in_l.set(ptr::null());
            self.inner.ctx.in_r.set(ptr::null());
            done += n;
        }
    }

    /// Render the instrument's next block, ADDING it into `left` / `right` (same length).
    pub fn render_instrument(&mut self, left: &mut [f32], right: &mut [f32]) {
        let total = left.len().min(right.len());
        let mut done = 0;
        while done < total {
            let n = (total - done).min(MAX_BLOCK);
            if let Some((ol, or)) = self.render(n) {
                // SAFETY: as in `process_effect`.
                let (ol, or) = unsafe {
                    (
                        std::slice::from_raw_parts(ol, n),
                        std::slice::from_raw_parts(or, n),
                    )
                };
                for i in 0..n {
                    left[done + i] += ol[i];
                    right[done + i] += or[i];
                }
            }
            done += n;
        }
    }

    /// A MIDI message for the next block, `offset` frames into it (note on / off, controllers,
    /// pitch bend).
    pub fn midi(&mut self, status: u8, data1: u8, data2: u8, offset: u32) {
        // SAFETY: an initialised unit; plain integers.
        unsafe {
            MusicDeviceMIDIEvent(
                self.inner.unit,
                status as u32,
                data1 as u32,
                data2 as u32,
                offset,
            );
        }
    }

    /// Clear the plugin's tails and voices (a transport stop, a non-finite output).
    pub fn reset(&mut self) {
        // SAFETY: an initialised unit.
        unsafe { AudioUnitReset(self.inner.unit, kAudioUnitScope_Global, 0) };
    }
}
