//! Ports of `useMidiNotes.test.ts`, `useMidiNotes.routing.test.ts` and the routing cases of
//! `src/test/stuckNotes.test.tsx`. The fake synth models the worst case of a Tone.PolySynth
//! (`fakeAudioEngine.ts`): every note-on adds a voice, every note-off removes one, so a
//! double attack or a lost note-off shows as a leaked voice. Note-repeat timing cases live in
//! the core (phase 3: `core/tests_repeat.rs`).

use mdaw_model::controller::{
    ControllerMapping, KnobEvent, MemoryProfileStore, PadLearn, create_default_profile,
};
use mdaw_model::types::KnobTarget;

use super::*;

#[derive(Default)]
struct Fake {
    voices: Vec<i64>,
    log: Vec<Action>,
    drum_hits: Vec<(PadIndex, f64)>,
    drum_releases: Vec<PadIndex>,
    repeat_press: Vec<PadIndex>,
    captures: Vec<Action>,
    knobs: Vec<KnobEvent>,
    legacy: Vec<(u8, u8)>,
    bus: Vec<BusEvent>,
}

impl Fake {
    fn sounding(&self) -> Vec<i64> {
        let mut v = self.voices.clone();
        v.sort_unstable();
        v
    }
}

impl RouterOutput for Fake {
    fn emit(&mut self, a: Action) {
        match &a {
            Action::SynthOn { note, .. } => self.voices.push(*note),
            Action::SynthOff { note, .. } => {
                if let Some(i) = self.voices.iter().position(|n| n == note) {
                    self.voices.remove(i);
                }
            }
            Action::SynthReleaseAll { .. } => self.voices.clear(),
            Action::PadDown { pad, velocity, .. } => {
                self.drum_hits.push((*pad, *velocity));
                self.repeat_press.push(*pad);
            }
            Action::PadHit { pad, velocity, .. } => self.drum_hits.push((*pad, *velocity)),
            Action::PadUp { pad, .. } => self.drum_releases.push(*pad),
            Action::CaptureNoteOn { .. } | Action::CaptureNoteOff { .. } => {
                self.captures.push(a.clone())
            }
            Action::Knob(k) => self.knobs.push(*k),
            Action::LegacyCc { cc, value, .. } => self.legacy.push((*cc, *value)),
            Action::Bus(b) => self.bus.push(*b),
            _ => {}
        }
        self.log.push(a);
    }
}

fn router() -> Router<MemoryProfileStore> {
    Router::new(ControllerMapping::new(
        MemoryProfileStore::default(),
        Box::new(|| 0.0),
    ))
}

// --- useMidiNotes.test.ts ---------------------------------------------------------------

#[test]
fn plays_and_releases_a_keyboard_note() {
    let (mut r, mut f) = (router(), Fake::default());
    r.midi_note_on(60, 0.8, 1, None, &mut f);
    assert_eq!(f.sounding(), vec![60]);
    assert_eq!(
        r.active_notes(),
        vec![ActiveNote {
            note: 60,
            velocity: 0.8
        }]
    );
    r.midi_note_off(60, 1, None, &mut f);
    assert!(f.sounding().is_empty());
    assert!(r.active_notes().is_empty());
}

// --- useMidiNotes.routing.test.ts -------------------------------------------------------

#[test]
fn a_mapped_pad_note_hits_the_pad_and_releases_on_note_off() {
    let (mut r, mut f) = (router(), Fake::default());
    r.midi_note_on(38, 0.9, 10, None, &mut f);
    assert_eq!(f.drum_hits, vec![(2, 0.9)]);
    assert!(f.sounding().is_empty());
    r.midi_note_off(38, 10, None, &mut f);
    assert_eq!(f.drum_releases, vec![2]);
}

#[test]
fn uses_the_controller_profile_not_gm_numbers() {
    let (mut r, mut f) = (router(), Fake::default());
    let mut p = create_default_profile();
    p.pad_map.notes[0] = Some(60);
    p.pad_map.notes[15] = None;
    r.mapping.set_profile(&p);
    r.midi_note_on(60, 0.5, 10, None, &mut f);
    assert_eq!(f.drum_hits, vec![(0, 0.5)]);
    r.midi_note_off(60, 10, None, &mut f);
    assert_eq!(f.drum_releases, vec![0]);
}

