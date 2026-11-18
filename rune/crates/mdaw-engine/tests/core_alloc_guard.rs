//! `Core::process` must not allocate or free (audio-thread discipline). A counting global
//! allocator counts every allocation / reallocation / free made on this thread while armed;
//! the core runs every path that matters (pattern playback with loop wraps and gates, tempo
//! change, metronome, count-in, live hits from both producers, note repeat on the transport and
//! free-running, schedule and instrument swaps whose old values go to the garbage ring,
//! odd buffer sizes) with the guard armed.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use mdaw_engine::contract::Instrument;
use mdaw_engine::core::{
    Core, CoreConfig, DrumInstrument, ScheduledNote, TrackInstrument, TrackSchedule,
};
use mdaw_model::PadIndex;

struct Counting;

static COUNT: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    static ARMED: Cell<bool> = const { Cell::new(false) };
}

fn note() {
    if ARMED.try_with(Cell::get).unwrap_or(false) {
        COUNT.fetch_add(1, Ordering::Relaxed);
    }
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

    assert_eq!(
        COUNT.load(Ordering::Relaxed),
        0,
        "allocations/frees inside Core::process"
    );
    let mut events = 0;
    while h.poll_event().is_some() {
        events += 1;
    }
    assert!(
        events > 100,
        "the guarded run did real work ({events} events)"
    );
    assert!(
        h.collect_garbage() >= 2,
        "old schedule + instrument returned for freeing"
    );
    assert_eq!(h.shared().leaked_garbage(), 0);
}
