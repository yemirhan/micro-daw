//! Voice allocation (`src/audio/voiceAllocator.ts`): which voice plays which MIDI note.
//!
//! - A note is held by at most one voice; re-striking a sounding (held or releasing) note reuses
//!   its voice.
//! - Otherwise: a free voice (the least recently finished), then grow the pool up to the
//!   polyphony, then steal: a voice already in its release tail (released longest ago), else the
//!   oldest held voice, sparing the lowest note (the bass) when there is a choice.
//! - Times are context seconds; scheduled one-shots book their release and end up front.
//!
//! [`MonoNoteStack`]: last-note priority for mono presets (legato glides, falling back to a
//! still-held key always glides).
//!
//! Both are realtime-safe: fixed capacity, no allocation after construction.

const EPS: f64 = 1e-6;

/// Hard cap on voices per instrument (the voice storage is allocated up front, so a polyphony
/// change never allocates on the audio thread). The TS has no cap; a preset asking for more
/// than this plays with this many voices (documented limit).
pub const MAX_VOICES: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoiceSlot {
    pub index: usize,
    /// MIDI note this voice is assigned to (None = never used / hard-stopped).
    pub note: Option<i32>,
    pub velocity: f64,
    pub start_time: f64,
    /// When the key is/was released (∞ while held).
    pub release_time: f64,
    /// When the release tail is over.
    pub end_time: f64,
}

