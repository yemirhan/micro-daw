//! Edits during playback, rendered offline through the engine and core: a clip that is still
//! there keeps playing untouched, a removed one ends without a click, and only the exact note
//! instances an edit removed are cut.

use std::sync::Arc;

use mdaw_dsp::buffer::AudioBuffer;
use mdaw_model::types::SampleRef;
use mdaw_model::{InstrumentType, Region, RegionNote};

use super::test_rig::*;

/// A 220 Hz sine (amplitude 0.5), `seconds` long.
fn sine(seconds: f64) -> Arc<AudioBuffer> {
    let n = (seconds * SR as f64) as usize;
    Arc::new(AudioBuffer::mono(
        SR,
        (0..n)
            .map(|i| {
                (0.5 * (2.0 * std::f64::consts::PI * 220.0 * i as f64 / SR as f64).sin()) as f32
            })
            .collect(),
    ))
}

/// An audio track with one clip per `(sample, beat)` (each sample a 4 s sine); returns the
/// track id and the region ids.
fn audio_clips(r: &mut Rig, clips: &[(&str, f64)]) -> (String, Vec<String>) {
    let t = r.e.edit(|d| d.add_track(InstrumentType::Audio, 0)).id;
    let mut ids = Vec::new();
    for (sample, at) in clips {
        r.samples
            .buffers
            .lock()
            .unwrap()
            .insert((*sample).into(), sine(4.0));
        let s = SampleRef {
            id: (*sample).into(),
            name: (*sample).into(),
            path: format!("/data/{sample}.wav"),
            duration_seconds: 4.0,
            sample_rate: SR as f64,
            channels: 1,
        };
        let region = r.e.edit(|d| d.add_audio_region(&t, Some(&s), *at)).unwrap();
        ids.push(region.id);
    }
    (t, ids)
}

/// Largest sample-to-sample step of the left channel over context seconds [a, b).
fn max_step(r: &Rig, a: f64, b: f64) -> f32 {
    let (fa, fb) = ((a * SR as f64) as usize, (b * SR as f64) as usize);
    (fa.max(1)..fb.min(r.left.len()))
        .map(|i| (r.left[i] - r.left[i - 1]).abs())
        .fold(0.0, f32::max)
}

/// Largest |difference| between two renders' left channels over [a, b).
fn max_diff(x: &Rig, y: &Rig, a: f64, b: f64) -> f32 {
    let (fa, fb) = ((a * SR as f64) as usize, (b * SR as f64) as usize);
    (fa..fb.min(x.left.len()).min(y.left.len()))
        .map(|i| (x.left[i] - y.left[i]).abs())
        .fold(0.0, f32::max)
}

#[test]
fn moving_another_clip_leaves_the_playing_clip_sample_continuous() {
    // clip A plays from beat 0 (4 s); clip B waits at beat 6. Moving B at 1 s, or adding a
    // third clip, must not touch A: the render equals the one without the edits.
    let build = || {
        let mut r = Rig::new();
        let ids = audio_clips(&mut r, &[("a", 0.0), ("b", 6.0)]);
        r.e.play();
        r.run_until(1.0);
        (r, ids)
    };
    let (mut edited, (track, ids)) = build();
    let (mut plain, _) = build();
    assert!(edited.rms(0.5, 1.0) > 0.1, "A plays");
    let te = edited.now();
    edited.e.edit(|d| d.move_region(&track, &ids[1], 7.0));
    edited.run(0.3);
    let s = SampleRef {
        id: "b".into(),
        name: "b".into(),
        path: "/data/b.wav".into(),
        duration_seconds: 4.0,
        sample_rate: SR as f64,
        channels: 1,
    };
    edited
        .e
        .edit(|d| d.add_audio_region(&track, Some(&s), 12.0));
    edited.run(1.0);
    plain.run(1.3);
    let d = max_diff(&edited, &plain, te - 0.1, te + 1.25);
    assert!(d < 1e-6, "the playing clip is untouched by the edits: {d}");
    assert!(edited.rms(te + 0.5, te + 1.2) > 0.1, "and keeps playing");
}

#[test]
fn deleting_the_playing_clip_fades_it_out_without_a_step() {
    let mut r = Rig::new();
    let (track, ids) = audio_clips(&mut r, &[("a", 0.0)]);
    r.e.play();
    r.run_until(1.0);
    let natural = max_step(&r, 0.5, 1.0);
    let level = r.rms(0.5, 1.0);
    assert!(level > 0.1 && natural > 0.0);
    let te = r.now();
    r.e.edit(|d| d.remove_region(&track, &ids[0]));
    r.run(0.5);
    let step = max_step(&r, te - 0.01, te + 0.05);
    assert!(
        step <= natural * 1.5,
        "no click: largest step {step} vs the sine's own {natural}"
    );
    assert!(
        r.rms(te + 0.02, te + 0.4) < level * 0.01,
        "gone after the fade"
    );
}