#[test]
fn unmapped_channel_10_notes_fall_through_to_the_synth() {
    let (mut r, mut f) = (router(), Fake::default());
    r.midi_note_on(70, 0.8, 10, None, &mut f);
    assert!(f.drum_hits.is_empty());
    assert_eq!(f.sounding(), vec![70]);
    r.midi_note_off(70, 10, None, &mut f);
    assert!(f.sounding().is_empty());
}

#[test]
fn pad_map_channel_filter_sends_other_channels_to_the_synth() {
    let (mut r, mut f) = (router(), Fake::default());
    let mut p = create_default_profile();
    p.pad_map.channel = Some(10);
    r.mapping.set_profile(&p);
    r.midi_note_on(48, 0.8, 1, None, &mut f);
    assert!(f.drum_hits.is_empty());
    assert_eq!(f.sounding(), vec![48]);
    r.midi_note_off(48, 1, None, &mut f);
    assert!(f.sounding().is_empty());
    r.midi_note_on(48, 0.8, 10, None, &mut f);
    assert_eq!(f.drum_hits, vec![(12, 0.8)]);
}

#[test]
fn keys_routing_sends_mapped_notes_to_the_synth() {
    let (mut r, mut f) = (router(), Fake::default());
    r.set_routing_mode(RoutingMode::Keys);
    r.midi_note_on(36, 0.8, 10, None, &mut f);
    assert!(f.drum_hits.is_empty());
    assert_eq!(f.sounding(), vec![36]);
    r.midi_note_off(36, 10, None, &mut f);
    assert!(f.sounding().is_empty());
}

#[test]
fn split_behaves_like_auto() {
    for mode in [RoutingMode::Auto, RoutingMode::Split] {
        let (mut r, mut f) = (router(), Fake::default());
        r.set_routing_mode(mode);
        r.midi_note_on(36, 0.8, 1, None, &mut f);
        r.midi_note_on(30, 0.8, 1, None, &mut f);
        assert_eq!(f.drum_hits, vec![(0, 0.8)]);
        assert_eq!(f.sounding(), vec![30]);
    }
}

#[test]
fn a_remap_while_a_pad_is_held_still_releases_the_pad_it_hit() {
    let (mut r, mut f) = (router(), Fake::default());
    r.midi_note_on(36, 0.8, 10, None, &mut f);
    let mut p = create_default_profile();
    p.pad_map.notes[0] = None;
    r.mapping.set_profile(&p);
    r.midi_note_off(36, 10, None, &mut f);
    assert_eq!(f.drum_releases, vec![0]);
    assert!(f.sounding().is_empty());
}

#[test]
fn all_notes_off_releases_held_pads_once() {
    let (mut r, mut f) = (router(), Fake::default());
    r.midi_note_on(36, 0.8, 10, None, &mut f);
    r.all_notes_off(&mut f);
    assert_eq!(f.drum_releases, vec![0]);
    r.midi_note_off(36, 10, None, &mut f); // late note-off is harmless
    assert_eq!(f.drum_releases, vec![0]);
}

#[test]
fn on_screen_drum_hit_accepts_all_16_pads_and_is_never_repeated() {
    let (mut r, mut f) = (router(), Fake::default());
    r.drum_hit(12.0, 0.6, &mut f);
    assert_eq!(f.drum_hits, vec![(12, 0.6)]);
    r.drum_hit(16.0, 0.6, &mut f);
    r.drum_hit(-1.0, 0.6, &mut f);
    r.drum_hit(2.5, 0.6, &mut f);
    assert_eq!(f.drum_hits.len(), 1);
    assert!(f.repeat_press.is_empty());
}

#[test]
fn while_pad_learning_the_hit_is_consumed_and_its_note_off_swallowed() {
    let (mut r, mut f) = (router(), Fake::default());
    r.mapping.start_pad_learn(PadLearn::Pad(4));
    r.midi_note_on(64, 0.8, 1, None, &mut f);
    assert!(f.drum_hits.is_empty());
    assert!(f.sounding().is_empty());
    r.midi_note_off(64, 1, None, &mut f);
    assert!(f.drum_releases.is_empty());
    assert!(f.log.is_empty());
    r.midi_note_on(64, 0.8, 1, None, &mut f);
    assert_eq!(f.drum_hits, vec![(4, 0.8)]);
}

