//! Ports of `src/audio/sampler/SamplerKit.test.ts` and `SamplerKit.loader.test.ts` (the TS checks
//! the calls on a Tone mock; here the same behaviour is checked on the voices' state and their
//! envelope timelines), plus audio-thread rules (no allocation, garbage return).

use super::control::{KitController, PadLoader};
use super::*;
use crate::buffers::{BufferLoadError, PadBufferCache};
use crate::test_alloc::allocations_during;
use mdaw_dsp::buffer::AudioBuffer;
use mdaw_model::kit::create_factory_kit;
use mdaw_model::types::{KitState, PadSample, PadSampleSource};
use std::sync::atomic::{AtomicUsize, Ordering};

const SR: u32 = 48000;

/// Mock factory buffers are 10 ms long (`createFakeAudioBuffer` in the TS mock).
fn short_buffer(frames: usize) -> SharedBuffer {
    Arc::new(AudioBuffer::mono(
        SR,
        (0..frames).map(|i| ((i as f32) * 0.01).sin()).collect(),
    ))
}

/// A loader handing every pad with a sample the same `frames`-long buffer.
fn loader(frames: usize) -> Arc<PadLoader> {
    let b = short_buffer(frames);
    Arc::new(move |_, _| Ok(Some(b.clone())))
}

fn ready_kit_with(kit: &KitState, frames: usize) -> (KitController, SamplerKit) {
    let (mut ctl, mut cmds) =
        KitController::new(kit, PadBufferCache::with_fs(SR, None), Some(loader(frames)));
    cmds.extend(ctl.wait_ready());
    let mut s = SamplerKit::new(SR, ctl.pad_params(), 0);
    for c in cmds {
        s.apply(c);
    }
    (ctl, s)
}

fn ready_kit(kit: &KitState) -> SamplerKit {
    ready_kit_with(kit, 480).1
}

fn voice(s: &SamplerKit, h: &VoiceHandle) -> Voice {
    s.voices[h.slot as usize].clone()
}

fn alive(s: &SamplerKit, h: &VoiceHandle) -> bool {
    s.voices[h.slot as usize].serial == h.serial
}

#[test]
fn compute_slice_applies_trim_and_pitch() {
    let base = PadParams::default();
    let s = compute_slice(
        &PadParams {
            start_sec: 0.25,
            end_sec: Some(0.75),
            ..base
        },
        1.0,
    )
    .unwrap();
    assert_eq!(
        s,
        Slice {
            offset: 0.25,
            length: 0.5,
            rate: 1.0,
            real_duration: 0.5
        }
    );
    let up = compute_slice(
        &PadParams {
            pitch: 12.0,
            ..base
        },
        1.0,
    )
    .unwrap();
    assert!((up.rate - 2.0).abs() < 1e-12);
    assert!((up.real_duration - 0.5).abs() < 1e-12);
}

#[test]
fn compute_slice_mirrors_the_offset_when_reversed_and_rejects_empty_slices() {
    let base = PadParams::default();
    let r = compute_slice(
        &PadParams {
            start_sec: 0.1,
            end_sec: Some(0.4),
            reverse: true,
            ..base
        },
        1.0,
    )
    .unwrap();
    assert!((r.offset - 0.6).abs() < 1e-12);
    assert!(
        compute_slice(
            &PadParams {
                start_sec: 0.5,
                end_sec: Some(0.5),
                ..base
            },
            1.0
        )
        .is_none()
    );
    // ≤ 0.5 ms is silent too.
    assert!(
        compute_slice(
            &PadParams {
                end_sec: Some(0.0005),
                ..base
            },
            1.0
        )
        .is_none()
    );
}

#[test]
fn voice_envelope_holds_then_fades_linearly() {
    use EnvelopePoint::*;
    let (p, n) = voice_envelope(1.0, 2.0, 0.8, 0.0, 0.5);
    assert_eq!(
        &p[..n],
        &[
            Set {
                value: 0.8,
                time: 1.0
            },
            Set {
                value: 0.8,
                time: 2.5
            },
            Ramp {
                value: 0.0,
                time: 3.0
            }
        ]
    );
    let (p, n) = voice_envelope(0.0, 1.0, 1.0, 0.002, 1.0);
    assert_eq!(
        &p[..n],
        &[
            Set {
                value: 0.0,
                time: 0.0
            },
            Ramp {
                value: 1.0,
                time: 0.002
            },
            Ramp {
                value: 0.0,
                time: 1.0
            }
        ]
    );
    let (p, n) = voice_envelope(3.0, 0.01, 1.0, 0.002, 4.0);
    let t = |e: &EnvelopePoint| match *e {
        Set { time, .. } | Ramp { time, .. } => time,
    };
    for w in p[..n].windows(2) {
        assert!(t(&w[1]) >= t(&w[0]));
    }
    assert_eq!(
        p[n - 1],
        Ramp {
            value: 0.0,
            time: 3.01
        }
    );
}

