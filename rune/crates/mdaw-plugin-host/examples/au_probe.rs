//! Lists the installed plugins, or loads one and renders a few blocks through it.
//!
//!   cargo run -p mdaw-plugin-host --example au_probe                 # list
//!   cargo run -p mdaw-plugin-host --example au_probe -- "Guitar Rig" # load + render + state

use mdaw_plugin_host::{Instance, PluginKind, scan};

fn main() {
    let plugins = scan();
    let Some(query) = std::env::args().nth(1) else {
        for p in &plugins {
            let (t, s, m) = p.id.codes();
            println!("{:?}\t{t} {s} {m}\t{}: {}", p.kind, p.vendor, p.name);
        }
        println!("{} plugins", plugins.len());
        return;
    };
    let q = query.to_lowercase();
    let Some(p) = plugins.iter().find(|p| p.name.to_lowercase().contains(&q)) else {
        eprintln!("no plugin matches {query:?}");
        std::process::exit(1);
    };
    println!("opening {}: {} ({:?})", p.vendor, p.name, p.kind);
    let t0 = std::time::Instant::now();
    let inst = match Instance::open(p.id, 48_000.0) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("open failed: {e}");
            std::process::exit(2);
        }
    };
    println!("opened in {:?}", t0.elapsed());
    println!("latency {:.2} ms", inst.latency_seconds() * 1000.0);
    println!("has editor: {}", inst.has_editor());
    let mut r = inst.renderer();
    let mut peak = 0f32;
    let (mut l, mut rr) = ([0f32; 128], [0f32; 128]);
    if p.kind == PluginKind::Instrument {
        r.midi(0x90, 60, 100, 0);
    }
    let t0 = std::time::Instant::now();
    for block in 0..750 {
        for i in 0..128 {
            let v = if p.kind == PluginKind::Effect {
                let t = (block * 128 + i) as f32 / 48_000.0;
                0.2 * (t * 220.0 * std::f32::consts::TAU).sin()
            } else {
                0.0
            };
            l[i] = v;
            rr[i] = v;
        }
        match p.kind {
            PluginKind::Effect => r.process_effect(&mut l, &mut rr),
            PluginKind::Instrument => r.render_instrument(&mut l, &mut rr),
        }
        peak = l.iter().chain(rr.iter()).fold(peak, |m, v| m.max(v.abs()));
    }
    println!(
        "rendered 2 s in {:?}: peak {peak:.4}, {} failed blocks",
        t0.elapsed(),
        r.render_errors()
    );
    match inst.state() {
        Some(s) => {
            let again = inst.state();
            println!(
                "state: {} bytes, a second read is {}",
                s.len(),
                if again.as_deref() == Some(s.as_slice()) {
                    "identical"
                } else {
                    "DIFFERENT"
                }
            );
            println!("restore: {:?}", inst.set_state(&s));
        }
        None => println!("state: none"),
    }
    if std::env::args().nth(2).as_deref() == Some("--editor") {
        match inst.open_editor(&p.name) {
            Ok(w) => println!("editor open: {}", w.is_open()),
            Err(e) => println!("editor failed: {e}"),
        }
    }
}
