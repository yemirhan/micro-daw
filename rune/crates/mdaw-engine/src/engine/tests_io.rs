//! Live input, arming, hosted plugins and audio takes on the real engine + core (offline).

use std::sync::{Arc, Mutex};

use mdaw_model::{InstrumentType, PluginSlot, TrackInput};

use super::test_rig::{Rig, SR};
use super::{TransportState, render_export_with};
use crate::io::{InputFeed, input_pair};
use crate::plugin::test_support::{Call, Probe};
use crate::plugin::{PluginFactory, PluginProcessor};

type Log = Arc<Mutex<Vec<Call>>>;

/// Plugins named "gain" (an effect: ×0.5) and "tone" (an instrument: +0.25 while a note is
/// down); anything else is "not installed". Logs per plugin instance id.
#[derive(Default)]
struct TestPlugins {
    logs: Arc<Mutex<Vec<(String, Log)>>>,
    offline: Arc<Mutex<Vec<String>>>,
}

impl TestPlugins {
    fn make(&self, p: &PluginSlot) -> Option<Box<dyn PluginProcessor>> {
        let (probe, log) = match p.name.as_str() {
            "gain" => Probe::effect(0.5),
            "tone" => Probe::instrument(0.25),
            _ => return None,
        };
        self.logs.lock().unwrap().push((p.id.clone(), log));
        Some(Box::new(probe))
    }
}

impl PluginFactory for TestPlugins {
    fn processor(&mut self, p: &PluginSlot, _: u32) -> Option<Box<dyn PluginProcessor>> {
        self.make(p)
    }
    fn offline_processor(&mut self, p: &PluginSlot, _: u32) -> Option<Box<dyn PluginProcessor>> {
        self.offline.lock().unwrap().push(p.id.clone());
        self.make(p)
    }
}

fn plugin(id: &str, name: &str) -> PluginSlot {
    PluginSlot {
        id: id.to_owned(),
        format: "au".into(),
        kind: if name == "tone" { "aumu" } else { "aufx" }.into(),
        subtype: "test".into(),
        manufacturer: "mdaw".into(),
        name: name.to_owned(),
        vendor: String::new(),
        bypassed: false,
        state: None,
        extra: Default::default(),
    }
}

/// A two-channel input: channel 0 a DC of 0.5, channel 1 of −0.25.
fn feed_input(rig: &mut Rig) -> InputFeed {
    let (feed, source) = input_pair(2, 128, SR);
    rig.e.set_input_source(Some(source));
    feed
}

/// Render `seconds`, pushing one input block before every quantum (as a device would).
fn run_with_input(rig: &mut Rig, feed: &mut InputFeed, seconds: f64) {
    let quanta = ((seconds * SR as f64) / 128.0).ceil() as usize;
    let block: Vec<f32> = (0..128).flat_map(|_| [0.5f32, -0.25]).collect();
    for _ in 0..quanta {
        feed.push_interleaved(&block);
        rig.run(128.0 / SR as f64);
    }
}

/// The track's fader at unity and no pan, so the output is the slot's signal through the strip.
fn unity(rig: &mut Rig, id: &str) {
    rig.e.edit(|d| d.set_track_volume(id, 0.0));
}

fn tail(v: &[f32]) -> f32 {
    v[v.len() - 64]
}

