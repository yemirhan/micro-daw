//! Hot-plug of a port owned by **another process** (phase-5 review finding 12): a child process
//! (this test binary, re-run as `child_virtual_port`) creates a virtual MIDI source; the host
//! must see it appear, receive from it, and see it disappear when the child exits, with the
//! held note released. On macOS this needs the CoreMIDI run loop of the process's first client
//! to be pumped (`notify.rs`): the host is enabled before anything else touches CoreMIDI here.
//!
//! Needs a MIDI backend (CoreMIDI / ALSA), so both tests are `#[ignore]`d; run with
//! `cargo test -p mdaw-midi --test hotplug_external -- --ignored --nocapture`. Without MIDI the
//! test fails instead of passing silently.

#![cfg(any(target_os = "macos", target_os = "linux"))]

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use mdaw_engine::buffers::PadBufferCache;
use mdaw_engine::engine::{ArrangementEngine, NoSamples};
use mdaw_midi::build;
use mdaw_model::controller::MemoryProfileStore;

const CHILD_ENV: &str = "MDAW_CHILD_PORT";

/// The child: creates the virtual port named by `MDAW_CHILD_PORT`, prints READY, sends a note
/// on SEND, exits on QUIT / EOF. Does nothing in a normal run.
#[test]
#[ignore = "helper process of external_process_hotplug"]
fn child_virtual_port() {
    use midir::MidiOutput;
    use midir::os::unix::VirtualOutput;
    let Ok(name) = std::env::var(CHILD_ENV) else {
        return;
    };
    let out = MidiOutput::new("mdaw-child").expect("child: no MIDI");
    let mut port = out.create_virtual(&name).expect("child: no virtual port");
    println!("READY");
    std::io::stdout().flush().unwrap();
    for line in std::io::stdin().lock().lines() {
        match line.as_deref() {
            Ok("SEND") => {
                port.send(&[0x90, 64, 100]).unwrap();
                println!("SENT");
                std::io::stdout().flush().unwrap();
            }
            _ => break,
        }
    }
}

#[test]
#[ignore = "needs a MIDI backend (CoreMIDI / ALSA); run with --ignored"]
fn external_process_hotplug() {
    let (engine, _core, live) = ArrangementEngine::new(
        48_000,
        PadBufferCache::with_fs(48_000, None),
        Arc::new(NoSamples),
    );
    let epoch = Instant::now();
    let (mut host, _ui) = build(&engine, live, MemoryProfileStore::default(), move || {
        epoch.elapsed().as_secs_f64() * 1000.0
    });
    let woke = Arc::new(AtomicUsize::new(0));
    let w = woke.clone();
    host.set_poll_interval(Duration::from_millis(100));
    assert!(host.enable(None), "no MIDI backend on this machine");
    // set after enable: the shared waker slot still reaches the watcher
    host.set_waker(Arc::new(move || {
        w.fetch_add(1, Ordering::Relaxed);
    }));
    let name = format!("mdaw-ext-{}", std::process::id());
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["child_virtual_port", "--exact", "--ignored", "--nocapture"])
        .env(CHILD_ENV, &name)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn the child process");
    let mut stdin = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut wait_line = |want: &str| loop {
        let l = lines.next().expect("child exited").unwrap();
        if l.trim() == want {
            break;
        }
    };
    wait_line("READY");
    let created = Instant::now();
    let find = |h: &mdaw_midi::MidiHost<MemoryProfileStore>| {
        h.devices()
            .iter()
            .find(|d| d.name.contains(&name))
            .map(|d| d.id.clone())
    };
    let mut id = None;
    while id.is_none() && created.elapsed() < Duration::from_secs(5) {
        host.poll();
        id = find(&host);
        std::thread::sleep(Duration::from_millis(2));
    }
    let id = id.expect("the external port never appeared");
    let appeared = created.elapsed();
    assert!(woke.load(Ordering::Relaxed) > 0, "the UI was never woken");
    host.select_device(&id);
    assert!(host.is_connected(), "{:?}", host.connection());
    writeln!(stdin, "SEND").unwrap();
    wait_line("SENT");
    let t = Instant::now();
    while host.shared().lock().router.sounding_notes().is_empty()
        && t.elapsed() < Duration::from_secs(3)
    {
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(host.shared().lock().router.sounding_notes(), vec![64]);
    // the child exits with the key held: its port disappears
    drop(stdin);
    let _ = child.wait();
    let gone_at = Instant::now();
    while find(&host).is_some() && gone_at.elapsed() < Duration::from_secs(5) {
        host.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    let gone = gone_at.elapsed();
    assert!(find(&host).is_none(), "the external port never disappeared");
    assert_ne!(host.active_device_id(), Some(id.as_str()));
    assert!(host.shared().lock().router.sounding_notes().is_empty());
    eprintln!(
        "external port: listed after {:.1} ms, removal seen after {:.1} ms (poll interval 100 ms; \
         CoreMIDI notifications: {})",
        appeared.as_secs_f64() * 1e3,
        gone.as_secs_f64() * 1e3,
        mdaw_midi::notify::events().changes()
    );
    host.dispose();
}
