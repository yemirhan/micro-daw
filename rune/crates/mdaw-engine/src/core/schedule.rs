//! Immutable per-track note schedules (≙ the TS `Tone.Part` of each MIDI/drum track).
//!
//! Built on the control side from the arrangement, swapped into the core as
//! `Arc<TrackSchedule>` (the old one comes back through the garbage ring). Events are in
//! transport ticks; the transport loop re-fires them on every pass.
//!
//! - Drum notes play at `beat + swingOffsetBeats(beat, swingGrid, swing)` on the **absolute**
//!   beat (`AE:1394-1400`), computed here at build time exactly like the TS does when it builds
//!   the Part. Synth notes are never swung.
//! - A `Tone.Part` note fires on the NEAREST integer tick: the Part keeps each note's offset in
//!   (fractional) ticks, but schedules its `ToneEvent` at
//!   `event.time + Math.round(startOffset / playbackRate)` (`ToneEvent._rescheduleEvents`), so
//!   e.g. swing 0.3 on the 1/8 grid (0.15 beat = 28.8 ticks) plays at tick 29. Schedules built
//!   by [`TrackSchedule::from_track`] hold those rounded ticks (verified against the Electron
//!   app's WAV export, `engine/tests_export.rs`). A fractional tick in a schedule (e.g. audio
//!   regions: `transport.schedule` keeps the remainder, `TransportEvent.invoke`) is picked up on
//!   `floor(tick)` and fired at `time + remainder·tickDuration`.
//! - Automation callbacks (`fx/automation.rs`) ride in the same schedule: they fire on the floor
//!   of their (fractional) tick, at `time + remainder·tickDuration`, before the notes of that
//!   tick (Tone inserts a track's automation events before its Part's), on every loop pass.
//! - Solo/mute at schedule time (`AE:1338-1351`): a track that should not play gets **no**
//!   schedule ([`playing_tracks`]).

use mdaw_model::arrangement_helpers::swing_offset_beats;
use mdaw_model::{Arrangement, InstrumentType, LoopRecordSettings, PadIndex, Track, note_to_pad};