#[test]
fn an_armed_audio_track_is_heard_through_its_inserts_and_silent_when_disarmed() {
    let mut rig = Rig::new();
    let mut feed = feed_input(&mut rig);
    let id = rig.e.edit(|d| d.add_track(InstrumentType::Audio, 0).id);
    unity(&mut rig, &id);
    run_with_input(&mut rig, &mut feed, 0.2);
    assert_eq!(rig.rms(0.0, 0.2), 0.0, "not armed: the input is not heard");

    rig.e.edit(|d| d.set_track_armed(&id, true));
    run_with_input(&mut rig, &mut feed, 0.3);
    let dry = tail(&rig.left);
    assert!(dry > 0.2, "armed: input 1 is heard ({dry})");
    assert!(
        (tail(&rig.left) - tail(&rig.right)).abs() < 1e-3,
        "a mono input sits in the centre"
    );

    // an insert halves it
    rig.e.set_plugin_factory(Box::new(TestPlugins::default()));
    rig.e
        .edit(|d| d.add_track_plugin(&id, plugin("p1", "gain")));
    run_with_input(&mut rig, &mut feed, 0.3);
    let wet = tail(&rig.left);
    assert!((wet / dry - 0.5).abs() < 0.02, "{wet} vs {dry}");
    // bypassed: dry again; a plugin that is not installed is skipped
    rig.e.edit(|d| {
        d.set_track_plugin_bypassed(&id, "p1", true);
        d.add_track_plugin(&id, plugin("p2", "missing"));
    });
    run_with_input(&mut rig, &mut feed, 0.3);
    assert!((tail(&rig.left) / dry - 1.0).abs() < 0.02);

    // a stereo pair: left = input 1, right = input 2
    rig.e.edit(|d| {
        d.set_track_input(
            &id,
            TrackInput {
                channel: 0,
                stereo: true,
            },
        )
    });
    run_with_input(&mut rig, &mut feed, 0.3);
    assert!(tail(&rig.left) > 0.2 && tail(&rig.right) < -0.1);

    rig.e.edit(|d| d.set_track_armed(&id, false));
    run_with_input(&mut rig, &mut feed, 0.3);
    assert!(tail(&rig.left).abs() < 1e-3, "disarmed: silent again");
    // removing the armed track leaves nothing behind
    rig.e.edit(|d| d.set_track_armed(&id, true));
    rig.e.edit(|d| d.remove_track(&id));
    run_with_input(&mut rig, &mut feed, 0.2);
    assert_eq!(tail(&rig.left), 0.0);
}

#[test]
fn keys_play_the_armed_synth_tracks_else_the_live_synth() {
    let mut rig = Rig::new();
    let targets = rig.e.synth_targets();
    let live = rig.e.live_synth_slot();
    let mut seen = Vec::new();
    targets.for_each(|s| seen.push(s));
    assert_eq!(seen, vec![live]);

    let a = rig.e.edit(|d| d.add_track(InstrumentType::Synth, 1).id);
    let b = rig.e.edit(|d| d.add_track(InstrumentType::Synth, 2).id);
    let audio = rig.e.edit(|d| d.add_track(InstrumentType::Audio, 0).id);
    rig.e.edit(|d| {
        d.set_track_armed(&a, true);
        d.set_track_armed(&b, true);
        d.set_track_armed(&audio, true);
    });
    let (sa, sb) = (rig.e.track_slot(&a).unwrap(), rig.e.track_slot(&b).unwrap());
    assert_eq!(targets.armed(), vec![sa, sb], "audio tracks take no keys");
    assert_eq!(rig.e.doc().get_armed_track_id(), Some(b.as_str()));

    // an instrument change rebuilds the track in (possibly) another slot: the keys follow
    rig.e.edit(|d| d.set_track_armed(&a, false));
    assert_eq!(targets.armed(), vec![sb]);
    rig.e.edit(|d| d.remove_track(&b));
    assert!(targets.armed().is_empty());
    assert_eq!(rig.e.doc().armed_track_ids(), vec![audio.as_str()]);
}