#[test]
fn mapped_ccs_go_to_knobs_unmapped_ones_keep_the_legacy_path() {
    let (mut r, mut f) = (router(), Fake::default());
    r.handle_cc(76, 127, 1, None, &mut f);
    assert_eq!(
        f.knobs,
        vec![KnobEvent {
            target: KnobTarget::TrackVolume,
            value01: 1.0
        }]
    );
    assert!(f.legacy.is_empty());
    r.handle_cc(5, 127, 1, None, &mut f);
    assert_eq!(f.legacy, vec![(5, 127)]);
    assert_eq!(f.knobs.len(), 1);
    r.handle_cc(20, 1, 1, None, &mut f); // neither mapped nor legacy: nothing
    assert_eq!(f.legacy.len(), 1);
}

#[test]
fn knob_learn_consumes_the_next_cc() {
    let (mut r, mut f) = (router(), Fake::default());
    r.mapping.start_knob_learn(KnobTarget::Swing, None);
    r.handle_cc(21, 64, 1, None, &mut f);
    assert!(f.knobs.is_empty());
    r.handle_cc(21, 127, 1, None, &mut f);
    assert_eq!(
        f.knobs,
        vec![KnobEvent {
            target: KnobTarget::Swing,
            value01: 1.0
        }]
    );
}

#[test]
fn sustain_is_never_routed_to_knobs() {
    let (mut r, mut f) = (router(), Fake::default());
    r.mapping.start_knob_learn(KnobTarget::Swing, None);
    r.handle_cc(64, 127, 1, None, &mut f);
    assert!(r.sustain_on());
    assert!(r.mapping.is_learning());
}

#[test]
fn held_pads_feed_note_repeat_one_shots_do_not() {
    let (mut r, mut f) = (router(), Fake::default());
    r.midi_note_on(36, 1.0, 10, None, &mut f);
    r.midi_note_off(36, 10, None, &mut f);
    r.drum_press(5.0, 0.7, &mut f);
    r.drum_release(5.0, &mut f);
    r.drum_hit(3.0, 0.5, &mut f);
    assert_eq!(f.repeat_press, vec![0, 5]);
    assert_eq!(f.drum_releases, vec![0, 5]);
}

#[test]
fn recording_captures_synth_notes_and_retriggers() {
    let (mut r, mut f) = (router(), Fake::default());
    r.midi_note_on(60, 0.5, 1, Some(1.5), &mut f);
    r.midi_note_on(60, 0.6, 1, Some(1.6), &mut f); // re-press closes the old capture
    r.midi_note_off(60, 1, Some(1.7), &mut f);
    assert_eq!(
        f.captures,
        vec![
            Action::CaptureNoteOn {
                note: 60,
                velocity: 0.5,
                time: Some(1.5)
            },
            Action::CaptureNoteOff {
                note: 60,
                time: Some(1.6)
            },
            Action::CaptureNoteOn {
                note: 60,
                velocity: 0.6,
                time: Some(1.6)
            },
            Action::CaptureNoteOff {
                note: 60,
                time: Some(1.7)
            },
        ]
    );
}

// --- stuckNotes.test.tsx (routing cases) -----------------------------------------------

#[test]
fn duplicate_note_on_for_a_held_key_does_not_leak_a_voice() {
    let (mut r, mut f) = (router(), Fake::default());
    r.midi_note_on(60, 0.8, 1, None, &mut f);
    r.midi_note_on(60, 0.8, 1, None, &mut f);
    assert_eq!(f.sounding(), vec![60]);
    r.midi_note_off(60, 1, None, &mut f);
    assert!(f.sounding().is_empty());
}

#[test]
fn midi_key_and_on_screen_key_on_the_same_pitch_share_one_voice() {
    let (mut r, mut f) = (router(), Fake::default());
    r.midi_note_on(64, 0.8, 1, None, &mut f);
    r.note_on(64, 0.7, &mut f);
    r.note_off(64, &mut f);
    r.midi_note_off(64, 1, None, &mut f);
    assert!(f.sounding().is_empty());
}

#[test]
fn note_on_with_velocity_0_is_a_note_off() {
    let (mut r, mut f) = (router(), Fake::default());
    r.midi_note_on(60, 0.8, 1, None, &mut f);
    r.midi_note_on(60, 0.0, 1, None, &mut f);
    assert!(f.sounding().is_empty());
    assert!(r.active_notes().is_empty());
}

#[test]
fn changing_routing_mode_while_a_key_is_held_still_releases_it() {
    let (mut r, mut f) = (router(), Fake::default());
    r.set_routing_mode(RoutingMode::Keys);
    r.midi_note_on(36, 0.8, 1, None, &mut f);
    r.set_routing_mode(RoutingMode::Auto);
    r.midi_note_off(36, 1, None, &mut f);
    assert!(f.sounding().is_empty());
}