use crate::contract::PPQ;
use crate::fx::automation::AutoEvent;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScheduledNote {
    /// Play position in ticks (swing applied).
    pub tick: f64,
    /// Absolute straight beat (the LoopRecorder suppression key).
    pub beat: f64,
    pub note: f64,
    pub velocity: f64,
    pub duration_beats: f64,
    /// Drum pad (drum schedules; notes outside the pad range are not scheduled).
    pub pad: Option<PadIndex>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrackSchedule {
    pub is_drums: bool,
    /// Sorted by `tick`.
    events: Vec<ScheduledNote>,
    /// Automation callbacks, sorted by floor tick (lane order within a tick).
    automation: Vec<AutoEvent>,
}

fn floor_tick(t: f64) -> f64 {
    (t + 1e-9).floor()
}

impl TrackSchedule {
    pub fn new(is_drums: bool, mut events: Vec<ScheduledNote>) -> Self {
        events.sort_by(|a, b| a.tick.total_cmp(&b.tick));
        Self {
            is_drums,
            events,
            automation: Vec::new(),
        }
    }

    /// The same schedule with these automation callbacks.
    pub fn with_automation(mut self, mut automation: Vec<AutoEvent>) -> Self {
        automation.sort_by(|a, b| floor_tick(a.tick).total_cmp(&floor_tick(b.tick)));
        self.automation = automation;
        self
    }

    pub fn automation(&self) -> &[AutoEvent] {
        &self.automation
    }

    /// Automation callbacks picked up on integer ticks `[lo, hi)`.
    pub fn automation_in_ticks(&self, lo: i64, hi: i64) -> &[AutoEvent] {
        let a = self
            .automation
            .partition_point(|e| floor_tick(e.tick) < lo as f64);
        let b = self
            .automation
            .partition_point(|e| floor_tick(e.tick) < hi as f64);
        &self.automation[a..b.max(a)]
    }

    /// The first integer tick in `[lo, hi)` with a note or an automation callback.
    pub fn next_tick(&self, lo: i64, hi: i64, notes: bool) -> Option<i64> {
        let n = notes
            .then(|| {
                self.in_ticks(lo, hi)
                    .first()
                    .map(|e| floor_tick(e.tick) as i64)
            })
            .flatten();
        let a = self
            .automation_in_ticks(lo, hi)
            .first()
            .map(|e| floor_tick(e.tick) as i64);
        match (n, a) {
            (Some(x), Some(y)) => Some(x.min(y)),
            (x, y) => x.or(y),
        }
    }

    pub fn events(&self) -> &[ScheduledNote] {
        &self.events
    }

    /// Events picked up on integer ticks `[lo, hi)`, i.e. with `lo ≤ floor(tick) < hi`.
    pub fn in_ticks(&self, lo: i64, hi: i64) -> &[ScheduledNote] {
        let a = self
            .events
            .partition_point(|e| floor_tick(e.tick) < lo as f64);
        let b = self
            .events
            .partition_point(|e| floor_tick(e.tick) < hi as f64);
        &self.events[a..b.max(a)]
    }

    /// The schedule of one MIDI/drum track (None for audio tracks: phase 4).
    pub fn from_track(track: &Track, loop_settings: &LoopRecordSettings) -> Option<Self> {
        let is_drums = match track.instrument.kind {
            InstrumentType::Drums => true,
            InstrumentType::Synth => false,
            InstrumentType::Audio => return None,
        };
        let mut events = Vec::new();
        for region in &track.regions {
            for n in &region.notes {
                let beat = region.start_beat + n.start_beat;
                let play_beat = if is_drums {
                    beat + swing_offset_beats(beat, loop_settings.swing_grid, loop_settings.swing)
                } else {
                    beat
                };
                let pad = if is_drums {
                    match note_to_pad(n.note.round()) {
                        Some(p) => Some(p),
                        None => continue,
                    }
                } else {
                    None
                };
                events.push(ScheduledNote {
                    tick: crate::core::js_round(play_beat * PPQ as f64),
                    beat,
                    note: n.note,
                    velocity: n.velocity,
                    duration_beats: n.duration_beats,
                    pad,
                });
            }
        }
        Some(Self::new(is_drums, events))
    }
}

/// Tracks that play at schedule time: with any solo, the soloed ones; else the unmuted ones.
pub fn playing_tracks(arrangement: &Arrangement) -> impl Iterator<Item = &Track> {
    let has_solo = arrangement.tracks.iter().any(|t| t.solo);
    arrangement
        .tracks
        .iter()
        .filter(move |t| if has_solo { t.solo } else { !t.muted })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdaw_model::arrangement_helpers::DEFAULT_LOOP_SETTINGS;
    use mdaw_model::{QuantizeGrid, Region, RegionNote, SwingGrid, TrackInstrument};

    fn note(n: f64, start: f64) -> RegionNote {
        RegionNote {
            note: n,
            velocity: 0.8,
            start_beat: start,
            duration_beats: 0.25,
            is_drum: true,
        }
    }

    pub(crate) fn drum_track(id: &str, notes: Vec<RegionNote>, region_start: f64) -> Track {
        Track {
            id: id.into(),
            name: id.into(),
            instrument: TrackInstrument {
                kind: InstrumentType::Drums,
                preset_index: 0,
                synth_overrides: None,
                kit: None,
                extra: Default::default(),
            },
            regions: vec![Region {
                id: "r".into(),
                start_beat: region_start,
                length_beats: 16.0,
                notes,
                color: None,
                name: None,
                audio: None,
                extra: Default::default(),
            }],
            volume: 0.0,
            pan: 0.0,
            muted: false,
            solo: false,
            color: String::new(),
            effects: None,
            automation: None,
            extra: Default::default(),
        }
    }

    #[test]
    fn swing_on_the_absolute_beat_drums_only_and_sorted() {
        let mut s = DEFAULT_LOOP_SETTINGS;
        s.swing = 0.5;
        s.swing_grid = SwingGrid::S16;
        // region at beat 4: note 0.25 → absolute 4.25 (odd 16th): +0.125
        let t = drum_track(
            "d",
            vec![note(37.0, 0.25), note(36.0, 0.0), note(20.0, 1.0)],
            4.0,
        );
        let sch = TrackSchedule::from_track(&t, &s).unwrap();
        let ev = sch.events();
        assert_eq!(ev.len(), 2, "out-of-range drum notes are not scheduled");
        assert_eq!((ev[0].tick, ev[0].pad), (768.0, Some(0)));
        assert_eq!(
            (ev[1].tick, ev[1].beat, ev[1].pad),
            (4.375 * 192.0, 4.25, Some(1))
        );
        let mut synth = t.clone();
        synth.instrument.kind = InstrumentType::Synth;
        let ev = TrackSchedule::from_track(&synth, &s).unwrap();
        assert_eq!(
            ev.events()[1].tick,
            4.25 * 192.0,
            "synth notes are never swung"
        );
    }

    #[test]
    fn part_notes_fire_on_the_nearest_integer_tick() {
        let mut s = DEFAULT_LOOP_SETTINGS;
        s.swing = 0.3;
        s.swing_grid = SwingGrid::S8;
        // 0.5 + 0.15 swing = 0.65 beat = 124.8 ticks → 125; 0.3 (unswung, 57.6) → 58
        let t = drum_track("d", vec![note(36.0, 0.5), note(37.0, 0.3)], 0.0);
        let sch = TrackSchedule::from_track(&t, &s).unwrap();
        let ticks: Vec<f64> = sch.events().iter().map(|e| e.tick).collect();
        assert_eq!(ticks, [58.0, 125.0]);
        assert_eq!(
            sch.events()[1].beat,
            0.5,
            "the suppression key stays straight"
        );
    }

    #[test]
    fn quantize_quarter_with_swing_does_not_delay_quarters() {
        // review #7: swing has its own grid; beats 1..3 are even 16ths
        let mut s = DEFAULT_LOOP_SETTINGS;
        s.quantize = QuantizeGrid::Q4;
        s.swing = 0.5;
        let notes = (0..4).map(|b| note(36.0, b as f64)).collect();
        let sch = TrackSchedule::from_track(&drum_track("d", notes, 0.0), &s).unwrap();
        let ticks: Vec<f64> = sch.events().iter().map(|e| e.tick).collect();
        assert_eq!(ticks, [0.0, 192.0, 384.0, 576.0]);
    }

    #[test]
    fn picks_events_by_floor_tick() {
        let ev = |tick| ScheduledNote {
            tick,
            beat: 0.0,
            note: 36.0,
            velocity: 1.0,
            duration_beats: 0.25,
            pad: Some(0),
        };
        let s = TrackSchedule::new(true, vec![ev(10.5), ev(0.0), ev(11.0), ev(12.0)]);
        assert_eq!(s.in_ticks(0, 1).len(), 1);
        assert_eq!(s.in_ticks(10, 11).len(), 1);
        assert_eq!(s.in_ticks(11, 13).len(), 2);
        assert!(s.in_ticks(1, 10).is_empty());
    }

    #[test]
    fn solo_and_mute_at_schedule_time() {
        let mut arr: Arrangement = serde_json::from_str(r#"{"id":"a","bpm":120}"#).unwrap();
        let mut a = drum_track("a", vec![], 0.0);
        let mut b = drum_track("b", vec![], 0.0);
        let c = drum_track("c", vec![], 0.0);
        b.muted = true;
        arr.tracks = vec![a.clone(), b.clone(), c.clone()];
        let ids = |arr: &Arrangement| {
            playing_tracks(arr)
                .map(|t| t.id.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&arr), ["a", "c"]);
        a.solo = true;
        b.solo = true;
        arr.tracks = vec![a, b, c];
        assert_eq!(
            ids(&arr),
            ["a", "b"],
            "a muted soloed track plays (solo wins)"
        );
    }
}