#[test]
fn velocity_normalisation_and_gain_curve() {
    assert_eq!(norm_velocity(f64::NAN), 0.8);
    assert_eq!(norm_velocity(127.0), 1.0);
    assert!((norm_velocity(64.0) - 64.0 / 127.0).abs() < 1e-15);
    assert_eq!(norm_velocity(1.0), 1.0);
    assert_eq!(norm_velocity(-3.0), 0.0);
    assert_eq!(norm_velocity(500.0), 1.0);
    assert!((velocity_gain(1.0, 0.85) - 1.0).abs() < 1e-15);
    assert!((velocity_gain(0.0, 0.85) - 0.15).abs() < 1e-15);
}

#[test]
fn plays_a_pad_with_velocity_gain_pitch_and_its_own_envelope() {
    let mut kit = create_factory_kit();
    kit.pads[0].pitch = 12.0;
    let mut s = ready_kit(&kit);
    let h = s.trigger(0, 127.0, 1.0).unwrap();
    let v = voice(&s, &h);
    assert!((v.player.as_ref().unwrap().computed_rate() - 2.0).abs() < 1e-6);
    assert_eq!(v.env.get_value_at_time(1.0), 1.0);
    let soft = s.trigger(0, 0.5, 2.0).unwrap();
    assert!(voice(&s, &soft).env.get_value_at_time(2.0) < 0.5);
}

#[test]
fn ignores_invalid_pads_zero_velocity_and_empty_pads() {
    let mut kit = create_factory_kit();
    kit.pads[4].sample = None;
    let mut s = ready_kit(&kit);
    assert!(s.trigger(4, 1.0, 0.0).is_none());
    assert!(s.trigger(16, 1.0, 0.0).is_none());
    assert!(s.trigger(0, 0.0, 0.0).is_none());
    assert!(s.voices.iter().all(|v| v.serial == 0));
}

#[test]
fn chokes_other_pads_of_the_group_from_the_held_level() {
    let mut s = ready_kit(&create_factory_kit());
    let open = s.trigger(3, 1.0, 0.0).unwrap(); // open hat, choke group 1
    s.trigger(2, 1.0, 0.001).unwrap(); // closed hat
    let v = voice(&s, &open);
    assert_eq!(v.stop_at, 0.001);
    assert!((v.end - (0.001 + CHOKE_FADE)).abs() < 1e-15);
    assert!(v.env.get_value_at_time(0.001) > 0.85);
    assert_eq!(v.env.get_value_at_time(0.001 + CHOKE_FADE), 0.0);
}

#[test]
fn a_choke_does_not_silence_a_voice_scheduled_after_the_choking_hit() {
    let mut kit = create_factory_kit();
    kit.pads[3].decay = 0.005;
    let mut s = ready_kit(&kit);
    let open = s.trigger(3, 1.0, 0.5).unwrap();
    let before = voice(&s, &open);
    s.trigger(2, 1.0, 0.4).unwrap();
    assert_eq!(voice(&s, &open).end, before.end);
    s.trigger(2, 1.0, 0.502).unwrap(); // while it sounds (10 ms buffers)
    assert_eq!(voice(&s, &open).stop_at, 0.502);
}

#[test]
fn a_second_choke_during_a_fade_does_nothing() {
    let mut kit = create_factory_kit();
    kit.pads[3].decay = 4.0;
    let mut s = ready_kit(&kit);
    let open = s.trigger(3, 1.0, 0.0).unwrap();
    s.trigger(2, 1.0, 0.001).unwrap();
    let after_first = voice(&s, &open);
    s.trigger(2, 1.0, 0.0012).unwrap();
    let v = voice(&s, &open);
    assert_eq!((v.stop_at, v.end), (after_first.stop_at, after_first.end));
}

