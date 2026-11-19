//! Port of `src/audio/voiceAllocator.test.ts`.

use super::alloc::{MonoAction, MonoNoteStack, PolyVoiceAllocator};

fn released(a: &mut PolyVoiceAllocator, note: i32, time: f64, rel: f64) -> Vec<usize> {
    let mut v = Vec::new();
    a.note_off(note, time, rel, |i| v.push(i));
    v
}

fn release_all(a: &mut PolyVoiceAllocator, time: f64) -> Vec<usize> {
    let mut v = Vec::new();
    a.release_all(time, |i| v.push(i));
    v
}

#[test]
fn assigns_distinct_voices_to_distinct_notes_and_releases_them() {
    let mut a = PolyVoiceAllocator::new(8);
    let v1 = a.note_on(60, 0.0, 1.0);
    let v2 = a.note_on(64, 0.0, 1.0);
    assert_ne!(v1.voice, v2.voice);
    assert_eq!(a.held_notes(), vec![60, 64]);
    assert_eq!(released(&mut a, 60, 1.0, 0.5), vec![v1.voice]);
    assert_eq!(a.held_notes(), vec![64]);
    assert_eq!(a.active_voice_count(1.2), 2); // 60 still in its release tail
    assert_eq!(a.active_voice_count(1.6), 1);
}

#[test]
fn retriggering_a_held_note_reuses_the_same_voice() {
    let mut a = PolyVoiceAllocator::new(8);
    let first = a.note_on(60, 0.0, 1.0);
    let again = a.note_on(60, 0.1, 1.0);
    assert_eq!(again.voice, first.voice);
    assert!(again.retrigger);
    assert_eq!(a.pool_size(), 1);
    assert_eq!(released(&mut a, 60, 0.2, 0.3), vec![first.voice]);
    assert!(a.held_notes().is_empty());
    assert_eq!(a.active_voice_count(1.0), 0);
}

#[test]
fn retriggering_a_note_in_its_release_tail_reuses_that_voice() {
    let mut a = PolyVoiceAllocator::new(8);
    let first = a.note_on(60, 0.0, 1.0);
    released(&mut a, 60, 0.5, 2.0);
    let again = a.note_on(60, 1.0, 1.0);
    assert_eq!(again.voice, first.voice);
    assert!(again.retrigger);
}

#[test]
fn note_off_for_an_unknown_note_is_a_no_op() {
    let mut a = PolyVoiceAllocator::new(4);
    a.note_on(60, 0.0, 1.0);
    assert!(released(&mut a, 61, 0.1, 0.2).is_empty());
    assert_eq!(a.held_notes(), vec![60]);
}

#[test]
fn note_off_twice_only_releases_once() {
    let mut a = PolyVoiceAllocator::new(4);
    a.note_on(60, 0.0, 1.0);
    assert_eq!(released(&mut a, 60, 0.1, 0.2).len(), 1);
    assert_eq!(released(&mut a, 60, 0.2, 0.2).len(), 0);
}

#[test]
fn release_all_returns_every_busy_voice_and_clears_state() {
    let mut a = PolyVoiceAllocator::new(8);
    a.note_on(60, 0.0, 1.0);
    a.note_on(64, 0.0, 1.0);
    a.note_on(67, 0.0, 1.0);
    released(&mut a, 67, 0.1, 5.0); // still ringing
    let mut busy = release_all(&mut a, 0.2);
    busy.sort_unstable();
    assert_eq!(busy, vec![0, 1, 2]);
    assert!(a.held_notes().is_empty());
    assert_eq!(a.active_voice_count(0.2), 0);
    // A late noteOff after releaseAll does nothing.
    assert!(released(&mut a, 60, 0.3, 0.2).is_empty());
}

#[test]
fn reuses_free_voices_before_growing_the_pool() {
    let mut a = PolyVoiceAllocator::new(8);
    a.note_on(60, 0.0, 1.0);
    released(&mut a, 60, 0.1, 0.1);
    let next = a.note_on(62, 1.0, 1.0);
    assert_eq!(next.voice, 0);
    assert!(!next.stolen);
    assert_eq!(a.pool_size(), 1);
}

#[test]
fn steals_a_releasing_voice_before_a_held_one() {
    let mut a = PolyVoiceAllocator::new(2);
    a.note_on(60, 0.0, 1.0); // voice 0 held
    a.note_on(64, 0.1, 1.0); // voice 1
    released(&mut a, 64, 0.2, 5.0); // voice 1 releasing (long tail)
    let third = a.note_on(67, 0.3, 1.0);
    assert_eq!(third.voice, 1);
    assert!(third.stolen);
    assert_eq!(third.previous_note, Some(64));
    assert_eq!(a.held_notes(), vec![60, 67]);
}

