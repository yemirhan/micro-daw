//! The host side of the device callback (`RenderPipe::fill`) never allocates, for any buffer size
//! or format.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;

use mdaw_audio_host::{HostStats, RenderPipe, shared_render};

struct Counting;

thread_local! {
    static ARMED: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if ARMED.with(|a| a.get()) {
            COUNT.with(|c| c.set(c.get() + 1));
        }
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static A: Counting = Counting;

#[test]
fn fill_does_not_allocate() {
    let mut phase = 0.0f32;
    let render = shared_render(move |l, r| {
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            *a = phase.sin();
            *b = -*a;
            phase += 0.01;
        }
    });
    let mut pipe = RenderPipe::new(render, 512, Arc::new(HostStats::default()));
    let mut f32s = vec![0f32; 8192 * 2];
    let mut i16s = vec![0i16; 3000 * 6];
    let mut mono = vec![0u16; 777];

    ARMED.with(|a| a.set(true));
    for frames in [1usize, 128, 511, 512, 513, 4096, 8192] {
        pipe.fill(&mut f32s[..frames * 2], 2);
    }
    pipe.fill(&mut i16s, 6);
    pipe.fill(&mut mono, 1);
    ARMED.with(|a| a.set(false));

    assert_eq!(COUNT.with(|c| c.get()), 0);
    assert!(f32s[2] != 0.0);
}