#[test]
fn caps_polyphony_per_pad_by_fading_out_the_oldest_voice() {
    let mut s = ready_kit(&create_factory_kit());
    let hs: Vec<_> = (0..=MAX_VOICES_PER_PAD)
        .map(|i| s.trigger(0, 1.0, i as f64 * 0.0001).unwrap())
        .collect();
    let first = voice(&s, &hs[0]);
    assert!(!first.listed);
    assert_eq!(first.stop_at, MAX_VOICES_PER_PAD as f64 * 0.0001);
    assert_eq!(s.live_voices(0), MAX_VOICES_PER_PAD);
    // A voice that has not started is cancelled instead.
    let mut s = ready_kit(&create_factory_kit());
    let hs: Vec<_> = (0..MAX_VOICES_PER_PAD)
        .map(|i| s.trigger(0, 1.0, 1.0 + i as f64 * 0.001).unwrap())
        .collect();
    s.trigger(0, 1.0, 0.5).unwrap();
    assert!(!alive(&s, &hs[0]));
    assert!(alive(&s, &hs[1]));
}

#[test]
fn gate_mode_releases_one_shot_ignores_it() {
    let mut kit = create_factory_kit();
    kit.pads[0].mode = PadPlayMode::Gate;
    kit.pads[0].decay = 0.002;
    let mut s = ready_kit(&kit);
    let gate = s.trigger(0, 1.0, 0.0).unwrap();
    let oneshot = s.trigger(1, 1.0, 0.0).unwrap();
    let os_before = voice(&s, &oneshot);
    s.release(0, 0.001);
    s.release(1, 0.001);
    assert_eq!(voice(&s, &gate).stop_at, 0.001);
    let g = voice(&s, &gate);
    // decay is clamped to ≥ 5 ms.
    assert!((g.end - 0.006).abs() < 1e-15, "{} {}", g.end, g.stop_at);
    assert_eq!(voice(&s, &oneshot).stop_at, os_before.stop_at);
}

#[test]
fn release_cancels_gate_voices_scheduled_after_it() {
    let mut kit = create_factory_kit();
    kit.pads[0].mode = PadPlayMode::Gate;
    let mut s = ready_kit(&kit);
    let future = s.trigger(0, 1.0, 0.2).unwrap();
    s.release(0, 0.1);
    assert!(!alive(&s, &future));
}

#[test]
fn a_gate_release_past_the_slice_end_does_not_untrack_the_voice() {
    let mut kit = create_factory_kit();
    kit.pads[0].mode = PadPlayMode::Gate;
    kit.pads[0].decay = 0.005;
    let mut s = ready_kit(&kit);
    let h = s.trigger(0, 1.0, 0.0).unwrap(); // 10 ms slice
    s.release(0, 0.5);
    s.stop_all(Some(0.002));
    assert_eq!(voice(&s, &h).stop_at, 0.002);
}

#[test]
fn voice_handles_cancel_only_voices_that_have_not_started() {
    let mut s = ready_kit(&create_factory_kit());
    s.set_now_frame(SR as u64); // now = 1 s
    let a = s.trigger(0, 1.0, 1.0).unwrap();
    let b = s.trigger(0, 1.0, 1.05).unwrap();
    assert_eq!((a.pad, a.start, b.start), (0, 1.0, 1.05));
    assert!(!s.cancel_if_pending(&a));
    assert!(s.cancel_if_pending(&b));
    assert!(!alive(&s, &b));
    assert!(alive(&s, &a));
    // A stale handle (slot reused) is ignored.
    assert!(!s.cancel_if_pending(&b));
}

#[test]
fn stop_all_cancels_voices_that_have_not_started() {
    let mut s = ready_kit(&create_factory_kit());
    let now = s.trigger(0, 1.0, 0.0).unwrap();
    let later = s.trigger(1, 1.0, 5.0).unwrap();
    s.stop_all(Some(0.001));
    assert_eq!(voice(&s, &now).stop_at, 0.001);
    assert!(!alive(&s, &later));
}

