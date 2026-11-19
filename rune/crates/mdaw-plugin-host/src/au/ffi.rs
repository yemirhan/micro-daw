//! The slice of AudioToolbox the host uses (the C API of Audio Unit v2; stable ABI).

#![allow(non_snake_case, non_upper_case_globals, non_camel_case_types)]

use std::ffi::c_void;

use core_foundation::string::CFStringRef;

pub type OSStatus = i32;
pub type AudioComponent = *mut c_void;
pub type AudioUnit = *mut c_void;

pub const fn fourcc(s: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*s)
}

pub const kAudioUnitType_Effect: u32 = fourcc(b"aufx");
pub const kAudioUnitType_MusicEffect: u32 = fourcc(b"aumf");
pub const kAudioUnitType_MusicDevice: u32 = fourcc(b"aumu");

pub const kAudioUnitScope_Global: u32 = 0;
pub const kAudioUnitScope_Input: u32 = 1;
pub const kAudioUnitScope_Output: u32 = 2;

pub const kAudioUnitProperty_ClassInfo: u32 = 0;
pub const kAudioUnitProperty_SampleRate: u32 = 2;
pub const kAudioUnitProperty_StreamFormat: u32 = 8;
pub const kAudioUnitProperty_Latency: u32 = 12;
pub const kAudioUnitProperty_MaximumFramesPerSlice: u32 = 14;
pub const kAudioUnitProperty_SetRenderCallback: u32 = 23;
pub const kAudioUnitProperty_HostCallbacks: u32 = 27;
pub const kAudioUnitProperty_CocoaUI: u32 = 31;
pub const kAudioUnitProperty_OfflineRender: u32 = 37;

pub const kAudioFormatLinearPCM: u32 = fourcc(b"lpcm");
pub const kAudioFormatFlagIsFloat: u32 = 1;
pub const kAudioFormatFlagIsPacked: u32 = 8;
pub const kAudioFormatFlagIsNonInterleaved: u32 = 32;

