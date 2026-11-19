//! Ports of the bufferCache cases of `src/audio/sampler/SamplerKit.test.ts` and the runtime
//! parts of `src/audio/sampler/packSources.test.ts` (pack loading, MissingPackSampleError,
//! invalidation, live samplers reloading), plus the Rust-specific threading contract (in-flight
//! dedup across threads, shared failures, retry after failure).

use super::*;
use crate::sampler::SamplerKit;
use crate::sampler::control::KitController;
use mdaw_dsp::audio_io::encode_wav;
use mdaw_model::kit::create_factory_kit;
use mdaw_model::types::{ClassicMachineId, FactoryVoiceId, PadSample};
use std::collections::HashSet;
use std::sync::atomic::Ordering;
use std::sync::mpsc;

const PACKS: &str = "/data/packs";
const RATE: u32 = 48000;

fn kick() -> PadSampleSource {
    PadSampleSource::Pack {
        pack_id: "tr808-fischer".into(),
        file: "bass-drum/bd0010.wav".into(),
    }
}

fn file(path: &str) -> PadSampleSource {
    PadSampleSource::File {
        path: path.into(),
        sample_id: None,
    }
}

fn wav() -> Vec<u8> {
    encode_wav(&AudioBuffer::mono(
        44100,
        (0..441).map(|i| (i as f32 * 0.05).sin() * 0.5).collect(),
    ))
}

/// `electronAPI.sampleReadFile` mock: every path reads as a small WAV unless it is in `missing`;
/// `gate` (when set) blocks each read until a message arrives.
#[derive(Default)]
struct Reader {
    reads: Mutex<Vec<String>>,
    missing: Mutex<HashSet<String>>,
    fail_all: std::sync::atomic::AtomicBool,
    gate: Mutex<Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>>,
}

impl Reader {
    fn count(&self, path: &str) -> usize {
        self.reads
            .lock()
            .unwrap()
            .iter()
            .filter(|p| *p == path)
            .count()
    }
}

impl SampleReader for Reader {
    fn read(&self, path: &str) -> std::io::Result<Vec<u8>> {
        self.reads.lock().unwrap().push(path.to_owned());
        if let Some((entered, release)) = &*self.gate.lock().unwrap() {
            entered.send(()).unwrap();
            release.recv().unwrap();
        }
        if self.fail_all.load(Ordering::SeqCst) || self.missing.lock().unwrap().contains(path) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "ENOENT: no such file or directory",
            ));
        }
        Ok(wav())
    }
}

/// SampleManager mock: counts registrations and in-place reloads.
#[derive(Default)]
struct Files {
    loads: Mutex<Vec<String>>,
    reloads: Mutex<Vec<String>>,
}

impl FileLoader for Files {
    fn load_file(&self, cache: &PadBufferCache, path: &str) -> LoadResult {
        self.loads.lock().unwrap().push(path.to_owned());
        cache.decode_file_direct(path)
    }
    fn reload_by_path_prefix(&self, prefix: &str) {
        self.reloads.lock().unwrap().push(prefix.to_owned());
    }
}

fn setup() -> (PadBufferCache, Arc<Reader>, Arc<Files>) {
    let reader = Arc::new(Reader::default());
    let files = Arc::new(Files::default());
    let cache = PadBufferCache::new(RATE, Some(PACKS.into()), reader.clone(), files.clone());
    (cache, reader, files)
}

impl PadBufferCache {
    fn listener_count(&self) -> usize {
        lock(&self.inner.state).listeners.len()
    }
}

#[test]
fn keys_match_the_ts_format() {
    let (cache, ..) = setup();
    assert_eq!(
        cache.key(&PadSampleSource::Factory {
            voice: FactoryVoiceId::Kick
        }),
        "factory:kick"
    );
    assert_eq!(
        cache.key(&PadSampleSource::Classic {
            machine: ClassicMachineId::Tr808,
            voice: "chh".into()
        }),
        "classic:tr808:chh"
    );
    assert_eq!(
        cache.key(&kick()),
        "pack:tr808-fischer@0:bass-drum/bd0010.wav"
    );
    assert_eq!(cache.key(&file("/s/hit.wav")), "file:/s/hit.wav");
    let legacy = file(&format!("{PACKS}/tr808-fischer/snare/sd0000.wav"));
    assert_eq!(
        cache.key(&legacy),
        format!("file@0:{PACKS}/tr808-fischer/snare/sd0000.wav")
    );
    cache.invalidate_pack_buffers("tr808-fischer");
    assert_eq!(
        cache.key(&kick()),
        "pack:tr808-fischer@1:bass-drum/bd0010.wav"
    );
    assert_eq!(
        cache.key(&legacy),
        format!("file@1:{PACKS}/tr808-fischer/snare/sd0000.wav")
    );
}

