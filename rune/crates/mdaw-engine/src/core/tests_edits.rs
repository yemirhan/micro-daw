//! Edits during playback on the real core: a replaced schedule ends only the sounding events it
//! dropped, with work bounded by what sounds (not by the song's history).

use std::sync::{Arc, Mutex};

use super::schedule::LOOKUPS;
use super::sounding::SOUNDING_CAP;
use super::test_support::*;
use super::*;

fn ev(id: u64, tick: f64, note: f64, dur: f64) -> ScheduledNote {
    ScheduledNote {
        id,
        tick,
        beat: tick / PPQ as f64,
        note,
        velocity: 0.8,
        duration_beats: dur,
        pad: None,
    }
}

fn cuts(log: &Log) -> Vec<(f64, f64, f64)> {
    log.lock()
        .unwrap()
        .iter()
        .filter_map(|(_, c)| match *c {
            Call::Cut {
                note,
                started,
                time,
            } => Some((note, started, time)),
            _ => None,
        })
        .collect()
}

#[test]
fn reconciling_a_replaced_schedule_costs_what_sounds_not_the_history() {
    let mut r = Rig::new();
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    r.h.set_instrument(
        1,
        Some(TrackInstrument::Synth(Box::new(StubSynth {
            log: log.clone(),
        }))),
    );
    // a million expired notes before the play position, three long ones just after it
    const HISTORY: usize = 1_000_000;
    let at = HISTORY as f64;
    let mut events: Vec<ScheduledNote> = (0..HISTORY)
        .map(|i| ev(i as u64 + 10, i as f64, 60.0, 0.25))
        .collect();
    let long = [
        ev(1, at + 1.0, 61.0, 8.0),
        ev(2, at + 2.0, 62.0, 8.0),
        ev(3, at + 3.0, 63.0, 8.0),
    ];
    events.extend(long);
    r.h.set_schedule(1, Some(Arc::new(TrackSchedule::new(false, events.clone()))));
    r.h.start(None, at / PPQ as f64);
    r.run(0.1);
    assert!(cuts(&log).is_empty());

    // drop the second long note (it sounds) and a hundred thousand expired ones
    let edited: Vec<ScheduledNote> = events
        .iter()
        .copied()
        .filter(|e| e.id != 2 && e.id % 10 != 0)
        .collect();
    LOOKUPS.with(|c| c.set(0));
    r.h.set_schedule(1, Some(Arc::new(TrackSchedule::new(false, edited))));
    let t = r.now();
    r.run(0.01);
    assert_eq!(
        LOOKUPS.with(|c| c.get()),
        3,
        "one lookup per sounding note, none for the expired history"
    );
    let c = cuts(&log);
    assert_eq!(c.len(), 1, "{c:?}");
    assert_eq!(c[0].0, 62.0);
    assert!((c[0].2 - t).abs() < 0.01, "cut at the edit: {c:?}");
    assert!(r.core.tracks[1].sounding.len() <= SOUNDING_CAP);
    assert_eq!(r.core.tracks[1].sounding.len(), 2);
}

#[test]
fn a_kept_note_is_never_cut_and_a_shortened_one_ends_at_its_new_end() {
    let mut r = Rig::new();
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    r.h.set_instrument(
        1,
        Some(TrackInstrument::Synth(Box::new(StubSynth {
            log: log.clone(),
        }))),
    );
    // two C4s from two clips (ids 1, 2) overlapping, and a D4
    let events = vec![
        ev(1, 0.0, 60.0, 8.0),
        ev(2, 96.0, 60.0, 8.0),
        ev(3, 0.0, 62.0, 8.0),
    ];
    r.h.set_schedule(1, Some(Arc::new(TrackSchedule::new(false, events.clone()))));
    r.h.start(None, 0.0);
    let t0 = r.now();
    r.run(0.6); // past beat 1: all three sound
    // clip 1 deleted, the D4's velocity edited, the second C4 shortened to 2 beats
    let mut d4 = events[2];
    d4.velocity = 0.2;
    let mut c4 = events[1];
    c4.duration_beats = 2.0;
    r.h.set_schedule(1, Some(Arc::new(TrackSchedule::new(false, vec![c4, d4]))));
    let t = r.now();
    r.run(0.01);
    let c = cuts(&log);
    assert_eq!(c.len(), 2, "{c:?}");
    // the deleted clip's C4: that instance (onset t0), now
    let gone = c.iter().find(|x| (x.1 - t0).abs() < 1e-9).expect("cut");
    assert!(gone.0 == 60.0 && (gone.2 - t).abs() < 0.01, "{c:?}");
    // the other C4: its own onset (beat 0.5), at its new end (beat 2.5 = t0 + 1.25 s)
    let short = c
        .iter()
        .find(|x| (x.1 - (t0 + 0.25)).abs() < 1e-9)
        .expect("cut");
    assert!(
        short.0 == 60.0 && (short.2 - (t0 + 1.25)).abs() < 1e-9,
        "{c:?}"
    );
}