#[test]
fn song_recording_records_the_armed_audio_tracks_input_from_the_transport_start() {
    let mut rig = Rig::new();
    let mut feed = feed_input(&mut rig);
    let id = rig.e.edit(|d| d.add_track(InstrumentType::Audio, 0).id);
    rig.e.edit(|d| d.set_track_armed(&id, true));
    // prime the input, then record 0.5 s
    run_with_input(&mut rig, &mut feed, 0.1);
    rig.e.set_cue(2.0);
    rig.e.start_recording_to_track(None);
    assert_eq!(rig.e.get_state(), TransportState::Recording);
    assert_eq!(rig.e.recording_audio_tracks(), vec![(id.as_str(), 2.0)]);
    run_with_input(&mut rig, &mut feed, 0.5);
    assert!(!rig.e.live_take_peaks(&id).is_empty());
    assert_eq!(
        rig.e.stop_recording_to_track(),
        None,
        "no note track: no region here"
    );
    assert_eq!(rig.e.get_state(), TransportState::Stopped);

    let takes = rig.e.take_recorded_audio();
    assert_eq!(takes.len(), 1);
    let t = &takes[0];
    assert_eq!(
        (t.track_id.as_str(), t.start_beat, t.sample_rate),
        (id.as_str(), 2.0, SR)
    );
    assert_eq!(t.channels.len(), 1, "a mono input records one channel");
    let frames = t.channels[0].len() as f64;
    assert!(
        (frames - 0.5 * SR as f64).abs() <= 256.0,
        "about half a second ({frames} frames)"
    );
    assert!(
        t.channels[0].iter().all(|v| (*v - 0.5).abs() < 1e-6),
        "the dry input"
    );
    assert_eq!(t.dropped_frames, 0);
    assert!(rig.e.take_recorded_audio().is_empty());
    assert!(rig.e.recording_audio_tracks().is_empty());
}

#[test]
fn latency_compensation_moves_a_take_earlier() {
    let take_len = |latency: f64| {
        let mut rig = Rig::new();
        let mut feed = feed_input(&mut rig);
        let id = rig.e.edit(|d| d.add_track(InstrumentType::Audio, 0).id);
        rig.e.edit(|d| {
            d.set_track_armed(&id, true);
            d.set_track_input(
                &id,
                TrackInput {
                    channel: 0,
                    stereo: true,
                },
            );
        });
        rig.e.set_record_latency(latency);
        run_with_input(&mut rig, &mut feed, 0.1);
        rig.e.start_recording_to_track(None);
        run_with_input(&mut rig, &mut feed, 0.25);
        rig.e.stop();
        let t = rig.e.take_recorded_audio().remove(0);
        assert_eq!(t.channels.len(), 2);
        assert!(t.channels[1].iter().all(|v| (*v + 0.25).abs() < 1e-6));
        t.channels[0].len()
    };
    // The first 10 ms captured after the transport start were played before it: cut. The last
    // thing captured was played 10 ms before the stop, so the take is that much shorter.
    assert_eq!(take_len(0.0) - take_len(0.010), 480);
}

#[test]
fn a_note_track_and_audio_tracks_record_together_and_the_song_plays_meanwhile() {
    let mut rig = Rig::new();
    let mut feed = feed_input(&mut rig);
    let drums = rig.drum_track();
    // a pattern hit on beat 1 that must be heard while recording
    rig.e.edit(|d| d.toggle_step(&drums, 0, 4, 0.25, None));
    let synth = rig.e.edit(|d| d.add_track(InstrumentType::Synth, 1).id);
    let audio = rig.e.edit(|d| d.add_track(InstrumentType::Audio, 0).id);
    rig.e.edit(|d| {
        d.set_track_armed(&synth, true);
        d.set_track_armed(&audio, true);
        d.set_track_mute(&audio, true);
    });
    run_with_input(&mut rig, &mut feed, 0.1);
    rig.e.start_recording_to_track(None);
    assert_eq!(rig.e.get_recording_track_id(), Some(synth.as_str()));
    let t0 = rig.now();
    rig.e.capture_note_on(60.0, 0.8, None);
    run_with_input(&mut rig, &mut feed, 0.2);
    rig.e.capture_note_off(60.0, None);
    run_with_input(&mut rig, &mut feed, 0.6);
    assert!(
        rig.pattern_hits()
            .iter()
            .any(|(t, _, at)| *t == drums && *at > t0),
        "the drum pattern plays during the recording"
    );
    let region = rig.e.stop_recording_to_track().expect("the notes' region");
    assert_eq!(region.notes.len(), 1);
    assert_eq!(rig.e.take_recorded_audio().len(), 1);
}