#[test]
fn renders_each_factory_voice_once_and_caches_it() {
    let (cache, ..) = setup();
    let src = PadSampleSource::Factory {
        voice: FactoryVoiceId::Rim,
    };
    let a = cache.load_pad_buffer(&src).unwrap();
    let b = cache.load_pad_buffer(&src).unwrap();
    assert!(Arc::ptr_eq(&a, &b));
    assert_eq!(a.sample_rate, RATE);
    assert!(Arc::ptr_eq(&cache.get_cached(&src).unwrap(), &a));
    assert!(
        cache
            .get_cached(&PadSampleSource::Factory {
                voice: FactoryVoiceId::Snare
            })
            .is_none()
    );
}

#[test]
fn classic_voices_stay_at_44_1_khz_and_unknown_voices_fail() {
    let (cache, ..) = setup();
    let b = cache
        .load_pad_buffer(&PadSampleSource::Classic {
            machine: ClassicMachineId::Tr808,
            voice: "chh".into(),
        })
        .unwrap();
    assert_eq!(b.sample_rate, 44100);
    let err = cache
        .load_pad_buffer(&PadSampleSource::Classic {
            machine: ClassicMachineId::Tr808,
            voice: "nope".into(),
        })
        .unwrap_err();
    assert!(matches!(err, BufferLoadError::UnknownClassicVoice { .. }));
}

#[test]
fn loads_files_through_the_file_loader_at_the_device_rate() {
    let (cache, reader, files) = setup();
    let b = cache.load_pad_buffer(&file("/s/hit.wav")).unwrap();
    assert_eq!(*files.loads.lock().unwrap(), vec!["/s/hit.wav".to_owned()]);
    assert_eq!(b.sample_rate, RATE); // decodeAudioData resamples to the context rate
    cache.load_pad_buffer(&file("/s/hit.wav")).unwrap();
    assert_eq!(reader.count("/s/hit.wav"), 1);
}

#[test]
fn a_failed_load_is_not_cached_and_the_next_request_retries() {
    let (cache, reader, _) = setup();
    reader.missing.lock().unwrap().insert("/s/a.wav".into());
    let err = cache.load_pad_buffer(&file("/s/a.wav")).unwrap_err();
    assert!(matches!(err, BufferLoadError::Read { .. }));
    reader.missing.lock().unwrap().clear();
    assert!(cache.load_pad_buffer(&file("/s/a.wav")).is_ok());
    assert_eq!(reader.count("/s/a.wav"), 2);
}

#[test]
fn concurrent_requests_share_one_load_and_its_result() {
    let (cache, reader, _) = setup();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    *reader.gate.lock().unwrap() = Some((entered_tx, release_rx));
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let c = cache.clone();
            std::thread::spawn(move || c.load_pad_buffer(&kick()))
        })
        .collect();
    entered_rx.recv().unwrap(); // one reader is in
    // Give the other threads time to find the in-flight entry.
    while Arc::strong_count(&cache.inner) < 5 {
        std::thread::yield_now();
    }
    std::thread::sleep(std::time::Duration::from_millis(20));
    release_tx.send(()).unwrap();
    let results: Vec<_> = handles
        .into_iter()
        .map(|h| h.join().unwrap().unwrap())
        .collect();
    assert!(results.iter().all(|b| Arc::ptr_eq(b, &results[0])));
    assert_eq!(reader.reads.lock().unwrap().len(), 1);

    // A failing load fails every waiter the same way, and is retried afterwards.
    reader.fail_all.store(true, Ordering::SeqCst);
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    *reader.gate.lock().unwrap() = Some((entered_tx, release_rx));
    let src = PadSampleSource::Pack {
        pack_id: "uzu-drumkit".into(),
        file: "kick/a.wav".into(),
    };
    let handles: Vec<_> = (0..3)
        .map(|_| {
            let (c, s) = (cache.clone(), src.clone());
            std::thread::spawn(move || c.load_pad_buffer(&s))
        })
        .collect();
    entered_rx.recv().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    release_tx.send(()).unwrap();
    let errs: Vec<_> = handles
        .into_iter()
        .map(|h| h.join().unwrap().unwrap_err())
        .collect();
    assert!(errs.iter().all(|e| e == &errs[0]));
    assert!(matches!(errs[0], BufferLoadError::MissingPackSample { .. }));
    *reader.gate.lock().unwrap() = None;
    reader.fail_all.store(false, Ordering::SeqCst);
    assert!(cache.load_pad_buffer(&src).is_ok());
}