#[test]
fn bypasses_the_filter_when_open_and_inserts_it_when_closed() {
    let (mut ctl, mut s) = ready_kit_with(&create_factory_kit(), 480);
    let mut kit = ctl.kit().clone();
    kit.pads[0].filter_cutoff = 800.0;
    s.apply(ctl.set_kit(&kit));
    assert!(s.channels[0].is_filtered());
    assert!(!s.channels[1].is_filtered());
    kit.pads[1].filter_cutoff = 20000.0;
    s.apply(ctl.set_kit(&kit));
    assert!(!s.channels[1].is_filtered());
    kit.pads[0].filter_cutoff = 20000.0;
    s.apply(ctl.set_kit(&kit));
    assert!(!s.channels[0].is_filtered());
    assert!(s.channels[0].has_filter());
    assert!(!s.channels[1].has_filter());
}

#[test]
fn reloads_only_pads_whose_sample_changed() {
    let calls = Arc::new(AtomicUsize::new(0));
    let c = calls.clone();
    let b = short_buffer(480);
    let load: Arc<PadLoader> = Arc::new(move |_, _| {
        c.fetch_add(1, Ordering::SeqCst);
        Ok(Some(b.clone()))
    });
    let (mut ctl, mut cmds) = KitController::new(
        &create_factory_kit(),
        PadBufferCache::with_fs(SR, None),
        Some(load),
    );
    cmds.extend(ctl.wait_ready());
    let mut s = SamplerKit::new(SR, ctl.pad_params(), 0);
    cmds.into_iter().for_each(|c| s.apply(c));
    let mut kit = ctl.kit().clone();
    kit.pads[5].sample = Some(PadSample {
        name: "hit".into(),
        source: PadSampleSource::File {
            path: "/x/hit.wav".into(),
            sample_id: None,
        },
    });
    kit.pads[0].gain_db = -6.0;
    calls.store(0, Ordering::SeqCst);
    let SamplerCommand::SetKit { changes, .. } = ctl.set_kit(&kit) else {
        panic!()
    };
    let restarted: Vec<usize> = (0..PAD_COUNT)
        .filter(|&i| matches!(changes[i], PadChange::Restart(_)))
        .collect();
    assert_eq!(restarted, vec![5]);
    assert!(
        changes
            .iter()
            .enumerate()
            .all(|(i, c)| i == 5 || matches!(c, PadChange::Keep))
    );
    s.apply(SamplerCommand::SetKit {
        pads: ctl.pad_params(),
        changes,
    });
    assert!(!s.is_pad_ready(5)); // silent while loading
    for c in ctl.wait_ready() {
        s.apply(c);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(s.is_pad_ready(5));
}

#[test]
fn plays_reversed_slices_from_a_reversed_buffer() {
    let mut kit = create_factory_kit();
    kit.pads[0].reverse = true;
    let (_ctl, mut s) = ready_kit_with(&kit, 480);
    let h = s.trigger(0, 1.0, 0.0).unwrap();
    let v = voice(&s, &h);
    let played = v.player.as_ref().unwrap().buffer().clone();
    let fwd = s.buffers[0].as_ref().unwrap().forward.clone();
    assert!(!Arc::ptr_eq(&played, &fwd));
    let n = fwd.frames();
    assert_eq!(played.channels[0][0], fwd.channels[0][n - 1]);
    // Switching reverse on later hands over the reversed copy without a reload.
    let (mut ctl, mut s) = ready_kit_with(&create_factory_kit(), 480);
    let mut k = ctl.kit().clone();
    k.pads[1].reverse = true;
    s.apply(ctl.set_kit(&k));
    assert!(s.buffers[1].as_ref().unwrap().reversed.is_some());
    assert!(s.trigger(1, 1.0, 0.0).is_some());
}

#[test]
fn trigger_release_stop_and_process_never_allocate() {
    let mut kit = create_factory_kit();
    kit.pads[0].filter_cutoff = 1000.0;
    kit.pads[1].mode = PadPlayMode::Gate;
    kit.pads[2].reverse = true;
    let (mut ctl, mut s) = ready_kit_with(&kit, 4800);
    let mut k2 = ctl.kit().clone();
    k2.pads[0].pan = 0.5;
    k2.pads[0].filter_cutoff = 3000.0;
    let cmd = ctl.set_kit(&k2);
    let (mut l, mut r) = (vec![0f32; 128], vec![0f32; 128]);
    let n = allocations_during(|| {
        for i in 0..40 {
            s.trigger(i % 4, 0.9, i as f64 * 0.003);
        }
        s.release(1, 0.05);
        s.apply(cmd);
        for q in 0..200u64 {
            s.process(q * 128, &mut l, &mut r);
            if q == 50 {
                s.stop_all(None);
            }
            s.drain_garbage(drop);
        }
    });
    assert_eq!(n, 0, "allocations on the audio path");
}

#[test]
fn replaced_buffers_are_returned_as_garbage_never_freed_in_process() {
    let (_ctl, mut s) = ready_kit_with(&create_factory_kit(), 4800);
    let h = s.trigger(0, 1.0, 0.0).unwrap();
    let played = voice(&s, &h).player.unwrap().buffer().clone();
    s.set_pad_buffer(0, None);
    let mut got = Vec::new();
    s.drain_garbage(|b| got.push(b));
    assert_eq!(got.len(), 1);
    let (mut l, mut r) = (vec![0f32; 128], vec![0f32; 128]);
    for q in 0..40u64 {
        s.process(q * 128, &mut l, &mut r);
    }
    // Pruning the finished voice hands its buffer back too.
    assert_eq!(s.live_voices(0), 0);
    s.drain_garbage(|b| got.push(b));
    assert_eq!(got.len(), 2);
    assert!(got.iter().all(|b| Arc::ptr_eq(b, &played)));
    let before = Arc::strong_count(&played);
    drop(got);
    assert_eq!(Arc::strong_count(&played), before - 2);
}

#[test]
fn loads_pads_through_an_injected_loader_instead_of_the_cache() {
    let mut kit = create_factory_kit();
    kit.pads[2].sample = Some(PadSample {
        name: "remote".into(),
        source: PadSampleSource::File {
            path: "remote:https://x/y.wav".into(),
            sample_id: None,
        },
    });
    let remote = short_buffer(4410);
    let r2 = remote.clone();
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen2 = seen.clone();
    let load: Arc<PadLoader> = Arc::new(move |pad, sample| {
        seen2.lock().unwrap().push((pad, sample.clone()));
        Ok((pad == 2).then(|| r2.clone()))
    });
    let cache = PadBufferCache::with_fs(SR, None);
    let (mut ctl, mut cmds) = KitController::new(&kit, cache.clone(), Some(load));
    cmds.extend(ctl.wait_ready());
    let mut s = SamplerKit::new(SR, ctl.pad_params(), 0);
    cmds.into_iter().for_each(|c| s.apply(c));
    assert!(
        seen.lock()
            .unwrap()
            .iter()
            .any(|(p, smp)| *p == 2 && Some(smp) == kit.pads[2].sample.as_ref())
    );
    assert!(
        cache
            .get_cached(&kit.pads[2].sample.as_ref().unwrap().source)
            .is_none()
    );
    assert!(s.is_pad_ready(2));
    assert!(!s.is_pad_ready(0)); // the loader said "no sound"
    let h = s.trigger(2, 1.0, 0.0).unwrap();
    assert!(Arc::ptr_eq(voice(&s, &h).player.unwrap().buffer(), &remote));
}

#[test]
fn a_rejecting_injected_loader_leaves_the_pad_silent_and_still_settles() {
    let load: Arc<PadLoader> = Arc::new(|_, _| {
        Err(BufferLoadError::Read {
            path: "x".into(),
            message: "offline".into(),
        })
    });
    let (mut ctl, mut cmds) = KitController::new(
        &create_factory_kit(),
        PadBufferCache::with_fs(SR, None),
        Some(load),
    );
    cmds.extend(ctl.wait_ready());
    assert!(!ctl.is_loading());
    assert_eq!(ctl.take_errors().len(), 16);
    let mut s = SamplerKit::new(SR, ctl.pad_params(), 0);
    cmds.into_iter().for_each(|c| s.apply(c));
    assert!(!s.is_pad_ready(0));
}

#[test]
fn prune_uses_the_current_time_never_the_event_time() {
    let mut kit = create_factory_kit();
    kit.pads[0].mode = PadPlayMode::Gate;
    let mut s = ready_kit(&kit);
    let h = s.trigger(0, 1.0, 0.0).unwrap(); // ends at 10 ms
    // A release scheduled after the voice's end must still find it while it is heard.
    s.release(0, 0.02);
    assert!(alive(&s, &h));
    s.set_now_frame((0.011 * SR as f64) as u64);
    assert_eq!(s.live_voices(0), 0);
    assert!(!alive(&s, &h));
}
