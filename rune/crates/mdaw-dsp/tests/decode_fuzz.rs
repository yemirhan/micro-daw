//! Deterministic decode fuzzing: seeded mutations / truncations of every file in `testdata/`
//! (and `testdata/edge/`) through `decode_vec`. Asserts for every case:
//! - no panic escapes (symphonia panics are caught at the decode boundary and become errors);
//! - bounded memory: the peak live heap of the decoding thread stays under
//!   [`MAX_PEAK_BYTES`] (a counting global allocator, per thread);
//! - a successful decode respects the frame budget and Chromium's rate / channel limits.
//!
//! `cargo test -p mdaw-dsp --test decode_fuzz` runs 20 000 cases; the ignored
//! `decode_fuzz_long` runs 100 000 (`-- --ignored`, set `FUZZ_CASES` / `FUZZ_SEED` to vary).

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::path::PathBuf;

use mdaw_dsp::audio_io::guard::{FrameBudget, MAX_CHANNELS, MAX_SAMPLE_RATE, MIN_SAMPLE_RATE};
use mdaw_dsp::audio_io::{DecodeError, decode_vec};

struct Counting;

thread_local! {
    static LIVE: Cell<isize> = const { Cell::new(0) };
    static PEAK: Cell<isize> = const { Cell::new(0) };
}

fn track(delta: isize) {
    let _ = LIVE.try_with(|l| {
        let v = l.get() + delta;
        l.set(v);
        let _ = PEAK.try_with(|p| {
            if v > p.get() {
                p.set(v);
            }
        });
    });
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            track(layout.size() as isize);
        }
        p
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        track(-(layout.size() as isize));
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            track(layout.size() as isize);
        }
        p
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            track(new_size as isize - layout.size() as isize);
        }
        p
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// Peak extra heap of one decode of a ≤ 120 KB input. Real inputs peak at a few MB (decoded
/// output ×2 for `Vec` growth plus symphonia's packet buffers).
const MAX_PEAK_BYTES: isize = 32 << 20;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // splitmix64
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn corpus() -> Vec<(String, Vec<u8>)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata");
    let mut out = Vec::new();
    for dir in [root.clone(), root.join("edge")] {
        let mut names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| mdaw_dsp::audio_io::SUPPORTED_EXTENSIONS.contains(&e))
            })
            .collect();
        names.sort();
        for p in names {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            out.push((name, std::fs::read(&p).unwrap()));
        }
    }
    out
}

const INTERESTING: [u32; 10] = [
    0,
    1,
    0x7f,
    0x80,
    0xff,
    0x7fff,
    0xffff,
    0x7fff_ffff,
    0x8000_0000,
    0xffff_ffff,
];

/// 1–4 mutations: byte flips, interesting 8/16/32-bit values (aimed at header fields), a
/// truncation, a cut-out or duplicated range.
fn mutate(rng: &mut Rng, mut b: Vec<u8>) -> Vec<u8> {
    for _ in 0..1 + rng.below(4) {
        if b.is_empty() {
            break;
        }
        // Header fields live near the start (and MP4 `moov` near the end): bias positions.
        let pos = match rng.below(3) {
            0 => rng.below(b.len().min(512)),
            1 => b.len() - 1 - rng.below(b.len().min(1024)),
            _ => rng.below(b.len()),
        };
        match rng.below(6) {
            0 => {
                for _ in 0..1 + rng.below(16) {
                    let i = rng.below(b.len());
                    b[i] ^= 1 << rng.below(8);
                }
            }
            1 | 2 => {
                let v = INTERESTING[rng.below(INTERESTING.len())];
                let width = [1, 2, 4][rng.below(3)];
                let bytes = if rng.below(2) == 0 {
                    v.to_le_bytes()
                } else {
                    v.to_be_bytes()
                };
                for (k, &x) in bytes.iter().take(width).enumerate() {
                    if let Some(slot) = b.get_mut(pos + k) {
                        *slot = x;
                    }
                }
            }
            3 => b.truncate(pos.max(1)),
            4 => {
                let end = (pos + 1 + rng.below(256)).min(b.len());
                b.drain(pos..end);
            }
            _ => {
                let end = (pos + 1 + rng.below(4096)).min(b.len());
                let chunk = b[pos..end].to_vec();
                let at = rng.below(b.len());
                b.splice(at..at, chunk);
            }
        }
    }
    b
}

