//! `components/packs/kitPreview.ts` + `remoteKit.ts`: "Preview kit", the built-in 2-bar demo
//! groove (`demo_pattern`, 96 BPM, 32 sixteenths) on a kit, one preview at a time.
//!
//! The TS schedules the hits on a temporary `SamplerKit` → `Tone.Volume(-6)` at
//! `Tone.immediate() + 0.08`. Here a worker loads the pads' buffers (an installed or built-in kit
//! through the engine's pad buffer cache; a not-installed catalog kit by streaming each pad's
//! file through the pack store's preview fetcher, decoded buffers kept in an LRU of 32 like
//! `remoteKit.ts`), renders the groove offline through the engine's own `SamplerKit` (pad
//! settings, choke, envelopes), and the UI plays the result on the engine's preview slot with
//! its 0.9 × −6 dB source gain compensated to −6 dB. The UI clock drives the faceplate flashes
//! (`padsAt`) and the auto-stop after `32 · step + 1.2` s.

use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use mdaw_dsp::buffer::{AudioBuffer, SharedBuffer};
use mdaw_engine::buffers::PadBufferCache;
use mdaw_engine::contract::Instrument;
use mdaw_engine::engine::{PREVIEW_GAIN, db_gain};
use mdaw_engine::sampler::{PadBuffer, PadParams, SamplerKit};
use mdaw_model::packs::kit_preview::{
    DEMO_BPM, DEMO_STEPS, DemoHit, PreviewSession, demo_pattern, pads_at, step_seconds,
};
use mdaw_model::types::{KitState, PAD_COUNT};
use mdaw_packs::store::PackStore;

/// Remote buffers kept decoded (`new LruCache(32)`).
pub const REMOTE_LRU: usize = 32;
/// `Tone.immediate() + 0.08`.
pub const LEAD: f64 = 0.08;
/// Ring-out after the last step (`32·dur + 1.2`).
pub const TAIL: f64 = 1.2;

/// Where the pads' sounds come from.
pub enum Sounds {
    /// The kit's own sources (installed pack, built-in machine).
    Kit,
    /// A not-installed catalog kit: one remote URL per pad.
    Remote(Vec<Option<String>>),
}

struct Rendered {
    token: u64,
    id: String,
    result: Result<(SharedBuffer, Option<String>, Vec<DemoHit>), String>,
}

/// The LRU of decoded remote buffers (url → buffer), shared with the workers.
pub type RemoteCache = Arc<Mutex<VecDeque<(String, SharedBuffer)>>>;

/// `KitPreviewPlayer`.
pub struct KitPreview {
    /// Key (`${item.id}/${kitId}`) of the preview loading or playing (`playing`).
    pub playing: Option<String>,
    /// `current` (set once the groove plays): t0 on the UI clock.
    pub session: Option<PreviewSession>,
    /// When the preview stops by itself (UI clock).
    pub stop_at: Option<f64>,
    /// Errors / partial-failure messages by key (`kitErrors`).
    pub errors: std::collections::HashMap<String, String>,
    token: u64,
    tx: Sender<Rendered>,
    rx: Receiver<Rendered>,
    pub remote: RemoteCache,
    /// Headless: render synchronously.
    pub sync: bool,
    pub waker: Option<rune::Waker>,
}

impl Default for KitPreview {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self {
            playing: None,
            session: None,
            stop_at: None,
            errors: Default::default(),
            token: 0,
            tx,
            rx,
            remote: Arc::new(Mutex::new(VecDeque::new())),
            sync: false,
            waker: None,
        }
    }
}

fn remote_get(cache: &RemoteCache, url: &str) -> Option<SharedBuffer> {
    let mut c = cache.lock().unwrap_or_else(|e| e.into_inner());
    let i = c.iter().position(|(u, _)| u == url)?;
    let hit = c.remove(i)?;
    let b = hit.1.clone();
    c.push_back(hit);
    Some(b)
}

fn remote_put(cache: &RemoteCache, url: &str, b: SharedBuffer) {
    let mut c = cache.lock().unwrap_or_else(|e| e.into_inner());
    c.retain(|(u, _)| u != url);
    c.push_back((url.to_owned(), b));
    while c.len() > REMOTE_LRU {
        c.pop_front();
    }
}