#[test]
fn toggling_mute_while_a_key_is_held_still_releases_it() {
    let (mut r, mut f) = (router(), Fake::default());
    r.midi_note_on(60, 0.8, 1, None, &mut f);
    r.set_muted(true, &mut f);
    r.midi_note_off(60, 1, None, &mut f);
    assert!(f.sounding().is_empty());
    // a key pressed while muted is tracked but never attacked
    r.midi_note_on(61, 0.8, 1, None, &mut f);
    assert!(f.sounding().is_empty());
    assert_eq!(r.active_notes().len(), 1);
}

#[test]
fn on_screen_drum_pad_in_keys_routing_hits_the_drum_engine() {
    let (mut r, mut f) = (router(), Fake::default());
    r.set_routing_mode(RoutingMode::Keys);
    r.drum_hit(0.0, 0.8, &mut f);
    assert!(f.sounding().is_empty());
    assert_eq!(f.drum_hits, vec![(0, 0.8)]);
}

#[test]
fn on_screen_piano_keys_in_the_drum_range_play_the_synth() {
    let (mut r, mut f) = (router(), Fake::default());
    r.note_on(36, 0.7, &mut f);
    assert_eq!(f.sounding(), vec![36]);
    assert!(f.drum_hits.is_empty());
    r.note_off(36, &mut f);
    assert!(f.sounding().is_empty());
}

#[test]
fn sustain_holds_released_notes_until_pedal_up_and_restrikes_do_not_stack() {
    let (mut r, mut f) = (router(), Fake::default());
    r.handle_cc(CC_SUSTAIN, 127, 1, None, &mut f);
    r.midi_note_on(60, 0.8, 1, None, &mut f);
    r.midi_note_off(60, 1, None, &mut f);
    assert_eq!(f.sounding(), vec![60]);
    assert!(r.active_notes().is_empty());
    r.midi_note_on(60, 0.8, 1, None, &mut f);
    r.midi_note_off(60, 1, None, &mut f);
    r.midi_note_on(64, 0.8, 1, None, &mut f);
    assert_eq!(f.sounding(), vec![60, 64]);
    r.handle_cc(CC_SUSTAIN, 0, 1, None, &mut f);
    assert_eq!(f.sounding(), vec![64]);
    r.midi_note_off(64, 1, None, &mut f);
    assert!(f.sounding().is_empty());
}

#[test]
fn all_notes_off_cc_and_panic_silence_everything() {
    let (mut r, mut f) = (router(), Fake::default());
    r.midi_note_on(60, 0.8, 1, None, &mut f);
    r.midi_note_on(64, 0.8, 1, None, &mut f);
    r.handle_cc(CC_ALL_NOTES_OFF, 0, 1, None, &mut f);
    assert!(f.sounding().is_empty());
    assert!(
        f.log
            .iter()
            .any(|a| matches!(a, Action::SynthReleaseAll { .. }))
    );
    assert!(r.active_notes().is_empty());
    r.midi_note_off(60, 1, None, &mut f);
    r.midi_note_on(67, 0.8, 1, None, &mut f);
    r.all_notes_off(&mut f);
    assert!(f.sounding().is_empty());
    assert!(r.sounding_notes().is_empty());
    assert_eq!(f.bus.last(), Some(&BusEvent::AllNotesOff));
}

#[test]
fn engine_actions_carry_the_event_time() {
    let (mut r, mut f) = (router(), Fake::default());
    r.midi_note_on(36, 0.5, 1, Some(2.0), &mut f);
    r.midi_note_on(60, 0.5, 1, Some(2.5), &mut f);
    r.handle_cc(123, 0, 1, Some(3.0), &mut f);
    let engine: Vec<&Action> = f.log.iter().filter(|a| a.is_engine()).collect();
    assert_eq!(
        engine[..2],
        [
            &Action::PadDown {
                pad: 0,
                velocity: 0.5,
                time: Some(2.0)
            },
            &Action::SynthOn {
                note: 60,
                velocity: 0.5,
                time: Some(2.5)
            },
        ]
    );
    assert!(engine[2..].iter().all(|a| match a {
        Action::SynthOff { time, .. }
        | Action::PadUp { time, .. }
        | Action::RepeatReleaseAll { time }
        | Action::SynthReleaseAll { time } => *time == Some(3.0),
        _ => false,
    }));
}