#[test]
fn steals_the_oldest_held_voice_but_spares_the_lowest_note() {
    let mut a = PolyVoiceAllocator::new(3);
    a.note_on(48, 0.0, 1.0); // bass, oldest
    a.note_on(64, 0.1, 1.0);
    a.note_on(67, 0.2, 1.0);
    let fourth = a.note_on(72, 0.3, 1.0);
    assert!(fourth.stolen);
    assert_eq!(fourth.previous_note, Some(64));
    assert_eq!(a.held_notes(), vec![48, 67, 72]);
    // The stolen note's later noteOff must not release anything else.
    assert!(released(&mut a, 64, 0.4, 0.1).is_empty());
    assert_eq!(a.held_notes(), vec![48, 67, 72]);
}

#[test]
fn never_exceeds_max_polyphony() {
    let mut a = PolyVoiceAllocator::new(4);
    for n in 0..20 {
        a.note_on(40 + n, n as f64 * 0.01, 1.0);
    }
    assert_eq!(a.pool_size(), 4);
    assert_eq!(a.held_notes().len(), 4);
}

#[test]
fn books_scheduled_one_shots_against_event_time() {
    let mut a = PolyVoiceAllocator::new(2);
    a.one_shot(60, 10.0, 1.0, 0.5, 1.0); // busy until 11.5
    a.one_shot(62, 10.0, 1.0, 0.5, 1.0);
    // At t=12 both are free again: no stealing.
    let later = a.one_shot(64, 12.0, 1.0, 0.5, 1.0);
    assert!(!later.stolen);
    assert_eq!(a.active_voice_count(12.1), 1);
    assert_eq!(a.active_voice_count(14.0), 0);
}

#[test]
fn silence_frees_a_voice_after_the_fade() {
    let mut a = PolyVoiceAllocator::new(2);
    a.note_on(60, 0.0, 1.0);
    a.silence(0, 1.0, 0.02);
    assert_eq!(a.active_voice_count(1.01), 1);
    assert_eq!(a.active_voice_count(1.03), 0);
}

#[test]
fn mono_first_key_attacks_release_when_all_keys_are_up() {
    let mut s = MonoNoteStack::new(true);
    assert_eq!(
        s.note_on(60, 0.8),
        MonoAction::Attack {
            note: 60,
            velocity: 0.8,
            from: None
        }
    );
    assert_eq!(s.note_off(60), MonoAction::Release { note: 60 });
    assert_eq!(s.current(), None);
}

#[test]
fn mono_legato_overlapping_key_glides_and_falls_back_to_the_held_key() {
    let mut s = MonoNoteStack::new(true);
    s.note_on(60, 0.8);
    assert_eq!(
        s.note_on(64, 0.7),
        MonoAction::Legato {
            note: 64,
            velocity: 0.7,
            from: 60
        }
    );
    assert_eq!(
        s.note_off(64),
        MonoAction::Legato {
            note: 60,
            velocity: 0.8,
            from: 64
        }
    );
    assert_eq!(s.note_off(60), MonoAction::Release { note: 60 });
}

#[test]
fn mono_releasing_a_key_that_is_not_sounding_changes_nothing() {
    let mut s = MonoNoteStack::new(true);
    s.note_on(60, 0.8);
    s.note_on(64, 0.8);
    assert_eq!(s.note_off(60), MonoAction::None);
    assert_eq!(s.current(), Some(64));
    assert_eq!(s.note_off(64), MonoAction::Release { note: 64 });
}

#[test]
fn mono_non_legato_mode_re_attacks_each_new_key() {
    let mut s = MonoNoteStack::new(false);
    s.note_on(60, 0.8);
    assert_eq!(
        s.note_on(64, 0.8),
        MonoAction::Attack {
            note: 64,
            velocity: 0.8,
            from: Some(60)
        }
    );
}

#[test]
fn mono_re_pressing_a_held_key_moves_it_to_the_top_without_duplicates() {
    let mut s = MonoNoteStack::new(true);
    s.note_on(60, 0.8);
    s.note_on(64, 0.8);
    s.note_on(60, 0.9);
    assert_eq!(s.held_notes().collect::<Vec<_>>(), vec![64, 60]);
    assert!(matches!(
        s.note_off(60),
        MonoAction::Legato { note: 64, .. }
    ));
    assert!(matches!(s.note_off(64), MonoAction::Release { .. }));
}

#[test]
fn mono_release_all_clears_the_stack_and_stray_note_offs_are_ignored() {
    let mut s = MonoNoteStack::new(true);
    s.note_on(60, 0.8);
    s.note_on(62, 0.8);
    s.release_all();
    assert_eq!(s.held_count(), 0);
    assert_eq!(s.note_off(62), MonoAction::None);
}
