//! Control side of a SamplerKit (UI / worker threads): the kit snapshot, buffer keys, loads and
//! the setKit diff of `SamplerKit.ts`, producing [`SamplerCommand`]s for the audio side.
//!
//! - `loadPad`: the pad's key is recorded first (`bufferKeys`); a cached buffer is handed over at
//!   once, otherwise the pad goes silent and a load starts on a worker thread. A finished load is
//!   only applied if the pad still wants that key and no newer load was started for it (latest
//!   wins); a failed load leaves the pad silent.
//! - `setKit`: pads whose sample key changed restart (their voices fade out over 5 ms, future
//!   ones are cancelled) and reload; pads whose loaded key is stale (pack generation) reload; all
//!   channels are re-applied with ramps (done by the audio side).
//! - Pack invalidation ([`PadBufferCache::on_pad_buffers_invalidated`]) marks the kit; the next
//!   [`KitController::poll`] reloads the pads whose key moved on (`reloadStalePads`).
//! - An injected [`PadLoader`] replaces the buffer cache for every pad (`SamplerKitOptions.
//!   loadBuffer`, kit previews of packs that are not installed); nothing is read from or written
//!   to the cache then.
//! - Reversed pads get their reversed copy from [`PadBufferCache::reversed_buffer`] together with
//!   the forward buffer (the audio side cannot build it).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

use mdaw_dsp::buffer::SharedBuffer;
use mdaw_model::types::{KitState, PAD_COUNT, PadSample, PadSettings};

use super::{PadBuffer, PadChange, PadParams, SamplerCommand};
use crate::buffers::{BufferLoadError, PadBufferCache, Subscription};

/// `PadBufferLoader`: the buffer for a pad (`Ok(None)` = no sound on this pad).
pub type PadLoader =
    dyn Fn(usize, &PadSample) -> Result<Option<SharedBuffer>, BufferLoadError> + Send + Sync;

struct Done {
    pad: usize,
    seq: u64,
    key: String,
    result: Result<Option<SharedBuffer>, BufferLoadError>,
}

/// See the module docs.
pub struct KitController {
    kit: KitState,
    cache: PadBufferCache,
    custom: Option<Arc<PadLoader>>,
    /// `bufferKeys`.
    keys: [Option<String>; PAD_COUNT],
    /// What the audio side holds per pad (forward buffer, reversed copy sent).
    loaded: [Option<PadBuffer>; PAD_COUNT],
    /// Latest load per pad.
    seq: [u64; PAD_COUNT],
    next_seq: u64,
    outstanding: usize,
    tx: Sender<Done>,
    rx: Receiver<Done>,
    stale: Arc<AtomicBool>,
    _watch: Subscription,
    /// Load failures not yet taken by [`KitController::take_errors`] (the TS logs them).
    errors: Vec<(usize, BufferLoadError)>,
}

fn sample_of(kit: &KitState, pad: usize) -> Option<&PadSample> {
    kit.pads.get(pad).and_then(|p| p.sample.as_ref())
}

/// `copyKit`: exactly PAD_COUNT pads (missing ones are defaults).
fn copy_kit(kit: &KitState) -> KitState {
    let mut k = kit.clone();
    k.pads = (0..PAD_COUNT)
        .map(|i| {
            kit.pads
                .get(i)
                .cloned()
                .unwrap_or_else(PadSettings::default)
        })
        .collect();
    k
}

impl KitController {
    /// `new SamplerKit(kit, opts)` (control side). Returns the controller and the commands that
    /// hand over the buffers already cached; build the audio side with
    /// `SamplerKit::new(rate, controller.pad_params(), now)` and apply them.
    pub fn new(
        kit: &KitState,
        cache: PadBufferCache,
        custom: Option<Arc<PadLoader>>,
    ) -> (Self, Vec<SamplerCommand>) {
        let (tx, rx) = channel();
        let stale = Arc::new(AtomicBool::new(false));
        let flag = stale.clone();
        let watch = cache.on_pad_buffers_invalidated(move |_| flag.store(true, Ordering::SeqCst));
        let mut c = Self {
            kit: copy_kit(kit),
            cache,
            custom,
            keys: Default::default(),
            loaded: Default::default(),
            seq: [0; PAD_COUNT],
            next_seq: 0,
            outstanding: 0,
            tx,
            rx,
            stale,
            _watch: watch,
            errors: Vec::new(),
        };
        let mut cmds = Vec::new();
        for pad in 0..PAD_COUNT {
            if let Some(buffer) = c.load_pad(pad) {
                cmds.push(SamplerCommand::SetPadBuffer {
                    pad,
                    buffer: Some(buffer),
                });
            }
        }
        (c, cmds)
    }

    pub fn kit(&self) -> &KitState {
        &self.kit
    }

    pub fn cache(&self) -> &PadBufferCache {
        &self.cache
    }

    /// The audio-side params of the current kit.
    pub fn pad_params(&self) -> [PadParams; PAD_COUNT] {
        std::array::from_fn(|i| PadParams::from(&self.kit.pads[i]))
    }