#[test]
fn reverses_buffers_once() {
    let (cache, ..) = setup();
    let buf: SharedBuffer = Arc::new(AudioBuffer::mono(44100, vec![1.0, 2.0, 3.0, 4.0]));
    let r = cache.reversed_buffer(&buf);
    assert_eq!(r.channels[0], vec![4.0, 3.0, 2.0, 1.0]);
    assert!(Arc::ptr_eq(&cache.reversed_buffer(&buf), &r));
    // Held weakly: a dropped buffer's copy is forgotten.
    drop(buf);
    let other: SharedBuffer = Arc::new(AudioBuffer::mono(44100, vec![5.0, 6.0]));
    assert_eq!(cache.reversed_buffer(&other).channels[0], vec![6.0, 5.0]);
    assert_eq!(lock(&cache.inner.state).reversed.len(), 1);
}

#[test]
fn lru_cache_evicts_the_least_recently_used_entry() {
    let mut lru = LruCache::new(2);
    lru.set("a", 1);
    lru.set("b", 2);
    assert_eq!(lru.get(&"a"), Some(1));
    lru.set("c", 3);
    assert_eq!(lru.keys(), vec!["a", "c"]);
    assert!(!lru.has(&"b"));
}

#[test]
fn auditioned_files_go_to_a_bounded_lru_never_to_the_file_loader() {
    let (cache, reader, files) = setup();
    for i in 0..PREVIEW_CACHE_SIZE + 4 {
        cache
            .load_preview_buffer(&file(&format!("/lib/{i}.wav")))
            .unwrap();
    }
    assert!(files.loads.lock().unwrap().is_empty());
    let keys = cache.preview_cache_keys();
    assert_eq!(keys.len(), PREVIEW_CACHE_SIZE);
    assert_eq!(keys[0], "file:/lib/4.wav");
    assert!(cache.get_cached(&file("/lib/11.wav")).is_none());
    // Re-auditioning a cached preview does not decode again.
    cache.load_preview_buffer(&file("/lib/11.wav")).unwrap();
    assert_eq!(reader.count("/lib/11.wav"), 1);
    // A file loaded as a pad sample is reused for previews and leaves the preview LRU.
    let pad = cache.load_pad_buffer(&file("/lib/11.wav")).unwrap();
    assert!(
        !cache
            .preview_cache_keys()
            .contains(&"file:/lib/11.wav".to_owned())
    );
    assert!(Arc::ptr_eq(
        &cache.load_preview_buffer(&file("/lib/11.wav")).unwrap(),
        &pad
    ));
}

#[test]
fn pack_sounds_resolve_against_the_packs_folder_and_are_cached() {
    let (cache, reader, files) = setup();
    let path = format!("{PACKS}/tr808-fischer/bass-drum/bd0010.wav");
    let buf = cache.load_pad_buffer(&kick()).unwrap();
    assert_eq!(reader.count(&path), 1);
    assert!(Arc::ptr_eq(&cache.get_cached(&kick()).unwrap(), &buf));
    cache.load_pad_buffer(&kick()).unwrap();
    assert_eq!(reader.count(&path), 1);
    assert!(files.loads.lock().unwrap().is_empty());
    assert_eq!(
        cache.pack_id_of_source(&kick()).as_deref(),
        Some("tr808-fischer")
    );
    assert_eq!(
        cache
            .pack_id_of_source(&file(&format!("{PACKS}/uzu-drumkit/kick/a.wav")))
            .as_deref(),
        Some("uzu-drumkit")
    );
}

