//! `MidiHost` against real midir ports (in-process virtual sources; CoreMIDI / ALSA):
//! - the last device persisted on disk and restored at the next start
//!   (`settings::enable_with_settings`), with the setting on / off and the device absent
//!   (phase-5 review finding 8);
//! - connection health: a failed open is reported to the listeners and retried on the next
//!   hot-plug change; a source replaced under the same name is reconnected (finding 9).
//!
//! Needs a MIDI backend, so every test is `#[ignore]`d; run with
//! `cargo test -p mdaw-midi --test host_virtual -- --ignored --test-threads=1`. Without MIDI they
//! fail instead of passing silently.

#![cfg(any(target_os = "macos", target_os = "linux"))]

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use mdaw_engine::buffers::PadBufferCache;
use mdaw_engine::engine::{ArrangementEngine, NoSamples};
use mdaw_midi::devices::{is_mpk_mini, normalize_port_name};
use mdaw_midi::settings::{enable_with_settings, read_last_device};
use mdaw_midi::{Connection, MidiHost, MidiUi, build};
use mdaw_model::controller::MemoryProfileStore;
use mdaw_model::settings::{SettingsStore, update_app_settings};
use midir::os::unix::VirtualOutput;
use midir::{MidiOutput, MidiOutputConnection};
use serde_json::json;

fn host() -> (MidiHost<MemoryProfileStore>, MidiUi, ArrangementEngine) {
    // as the app does first thing: the process's first CoreMIDI client on a pumped run loop
    mdaw_midi::notify::ensure_started();
    let (engine, _core, live) = ArrangementEngine::new(
        48_000,
        PadBufferCache::with_fs(48_000, None),
        Arc::new(NoSamples),
    );
    let epoch = Instant::now();
    let (mut host, ui) = build(&engine, live, MemoryProfileStore::default(), move || {
        epoch.elapsed().as_secs_f64() * 1000.0
    });
    host.set_poll_interval(Duration::from_millis(20));
    (host, ui, engine)
}

fn port(name: &str) -> MidiOutputConnection {
    mdaw_midi::notify::ensure_started();
    MidiOutput::new("mdaw-host-test")
        .expect("no MIDI backend on this machine")
        .create_virtual(name)
        .expect("virtual ports unsupported")
}

fn id_of(h: &MidiHost<MemoryProfileStore>, name: &str) -> Option<String> {
    h.devices()
        .iter()
        .find(|d| normalize_port_name(&d.name).ends_with(name))
        .map(|d| d.id.clone())
}

