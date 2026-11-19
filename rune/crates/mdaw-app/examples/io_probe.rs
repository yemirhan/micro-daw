//! A scripted run of the track I/O path on the real devices and the installed plugins (no
//! window): arm an audio track, open the input, put the first installed effect on it, record a
//! take, play the first installed instrument, read the plugin states back, export.
//!
//!   MDAW_DATA_DIR=/tmp/mdaw-probe cargo run -p mdaw-app --example io_probe [effect] [instrument]
//!
//! The output is turned down to −40 dB for the run.

use std::time::{Duration, Instant};

use mdaw_app::plugins::slot_for;
use mdaw_app::services::{RunMode, Services};
use mdaw_engine::engine::render_export_with;
use mdaw_model::InstrumentType;
use mdaw_model::settings::SettingsStore;
use mdaw_plugin_host::PluginKind;

fn pump(svc: &mut Services, t0: Instant, seconds: f64) {
    let end = Instant::now() + Duration::from_secs_f64(seconds);
    while Instant::now() < end {
        svc.advance(t0.elapsed().as_secs_f64());
        svc.pump_audio_io();
        std::thread::sleep(Duration::from_millis(8));
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let t0 = Instant::now();
    let mut svc = Services::new(RunMode::Window, SettingsStore::in_memory());
    svc.engine.set_master_volume_db(-40.0);
    let rate = svc.engine.sample_rate();
    println!("context rate {rate} Hz");
    let (effect, instrument) = {
        let m = svc.plugins.0.borrow();
        for p in m.catalog() {
            println!("  installed: {:?} {}: {}", p.kind, p.vendor, p.name);
        }
        let pick = |kind: PluginKind, want: Option<&String>| {
            m.of_kind(kind)
                .into_iter()
                .find(|p| want.is_none_or(|w| p.name.to_lowercase().contains(&w.to_lowercase())))
                .map(slot_for)
        };
        (
            pick(PluginKind::Effect, args.first()),
            pick(PluginKind::Instrument, args.get(1)),
        )
    };

    // --- an armed audio track opens the input
    let audio = svc
        .engine
        .edit(|d| d.add_track(InstrumentType::Audio, 0).id);
    svc.engine.edit(|d| d.set_track_armed(&audio, true));
    pump(&mut svc, t0, 1.5);
    println!(
        "input: {:?} error {:?}",
        svc.audio_io.info, svc.audio_io.error
    );
    println!("engine input channels: {}", svc.engine.input_channels());
    if let Some(stats) = svc.engine.input_stats().cloned() {
        pump(&mut svc, t0, 0.5);
        let peaks: Vec<String> = (0..svc.engine.input_channels())
            .map(|c| format!("{:.5}", stats.take_peak(c)))
            .collect();
        println!(
            "input block {} frames, underruns {}, dropped {}, peaks [{}]",
            stats.block_frames(),
            stats.underruns(),
            stats.dropped_frames(),
            peaks.join(", ")
        );
    }
    println!(
        "record latency compensation: {:.2} ms",
        svc.engine.record_latency() * 1000.0
    );

    // --- an insert effect
    if let Some(slot) = effect {
        let name = slot.name.clone();
        let t = Instant::now();
        let id = svc
            .engine
            .edit(|d| d.add_track_plugin(&audio, slot))
            .unwrap();
        println!(
            "effect {name}: loaded {} in {:?}, error {:?}",
            svc.plugins.0.borrow().is_loaded(&id),
            t.elapsed(),
            svc.plugins.0.borrow().error(&id).map(str::to_owned)
        );
        pump(&mut svc, t0, 1.0);
        println!(
            "  track level through it: {:.1} dB",
            svc.engine.get_track_level(&audio)
        );
    }

    // --- record a take
    svc.engine.start_recording_to_track(None);
    pump(&mut svc, t0, 1.5);
    svc.engine.stop();
    pump(&mut svc, t0, 0.2);
    for take in svc.engine.take_recorded_audio() {
        let peak = take.channels[0].iter().fold(0f32, |m, v| m.max(v.abs()));
        println!(
            "take: {} ch, {} frames ({:.3} s), peak {peak:.5}, dropped {}",
            take.channels.len(),
            take.channels[0].len(),
            take.channels[0].len() as f64 / take.sample_rate as f64,
            take.dropped_frames
        );
    }

    // --- a plugin instrument played by a note
    let synth = svc
        .engine
        .edit(|d| d.add_track(InstrumentType::Synth, 0).id);
    if let Some(slot) = instrument {
        let name = slot.name.clone();
        let t = Instant::now();
        svc.engine
            .edit(|d| d.set_track_plugin_instrument(&synth, Some(slot)));
        let id = svc
            .engine
            .doc()
            .plugin_slots()
            .into_iter()
            .find(|(t, _)| *t == synth)
            .map(|(_, p)| p.id.clone())
            .unwrap_or_default();
        println!(
            "instrument {name}: loaded {} in {:?}, error {:?}",
            svc.plugins.0.borrow().is_loaded(&id),
            t.elapsed(),
            svc.plugins.0.borrow().error(&id).map(str::to_owned)
        );
        svc.engine.edit(|d| d.set_track_armed(&synth, true));
        println!(
            "  keys go to slots {:?}",
            svc.engine.synth_targets().armed()
        );
        svc.engine.preview_note(&synth, 60.0, 0.8, 1.0);
        let mut loudest = f64::NEG_INFINITY;
        for _ in 0..20 {
            pump(&mut svc, t0, 0.05);
            loudest = loudest.max(svc.engine.get_track_level(&synth));
        }
        println!("  a note's level: {loudest:.1} dB");
    }

    // --- states and export
    let states = {
        let doc = svc.engine.doc();
        let slots = doc.plugin_slots();
        svc.plugins.0.borrow_mut().capture_states(true, |id| {
            slots
                .iter()
                .find(|(_, p)| p.id == id)
                .map(|(_, p)| p.state.clone())
        })
    };
    for (id, state) in &states {
        println!("state of {id}: {} base64 chars", state.len());
    }
    for (id, state) in states {
        svc.engine.edit(|d| d.set_plugin_state(&id, Some(state)));
    }
    svc.engine.edit(|d| {
        d.add_region(
            &synth,
            mdaw_model::Region {
                id: "probe".into(),
                start_beat: 0.0,
                length_beats: 4.0,
                notes: vec![mdaw_model::RegionNote {
                    note: 60.0,
                    velocity: 0.8,
                    start_beat: 0.0,
                    duration_beats: 2.0,
                    is_drum: false,
                }],
                color: None,
                name: None,
                audio: None,
                extra: Default::default(),
            },
        )
    });
    let arr = svc.engine.doc().get_arrangement().clone();
    let mut plugins = svc.plugins.clone();
    let t = Instant::now();
    match render_export_with(
        &arr,
        rate,
        svc.engine.cache(),
        &*svc.sample_store,
        &mut plugins,
    ) {
        Ok(b) => {
            let peak = b.channels[0].iter().fold(0f32, |m, v| m.max(v.abs()));
            println!(
                "export: {:.2} s rendered in {:?}, peak {peak:.4}",
                b.frames() as f64 / rate as f64,
                t.elapsed()
            );
        }
        Err(e) => println!("export failed: {e}"),
    }
    println!("done");
}
