//! `previewNote` (phase 8, the piano roll's audition): a fixed-length one-shot through the
//! track's own instrument, synth and drum tracks; unknown and audio tracks stay silent.

use mdaw_model::InstrumentType;

use super::test_rig::*;

#[test]
fn a_synth_track_preview_sounds_then_releases() {
    let mut r = Rig::new();
    let id = r.e.edit(|d| d.add_track(InstrumentType::Synth, 0)).id;
    r.run(0.05);
    let t0 = r.now();
    r.e.preview_note(&id, 60.0, 0.8, 0.3);
    r.run(3.0);
    assert!(r.rms(t0, t0 + 0.3) > 1e-3, "the note sounds");
    assert!(r.rms(t0 + 2.6, t0 + 3.0) < 1e-4, "and is released");
}

#[test]
fn a_drum_track_preview_plays_the_notes_pad() {
    let mut r = Rig::new();
    let kit = test_kit(&[]);
    let id = r.e.edit(|d| {
        let t = d.add_track(InstrumentType::Drums, 0);
        d.set_kit(&t.id, &kit, Default::default());
        t.id
    });
    r.e.wait_kits_ready();
    r.run(0.05);
    let t0 = r.now();
    r.e.preview_note(&id, 36.0, 0.8, 0.3);
    r.run(0.3);
    assert!(r.rms(t0, t0 + 0.2) > 1e-3, "pad A1 (MIDI 36) sounds");
}

#[test]
fn unknown_tracks_and_unmapped_notes_are_silent() {
    let mut r = Rig::new();
    let id = r.e.edit(|d| d.add_track(InstrumentType::Drums, 0)).id;
    r.e.wait_kits_ready();
    r.run(0.05);
    let t0 = r.now();
    r.e.preview_note("nope", 60.0, 0.8, 0.3);
    r.e.preview_note(&id, 5.0, 0.8, 0.3);
    r.run(0.3);
    assert!(r.rms(t0, t0 + 0.3) < 1e-6);
}