#[test]
fn a_clip_moved_under_the_playhead_resumes_at_the_current_offset() {
    // the edit moves the clip from beat 8 to beat 1 at beat 2: it resumes 1 beat (0.5 s) in,
    // exactly where a clip at beat 1 from the start would be (after a 5 ms fade-in)
    let (mut edited, (track, ids)) = {
        let mut r = Rig::new();
        let ids = audio_clips(&mut r, &[("a", 8.0)]);
        r.e.play();
        r.run_until(1.0);
        (r, ids)
    };
    let mut plain = Rig::new();
    audio_clips(&mut plain, &[("a", 1.0)]);
    plain.e.play();
    plain.run_until(1.0);
    assert!(edited.rms(0.5, 1.0) < 1e-6);
    let te = edited.now();
    edited.e.edit(|d| d.move_region(&track, &ids[0], 1.0));
    edited.run(0.5);
    plain.run(0.5);
    assert!(edited.rms(te + 0.01, te + 0.4) > 0.1, "resumed");
    let d = max_diff(&edited, &plain, te + 0.01, te + 0.4);
    assert!(d < 2e-3, "at the current offset: {d}");
    let natural = max_step(&plain, te, te + 0.4);
    assert!(
        max_step(&edited, te - 0.01, te + 0.02) <= natural * 1.5,
        "faded in, no click"
    );
}

fn note(n: f64, start: f64, dur: f64, velocity: f64) -> RegionNote {
    RegionNote {
        note: n,
        velocity,
        start_beat: start,
        duration_beats: dur,
        is_drum: false,
    }
}

/// A warm-pad track (release 0.8 s) with one clip per entry, each holding its notes.
fn synth_clips(r: &mut Rig, clips: Vec<Vec<RegionNote>>) -> (String, Vec<String>) {
    let id = r.e.edit(|d| d.add_track(InstrumentType::Synth, 3)).id;
    let mut ids = Vec::new();
    for (i, notes) in clips.into_iter().enumerate() {
        let rid = format!("{id}-r{i}");
        let region = Region {
            id: rid.clone(),
            start_beat: 0.0,
            length_beats: 16.0,
            notes,
            color: None,
            name: None,
            audio: None,
            extra: Default::default(),
        };
        r.e.edit(|d| d.add_region(&id, region));
        ids.push(rid);
    }
    (id, ids)
}

#[test]
fn deleting_one_of_two_overlapping_clips_keeps_the_other_clips_note() {
    // clip 0: C4 over beats 0..6; clip 1: C4 over beats 1..7 (the same pitch, overlapping).
    // Delete either at beat 2: the other's C4 keeps sounding to its own end.
    for (deleted, end_beat) in [(0usize, 7.0), (1, 6.0)] {
        let mut r = Rig::new();
        let (id, rids) = synth_clips(
            &mut r,
            vec![
                vec![note(60.0, 0.0, 6.0, 0.8)],
                vec![note(60.0, 1.0, 6.0, 0.8)],
            ],
        );
        r.e.play();
        let t0 = r.time_of_beat(0.0);
        r.run_until(t0 + 1.0);
        let te = r.now();
        r.e.edit(|d| d.remove_region(&id, &rids[deleted]));
        let end = t0 + end_beat * 0.5;
        r.run_until(end + 2.5);
        assert!(
            r.rms(end - 0.6, end - 0.1) > 1e-3,
            "clip {deleted} deleted: the other C4 still sounds before beat {end_beat}"
        );
        assert!(
            r.rms(end + 1.8, end + 2.4) < 1e-5,
            "and is released at its end"
        );
        assert!(te < end - 1.0);
    }
}

#[test]
fn editing_a_sounding_notes_velocity_does_not_cut_it() {
    let mut r = Rig::new();
    let (id, rids) = synth_clips(&mut r, vec![vec![note(60.0, 0.0, 6.0, 0.8)]]);
    r.e.play();
    let t0 = r.time_of_beat(0.0);
    r.run_until(t0 + 1.0);
    let before = r.rms(t0 + 0.8, t0 + 1.0);
    r.e.edit(|d| d.edit_region_notes(&id, &rids[0], vec![note(60.0, 0.0, 6.0, 0.3)], None, None));
    let te = r.now();
    r.run(1.5);
    let after = r.rms(te + 1.2, te + 1.5);
    assert!(after > before * 0.5, "still sounding: {after} vs {before}");
}
