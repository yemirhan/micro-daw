//! `usePadBuffer` + the waveform peak cache (`waveform.ts`): a pad's decoded buffer for the
//! thumbnails and the trim waveform, loaded off the UI thread through the engine's
//! `PadBufferCache` (the same cache the samplers use, so a kit the engine loaded is ready at
//! once), with the statuses empty / loading / ready / missing.

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender, channel};

use mdaw_dsp::buffer::SharedBuffer;
use mdaw_engine::buffers::PadBufferCache;
use mdaw_model::beat_helpers::{Peaks, compute_peaks};
use mdaw_model::types::PadSampleSource;

/// `usePadBuffer`'s status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferStatus {
    Empty,
    Loading,
    Ready,
    Missing,
}

enum Entry {
    Loading,
    Ready(SharedBuffer),
    Missing,
}

/// Loaded pad buffers by cache key, plus their peaks per column count.
pub struct PadBuffers {
    entries: HashMap<String, Entry>,
    peaks: HashMap<(String, usize), Peaks>,
    tx: Sender<(String, Option<SharedBuffer>)>,
    rx: Receiver<(String, Option<SharedBuffer>)>,
    version: u64,
    /// Loads run synchronously (headless runs: a snapshot must show the waveform).
    pub sync: bool,
}

impl Default for PadBuffers {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self {
            entries: HashMap::new(),
            peaks: HashMap::new(),
            tx,
            rx,
            version: 0,
            sync: false,
        }
    }
}

impl PadBuffers {
    pub fn new(sync: bool) -> Self {
        Self {
            sync,
            ..Self::default()
        }
    }

    /// Applies finished loads. True when something changed.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok((key, buf)) = self.rx.try_recv() {
            let e = match buf {
                Some(b) => Entry::Ready(b),
                None => Entry::Missing,
            };
            self.entries.insert(key, e);
            changed = true;
        }
        if changed {
            self.version += 1;
        }
        changed
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn loading(&self) -> bool {
        self.entries.values().any(|e| matches!(e, Entry::Loading))
    }

    /// Drops every entry (a pack was installed / removed: its sounds load again).
    pub fn invalidate(&mut self) {
        self.entries.clear();
        self.peaks.clear();
        self.version += 1;
    }

    /// The buffer of `source` and its status; starts a load when it isn't known yet.
    pub fn get(
        &mut self,
        cache: &PadBufferCache,
        source: Option<&PadSampleSource>,
    ) -> (Option<SharedBuffer>, BufferStatus) {
        let Some(source) = source else {
            return (None, BufferStatus::Empty);
        };
        let key = cache.key(source);
        match self.entries.get(&key) {
            Some(Entry::Ready(b)) => return (Some(b.clone()), BufferStatus::Ready),
            Some(Entry::Missing) => return (None, BufferStatus::Missing),
            Some(Entry::Loading) => return (None, BufferStatus::Loading),
            None => {}
        }
        if let Some(b) = cache.get_cached(source) {
            self.entries.insert(key, Entry::Ready(b.clone()));
            return (Some(b), BufferStatus::Ready);
        }
        if self.sync {
            let r = cache.load_pad_buffer(source).ok();
            let status = if r.is_some() {
                BufferStatus::Ready
            } else {
                BufferStatus::Missing
            };
            self.entries.insert(
                key,
                match &r {
                    Some(b) => Entry::Ready(b.clone()),
                    None => Entry::Missing,
                },
            );
            return (r, status);
        }
        self.entries.insert(key.clone(), Entry::Loading);
        let tx = self.tx.clone();
        cache.load_pad_buffer_async(source.clone(), move |r| {
            let _ = tx.send((key, r.ok()));
        });
        (None, BufferStatus::Loading)
    }

    /// The status of `source` without starting a load.
    pub fn status(&self, cache: &PadBufferCache, source: Option<&PadSampleSource>) -> BufferStatus {
        let Some(source) = source else {
            return BufferStatus::Empty;
        };
        match self.entries.get(&cache.key(source)) {
            Some(Entry::Ready(_)) => BufferStatus::Ready,
            Some(Entry::Missing) => BufferStatus::Missing,
            _ => BufferStatus::Loading,
        }
    }

    /// `getPeaks(buffer, columns)`, cached per buffer key and column count.
    pub fn peaks(&mut self, key: &str, buffer: &SharedBuffer, columns: usize) -> &Peaks {
        let cols = columns.max(1);
        self.peaks
            .entry((key.to_owned(), cols))
            .or_insert_with(|| compute_peaks(buffer.as_ref(), cols as f64))
    }
}
