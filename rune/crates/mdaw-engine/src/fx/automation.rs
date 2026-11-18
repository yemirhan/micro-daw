//! Track automation playback (`ArrangementEngine.scheduleAutomation`, AE:1871-1962): the lanes of a
//! track turned into strip operations on the transport timeline.
//!
//! - Only visible lanes with points play (a hidden lane is silent: TS quirk kept).
//! - Each point is a `transport.schedule` at `beat·60/bpm` seconds. Tone converts that to
//!   (fractional) ticks; a `TransportEvent` fires on the floor tick and passes
//!   `time + remainder·tickDuration` to its callback. The callback does `setValueAtTime(v, time)`
//!   then, if there is a next point, `linearRampToValueAtTime(next, time + Δbeats·60/bpm)`: the
//!   ramp length is fixed in seconds at the tempo of the play start. The events re-fire on every
//!   loop pass.
//! - `chorusDepth` sets the depth (and wet 0.5 / 0) at once inside the callback (Tone invokes it up
//!   to the look-ahead early; the core fires it at its exact time, see `fx/track.rs`).
//! - Before scheduling: a `distortionWet` lane forces the distortion amount to `max(amount, 0.4)`;
//!   a `delayWet` lane sets the delay time / feedback to `time || 0.25` / `feedback || 0.3`
//!   ([`pre_ops`]).
//! - WAV export never schedules automation (TS quirk kept: `ExportService` does not call it).

use mdaw_model::automation::map_automation_value;
use mdaw_model::types::{AutomationParameter, Track, TrackEffectState};

use super::state::or_default;
use super::track::StripOp;
use crate::contract::PPQ;

/// One automation callback on the transport timeline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutoEvent {
    /// Transport position in (fractional) ticks.
    pub tick: f64,
    pub op: StripOp,
}

/// The lanes that play: visible, with points.
fn playing_lanes(track: &Track) -> impl Iterator<Item = &mdaw_model::types::AutomationLane> {
    track
        .automation
        .iter()
        .flatten()
        .filter(|l| l.visible && !l.points.is_empty())
}

/// The pre-pass of `scheduleAutomation` (effects made audible when their wet is automated).
pub fn pre_ops(track: &Track) -> Vec<StripOp> {
    let fx = track.effects.clone().unwrap_or_default();
    let mut ops = Vec::new();
    for lane in playing_lanes(track) {
        match lane.parameter {
            AutomationParameter::DistortionWet => ops.push(StripOp::ForceDistortionAmount(
                fx.distortion.amount.max(0.4),
            )),
            AutomationParameter::DelayWet => ops.push(StripOp::ForceDelayTimes {
                time: or_default(fx.delay.time, 0.25),
                feedback: or_default(fx.delay.feedback, 0.3),
            }),
            _ => {}
        }
    }
    ops
}

/// `TransportTime(seconds).toTicks()` at a constant `bpm`.
fn seconds_to_ticks(seconds: f64, bpm: f64) -> f64 {
    let quarter = 60.0 / bpm;
    seconds / quarter * PPQ as f64
}

/// Every automation callback of `track` at `bpm`, sorted by tick (lane order within a tick, as
/// Tone's timeline keeps insertion order).
pub fn events(track: &Track, bpm: f64) -> Vec<AutoEvent> {
    let mut out = Vec::new();
    for lane in playing_lanes(track) {
        let p = lane.parameter;
        for (i, pt) in lane.points.iter().enumerate() {
            let seconds = pt.beat / bpm * 60.0;
            let value = map_automation_value(p, pt.value);
            let ramp = lane.points.get(i + 1).map(|next| {
                (
                    map_automation_value(p, next.value),
                    (next.beat - pt.beat) / bpm * 60.0,
                )
            });
            out.push(AutoEvent {
                tick: seconds_to_ticks(seconds, bpm),
                op: StripOp::Automation {
                    param: p,
                    value,
                    ramp,
                },
            });
        }
    }
    // stable: same-tick events keep lane order
    out.sort_by(|a, b| a.tick.floor().total_cmp(&b.tick.floor()));
    out
}

/// The effect settings a track's strip is built / reset with.
pub fn track_effects(track: &Track) -> TrackEffectState {
    track.effects.clone().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdaw_model::types::{AutomationLane, AutomationPoint};

    fn track(lanes: Vec<AutomationLane>) -> Track {
        let mut t: Track = serde_json::from_str(
            r#"{"id":"t","name":"t","instrument":{"type":"synth","presetIndex":0},"regions":[],
                "volume":0,"pan":0,"muted":false,"solo":false,"color":""}"#,
        )
        .unwrap();
        t.automation = Some(lanes);
        t
    }

    fn lane(p: AutomationParameter, pts: &[(f64, f64)], visible: bool) -> AutomationLane {
        AutomationLane {
            parameter: p,
            points: pts
                .iter()
                .map(|&(beat, value)| AutomationPoint { beat, value })
                .collect(),
            visible,
        }
    }

    #[test]
    fn points_become_set_and_ramp_ops_on_ticks() {
        let t = track(vec![
            lane(AutomationParameter::Volume, &[(0.0, 1.0), (2.0, 0.5)], true),
            lane(AutomationParameter::Pan, &[(1.0, 0.0)], true),
            lane(AutomationParameter::ReverbWet, &[(1.0, 1.0)], false),
            lane(AutomationParameter::DelayWet, &[], true),
        ]);
        let ev = events(&t, 120.0);
        assert_eq!(ev.len(), 3, "hidden and empty lanes do not play");
        assert_eq!(ev[0].tick, 0.0);
        assert_eq!(
            ev[0].op,
            StripOp::Automation {
                param: AutomationParameter::Volume,
                value: 0.0,
                ramp: Some((-20.0, 1.0)),
            }
        );
        assert!((ev[1].tick - 192.0).abs() < 1e-9);
        assert_eq!(
            ev[1].op,
            StripOp::Automation {
                param: AutomationParameter::Pan,
                value: -1.0,
                ramp: None,
            }
        );
        assert!((ev[2].tick - 384.0).abs() < 1e-9);
    }

    #[test]
    fn pre_pass_forces_audible_distortion_and_delay() {
        let mut t = track(vec![
            lane(AutomationParameter::DistortionWet, &[(0.0, 0.5)], true),
            lane(AutomationParameter::DelayWet, &[(0.0, 0.5)], true),
        ]);
        let mut fx = TrackEffectState::default();
        fx.distortion.amount = 0.1;
        fx.delay.feedback = 0.0;
        fx.delay.time = 0.5;
        t.effects = Some(fx);
        assert_eq!(
            pre_ops(&t),
            [
                StripOp::ForceDistortionAmount(0.4),
                StripOp::ForceDelayTimes {
                    time: 0.5,
                    feedback: 0.3
                }
            ]
        );
    }
}
