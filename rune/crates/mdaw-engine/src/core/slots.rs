//! A fixed-size set of track slots (no allocation), for the per-pad live targets.

use super::api::MAX_SLOTS;

const WORDS: usize = MAX_SLOTS.div_ceil(64);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SlotSet([u64; WORDS]);

impl SlotSet {
    pub const EMPTY: SlotSet = SlotSet([0; WORDS]);

    pub fn single(slot: usize) -> Self {
        let mut s = Self::EMPTY;
        s.insert(slot);
        s
    }
    pub fn insert(&mut self, slot: usize) {
        if slot < MAX_SLOTS {
            self.0[slot / 64] |= 1 << (slot % 64);
        }
    }
    pub fn remove(&mut self, slot: usize) {
        if slot < MAX_SLOTS {
            self.0[slot / 64] &= !(1 << (slot % 64));
        }
    }
    pub fn contains(&self, slot: usize) -> bool {
        slot < MAX_SLOTS && self.0[slot / 64] & (1 << (slot % 64)) != 0
    }
    pub fn is_empty(&self) -> bool {
        self.0.iter().all(|w| *w == 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covers_every_slot() {
        let mut s = SlotSet::EMPTY;
        for i in [0, 63, 64, 200, MAX_SLOTS - 1] {
            s.insert(i);
            assert!(s.contains(i));
        }
        assert!(!s.contains(1) && !s.contains(MAX_SLOTS));
        for i in [0, 63, 64, 200, MAX_SLOTS - 1] {
            s.remove(i);
        }
        assert!(s.is_empty());
        assert!(SlotSet::single(256).contains(256));
    }
}