#[derive(Default)]
struct Summary {
    ok: usize,
    errors: std::collections::BTreeMap<&'static str, usize>,
    panics_caught: usize,
    worst_peak: isize,
    violations: Vec<String>,
}

fn run(cases: usize, seed: u64) -> Summary {
    let corpus = corpus();
    assert!(corpus.len() >= 30);
    let mut rng = Rng(seed);
    let mut s = Summary::default();
    for case in 0..cases {
        let (name, original) = &corpus[rng.below(corpus.len())];
        let input = mutate(&mut rng, original.clone());
        let len = input.len();
        // FUZZ_DUMP_DIR=<dir> [FUZZ_DUMP_CASE=<n>]: write the inputs (all, or case n) for debugging.
        if let Ok(dir) = std::env::var("FUZZ_DUMP_DIR")
            && std::env::var("FUZZ_DUMP_CASE").map_or(true, |c| c == case.to_string())
        {
            std::fs::write(format!("{dir}/case{case}_{name}"), &input).unwrap();
        }
        let base = LIVE.with(Cell::get);
        PEAK.with(|p| p.set(base));
        let result = std::panic::catch_unwind(|| decode_vec(input, ""));
        let peak = PEAK.with(Cell::get) - base;
        s.worst_peak = s.worst_peak.max(peak);
        let result = result.unwrap_or_else(|_| panic!("case {case} ({name}): panic escaped"));
        if peak > MAX_PEAK_BYTES {
            s.violations.push(format!(
                "case {case} ({name}, {len} bytes): peak heap {} MB",
                peak >> 20
            ));
        }
        match result {
            Ok(b) => {
                s.ok += 1;
                let budget = FrameBudget::new(len, None, b.sample_rate);
                assert!(
                    (MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&b.sample_rate),
                    "case {case}"
                );
                assert!(
                    (1..=MAX_CHANNELS).contains(&b.channel_count()),
                    "case {case}"
                );
                assert!(
                    b.channels.iter().all(|c| c.len() == b.frames()),
                    "case {case}: ragged"
                );
                // Declared durations can raise the budget; the size-only budget bounds real files.
                assert!(
                    b.frames() as u64 <= budget.max_frames.max(1 << 24),
                    "case {case}"
                );
            }
            Err(e) => {
                let kind = match &e {
                    DecodeError::Malformed(m) if m.starts_with("decoder panicked") => {
                        s.panics_caught += 1;
                        "malformed (caught panic)"
                    }
                    DecodeError::Malformed(_) => "malformed",
                    DecodeError::Unsupported(_) => "unsupported",
                    DecodeError::Io(_) => "io",
                    DecodeError::NoAudioTrack => "no audio track",
                    DecodeError::Empty => "empty",
                };
                *s.errors.entry(kind).or_default() += 1;
            }
        }
    }
    println!(
        "{cases} cases: {} decoded, errors {:?}, worst peak heap {:.1} MB",
        s.ok,
        s.errors,
        s.worst_peak as f64 / 1048576.0
    );
    assert!(s.violations.is_empty(), "{:#?}", s.violations);
    s
}

static PANIC_SITES: std::sync::Mutex<std::collections::BTreeMap<String, usize>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

/// Silence the panic hook for panics caught inside the decoder (thousands of messages otherwise)
/// and count them per source location (printed by the long run).
fn quiet_decoder_panics() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let site = info.location().map(|l| {
            let file = l.file().rsplit("registry/src/").next().unwrap_or(l.file());
            format!("{file}:{}", l.line())
        });
        let from_decoder = site
            .as_deref()
            .is_some_and(|s| s.contains("symphonia") || s.contains("/rustc/"));
        if from_decoder {
            let mut sites = PANIC_SITES.lock().unwrap_or_else(|e| e.into_inner());
            *sites.entry(site.unwrap_or_default()).or_default() += 1;
        } else {
            default(info);
        }
    }));
}

#[test]
fn decode_fuzz() {
    quiet_decoder_panics();
    let s = run(20_000, 0x00de_c0de);
    assert!(
        s.ok > 2000,
        "too few successful decodes: the mutations are too destructive"
    );
}

#[test]
#[ignore]
fn decode_fuzz_long() {
    quiet_decoder_panics();
    let cases = std::env::var("FUZZ_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100_000);
    let seed = std::env::var("FUZZ_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0x5eed);
    run(cases, seed);
    let sites = PANIC_SITES.lock().unwrap_or_else(|e| e.into_inner());
    println!("caught decoder panics by site: {sites:#?}");
}
