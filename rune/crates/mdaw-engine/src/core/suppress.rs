//! LoopRecorder's suppressed pattern hits (`suppressedHits`, keyed by `note@round(beat·1000)`):
//! a hit the player just played live and that was snapped forward into the current pass is
//! not replayed by the pattern, once, if the pattern reaches it by `until`.

const SLOTS: usize = 64;

#[derive(Clone, Copy)]
struct Entry {
    note: f64,
    key: i64,
    until: f64,
}

/// `Math.round(beat * 1000)` (LoopRecorder hitKey).
fn hit_key(beat: f64) -> i64 {
    (beat * 1000.0 + 0.5).floor() as i64
}

/// Fixed-size table (no allocation on the audio thread).
pub(crate) struct Suppressed {
    slots: [Option<Entry>; SLOTS],
}

impl Suppressed {
    pub fn new() -> Self {
        Self {
            slots: [None; SLOTS],
        }
    }

    /// `suppressedHits.set(key, until)` (a full table replaces the entry that expires first).
    pub fn insert(&mut self, note: f64, beat: f64, until: f64) {
        let key = hit_key(beat);
        let until_of = |e: &Option<Entry>| e.map_or(f64::NEG_INFINITY, |e| e.until);
        let idx = self
            .slots
            .iter()
            .position(|s| s.is_some_and(|s| s.key == key && s.note == note))
            .or_else(|| self.slots.iter().position(Option::is_none))
            .or_else(|| {
                (0..SLOTS)
                    .min_by(|a, b| until_of(&self.slots[*a]).total_cmp(&until_of(&self.slots[*b])))
            })
            .unwrap_or(0);
        self.slots[idx] = Some(Entry { note, key, until });
    }

    /// `consumeSuppressedHit`: skip once if suppressed and `time ≤ until` (the entry is
    /// consumed either way).
    pub fn consume(&mut self, note: f64, beat: f64, time: f64) -> bool {
        let key = hit_key(beat);
        for s in self.slots.iter_mut() {
            if let Some(e) = *s
                && e.key == key
                && e.note == note
            {
                *s = None;
                return time <= e.until;
            }
        }
        false
    }
}
