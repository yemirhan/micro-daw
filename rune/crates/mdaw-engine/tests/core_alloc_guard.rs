//! `Core::process` must not allocate or free (audio-thread discipline). A counting global
//! allocator counts every allocation / reallocation / free made on this thread while armed;
//! the core runs every path that matters (pattern playback with loop wraps and gates, tempo
//! change, metronome, count-in, live hits from both producers, note repeat on the transport and
//! free-running, schedule and instrument swaps whose old values go to the garbage ring,
//! odd buffer sizes, track strips with every effect on and their ops, automation on the
//! transport, the live synth bus with every setter, the master fader ramp, synth tracks of every
//! engine and the live synth with its notes, preset switch and retirement) with the guard armed.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;

use mdaw_dsp::buffer::AudioBuffer;
use mdaw_engine::contract::Instrument;
use mdaw_engine::core::{
    Core, CoreConfig, CoreHandle, DrumInstrument, ScheduledNote, SlotFx, TrackInstrument,
    TrackSchedule,
};
use mdaw_engine::engine::{AudioRegions, RegionPlay};
use mdaw_engine::fx::automation::AutoEvent;
use mdaw_engine::fx::live::{LiveFx, LiveOp};
use mdaw_engine::fx::state::FxState;
use mdaw_engine::fx::track::{Effect, StripOp};
use mdaw_engine::sampler::{PadBuffer, PadParams, SamplerCommand, SamplerKit};
use mdaw_model::PadIndex;
use mdaw_model::types::{AutomationParameter, EffectParams, FilterType, LfoWaveform};

struct Counting;