/// Poll (re-enumerating) until `f` holds or 3 s passed.
fn settle(h: &mut MidiHost<MemoryProfileStore>, f: impl Fn(&MidiHost<MemoryProfileStore>) -> bool) {
    let t = Instant::now();
    while !f(h) && t.elapsed() < Duration::from_secs(3) {
        h.poll();
        h.refresh();
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The TS's automatic choice: an MPK mini, else the first input.
fn ts_choice(h: &MidiHost<MemoryProfileStore>) -> Option<String> {
    let d = h.devices();
    d.iter()
        .find(|d| is_mpk_mini(&d.name))
        .or_else(|| d.first())
        .map(|d| d.id.clone())
}

#[test]
#[ignore = "needs a MIDI backend (CoreMIDI / ALSA); run with --ignored"]
fn the_last_device_survives_a_restart_on_disk() {
    let pid = std::process::id();
    let (a, b) = (
        format!("mdaw-restart-a-{pid}"),
        format!("mdaw-restart-b-{pid}"),
    );
    let _pa = port(&a);
    let pb = port(&b);
    let dir = std::env::temp_dir().join(format!("mdaw-midi-restart-{pid}"));
    let _ = std::fs::remove_dir_all(&dir);
    let open = || Rc::new(RefCell::new(SettingsStore::open(&dir)));

    // first start: the user picks b explicitly → written to disk
    let (mut h1, _, _) = host();
    assert!(enable_with_settings(&mut h1, open()));
    settle(&mut h1, |h| id_of(h, &b).is_some());
    let b_id = id_of(&h1, &b).unwrap();
    h1.select_device(&b_id);
    assert!(h1.is_connected());
    let stored = read_last_device(&SettingsStore::open(&dir)).expect("persisted");
    assert_eq!(stored.id, b_id);
    assert!(stored.name.ends_with(&b));
    drop(h1);

    // restart, setting on (default): b is restored even though it is neither first nor an MPK
    let (mut h2, _, _) = host();
    assert!(enable_with_settings(&mut h2, open()));
    assert_eq!(h2.active_device_id(), Some(b_id.as_str()));
    drop(h2);

    // setting off: the TS behaviour (MPK, else first input), the stored device ignored
    update_app_settings(
        &mut SettingsStore::open(&dir),
        &json!({"midi": {"autoConnectLastDevice": false}}),
    )
    .unwrap();
    let (mut h3, _, _) = host();
    assert!(enable_with_settings(&mut h3, open()));
    assert_eq!(h3.active_device_id().map(str::to_owned), ts_choice(&h3));
    drop(h3);
    update_app_settings(
        &mut SettingsStore::open(&dir),
        &json!({"midi": {"autoConnectLastDevice": true}}),
    )
    .unwrap();

    // restart with b absent: the fallback is used and not persisted; b reattaches when it
    // comes back (a new port: new id, same name)
    drop(pb);
    let (mut h4, _, _) = host();
    assert!(enable_with_settings(&mut h4, open()));
    settle(&mut h4, |h| id_of(h, &b).is_none());
    assert_ne!(h4.active_device_id(), Some(b_id.as_str()));
    assert_eq!(
        read_last_device(&SettingsStore::open(&dir)).unwrap().id,
        b_id
    );
    let _pb2 = port(&b);
    settle(&mut h4, |h| {
        id_of(h, &b).is_some_and(|id| h.active_device_id() == Some(id.as_str()))
    });
    let b2 = id_of(&h4, &b).unwrap();
    assert_eq!(h4.active_device_id(), Some(b2.as_str()));
    assert!(h4.is_connected());
    eprintln!("restored {b} (id {b_id}), re-attached as id {b2} after a replug");
    h4.dispose();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
#[ignore = "needs a MIDI backend (CoreMIDI / ALSA); run with --ignored"]
fn a_failed_connection_is_reported_and_retried_and_a_replaced_source_reconnects() {
    let name = format!("mdaw-health-{}", std::process::id());
    let (mut h, _, _) = host();
    let seen = Rc::new(RefCell::new(Vec::<Connection>::new()));
    let s = seen.clone();
    h.on_connection_change(Box::new(move |c| s.borrow_mut().push(c.clone())));
    let p = port(&name);
    assert!(h.enable(None));
    settle(&mut h, |h| id_of(h, &name).is_some());
    let id = id_of(&h, &name).unwrap();
    h.select_device(&id);
    assert!(h.is_connected());

    // the port vanishes before the host re-enumerated: a (re)select fails, and says why
    drop(p);
    std::thread::sleep(Duration::from_millis(50));
    assert!(!h.reconnect());
    assert!(
        matches!(h.connection(), Connection::Failed { error, .. } if !error.is_empty()),
        "{:?}",
        h.connection()
    );
    assert!(
        seen.borrow()
            .iter()
            .any(|c| matches!(c, Connection::Failed { .. }))
    );

    // it comes back: the next hot-plug change retries and connects (by name if the id changed)
    let mut p = port(&name);
    settle(&mut h, |h| h.is_connected());
    assert!(h.is_connected(), "{:?}", h.connection());
    let active = h.active_device_id().unwrap().to_owned();

    // replaced under the same name between two polls (CoreMIDI gives the new source the same
    // id, so the port list does not change): the connection must not stay deaf
    let removed_before = mdaw_midi::notify::events().sources_removed();
    drop(p);
    p = port(&name);
    let t = Instant::now();
    while t.elapsed() < Duration::from_millis(300) {
        h.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
    eprintln!(
        "replacement: same id {}, sources removed +{}",
        id_of(&h, &name).as_deref() == Some(active.as_str()),
        mdaw_midi::notify::events().sources_removed() - removed_before
    );
    p.send(&[0x90, 61, 100]).unwrap();
    let t = Instant::now();
    while h.shared().lock().router.sounding_notes().is_empty()
        && t.elapsed() < Duration::from_secs(2)
    {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        h.shared().lock().router.sounding_notes(),
        vec![61],
        "connection deaf after the replacement (was {active}, now {:?})",
        h.active_device_id()
    );
    eprintln!("connection events: {:?}", seen.borrow());
    h.dispose();
}
