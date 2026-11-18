//! `Core::process` must not allocate or free (audio-thread discipline). A counting global
//! allocator counts every allocation / reallocation / free made on this thread while armed;
//! the core runs every path that matters (pattern playback with loop wraps and gates, tempo
//! change, metronome, count-in, live hits from both producers, note repeat on the transport and
//! free-running, schedule and instrument swaps whose old values go to the garbage ring,
//! odd buffer sizes) with the guard armed.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;

use mdaw_dsp::buffer::AudioBuffer;
use mdaw_engine::contract::Instrument;
use mdaw_engine::core::{
    Core, CoreConfig, DrumInstrument, ScheduledNote, TrackInstrument, TrackSchedule,
};
use mdaw_engine::engine::{AudioRegions, RegionPlay};
use mdaw_engine::sampler::{PadBuffer, PadParams, SamplerCommand, SamplerKit};
use mdaw_model::PadIndex;

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
    // the engine's setup: track strips (FX-chain leaks, panner), master bus, a real SamplerKit,
    // audio-region players
    h.set_track_strip(0, true);
    h.set_track_strip(1, true);
    h.set_track_pan(1, 0.3);
    h.set_master_bus(Some(1.0));
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
    h.set_track_strip(2, true);
    let play = RegionPlay {
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
        tick: 0.0,
        beat: 0.0,
        note: 0.0,
        velocity: 1.0,
        duration_beats: 1.0,
        pad: Some(0),
    };
    h.set_schedule(3, Some(Arc::new(TrackSchedule::new(false, vec![region]))));
    h.set_track_strip(3, true);
    run(&mut core, 20);

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
    h.set_track_strip(1, true); // the old strip leaves through the garbage ring
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