thread_local! {
    static ARMED: Cell<bool> = const { Cell::new(false) };
    /// Per thread, so tests running in parallel do not see each other's counts.
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

fn note() {
    if ARMED.try_with(Cell::get).unwrap_or(false) {
        let _ = COUNT.try_with(|c| c.set(c.get() + 1));
    }
}

fn count() -> usize {
    COUNT.with(Cell::get)
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note();
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        note();
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note();
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// A minimal drum instrument with fixed storage.
struct Kit {
    pending: [Option<(f64, f32)>; 64],
    sr: f64,
}

impl Instrument for Kit {
    fn process(&mut self, frame: u64, left: &mut [f32], right: &mut [f32]) {
        for p in self.pending.iter_mut() {
            if let Some((t, v)) = *p {
                let f = (t * self.sr).ceil() as u64;
                if f < frame + left.len() as u64 {
                    let i = f.saturating_sub(frame) as usize;
                    left[i] += v;
                    right[i] += v;
                    *p = None;
                }
            }
        }
    }
}

impl DrumInstrument for Kit {
    fn trigger(&mut self, _: PadIndex, velocity: f64, time: f64) {
        if let Some(p) = self.pending.iter_mut().find(|p| p.is_none()) {
            *p = Some((time, velocity as f32));
        }
    }
    fn release(&mut self, _: PadIndex, _: f64) {}
    fn stop_all(&mut self, _: f64) {
        self.pending = [None; 64];
    }
    fn cancel_pending_after(&mut self, _: Option<PadIndex>, _: f64) {}
    fn pad_is_gate(&self, pad: PadIndex) -> bool {
        pad == 0
    }
}

fn kit() -> Option<TrackInstrument> {
    Some(TrackInstrument::Drum(Box::new(Kit {
        pending: [None; 64],
        sr: 48000.0,
    })))
}

fn schedule(shift: f64) -> Option<Arc<TrackSchedule>> {
    let events = (0..16)
        .map(|i| ScheduledNote {
            id: 0,
            tick: (i as f64 * 0.25 + shift) * 192.0,
            beat: i as f64 * 0.25,
            note: 36.0 + (i % 4) as f64,
            velocity: 0.8,
            duration_beats: 0.25,
            pad: Some(i % 4),
        })
        .collect();
    Some(Arc::new(TrackSchedule::new(true, events)))
}

#[test]
fn process_does_not_allocate() {
    let (mut core, mut h, mut midi) = Core::new(CoreConfig::new(48000));
    let sizes = [128usize, 64, 441, 512, 1, 333, 1024, 127];
    let mut out_l = vec![0f32; 1024];
    let mut out_r = vec![0f32; 1024];
    let mut run = |core: &mut Core, blocks: usize| {
        for i in 0..blocks {
            let n = sizes[i % sizes.len()];
            ARMED.with(|a| a.set(true));
            core.process(&mut out_l[..n], &mut out_r[..n]);
            ARMED.with(|a| a.set(false));
        }
    };

    h.set_instrument(0, kit());
    h.set_instrument(1, kit());
    h.set_schedule(0, schedule(0.0));
    h.set_live_sink(Some(0));
    h.set_loop(Some((0.0, 4.0)));
    h.set_metronome(true);
    h.set_note_repeat(true, 0.25);
    // the engine's setup: track strips (default effects on one, every effect on the other),
    // master bus, a real SamplerKit, audio-region players, the live bus
    h.set_track_strip(0, Some(h.new_track_strip(&FxState::default(), 0.0, -6.0)));
    h.set_track_strip(1, Some(h.new_track_strip(&all_on(), 0.3, 0.0)));
    h.set_track_pan(1, 0.3);
    h.set_master_bus(Some(0.0));
    let live = EffectParams {
        filter_lfo_depth: 0.5,
        delay_wet: 0.3,
        chorus_depth: 0.4,
        comp_ratio: 4.0,
        distortion_amount: 0.3,
        distortion_wet: 0.5,
        eq_low: 3.0,
        ..EffectParams::default()
    };
    h.set_track_strip(
        4,
        Some(SlotFx::Live(Box::new(LiveFx::new(&live, 48000, 0.0)))),
    );
    h.set_instrument(4, kit());
    let buf = Arc::new(AudioBuffer::mono(48000, vec![0.3; 4800]));
    let mut sk = SamplerKit::new(48000, [PadParams::default(); 16], 0);
    sk.set_pad_buffer(
        0,
        Some(PadBuffer {
            forward: buf.clone(),
            reversed: None,
        }),
    );
    h.set_instrument(2, Some(TrackInstrument::Drum(Box::new(sk))));
    h.set_schedule(2, schedule(0.0));
    h.set_track_strip(2, Some(h.new_track_strip(&all_on(), -0.5, -3.0)));
    let play = RegionPlay {
        key: 1,
        buffer: Some(buf.clone()),
        offset: 0.0,
        duration: 0.3,
        gain: 1.0,
    };
    h.set_instrument(
        3,
        Some(TrackInstrument::Audio(Box::new(AudioRegions::new(
            48000,
            vec![play],
        )))),
    );
    let region = ScheduledNote {
        id: 0,
        tick: 0.0,
        beat: 0.0,
        note: 0.0,
        velocity: 1.0,
        duration_beats: 1.0,
        pad: Some(0),
    };
    h.set_schedule(
        3,
        Some(Arc::new(
            TrackSchedule::new(false, vec![region]).with_automation(automation()),
        )),
    );
    h.set_track_strip(3, Some(h.new_track_strip(&FxState::default(), 0.0, 0.0)));
    run(&mut core, 20);
    // live hits through the live bus
    h.set_live_sink(Some(4));
    h.pad_down(5, 0.8, None);
    strip_and_live_ops(&mut h);
    run(&mut core, 200);
    h.pad_up(5, None);
    h.set_live_sink(Some(0));

    // count-in, then the transport with pattern, loop wraps, metronome, gates
    let t0 = h.now() + 0.05;
    h.start_with_count_in(0.0, t0, 4, 0.5);
    midi.pad_down(1, 0.9, Some(t0 + 0.3)); // held through the count-in: repeats
    run(&mut core, 600);
    h.set_bpm(140.0);
    h.pad_down(2, 0.7, None);
    run(&mut core, 300);
    // swaps: the old schedule and instrument leave through the garbage ring
    h.set_schedule(0, schedule(0.125));
    h.set_instrument(1, kit());
    h.set_live_sink(Some(1));
    // the old strip leaves through the garbage ring
    h.set_track_strip(1, Some(h.new_track_strip(&all_on(), 0.0, 0.0)));
    strip_and_live_ops(&mut h);
    h.set_master_volume(-6.0);
    let fresh = Arc::new(AudioBuffer::mono(48000, vec![0.2; 2400]));
    h.kit_command(
        2,
        SamplerCommand::SetPadBuffer {
            pad: 0,
            buffer: Some(PadBuffer {
                forward: fresh.clone(),
                reversed: None,
            }),
        },
    );
    h.set_region_buffer(3, 0, Some(fresh));
    midi.pad_up(1, None);
    h.pad_up(2, None);
    h.seek(2.0);
    run(&mut core, 300);
    // stopped: free-running repeat, then a new run
    h.stop();
    h.pad_down(3, 0.5, None);
    run(&mut core, 300);
    h.pad_up(3, None);
    // the MIDI thread's phase-5 commands: one-shot hit, pad mute, repeat panic
    midi.pad_hit(4, 0.6, None);
    midi.set_pad_mute(true, None);
    midi.pad_down(6, 0.6, None);
    run(&mut core, 50);
    midi.repeat_release_all(None);
    midi.set_pad_mute(false, None);
    midi.pad_up(6, None);
    run(&mut core, 50);
    h.start(None, 0.0);
    run(&mut core, 300);
    h.stop();
    run(&mut core, 50);

    assert_eq!(count(), 0, "allocations/frees inside Core::process");
    let mut events = 0;
    while h.poll_event().is_some() {
        events += 1;
    }
    assert!(
        events > 100,
        "the guarded run did real work ({events} events)"
    );
    assert!(
        h.collect_garbage() >= 5,
        "old schedule + instrument + strip + buffers + kit command returned for freeing"
    );
    assert_eq!(h.shared().leaked_garbage(), 0);
}

/// Every track effect enabled (LFO on, compressor in line).
fn all_on() -> FxState {
    FxState {
        reverb_on: true,
        delay_on: true,
        chorus_on: true,
        dist_on: true,
        eq_on: true,
        eq: [3.0, -2.0, 4.0],
        comp_on: true,
        filter_on: true,
        cutoff: 2000.0,
        lfo_depth: 0.5,
        ..FxState::default()
    }
}

/// One automation lane per parameter, two points each (every op on the transport).
fn automation() -> Vec<AutoEvent> {
    use AutomationParameter as A;
    let params = [
        A::Volume,
        A::Pan,
        A::ReverbWet,
        A::DelayWet,
        A::ChorusDepth,
        A::DistortionWet,
        A::FilterCutoff,
        A::EqLow,
        A::EqMid,
        A::EqHigh,
    ];
    let mut out = Vec::new();
    for (i, p) in params.into_iter().enumerate() {
        for (beat, value) in [(0.25 * i as f64, 0.2), (1.0 + 0.25 * i as f64, 0.8)] {
            out.push(AutoEvent {
                tick: beat * 192.0 + 0.5,
                op: StripOp::Automation {
                    param: p,
                    value,
                    ramp: (beat < 1.0).then_some((0.9, 0.4)),
                },
            });
        }
    }
    out
}

/// Every strip op on slots 1 and 2, every live setter on slot 4 (rewires both ways).
fn strip_and_live_ops(h: &mut CoreHandle) {
    let mut fx = all_on();
    for e in [
        Effect::Reverb,
        Effect::Delay,
        Effect::Chorus,
        Effect::Distortion,
        Effect::Eq,
        Effect::Compressor,
        Effect::Filter,
    ] {
        h.strip_op(1, StripOp::Apply(e, fx));
    }
    fx.lfo_depth = 0.0; // LFO off (disposed)
    fx.comp_on = false; // compressor bypassed
    h.strip_op(1, StripOp::Apply(Effect::Filter, fx));
    h.strip_op(1, StripOp::Apply(Effect::Compressor, fx));
    h.strip_op(
        2,
        StripOp::Reset {
            fx,
            pan: 0.2,
            volume_db: -1.0,
        },
    );
    h.strip_op(
        2,
        StripOp::Reset {
            fx: all_on(),
            pan: 0.0,
            volume_db: 0.0,
        },
    );
    h.strip_op(2, StripOp::Volume(-4.0));
    h.strip_op(2, StripOp::ForceDistortionAmount(0.4));
    h.strip_op(
        2,
        StripOp::ForceDelayTimes {
            time: 0.3,
            feedback: 0.4,
        },
    );
    for op in [
        LiveOp::ReverbWet(0.0),
        LiveOp::ReverbWet(0.3),
        LiveOp::ChorusDepth(0.0),
        LiveOp::ChorusDepth(0.7),
        LiveOp::FilterCutoff(900.0),
        LiveOp::FilterResonance(4.0),
        LiveOp::FilterType(FilterType::Bandpass),
        LiveOp::FilterLfoRate(3.0),
        LiveOp::FilterLfoWave(LfoWaveform::Square),
        LiveOp::FilterLfoDepth(0.0),
        LiveOp::FilterLfoDepth(0.8),
        LiveOp::DelayTime(0.1),
        LiveOp::DelayFeedback(0.6),
        LiveOp::DelayWet(0.0),
        LiveOp::DelayWet(0.5),
        LiveOp::DistortionAmount(0.0),
        LiveOp::DistortionAmount(0.6),
        LiveOp::DistortionWet(0.4),
        LiveOp::EqLow(0.0),
        LiveOp::EqMid(-5.0),
        LiveOp::EqHigh(2.0),
        LiveOp::CompThreshold(-30.0),
        LiveOp::CompRatio(1.0),
        LiveOp::CompRatio(8.0),
        LiveOp::CompAttack(0.01),
        LiveOp::CompRelease(0.1),
        LiveOp::Volume(-3.0),
        LiveOp::Mute(true),
        LiveOp::Mute(false),
    ] {
        h.live_op(4, op);
    }
}

fn pad_buffer(value: f32, seconds: f64) -> PadBuffer {
    PadBuffer {
        forward: Arc::new(AudioBuffer::mono(
            48000,
            vec![value; (seconds * 48000.0) as usize],
        )),
        reversed: None,
    }
}

fn set_kit(choke: bool) -> SamplerCommand {
    let mut pads = [PadParams::default(); 16];
    if choke {
        for p in pads.iter_mut().take(4) {
            p.choke_group = 1;
        }
    }
    SamplerCommand::SetKit {
        pads,
        changes: std::array::from_fn(|i| {
            mdaw_engine::sampler::PadChange::Restart(Some(pad_buffer(0.1 + i as f32 * 0.01, 0.5)))
        }),
    }
}

/// Review finding 3: the saturated paths, on the REAL SamplerKit and AudioRegions: live kit swaps
/// (setKit restarts every pad: stop_pad + buffer replacement) while voices play, many swaps per
/// quantum, 300-hit bursts (voice pool exhausted), 120 region-buffer replacements in one
/// quantum, and a UI that does not collect garbage for a while. Before the fix the kit's /
/// regions' bounded garbage lists dropped the overflow on the audio thread (Sol measured 96
/// frees with 66 kit swaps, 108 with 100 region replacements).
#[test]
fn saturated_paths_never_free_on_the_audio_thread() {
    let (mut core, mut h, mut midi) = Core::new(CoreConfig::new(48000));
    let mut out_l = vec![0f32; 512];
    let mut out_r = vec![0f32; 512];
    let mut frees_in_process = 0;
    let mut run = |core: &mut Core, blocks: usize| {
        for i in 0..blocks {
            let n = [128usize, 256, 64, 512][i % 4];
            let before = count();
            ARMED.with(|a| a.set(true));
            core.process(&mut out_l[..n], &mut out_r[..n]);
            ARMED.with(|a| a.set(false));
            frees_in_process += count() - before;
        }
    };
    let mut sk = SamplerKit::new(48000, [PadParams::default(); 16], 0);
    for pad in 0..16 {
        sk.set_pad_buffer(pad, Some(pad_buffer(0.2, 0.5)));
    }
    h.set_instrument(0, Some(TrackInstrument::Drum(Box::new(sk))));
    h.set_live_sink(Some(0));
    let plays: Vec<RegionPlay> = (0..4)
        .map(|_| RegionPlay {
            key: 1,
            buffer: Some(Arc::new(AudioBuffer::mono(48000, vec![0.1; 48000]))),
            offset: 0.0,
            duration: 1.0,
            gain: 1.0,
        })
        .collect();
    h.set_instrument(
        1,
        Some(TrackInstrument::Audio(Box::new(AudioRegions::new(
            48000, plays,
        )))),
    );
    let regions = (0..4)
        .map(|i| ScheduledNote {
            id: 0,
            tick: i as f64 * 48.0,
            beat: i as f64 * 0.25,
            note: 0.0,
            velocity: 1.0,
            duration_beats: 2.0,
            pad: Some(i),
        })
        .collect();
    h.set_schedule(1, Some(Arc::new(TrackSchedule::new(false, regions))));
    h.set_loop(Some((0.0, 1.0)));
    h.start(None, 0.0);
    run(&mut core, 10);
    h.collect_garbage();

    // 300-hit bursts in one quantum (voice pool exhausted: 8 listed + stolen fading per pad)
    for burst in 0..3 {
        for i in 0..300 {
            midi.pad_down(i % 16, 0.9, None);
        }
        run(&mut core, 2);
        if burst == 1 {
            h.collect_garbage();
        }
    }
    // 66 live kit swaps while all those voices play, several per quantum, the UI collecting
    // only every few frames (backpressure keeps the rest queued)
    for i in 0..66 {
        h.kit_command(0, set_kit(i % 2 == 0));
        for _ in 0..20 {
            midi.pad_down(i % 16, 0.8, None);
        }
        if i % 5 == 4 {
            run(&mut core, 1);
        }
        if i % 11 == 10 {
            h.collect_garbage();
        }
        h.flush();
    }
    // 120 region-buffer replacements in one quantum while the regions play
    for i in 0..120 {
        h.set_region_buffer(
            1,
            i % 4,
            Some(Arc::new(AudioBuffer::mono(48000, vec![0.05; 4800]))),
        );
    }
    for _ in 0..200 {
        run(&mut core, 1);
        if h.queued() > 0 {
            h.collect_garbage();
            h.flush();
        }
    }
    h.stop();
    h.flush();
    run(&mut core, 50);

    assert_eq!(h.queued(), 0, "everything was eventually sent");
    assert_eq!(
        frees_in_process, 0,
        "allocations/frees inside Core::process"
    );
    assert_eq!(h.shared().leaked_garbage(), 0, "nothing leaked either");
    assert!(h.collect_garbage() > 100, "the retired buffers came back");
}

/// Synth tracks (every engine, the sampled piano with its samples, through strips with every
/// effect on) and the live instrument (`SynthPlayer` with its published status, on the live bus):
/// pattern notes via `triggerAttackRelease`, loop wraps, live notes from both producers,
/// `releaseAll`, a preset switch (`ReplaceSynth`: the old instrument releases, rings out on the
/// bus and is retired through the garbage ring) and Stop releasing every synth. Live parameter
/// edits (`SynthSet`) have their own case below.
#[test]
fn synth_tracks_and_the_live_synth_do_not_allocate() {
    use mdaw_engine::core::SynthNoteOp;
    use mdaw_engine::piano::PianoSlot;
    use mdaw_engine::piano::samples::piano_samples;
    use mdaw_engine::player::{SynthPlayer, SynthStatus};
    use mdaw_engine::synth::presets::preset_at;

    const SR: u32 = 48000;
    let piano = PianoSlot::ready(piano_samples(SR).expect("bundled piano samples"));
    let player = |i: f64| SynthPlayer::new(preset_at(i), SR, 0.0, &piano, 1);
    let synth = |i: f64| Some(TrackInstrument::Synth(Box::new(player(i))));
    let notes = |base: f64| -> Option<Arc<TrackSchedule>> {
        let events = (0..12)
            .map(|i| ScheduledNote {
                id: 0,
                tick: i as f64 * 0.33 * 192.0,
                beat: i as f64 * 0.33,
                note: base + (i * 5 % 12) as f64,
                velocity: 0.5 + 0.04 * i as f64,
                duration_beats: 0.2 + 0.1 * (i % 3) as f64,
                pad: None,
            })
            .collect();
        Some(Arc::new(TrackSchedule::new(false, events)))
    };

    let (mut core, mut h, mut midi) = Core::new(CoreConfig::new(SR));
    let sizes = [128usize, 64, 441, 512, 1, 333, 1024, 127];
    let mut out_l = vec![0f32; 1024];
    let mut out_r = vec![0f32; 1024];
    let mut run = |core: &mut Core, blocks: usize| {
        for i in 0..blocks {
            let n = sizes[i % sizes.len()];
            ARMED.with(|a| a.set(true));
            core.process(&mut out_l[..n], &mut out_r[..n]);
            ARMED.with(|a| a.set(false));
        }
    };
    // airy-pad (noise), fm-keys, harp (pluck), reese-bass (mono legato), grand piano (samples)
    for (slot, preset) in [(0, 23.0), (1, 2.0), (2, 29.0), (3, 24.0), (4, 0.0)] {
        h.set_instrument(slot, synth(preset));
        h.set_schedule(slot, notes(48.0 + 3.0 * slot as f64));
        h.set_track_strip(slot, Some(h.new_track_strip(&all_on(), 0.2, -3.0)));
    }
    h.set_master_bus(Some(0.0));
    let status = Arc::new(SynthStatus::default());
    let live_slot = 5;
    h.set_track_strip(
        live_slot,
        Some(SlotFx::Live(Box::new(LiveFx::new(
            &EffectParams {
                delay_wet: 0.3,
                chorus_depth: 0.4,
                ..EffectParams::default()
            },
            SR,
            0.0,
        )))),
    );
    h.set_instrument(
        live_slot,
        Some(TrackInstrument::Synth(Box::new(
            player(1.0).with_status(status.clone(), 1),
        ))),
    );
    h.set_loop(Some((0.0, 4.0)));
    run(&mut core, 10);
    h.start(None, 0.0);
    for k in 0..6 {
        h.synth_note(
            live_slot,
            SynthNoteOp::On {
                note: 60.0 + k as f64,
                velocity: 0.8,
            },
            0.004,
        );
        midi.synth_note(
            live_slot,
            SynthNoteOp::On {
                note: 72.0 + k as f64,
                velocity: 0.6,
            },
            0.004,
        );
        run(&mut core, 40);
        h.synth_note(
            live_slot,
            SynthNoteOp::Off {
                note: 60.0 + k as f64,
            },
            0.004,
        );
    }
    run(&mut core, 400);
    // preset switch with notes held: the old one releases (+0.1 s) and retires after 1.2 s
    let next = Some(TrackInstrument::Synth(Box::new(
        player(28.0).with_status(status.clone(), 2),
    )));
    h.replace_synth(live_slot, next, 0.1, 1.2);
    midi.synth_note(
        live_slot,
        SynthNoteOp::On {
            note: 64.0,
            velocity: 0.9,
        },
        0.004,
    );
    // MIDI-timestamped live notes (phase 5): at an audio time + the live offset
    let at = h.now() + 0.01;
    midi.synth_note_at(
        live_slot,
        SynthNoteOp::On {
            note: 67.0,
            velocity: 0.7,
        },
        Some(at),
        0.004,
    );
    midi.synth_note_at(
        live_slot,
        SynthNoteOp::Off { note: 67.0 },
        Some(at + 0.05),
        0.004,
    );
    run(&mut core, 700);
    h.synth_note(live_slot, SynthNoteOp::ReleaseAll, 0.004);
    h.set_bpm(150.0);
    run(&mut core, 300);
    h.stop();
    run(&mut core, 300);

    assert_eq!(count(), 0, "allocations/frees inside Core::process");
    assert!(status.active_voice_count() == 0 && status.held_notes().is_empty());
    assert!(
        h.collect_garbage() >= 1,
        "the retired live instrument is returned for freeing"
    );
    assert_eq!(h.shared().leaked_garbage(), 0);
}

/// Live parameter edits (`setSynthParam` → `Command::SynthSet`) while the instrument plays
/// pattern notes and held notes: everything that allocates is prepared on the control side
/// (`SynthPatch::prepare`), so applying them allocates and frees nothing. Cases: cutoff-only
/// edits (a slider drag), a unison change (fat oscillator rebuild), polyphony growth (fresh
/// voices into the pre-sized storage), a switch to pulse, harp (pluck) edits incl. polyphony
/// growth (comb buffers), and a sampled-piano edit. Prints each case's allocations and the
/// worst cost of the quantum that applied an edit.
#[test]
fn live_synth_parameter_edits_do_not_allocate() {
    use mdaw_engine::core::SynthNoteOp;
    use mdaw_engine::piano::PianoSlot;
    use mdaw_engine::piano::samples::piano_samples;
    use mdaw_engine::player::{SynthPatch, SynthPlayer, SynthStatus};
    use mdaw_engine::synth::params::merge_preset_params;
    use mdaw_engine::synth::presets::{SynthPreset, preset_at};
    use serde_json::json;
    use std::time::{Duration, Instant};

    const SR: u32 = 48000;
    let piano = PianoSlot::ready(piano_samples(SR).expect("bundled piano samples"));
    let cases: [(&str, f64, Vec<serde_json::Value>); 6] = [
        (
            "cutoff only (saw-lead)",
            1.0,
            (0..24)
                .map(|i| json!({ "filter": { "cutoff": 400.0 + 150.0 * i as f64 } }))
                .collect(),
        ),
        (
            "unison 1 → 5 → 3 (saw-lead)",
            1.0,
            vec![
                json!({ "oscA": { "unison": 5 } }),
                json!({ "oscA": { "unison": 3, "spread": 30 } }),
            ],
        ),
        (
            "polyphony 8 → 32 → 128 (saw-lead)",
            1.0,
            vec![json!({ "polyphony": 32 }), json!({ "polyphony": 128 })],
        ),
        (
            "saw → pulse, fat → pulse (saw-lead)",
            1.0,
            vec![
                json!({ "oscA": { "wave": "pulse", "width": 0.3 } }),
                json!({ "oscB": { "wave": "pulse" } }),
            ],
        ),
        (
            "harp: dampening, release, polyphony 8 → 24",
            29.0,
            vec![
                json!({ "dampening": 3000 }),
                json!({ "release": 2.5 }),
                json!({ "polyphony": 24 }),
            ],
        ),
        (
            "grand piano: release, gain, character",
            0.0,
            vec![
                json!({ "release": 1.2 }),
                json!({ "gain": -2 }),
                json!({ "character": { "drive": 0.3 } }),
            ],
        ),
    ];
    let mut report = Vec::new();
    for (name, preset, edits) in cases {
        let (mut core, mut h, _midi) = Core::new(CoreConfig::new(SR));
        let status = Arc::new(SynthStatus::default());
        let mut current: SynthPreset = preset_at(preset).clone();
        h.set_instrument(
            0,
            Some(TrackInstrument::Synth(Box::new(
                SynthPlayer::new(&current, SR, 0.0, &piano, 1).with_status(status.clone(), 1),
            ))),
        );
        let events = (0..16)
            .map(|i| ScheduledNote {
                id: 0,
                tick: i as f64 * 0.25 * 192.0,
                beat: i as f64 * 0.25,
                note: 55.0 + (i * 7 % 12) as f64,
                velocity: 0.4 + 0.035 * i as f64,
                duration_beats: 0.4,
                pad: None,
            })
            .collect();
        h.set_schedule(0, Some(Arc::new(TrackSchedule::new(false, events))));
        h.set_loop(Some((0.0, 4.0)));
        let mut out_l = [0f32; 128];
        let mut out_r = [0f32; 128];
        let mut worst = Duration::ZERO;
        let mut plain: Vec<Duration> = Vec::with_capacity(4096);
        let mut run = |core: &mut Core, quanta: usize, timed: bool, worst: &mut Duration| {
            for i in 0..quanta {
                ARMED.with(|a| a.set(true));
                let t0 = Instant::now();
                core.process(&mut out_l, &mut out_r);
                let dt = t0.elapsed();
                ARMED.with(|a| a.set(false));
                if timed && i == 0 {
                    *worst = (*worst).max(dt);
                } else if plain.len() < plain.capacity() {
                    plain.push(dt);
                }
            }
        };
        run(&mut core, 4, false, &mut worst);
        h.start(None, 0.0);
        for n in [60.0, 64.0, 67.0] {
            h.synth_note(
                0,
                SynthNoteOp::On {
                    note: n,
                    velocity: 1.0,
                },
                0.004,
            );
        }
        run(&mut core, 200, false, &mut worst);
        let before = count();
        let mut collected = 0;
        for (k, e) in edits.iter().enumerate() {
            current = merge_preset_params(&current, Some(e));
            let used = status.snapshot().used;
            let patch = SynthPatch::prepare(current.clone(), SR, h.now(), used);
            h.synth_set(0, patch);
            run(&mut core, 1, true, &mut worst);
            run(&mut core, 30, false, &mut worst);
            if k % 4 == 3 {
                collected += h.collect_garbage();
            }
        }
        h.synth_note(0, SynthNoteOp::ReleaseAll, 0.004);
        run(&mut core, 300, false, &mut worst);
        let allocs = count() - before;
        drop(run);
        plain.sort();
        report.push(format!(
            "{name}: {} edits, {allocs} allocations/frees on the audio thread, worst edit \
             quantum {:.1} µs (median quantum {:.1} µs)",
            edits.len(),
            worst.as_secs_f64() * 1e6,
            plain[plain.len() / 2].as_secs_f64() * 1e6
        ));
        assert_eq!(allocs, 0, "{name}: allocations/frees inside Core::process");
        collected += h.collect_garbage();
        assert!(collected >= edits.len(), "{name}: the patches come back");
        assert_eq!(h.shared().leaked_garbage(), 0);
        assert_eq!(h.shared().non_finite_outputs(), 0, "{name}");
    }
    eprintln!("live synth edits:\n  {}", report.join("\n  "));
}
