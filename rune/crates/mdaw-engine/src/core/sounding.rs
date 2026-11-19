//! The scheduled events sounding on a slot, so an edit during playback can end the ones it
//! removed (`Core::reconcile_sounding`).
//!
//! The work is bounded by polyphony, not by song history: the fire path records each pattern
//! note it starts (synth notes, gate-pad hits, audio region playbacks) with its event identity
//! ([`super::ScheduledNote::id`]) and onset in a fixed-capacity set; a schedule swap checks only
//! those against the new schedule (one binary search each). Entries leave when their scheduled
//! end has passed. Documented limit: more than [`SOUNDING_CAP`] events of one slot sounding at
//! once drops the one ending first from the set (an edit can no longer cut it early; it still
//! ends at its scheduled time).

/// Entries per slot (the synth voice cap, `synth::alloc::MAX_VOICES`).
pub(crate) const SOUNDING_CAP: usize = 128;

/// One scheduled event that is sounding.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Sounding {
    /// [`super::ScheduledNote::id`].
    pub id: u64,
    /// The event's tick (where its id is looked up in a new schedule).
    pub tick: f64,
    pub note: f64,
    /// Drum pad / audio region index.
    pub pad: Option<usize>,
    /// The instrument's voice handle (drums), 0 for none.
    pub voice: u64,
    /// Onset (context seconds): with the note, the instance a synth cut targets.
    pub start: f64,
    /// Its scheduled end (release / region end), context seconds.
    pub end: f64,
    /// Seconds per beat when it fired (its length in seconds).
    pub spb: f64,
}

/// See the module docs. Allocated once, never grows.
pub(crate) struct SoundingSet {
    items: Vec<Sounding>,
}

impl SoundingSet {
    pub fn new() -> Self {
        Self {
            items: Vec::with_capacity(SOUNDING_CAP),
        }
    }

    pub fn items(&self) -> &[Sounding] {
        &self.items
    }

    pub fn items_mut(&mut self) -> &mut [Sounding] {
        &mut self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Drop the entries whose end is at or before `now`.
    pub fn prune(&mut self, now: f64) {
        self.items.retain(|e| e.end > now);
    }

    /// Keep the entries `keep` says (by index), in order.
    pub fn retain_indexed(&mut self, keep: &[bool]) {
        let mut i = 0;
        self.items.retain(|_| {
            let k = keep.get(i).copied().unwrap_or(true);
            i += 1;
            k
        });
    }

    pub fn contains_id(&self, id: u64) -> bool {
        self.items.iter().any(|e| e.id == id)
    }

    /// Record `s` (allocation-free): when full, the ended entries go first, then the one
    /// ending first.
    pub fn push(&mut self, s: Sounding, now: f64) {
        if self.items.len() >= SOUNDING_CAP {
            self.prune(now);
        }
        if self.items.len() >= SOUNDING_CAP
            && let Some(i) = (0..self.items.len())
                .min_by(|&a, &b| self.items[a].end.total_cmp(&self.items[b].end))
        {
            self.items.swap_remove(i);
        }
        self.items.push(s);
    }
}