    /// True while a load is in flight (`ready` has not settled).
    pub fn is_loading(&self) -> bool {
        self.outstanding > 0
    }

    /// Load failures since the last call.
    pub fn take_errors(&mut self) -> Vec<(usize, BufferLoadError)> {
        std::mem::take(&mut self.errors)
    }

    fn key_of(&self, pad: usize) -> Option<String> {
        sample_of(&self.kit, pad).map(|s| self.cache.key(&s.source))
    }

    /// The buffer for the audio side, with a reversed copy when the pad plays reversed.
    fn pad_buffer(&self, pad: usize, forward: SharedBuffer) -> PadBuffer {
        let reversed = self.kit.pads[pad]
            .reverse
            .then(|| self.cache.reversed_buffer(&forward));
        PadBuffer { forward, reversed }
    }

    /// `loadPad`: Some(buffer) when it is available now, else None (silent until loaded).
    fn load_pad(&mut self, pad: usize) -> Option<PadBuffer> {
        let key = self.key_of(pad);
        self.keys[pad] = key.clone();
        self.loaded[pad] = None;
        self.next_seq += 1;
        self.seq[pad] = self.next_seq;
        let sample = sample_of(&self.kit, pad)?.clone();
        let key = key?;
        if self.custom.is_none()
            && let Some(hit) = self.cache.get_cached(&sample.source)
        {
            let b = self.pad_buffer(pad, hit);
            self.loaded[pad] = Some(b.clone());
            return Some(b);
        }
        let (tx, seq) = (self.tx.clone(), self.next_seq);
        self.outstanding += 1;
        match self.custom.clone() {
            Some(loader) => {
                std::thread::spawn(move || {
                    let result = loader(pad, &sample);
                    let _ = tx.send(Done {
                        pad,
                        seq,
                        key,
                        result,
                    });
                });
            }
            None => self.cache.load_pad_buffer_async(sample.source, move |r| {
                let _ = tx.send(Done {
                    pad,
                    seq,
                    key,
                    result: r.map(Some),
                });
            }),
        }
        None
    }

    /// `setKit`.
    pub fn set_kit(&mut self, kit: &KitState) -> SamplerCommand {
        let prev = std::mem::replace(&mut self.kit, copy_kit(kit));
        let changes = std::array::from_fn(|pad| {
            let next = self.key_of(pad);
            let before = sample_of(&prev, pad).map(|s| self.cache.key(&s.source));
            if before != next {
                PadChange::Restart(self.load_pad(pad))
            } else if self.keys[pad] != next {
                PadChange::Reload(self.load_pad(pad))
            } else if self.kit.pads[pad].reverse
                && let Some(b) = &self.loaded[pad]
                && b.reversed.is_none()
            {
                // Reverse switched on: hand over the reversed copy (the TS builds it at the
                // next hit).
                let b = self.pad_buffer(pad, b.forward.clone());
                self.loaded[pad] = Some(b.clone());
                PadChange::Reload(Some(b))
            } else {
                PadChange::Keep
            }
        });
        SamplerCommand::SetKit {
            pads: self.pad_params(),
            changes,
        }
    }

    fn reload_stale_pads(&mut self, out: &mut Vec<SamplerCommand>) {
        for pad in 0..PAD_COUNT {
            if self.keys[pad] != self.key_of(pad) {
                let buffer = self.load_pad(pad);
                out.push(SamplerCommand::SetPadBuffer { pad, buffer });
            }
        }
    }

    fn finish(&mut self, d: Done, out: &mut Vec<SamplerCommand>) {
        self.outstanding = self.outstanding.saturating_sub(1);
        match d.result {
            Ok(buf) => {
                if self.keys[d.pad].as_deref() == Some(d.key.as_str()) && self.seq[d.pad] == d.seq {
                    let buffer = buf.map(|b| self.pad_buffer(d.pad, b));
                    self.loaded[d.pad] = buffer.clone();
                    out.push(SamplerCommand::SetPadBuffer { pad: d.pad, buffer });
                }
            }
            Err(e) => self.errors.push((d.pad, e)),
        }
    }

    /// Finished loads and pack invalidations since the last call (non-blocking).
    pub fn poll(&mut self) -> Vec<SamplerCommand> {
        let mut out = Vec::new();
        if self.stale.swap(false, Ordering::SeqCst) {
            self.reload_stale_pads(&mut out);
        }
        while let Ok(d) = self.rx.try_recv() {
            self.finish(d, &mut out);
        }
        out
    }

    /// `await kit.ready`: block until every load in flight has settled; returns the commands.
    pub fn wait_ready(&mut self) -> Vec<SamplerCommand> {
        let mut out = self.poll();
        while self.outstanding > 0 {
            match self.rx.recv() {
                Ok(d) => self.finish(d, &mut out),
                Err(_) => break,
            }
            if self.stale.swap(false, Ordering::SeqCst) {
                self.reload_stale_pads(&mut out);
            }
        }
        out
    }
}

impl std::fmt::Debug for KitController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KitController")
            .field("keys", &self.keys)
            .field("outstanding", &self.outstanding)
            .finish_non_exhaustive()
    }
}
