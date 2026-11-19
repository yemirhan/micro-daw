//! `PolyVoiceAllocator` (`src/audio/voiceAllocator.ts`): the note bookkeeping of the sampled
//! piano. Same rules as the TS:
//! - a note is held by at most one slot; re-striking a sounding (held or releasing) note re-uses
//!   it (`retrigger`);
//! - otherwise the least-recently-finished free slot, then a new slot (up to `max_voices`), then a
//!   victim: the slot released longest ago, else the oldest held slot sparing the lowest note.
//!
//! Slots live in a vector allocated once with [`MAX_SLOTS`] capacity, so the audio thread never
//! allocates (the TS pool is unbounded; `max(8, polyphony)` is clamped to [`MAX_SLOTS`]).

/// Hard cap on the pool (and on `max_voices`).
pub const MAX_SLOTS: usize = 128;

const EPS: f64 = 1e-6;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoiceSlot {
    pub index: usize,
    /// Note assigned to this slot (None: never used / reset).
    pub note: Option<i32>,
    pub velocity: f64,
    pub start_time: f64,
    /// When the key was released (∞ while held).
    pub release_time: f64,
    /// When the release tail is over.
    pub end_time: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Allocation {
    pub voice: usize,
    /// The slot was still sounding a different note (cut off).
    pub stolen: bool,
    /// Same note re-struck on the slot that was holding / releasing it.
    pub retrigger: bool,
    /// Note the slot was playing before.
    pub previous_note: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct PolyVoiceAllocator {
    slots: Vec<VoiceSlot>,
    max_voices: usize,
}

impl PolyVoiceAllocator {
    pub fn new(max_voices: usize) -> Self {
        Self {
            slots: Vec::with_capacity(MAX_SLOTS),
            max_voices: max_voices.clamp(1, MAX_SLOTS),
        }
    }

    pub fn max_voices(&self) -> usize {
        self.max_voices
    }

    /// `maxVoices = n` (the pool never shrinks).
    pub fn set_max_voices(&mut self, n: usize) {
        self.max_voices = n.clamp(1, MAX_SLOTS);
    }

    pub fn pool_size(&self) -> usize {
        self.slots.len()
    }

    pub fn slot(&self, index: usize) -> Option<&VoiceSlot> {
        self.slots.get(index)
    }

    pub fn note_on(&mut self, note: i32, time: f64, velocity: f64) -> Allocation {
        if let Some(i) = self.find_voice_for_note(note, time) {
            return self.assign(i, note, time, velocity, false, true);
        }
        if let Some(i) = self.find_free_voice(time) {
            return self.assign(i, note, time, velocity, false, false);
        }
        if self.slots.len() < self.max_voices {
            let index = self.slots.len();
            self.slots.push(VoiceSlot {
                index,
                note: None,
                velocity: 0.0,
                start_time: f64::NEG_INFINITY,
                release_time: f64::NEG_INFINITY,
                end_time: f64::NEG_INFINITY,
            });
            return self.assign(index, note, time, velocity, false, false);
        }
        let victim = self.choose_victim(time);
        self.assign(victim, note, time, velocity, true, false)
    }

    /// Release the held slot(s) playing `note`; returns how many were released.
    pub fn note_off(&mut self, note: i32, time: f64, release_duration: f64) -> usize {
        let mut n = 0;
        for s in &mut self.slots {
            if s.note == Some(note) && s.release_time == f64::INFINITY {
                s.release_time = time.max(s.start_time);
                s.end_time = s.release_time + release_duration.max(0.0);
                n += 1;
            }
        }
        n
    }

    /// Bring forward to `time` the release of the self-releasing slot ([`Self::one_shot`]) of
    /// `note` struck at `started`, if its release is still after `time`; returns how many (0
    /// or 1). Held keys and other strikes are untouched.
    pub fn cut_one_shot(
        &mut self,
        note: i32,
        started: f64,
        time: f64,
        release_duration: f64,
    ) -> usize {
        let mut n = 0;
        for s in &mut self.slots {
            if s.note == Some(note)
                && (s.start_time - started).abs() <= EPS
                && s.release_time.is_finite()
                && s.release_time > time
            {
                s.release_time = time.max(s.start_time);
                s.end_time = s.release_time + release_duration.max(0.0);
                n += 1;
            }
        }
        n
    }

    /// A self-releasing note (attack at `time`, release at `time + duration`).
    pub fn one_shot(
        &mut self,
        note: i32,
        time: f64,
        duration: f64,
        release_duration: f64,
        velocity: f64,
    ) -> Allocation {
        let a = self.note_on(note, time, velocity);
        let s = &mut self.slots[a.voice];
        s.release_time = time + duration.max(0.0);
        s.end_time = s.release_time + release_duration.max(0.0);
        a
    }

    /// Reset every slot; returns how many were held or still sounding at `time`.
    pub fn release_all(&mut self, time: f64) -> usize {
        let mut busy = 0;
        for s in &mut self.slots {
            if s.note.is_some() && (s.release_time == f64::INFINITY || s.end_time > time + EPS) {
                busy += 1;
            }
            s.note = None;
            s.release_time = f64::NEG_INFINITY;
            s.end_time = f64::NEG_INFINITY;
        }
        busy
    }

    /// Slots held or still in their release tail at `time`.
    pub fn active_voice_count(&self, time: f64) -> usize {
        self.slots
            .iter()
            .filter(|s| {
                s.note.is_some() && (s.release_time == f64::INFINITY || s.end_time > time + EPS)
            })
            .count()
    }

    /// Whether `note` is held (no note-off yet).
    pub fn is_held(&self, note: i32) -> bool {
        self.slots
            .iter()
            .any(|s| s.note == Some(note) && s.release_time == f64::INFINITY)
    }

    /// Held notes, ascending (control side: allocates).
    pub fn held_notes(&self) -> Vec<i32> {
        let mut v: Vec<i32> = self
            .slots
            .iter()
            .filter(|s| s.release_time == f64::INFINITY)
            .filter_map(|s| s.note)
            .collect();
        v.sort_unstable();
        v
    }

    fn find_voice_for_note(&self, note: i32, time: f64) -> Option<usize> {
        self.slots.iter().position(|s| {
            s.note == Some(note) && (s.release_time == f64::INFINITY || s.end_time > time + EPS)
        })
    }

    fn find_free_voice(&self, time: f64) -> Option<usize> {
        let mut best: Option<usize> = None;
        for (i, s) in self.slots.iter().enumerate() {
            let free =
                s.note.is_none() || (s.release_time != f64::INFINITY && s.end_time <= time + EPS);
            if free && best.is_none_or(|b| s.end_time < self.slots[b].end_time) {
                best = Some(i);
            }
        }
        best
    }

    fn choose_victim(&self, time: f64) -> usize {
        let mut releasing: Option<usize> = None;
        for (i, s) in self.slots.iter().enumerate() {
            if s.release_time != f64::INFINITY
                && s.release_time <= time + EPS
                && releasing.is_none_or(|r| s.release_time < self.slots[r].release_time)
            {
                releasing = Some(i);
            }
        }
        if let Some(r) = releasing {
            return r;
        }
        let held = || self.slots.iter().filter(|s| s.note.is_some());
        let count = held().count();
        let mut lowest: Option<&VoiceSlot> = None;
        for s in held() {
            if lowest.is_none_or(|l| s.note < l.note) {
                lowest = Some(s);
            }
        }
        let mut oldest: Option<&VoiceSlot> = None;
        for s in held() {
            if count > 1 && lowest.is_some_and(|l| l.index == s.index) {
                continue;
            }
            if oldest.is_none_or(|o| s.start_time < o.start_time) {
                oldest = Some(s);
            }
        }
        oldest.map_or(0, |s| s.index)
    }

    fn assign(
        &mut self,
        i: usize,
        note: i32,
        time: f64,
        velocity: f64,
        stolen: bool,
        retrigger: bool,
    ) -> Allocation {
        let s = &mut self.slots[i];
        let previous_note = s.note;
        let was_sounding =
            s.note.is_some() && (s.release_time == f64::INFINITY || s.end_time > time + EPS);
        s.note = Some(note);
        s.velocity = velocity;
        s.start_time = time;
        s.release_time = f64::INFINITY;
        s.end_time = f64::INFINITY;
        Allocation {
            voice: i,
            stolen: stolen || (!retrigger && was_sounding),
            retrigger,
            previous_note,
        }
    }
}
