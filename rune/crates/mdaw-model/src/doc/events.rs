//! Side effects the engine performs for the doc, and the kit-selection event.

use serde::{Deserialize, Serialize};

use crate::migrate::DroppedDrumNotes;

use super::EffectName;

/// Who chose a whole kit (`setKit`). 'user': an explicit choice in any view; 'pack-load': a
/// pending "Get & load" applying its kit (review N2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum KitSelectionSource {
    #[default]
    #[serde(rename = "user")]
    User,
    #[serde(rename = "pack-load")]
    PackLoad,
}

/// `KitSelectedEvent` (the `onKitSelected` payload).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KitSelectedEvent {
    pub track_id: String,
    pub kit_id: String,
    pub source: KitSelectionSource,
}

/// `UnavailableAudioRegion` (review N5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnavailableAudioRegion {
    pub track_id: String,
    pub region_id: String,
    pub sample_id: String,
}

/// An audio / transport side effect of a doc edit, in emission order. Each variant names the TS
/// call it replaces; the engine drains them after every doc call.
#[derive(Debug, Clone, PartialEq)]
pub enum DocEvent {
    /// `createTrackAudio(track)` for a new track (addTrack, AE:455).
    TrackAdded(String),
    /// `disposeTrackAudio(trackId)` (removeTrack, AE:471).
    TrackRemoved(String),
    /// `withPlaybackRestart(() => { disposeTrackAudio(id); createTrackAudio(track) })`
    /// (setTrackInstrument AE:505, setTrackSynthOverrides AE:518).
    TrackAudioRebuilt(String),
    /// `audio.volume.volume.value = db` (setTrackVolume AE:529).
    TrackVolumeChanged { track_id: String, db: f64 },
    /// `audio.volume.mute = muted` (setTrackMute AE:537).
    TrackMuteChanged { track_id: String, muted: bool },
    /// `audio.panner.pan.value = pan` (setTrackPan AE:556).
    TrackPanChanged { track_id: String, pan: f64 },
    /// `updateSoloState()` (setTrackSolo AE:547).
    SoloChanged,
    /// `applyTrackEffect(audio, track.effects, effectName)` (setTrackEffect AE:571).
    TrackEffectChanged {
        track_id: String,
        effect: EffectName,
    },
    /// `resetTrackEffects(track, audio)` (removeAutomationLane AE:1761).
    TrackEffectsReset(String),
    /// `refreshPlaybackSchedule(trackId)`: rebuild the track's running Part (a no-op unless
    /// playing / loop recording). editRegionNotes AE:2279, toggleStep AE:2687,
    /// setStepVelocity AE:2702, clearPattern AE:2713, setLoopSettings AE:2537/2542, gate length
    /// (LoopRecorder captureRelease LR:370).
    RescheduleTrack(String),
    /// `this.trackAudio.get(track.id)?.sampler?.setKit(kit)` (applyKit AE:2483).
    KitApplied(String),
    /// `onKitSelected` listeners (setKit AE:2426-2433).
    KitSelected(KitSelectedEvent),
    /// `syncDrumSink()`: point drumEngine's live sink at the active drum track's sampler.
    SyncDrumSink,
    /// `applyLoopToTransport()` (setLoopEnabled AE:1105, setLoopMarkers AE:1116,
    /// applyPatternLoop AE:2585, setLoopSettings AE:2539).
    LoopRangeChanged,
    /// `Tone.getTransport().bpm.value = bpm; applyLoopToTransport()` (setBpm AE:1272-1274).
    BpmChanged(f64),
    /// `updateMetronome()` (setLoopSettings AE:2545).
    MetronomeUpdate,
    /// Swing or swing grid changed (setLoopSettings AE:2540-2543); every drum track also gets a
    /// `RescheduleTrack`.
    SwingChanged,
    /// Dispose every track's audio nodes and create them again, inside `withPlaybackRestart`
    /// (restoreFromSnapshot AE:1594-1606, loadFromLocalStorage AE:1541-1543).
    RebuildAll,
    /// A loop-recording take ended: `stopTransport()` (endLoopTake AE:2806). The recorder was
    /// already ended through the take-end hook.
    LoopTakeEnded { track_id: String },
    /// The set of armed tracks changed: live input monitoring and the keys' targets follow.
    ArmChanged,
    /// An audio track listens to another device input.
    TrackInputChanged(String),
    /// A track's insert plugins changed (added, removed, reordered, bypassed): rebuild its
    /// insert chain.
    TrackPluginsChanged(String),
    /// `console.warn` of a migration (legacy drum notes without a pad were dropped).
    MigrationWarning(DroppedDrumNotes),
}