pub const kAudioTimeStampSampleTimeValid: u32 = 1;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AudioComponentDescription {
    pub componentType: u32,
    pub componentSubType: u32,
    pub componentManufacturer: u32,
    pub componentFlags: u32,
    pub componentFlagsMask: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct AudioStreamBasicDescription {
    pub mSampleRate: f64,
    pub mFormatID: u32,
    pub mFormatFlags: u32,
    pub mBytesPerPacket: u32,
    pub mFramesPerPacket: u32,
    pub mBytesPerFrame: u32,
    pub mChannelsPerFrame: u32,
    pub mBitsPerChannel: u32,
    pub mReserved: u32,
}

impl AudioStreamBasicDescription {
    /// Float32, non-interleaved, native endian: the canonical Audio Unit stream format.
    pub fn float_planar(sample_rate: f64, channels: u32) -> Self {
        Self {
            mSampleRate: sample_rate,
            mFormatID: kAudioFormatLinearPCM,
            mFormatFlags: kAudioFormatFlagIsFloat
                | kAudioFormatFlagIsPacked
                | kAudioFormatFlagIsNonInterleaved,
            mBytesPerPacket: 4,
            mFramesPerPacket: 1,
            mBytesPerFrame: 4,
            mChannelsPerFrame: channels,
            mBitsPerChannel: 32,
            mReserved: 0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct SMPTETime {
    pub mSubframes: i16,
    pub mSubframeDivisor: i16,
    pub mCounter: u32,
    pub mType: u32,
    pub mFlags: u32,
    pub mHours: i16,
    pub mMinutes: i16,
    pub mSeconds: i16,
    pub mFrames: i16,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct AudioTimeStamp {
    pub mSampleTime: f64,
    pub mHostTime: u64,
    pub mRateScalar: f64,
    pub mWordClockTime: u64,
    pub mSMPTETime: SMPTETime,
    pub mFlags: u32,
    pub mReserved: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AudioBuffer {
    pub mNumberChannels: u32,
    pub mDataByteSize: u32,
    pub mData: *mut c_void,
}

/// An `AudioBufferList` with room for two (planar stereo) buffers.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AudioBufferList2 {
    pub mNumberBuffers: u32,
    pub mBuffers: [AudioBuffer; 2],
}

pub type AURenderCallback = unsafe extern "C" fn(
    inRefCon: *mut c_void,
    ioActionFlags: *mut u32,
    inTimeStamp: *const AudioTimeStamp,
    inBusNumber: u32,
    inNumberFrames: u32,
    ioData: *mut AudioBufferList2,
) -> OSStatus;

#[repr(C)]
pub struct AURenderCallbackStruct {
    pub inputProc: AURenderCallback,
    pub inputProcRefCon: *mut c_void,
}

pub type HostCallback_GetBeatAndTempo = unsafe extern "C" fn(
    inHostUserData: *mut c_void,
    outCurrentBeat: *mut f64,
    outCurrentTempo: *mut f64,
) -> OSStatus;

pub type HostCallback_GetMusicalTimeLocation = unsafe extern "C" fn(
    inHostUserData: *mut c_void,
    outDeltaSampleOffsetToNextBeat: *mut u32,
    outTimeSig_Numerator: *mut f32,
    outTimeSig_Denominator: *mut u32,
    outCurrentMeasureDownBeat: *mut f64,
) -> OSStatus;

pub type HostCallback_GetTransportState = unsafe extern "C" fn(
    inHostUserData: *mut c_void,
    outIsPlaying: *mut u8,
    outTransportStateChanged: *mut u8,
    outCurrentSampleInTimeLine: *mut f64,
    outIsCycling: *mut u8,
    outCycleStartBeat: *mut f64,
    outCycleEndBeat: *mut f64,
) -> OSStatus;

pub type HostCallback_GetTransportState2 = unsafe extern "C" fn(
    inHostUserData: *mut c_void,
    outIsPlaying: *mut u8,
    outIsRecording: *mut u8,
    outTransportStateChanged: *mut u8,
    outCurrentSampleInTimeLine: *mut f64,
    outIsCycling: *mut u8,
    outCycleStartBeat: *mut f64,
    outCycleEndBeat: *mut f64,
) -> OSStatus;

#[repr(C)]
pub struct HostCallbackInfo {
    pub hostUserData: *mut c_void,
    pub beatAndTempoProc: Option<HostCallback_GetBeatAndTempo>,
    pub musicalTimeLocationProc: Option<HostCallback_GetMusicalTimeLocation>,
    pub transportStateProc: Option<HostCallback_GetTransportState>,
    pub transportStateProc2: Option<HostCallback_GetTransportState2>,
}

/// `AudioUnitCocoaViewInfo` with one view class (the size the property reports says how many
/// class names follow; the host uses the first).
#[repr(C)]
pub struct AudioUnitCocoaViewInfo {
    pub mCocoaAUViewBundleLocation: *const c_void,
    pub mCocoaAUViewClass: [CFStringRef; 1],
}

#[link(name = "AudioToolbox", kind = "framework")]
unsafe extern "C" {
    pub fn AudioComponentFindNext(
        inComponent: AudioComponent,
        inDesc: *const AudioComponentDescription,
    ) -> AudioComponent;
    pub fn AudioComponentCopyName(
        inComponent: AudioComponent,
        outName: *mut CFStringRef,
    ) -> OSStatus;
    pub fn AudioComponentGetDescription(
        inComponent: AudioComponent,
        outDesc: *mut AudioComponentDescription,
    ) -> OSStatus;
    pub fn AudioComponentGetVersion(inComponent: AudioComponent, outVersion: *mut u32) -> OSStatus;
    pub fn AudioComponentInstanceNew(
        inComponent: AudioComponent,
        outInstance: *mut AudioUnit,
    ) -> OSStatus;
    pub fn AudioComponentInstanceDispose(inInstance: AudioUnit) -> OSStatus;
    pub fn AudioUnitInitialize(inUnit: AudioUnit) -> OSStatus;
    pub fn AudioUnitUninitialize(inUnit: AudioUnit) -> OSStatus;
    pub fn AudioUnitGetPropertyInfo(
        inUnit: AudioUnit,
        inID: u32,
        inScope: u32,
        inElement: u32,
        outDataSize: *mut u32,
        outWritable: *mut u8,
    ) -> OSStatus;
    pub fn AudioUnitGetProperty(
        inUnit: AudioUnit,
        inID: u32,
        inScope: u32,
        inElement: u32,
        outData: *mut c_void,
        ioDataSize: *mut u32,
    ) -> OSStatus;
    pub fn AudioUnitSetProperty(
        inUnit: AudioUnit,
        inID: u32,
        inScope: u32,
        inElement: u32,
        inData: *const c_void,
        inDataSize: u32,
    ) -> OSStatus;
    pub fn AudioUnitRender(
        inUnit: AudioUnit,
        ioActionFlags: *mut u32,
        inTimeStamp: *const AudioTimeStamp,
        inOutputBusNumber: u32,
        inNumberFrames: u32,
        ioData: *mut AudioBufferList2,
    ) -> OSStatus;
    pub fn AudioUnitReset(inUnit: AudioUnit, inScope: u32, inElement: u32) -> OSStatus;
    pub fn MusicDeviceMIDIEvent(
        inUnit: AudioUnit,
        inStatus: u32,
        inData1: u32,
        inData2: u32,
        inOffsetSampleFrame: u32,
    ) -> OSStatus;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn struct_layouts_match_the_c_headers() {
        assert_eq!(std::mem::size_of::<AudioComponentDescription>(), 20);
        assert_eq!(std::mem::size_of::<AudioStreamBasicDescription>(), 40);
        assert_eq!(std::mem::size_of::<SMPTETime>(), 24);
        assert_eq!(std::mem::size_of::<AudioTimeStamp>(), 64);
        assert_eq!(std::mem::size_of::<AudioBuffer>(), 16);
        assert_eq!(std::mem::size_of::<AudioBufferList2>(), 40);
        assert_eq!(std::mem::offset_of!(AudioBufferList2, mBuffers), 8);
        assert_eq!(std::mem::offset_of!(AudioTimeStamp, mFlags), 56);
    }
}
