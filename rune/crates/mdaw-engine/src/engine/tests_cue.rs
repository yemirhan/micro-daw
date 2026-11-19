//! Phase-8 review #14 (behaviour change in both apps, user decision 2026-11-18): the Song cue.
//! TS: `src/services/ArrangementEngine.cue.test.ts`. The phase-3 transport tests keep passing:
//! `set_position` while stopped moves the core only, `play()` starts at the cue (0 by default).

use super::TransportState;
use super::test_rig::*;

#[test]
fn the_cue_is_the_stopped_playhead_play_starts_there_and_stop_returns_to_it() {
    let mut r = Rig::new();
    r.drum_track();
    assert_eq!(r.e.get_position(), 0.0);
    r.e.locate(12.0);
    assert_eq!(r.e.get_cue(), 12.0);
    assert_eq!(r.e.get_position(), 12.0);
    r.e.play();
    assert_eq!(r.e.get_state(), TransportState::Playing);
    assert_eq!(r.e.get_position(), 12.0);
    r.run(1.0);
    assert!(
        (r.e.get_position() - 14.0).abs() < 0.01,
        "{}",
        r.e.get_position()
    );
    // Rolling: locate seeks, the cue stays.
    r.e.locate(20.0);
    assert_eq!(r.e.get_cue(), 12.0);
    assert!((r.e.get_position() - 20.0).abs() < 0.01);
    r.e.stop();
    assert_eq!(r.e.get_position(), 12.0);
    r.e.locate(-3.0);
    assert_eq!(r.e.get_position(), 0.0);
    r.e.set_cue(f64::NAN);
    assert_eq!(r.e.get_cue(), 0.0);
}

#[test]
fn set_position_while_stopped_leaves_the_cue_and_play_from_ignores_it() {
    let mut r = Rig::new();
    r.drum_track();
    r.e.set_cue(8.0);
    r.e.set_position(5.0);
    assert_eq!(r.e.get_position(), 8.0);
    r.e.play_from(2.0);
    assert_eq!(r.e.get_position(), 2.0);
    r.e.stop();
    assert_eq!(r.e.get_position(), 8.0);
}

#[test]
fn another_project_starts_at_zero() {
    let mut r = Rig::new();
    r.e.set_cue(16.0);
    let a = r.e.doc().get_arrangement().clone();
    r.e.edit(|d| d.load_project(&a));
    assert_eq!(r.e.get_cue(), 0.0);
    assert_eq!(r.e.get_position(), 0.0);
}