#[test]
fn a_missing_pack_fails_with_missing_pack_sample() {
    let (cache, reader, _) = setup();
    reader
        .missing
        .lock()
        .unwrap()
        .insert(format!("{PACKS}/tr808-fischer/bass-drum/bd0010.wav"));
    let err = cache.load_pad_buffer(&kick()).unwrap_err();
    match &err {
        BufferLoadError::MissingPackSample {
            pack_id,
            file,
            cause,
        } => {
            assert_eq!(
                (pack_id.as_str(), file.as_str()),
                ("tr808-fischer", "bass-drum/bd0010.wav")
            );
            assert!(cause.as_deref().unwrap().contains("ENOENT"));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        err.to_string(),
        "Pack not installed: tr808-fischer (bass-drum/bd0010.wav)"
    );
    cache.set_packs_dir(None);
    assert!(matches!(
        cache.load_pad_buffer(&kick()),
        Err(BufferLoadError::MissingPackSample { .. })
    ));
}

#[test]
fn previews_pack_sounds_without_registering_them() {
    let (cache, reader, files) = setup();
    cache.load_preview_buffer(&kick()).unwrap();
    assert_eq!(
        reader.count(&format!("{PACKS}/tr808-fischer/bass-drum/bd0010.wav")),
        1
    );
    assert!(files.loads.lock().unwrap().is_empty());
}

#[test]
fn invalidation_drops_that_pack_only_changes_its_keys_and_notifies() {
    let (cache, reader, files) = setup();
    let other = PadSampleSource::Pack {
        pack_id: "uzu-drumkit".into(),
        file: "kick/a.wav".into(),
    };
    let legacy_path = format!("{PACKS}/tr808-fischer/snare/sd0000.wav");
    let legacy = file(&legacy_path);
    for s in [kick(), other.clone(), legacy.clone()] {
        cache.load_pad_buffer(&s).unwrap();
    }
    assert_eq!(*files.loads.lock().unwrap(), vec![legacy_path.clone()]);
    let (k0, l0) = (cache.key(&kick()), cache.key(&legacy));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s2 = seen.clone();
    let sub = cache.on_pad_buffers_invalidated(move |id| s2.lock().unwrap().push(id.to_owned()));
    // A panicking listener does not stop the others.
    let bad = cache.on_pad_buffers_invalidated(|_| panic!("listener failed"));
    cache.invalidate_pack_buffers("tr808-fischer");
    drop((sub, bad));
    assert_eq!(*seen.lock().unwrap(), vec!["tr808-fischer".to_owned()]);
    assert_ne!(cache.key(&kick()), k0);
    assert_ne!(cache.key(&legacy), l0);
    assert!(cache.get_cached(&kick()).is_none());
    assert!(cache.get_cached(&legacy).is_none());
    assert!(cache.get_cached(&other).is_some());
    // B3: samples under the pack folder are reloaded in place (the file loader's job).
    assert_eq!(
        *files.reloads.lock().unwrap(),
        vec![format!("{PACKS}/tr808-fischer/")]
    );
    cache.load_pad_buffer(&kick()).unwrap();
    assert_eq!(
        reader.count(&format!("{PACKS}/tr808-fischer/bass-drum/bd0010.wav")),
        2
    );
    assert_eq!(cache.listener_count(), 0);
}

#[test]
fn a_load_finishing_after_the_invalidation_is_not_cached_under_the_new_key() {
    let (cache, reader, _) = setup();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    *reader.gate.lock().unwrap() = Some((entered_tx, release_rx));
    let c = cache.clone();
    let h = std::thread::spawn(move || c.load_pad_buffer(&kick()));
    entered_rx.recv().unwrap();
    cache.invalidate_pack_buffers("tr808-fischer");
    release_tx.send(()).unwrap();
    assert!(h.join().unwrap().is_ok());
    *reader.gate.lock().unwrap() = None;
    assert!(cache.get_cached(&kick()).is_none());
}

fn pack_kit() -> KitState {
    let mut kit = create_factory_kit();
    for (i, p) in kit.pads.iter_mut().enumerate() {
        p.sample = (i == 0).then(|| PadSample {
            source: kick(),
            name: "Kick".into(),
        });
    }
    kit
}

fn settle(ctl: &mut KitController, s: &mut SamplerKit) {
    for c in ctl.poll() {
        s.apply(c);
    }
    for c in ctl.wait_ready() {
        s.apply(c);
    }
}

#[test]
fn a_live_kit_reloads_pads_of_an_invalidated_pack_and_goes_silent_on_uninstall() {
    let (cache, reader, _) = setup();
    let path = format!("{PACKS}/tr808-fischer/bass-drum/bd0010.wav");
    let (mut ctl, cmds) = KitController::new(&pack_kit(), cache.clone(), None);
    let mut s = SamplerKit::new(RATE, ctl.pad_params(), 0);
    cmds.into_iter().for_each(|c| s.apply(c));
    settle(&mut ctl, &mut s);
    assert_eq!(reader.count(&path), 1);
    assert!(s.is_pad_ready(0));

    // Update: same kit, new generation → the pad re-reads the file without a setKit.
    cache.invalidate_pack_buffers("tr808-fischer");
    settle(&mut ctl, &mut s);
    assert_eq!(reader.count(&path), 2);
    assert!(s.is_pad_ready(0));

    // Uninstall: the reload fails → the pad is silent.
    reader.fail_all.store(true, Ordering::SeqCst);
    cache.invalidate_pack_buffers("tr808-fischer");
    settle(&mut ctl, &mut s);
    assert!(!s.is_pad_ready(0));
    assert_eq!(ctl.take_errors().len(), 1);

    // Reinstall: back to life.
    reader.fail_all.store(false, Ordering::SeqCst);
    cache.invalidate_pack_buffers("tr808-fischer");
    settle(&mut ctl, &mut s);
    assert!(s.is_pad_ready(0));

    // A dropped controller stops listening.
    assert_eq!(cache.listener_count(), 1);
    drop(ctl);
    assert_eq!(cache.listener_count(), 0);
}

#[test]
fn set_kit_with_an_unchanged_kit_still_reloads_a_stale_pack_pad() {
    let (cache, reader, _) = setup();
    let state = pack_kit();
    let (mut ctl, cmds) = KitController::new(&state, cache.clone(), None);
    let mut s = SamplerKit::new(RATE, ctl.pad_params(), 0);
    cmds.into_iter().for_each(|c| s.apply(c));
    settle(&mut ctl, &mut s);
    cache.invalidate_pack_buffers("tr808-fischer");
    // setKit before the controller polled the invalidation.
    s.apply(ctl.set_kit(&state));
    settle(&mut ctl, &mut s);
    assert_eq!(
        reader.count(&format!("{PACKS}/tr808-fischer/bass-drum/bd0010.wav")),
        2
    );
    assert!(s.is_pad_ready(0));
}

#[test]
fn preload_reports_failures_without_stopping() {
    let (cache, reader, _) = setup();
    let mut kit = pack_kit();
    kit.pads[1].sample = Some(PadSample {
        source: file("/s/missing.wav"),
        name: "x".into(),
    });
    reader
        .missing
        .lock()
        .unwrap()
        .insert("/s/missing.wav".into());
    let errs = cache.preload_kit_buffers(&kit);
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].0, 1);
    assert!(cache.get_cached(&kick()).is_some());
}

#[test]
fn decoded_audio_is_sanitized_at_the_boundary() {
    let (cache, ..) = setup();
    let mut b = AudioBuffer::mono(48000, vec![0.5, f32::NAN, f32::INFINITY, -0.5]);
    // f32 WAV keeps NaN / Inf through decoding.
    let bytes = {
        let mut out = Vec::new();
        let data: Vec<u8> = b.channels[0].iter().flat_map(|x| x.to_le_bytes()).collect();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&3u16.to_le_bytes()); // IEEE float
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&48000u32.to_le_bytes());
        out.extend_from_slice(&(48000u32 * 4).to_le_bytes());
        out.extend_from_slice(&4u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&data);
        out
    };
    let d = cache.decode("/x.wav", bytes).unwrap();
    b.channels[0][1] = 0.0;
    b.channels[0][2] = 0.0;
    assert_eq!(d.channels[0], b.channels[0]);
}