impl VoiceSlot {
    fn sounding_at(&self, time: f64) -> bool {
        self.note.is_some() && (self.release_time == f64::INFINITY || self.end_time > time + EPS)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Allocation {
    pub voice: usize,
    /// The voice was still sounding a different note (cut off).
    pub stolen: bool,
    /// Same note re-struck on the voice that was holding/releasing it.
    pub retrigger: bool,
    pub previous_note: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct PolyVoiceAllocator {
    pub max_voices: usize,
    slots: Vec<VoiceSlot>,
}

impl PolyVoiceAllocator {
    pub fn new(max_voices: usize) -> Self {
        Self {
            max_voices: max_voices.clamp(1, MAX_VOICES),
            slots: Vec::with_capacity(MAX_VOICES),
        }
    }

    pub fn set_max_voices(&mut self, n: usize) {
        self.max_voices = n.clamp(1, MAX_VOICES);
    }

    /// Number of voice objects ever allocated (pool size).
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

    /// Release the held voice(s) playing `note`; calls `released` with each index.
    pub fn note_off(
        &mut self,
        note: i32,
        time: f64,
        release_duration: f64,
        mut released: impl FnMut(usize),
    ) {
        for slot in &mut self.slots {
            if slot.note == Some(note) && slot.release_time == f64::INFINITY {
                slot.release_time = time.max(slot.start_time);
                slot.end_time = slot.release_time + release_duration.max(0.0);
                released(slot.index);
            }
        }
    }

    /// Book a self-releasing note (attack at `time`, release at `time + duration`).
    pub fn one_shot(
        &mut self,
        note: i32,
        time: f64,
        duration: f64,
        release_duration: f64,
        velocity: f64,
    ) -> Allocation {
        let alloc = self.note_on(note, time, velocity);
        let slot = &mut self.slots[alloc.voice];
        slot.release_time = time + duration.max(0.0);
        slot.end_time = slot.release_time + release_duration.max(0.0);
        alloc
    }

    /// Mark a voice silent from `time` (hard stop / fast fade).
    pub fn silence(&mut self, index: usize, time: f64, fade: f64) {
        if let Some(slot) = self.slots.get_mut(index) {
            slot.release_time = slot.release_time.min(time);
            slot.end_time = time + fade;
        }
    }

    /// Reset every slot to idle; calls `busy` with each voice that was held or still sounding.
    pub fn release_all(&mut self, time: f64, mut busy: impl FnMut(usize)) {
        for slot in &mut self.slots {
            if slot.sounding_at(time) {
                busy(slot.index);
            }
            slot.note = None;
            slot.release_time = f64::NEG_INFINITY;
            slot.end_time = f64::NEG_INFINITY;
        }
    }

    /// Voices held or still in their release tail at `time`.
    pub fn active_voice_count(&self, time: f64) -> usize {
        self.slots.iter().filter(|s| s.sounding_at(time)).count()
    }

    /// Notes whose key is held, ascending (control side: allocates).
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

    /// Whether `note` is held (in [`Self::held_notes`]), without allocating.
    pub fn is_held(&self, note: i32) -> bool {
        self.slots
            .iter()
            .any(|s| s.release_time == f64::INFINITY && s.note == Some(note))
    }

    pub fn voice_for_note(&self, note: i32, time: f64) -> Option<usize> {
        self.find_voice_for_note(note, time)
    }

    fn find_voice_for_note(&self, note: i32, time: f64) -> Option<usize> {
        self.slots
            .iter()
            .find(|s| s.note == Some(note) && s.sounding_at(time))
            .map(|s| s.index)
    }

    fn find_free_voice(&self, time: f64) -> Option<usize> {
        let mut best: Option<&VoiceSlot> = None;
        for slot in &self.slots {
            let free = slot.note.is_none()
                || (slot.release_time != f64::INFINITY && slot.end_time <= time + EPS);
            if !free {
                continue;
            }
            if best.is_none_or(|b| slot.end_time < b.end_time) {
                best = Some(slot);
            }
        }
        best.map(|s| s.index)
    }

    fn choose_victim(&self, time: f64) -> usize {
        let mut releasing: Option<&VoiceSlot> = None;
        for slot in &self.slots {
            if slot.release_time != f64::INFINITY
                && slot.release_time <= time + EPS
                && releasing.is_none_or(|r| slot.release_time < r.release_time)
            {
                releasing = Some(slot);
            }
        }
        if let Some(r) = releasing {
            return r.index;
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
        oldest.map_or(0, |o| o.index)
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
        let slot = &mut self.slots[i];
        let previous_note = slot.note;
        let was_sounding = slot.sounding_at(time);
        slot.note = Some(note);
        slot.velocity = velocity;
        slot.start_time = time;
        slot.release_time = f64::INFINITY;
        slot.end_time = f64::INFINITY;
        Allocation {
            voice: i,
            stolen: stolen || (!retrigger && was_sounding),
            retrigger,
            previous_note,
        }
    }
}

/// What a mono key event does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MonoAction {
    /// Start a new note from silence (or re-articulate when legato is off).
    Attack {
        note: i32,
        velocity: f64,
        from: Option<i32>,
    },
    /// Glide to `note` without re-triggering envelopes.
    Legato { note: i32, velocity: f64, from: i32 },
    /// All keys up: release the voice.
    Release { note: i32 },
    /// Nothing audible changes.
    None,
}

/// Last-note-priority key stack (oldest first).
#[derive(Debug, Clone)]
pub struct MonoNoteStack {
    pub legato: bool,
    stack: [(i32, f64); 128],
    len: usize,
    sounding: Option<i32>,
}

impl MonoNoteStack {
    pub fn new(legato: bool) -> Self {
        Self {
            legato,
            stack: [(0, 0.0); 128],
            len: 0,
            sounding: None,
        }
    }

    fn remove(&mut self, note: i32) -> Option<usize> {
        let idx = self.stack[..self.len].iter().position(|e| e.0 == note)?;
        self.stack.copy_within(idx + 1..self.len, idx);
        self.len -= 1;
        Some(idx)
    }

    pub fn note_on(&mut self, note: i32, velocity: f64) -> MonoAction {
        let was_empty = self.len == 0;
        let from = self.sounding;
        self.remove(note);
        if self.len == self.stack.len() {
            self.stack.copy_within(1.., 0);
            self.len -= 1;
        }
        self.stack[self.len] = (note, velocity);
        self.len += 1;
        self.sounding = Some(note);
        match from {
            Some(from) if !was_empty && self.legato => MonoAction::Legato {
                note,
                velocity,
                from,
            },
            _ => MonoAction::Attack {
                note,
                velocity,
                from,
            },
        }
    }

    pub fn note_off(&mut self, note: i32) -> MonoAction {
        let len = self.len;
        let Some(idx) = self.remove(note) else {
            return MonoAction::None;
        };
        if idx != len - 1 {
            return MonoAction::None;
        }
        if self.len == 0 {
            return MonoAction::Release {
                note: self.sounding.unwrap_or(note),
            };
        }
        let top = self.stack[self.len - 1];
        let from = self.sounding.unwrap_or(note);
        self.sounding = Some(top.0);
        MonoAction::Legato {
            note: top.0,
            velocity: top.1,
            from,
        }
    }

    pub fn release_all(&mut self) {
        self.len = 0;
    }

    pub fn held_count(&self) -> usize {
        self.len
    }

    /// Keys held, oldest first.
    pub fn held_notes(&self) -> impl Iterator<Item = i32> + '_ {
        self.stack[..self.len].iter().map(|e| e.0)
    }

    pub fn current(&self) -> Option<i32> {
        (self.len > 0).then(|| self.stack[self.len - 1].0)
    }

    pub fn last_sounding(&self) -> Option<i32> {
        self.sounding
    }
}