/// Renders the groove: `hits` at `LEAD + step·dur` through a `SamplerKit` of `kit` at `rate`,
/// `32·dur + 1.2` s (+ the lead), gain −6 dB / PREVIEW_GAIN (the preview slot adds 0.9 × −6 dB).
pub fn render_groove(
    kit: &KitState,
    buffers: &[Option<SharedBuffer>],
    rate: u32,
    reversed: impl Fn(&SharedBuffer) -> SharedBuffer,
) -> (AudioBuffer, Vec<DemoHit>) {
    let hits = demo_pattern(kit);
    let dur = step_seconds(DEMO_BPM);
    let pads: [PadParams; PAD_COUNT] = std::array::from_fn(|i| PadParams::from(&kit.pads[i]));
    let mut sk = SamplerKit::new(rate, pads, 0);
    for (i, b) in buffers.iter().enumerate().take(PAD_COUNT) {
        if let Some(b) = b {
            let rev = kit.pads[i].reverse.then(|| reversed(b));
            sk.set_pad_buffer(
                i,
                Some(PadBuffer {
                    forward: b.clone(),
                    reversed: rev,
                }),
            );
        }
    }
    for h in &hits {
        if h.pad < PAD_COUNT && buffers.get(h.pad).is_some_and(Option::is_some) {
            sk.trigger(h.pad, h.velocity, LEAD + h.step as f64 * dur);
        }
    }
    let total = LEAD + DEMO_STEPS as f64 * dur + TAIL;
    let frames = (total * rate as f64).ceil() as usize;
    let q = 128;
    let (mut l, mut r) = (vec![0f32; frames], vec![0f32; frames]);
    let mut f = 0;
    while f < frames {
        let n = q.min(frames - f);
        sk.process(f as u64, &mut l[f..f + n], &mut r[f..f + n]);
        f += n;
    }
    let g = db_gain(-6.0) / (PREVIEW_GAIN as f32 * db_gain(-6.0));
    for x in l.iter_mut().chain(r.iter_mut()) {
        *x *= g;
    }
    (
        AudioBuffer {
            sample_rate: rate,
            channels: vec![l, r],
        },
        hits,
    )
}

impl KitPreview {
    /// `play(id, kit, opts)`: stops the current preview first.
    pub fn play(
        &mut self,
        id: &str,
        kit: KitState,
        sounds: Sounds,
        cache: PadBufferCache,
        store: Arc<PackStore>,
    ) {
        self.token += 1;
        self.playing = Some(id.to_owned());
        self.session = None;
        self.stop_at = None;
        self.errors.remove(id);
        let token = self.token;
        let tx = self.tx.clone();
        let remote = self.remote.clone();
        let waker = self.waker.clone();
        let id = id.to_owned();
        let job = move || {
            let rate = cache.device_rate();
            let mut buffers: Vec<Option<SharedBuffer>> = vec![None; PAD_COUNT];
            let mut partial = None;
            let result = (|| {
                match &sounds {
                    Sounds::Kit => {
                        for (i, p) in kit.pads.iter().enumerate().take(PAD_COUNT) {
                            if let Some(s) = &p.sample {
                                buffers[i] = cache.load_pad_buffer(&s.source).ok();
                            }
                        }
                    }
                    Sounds::Remote(urls) => {
                        let (mut failed, mut loaded) = (0usize, 0usize);
                        let mut first: Option<String> = None;
                        for (i, u) in urls.iter().enumerate().take(PAD_COUNT) {
                            let Some(u) = u else { continue };
                            let got = remote_get(&remote, u).map(Ok).unwrap_or_else(|| {
                                store
                                    .fetch_preview(u)
                                    .and_then(|bytes| {
                                        cache.decode(u, bytes).map_err(|e| e.to_string())
                                    })
                                    .inspect(|b| remote_put(&remote, u, b.clone()))
                            });
                            match got {
                                Ok(b) => {
                                    loaded += 1;
                                    buffers[i] = Some(b);
                                }
                                Err(e) => {
                                    failed += 1;
                                    first.get_or_insert(e);
                                }
                            }
                        }
                        let reason = first
                            .as_deref()
                            .filter(|m| !m.is_empty())
                            .map(|m| format!(" ({m})"))
                            .unwrap_or_default();
                        if loaded == 0 {
                            return Err(format!("Couldn't load this kit's sounds{reason}"));
                        }
                        if failed > 0 {
                            partial = Some(format!(
                                "{failed} of {} sounds couldn't load{reason}",
                                failed + loaded
                            ));
                        }
                    }
                }
                let (buf, hits) = render_groove(&kit, &buffers, rate, |b| cache.reversed_buffer(b));
                Ok((Arc::new(buf), partial.clone(), hits))
            })();
            let _ = tx.send(Rendered { token, id, result });
            if let Some(w) = waker {
                w();
            }
        };
        if self.sync {
            job();
        } else {
            std::thread::spawn(job);
        }
    }

    /// `stop()`.
    pub fn stop(&mut self) -> bool {
        self.token += 1;
        self.session = None;
        self.stop_at = None;
        self.playing.take().is_some()
    }

    /// A finished render to start now: (buffer, id). Errors and partial messages are recorded.
    pub fn take_ready(&mut self, now: f64) -> Option<SharedBuffer> {
        let mut out = None;
        while let Ok(r) = self.rx.try_recv() {
            if r.token != self.token {
                continue;
            }
            match r.result {
                Ok((buf, partial, hits)) => {
                    let dur = step_seconds(DEMO_BPM);
                    self.session = Some(PreviewSession {
                        id: r.id.clone(),
                        t0: now + LEAD,
                        step_dur: dur,
                        hits,
                    });
                    self.stop_at = Some(now + LEAD + DEMO_STEPS as f64 * dur + TAIL);
                    if let Some(p) = partial {
                        self.errors.insert(r.id, p);
                    }
                    out = Some(buf);
                }
                Err(e) => {
                    self.errors.insert(r.id, e);
                    self.playing = None;
                }
            }
        }
        out
    }

    /// Pads lit at `now` for items whose key starts with `prefix` (`usePreviewFlash`).
    pub fn flash(&self, prefix: &str, now: f64) -> Vec<usize> {
        match &self.session {
            Some(s) if s.id.starts_with(prefix) => pads_at(s, now),
            _ => Vec::new(),
        }
    }
}