#[test]
fn a_plugin_instrument_plays_the_tracks_notes_and_the_keys() {
    let mut rig = Rig::new();
    let plugins = TestPlugins::default();
    let logs = plugins.logs.clone();
    rig.e.set_plugin_factory(Box::new(plugins));
    let id = rig.e.edit(|d| d.add_track(InstrumentType::Synth, 3).id);
    unity(&mut rig, &id);
    rig.e
        .edit(|d| d.set_track_plugin_instrument(&id, Some(plugin("inst", "tone"))));
    assert_eq!(rig.e.doc().get_tracks()[0].name, "tone");
    rig.e.preview_note(&id, 64.0, 0.8, 0.1);
    rig.run(0.05);
    assert!(tail(&rig.left) > 0.1, "the plugin's output reaches the mix");
    rig.run(0.2);
    assert!(
        tail(&rig.left).abs() < 1e-3,
        "released after the preview length"
    );
    let log = logs.lock().unwrap().last().unwrap().1.clone();
    let midi: Vec<Call> = log
        .lock()
        .unwrap()
        .iter()
        .filter(|c| matches!(c, Call::Midi(..)))
        .cloned()
        .collect();
    assert!(matches!(midi[0], Call::Midi(0x90, 64, 102, _)));
    assert!(matches!(midi[1], Call::Midi(0x80, 64, 0, _)));

    // a plugin that is not installed: the built-in preset plays instead
    rig.e
        .edit(|d| d.set_track_plugin_instrument(&id, Some(plugin("gone", "missing"))));
    rig.e.preview_note(&id, 64.0, 0.8, 0.1);
    rig.run(0.05);
    assert!(rig.rms(rig.now() - 0.04, rig.now()) > 0.0);
    // and back to the preset
    rig.e.edit(|d| d.set_track_plugin_instrument(&id, None));
    assert!(rig.e.doc().get_tracks()[0].instrument.plugin.is_none());
}

#[test]
fn export_renders_plugins_with_fresh_offline_processors() {
    let mut rig = Rig::new();
    let id = rig.e.edit(|d| d.add_track(InstrumentType::Synth, 3).id);
    rig.e.edit(|d| {
        d.set_track_volume(&id, 0.0);
        d.set_track_plugin_instrument(&id, Some(plugin("inst", "tone")));
        d.add_track_plugin(&id, plugin("fx", "gain"));
        d.add_track_plugin(&id, plugin("off", "gain"));
        d.set_track_plugin_bypassed(&id, "off", true);
        let notes = vec![mdaw_model::RegionNote {
            note: 60.0,
            velocity: 0.8,
            start_beat: 0.0,
            duration_beats: 1.0,
            is_drum: false,
        }];
        d.add_region(
            &id,
            mdaw_model::Region {
                id: "r".into(),
                start_beat: 0.0,
                length_beats: 4.0,
                notes,
                color: None,
                name: None,
                audio: None,
                extra: Default::default(),
            },
        );
    });
    let mut plugins = TestPlugins::default();
    let offline = plugins.offline.clone();
    let out = render_export_with(
        rig.e.doc().get_arrangement(),
        SR,
        rig.e.cache(),
        &*rig.samples,
        &mut plugins,
    )
    .unwrap();
    assert_eq!(*offline.lock().unwrap(), vec!["fx", "inst"]);
    // 0.25 from the instrument, halved by the insert, through the strip and the master bus
    let at = (0.25 * SR as f64) as usize;
    assert!(out.channels[0][at] > 0.05, "{}", out.channels[0][at]);
    let after = (0.75 * SR as f64) as usize;
    assert!(out.channels[0][after].abs() < 1e-3, "the note ended");
}
