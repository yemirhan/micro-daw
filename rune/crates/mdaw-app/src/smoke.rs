//! `MDAW_SMOKE=1 micro-daw`: a scripted run of the real window app (the phase-7 smoke check):
//! starts the session, hits pad A1 through the MIDI router and the real output device, plays
//! the pattern, prints the device, its callback count and the master meter's peak, then quits.
//! `MDAW_SMOKE=song` then opens the rock-song template in the Song view and plays it (phase 8).

use rune::Tx;

use crate::app::{App, StartPhase};
use crate::services::Driver;

#[derive(Debug, Default)]
pub struct Smoke {
    step: u32,
    peak: f64,
}

pub fn enabled() -> bool {
    std::env::var_os("MDAW_SMOKE").is_some()
}

impl App {
    pub fn smoke(&mut self, t: f64, tx: &mut Tx) {
        let Some(sm) = self.smoke.as_mut() else {
            return;
        };
        let level = f32::from_bits(
            crate::services::OUTPUT_PEAK.swap(0, std::sync::atomic::Ordering::Relaxed),
        ) as f64;
        if sm.step >= 2 {
            sm.peak = sm.peak.max(level);
        }
        match sm.step {
            0 if t > 0.5 => {
                sm.step = 1;
                self.start_session();
            }
            1 if t > 1.2 => {
                sm.step = 2;
                eprintln!("[smoke] pad A1 hit");
                self.svc.drum_hit(0, 1.0);
            }
            2 if t > 2.0 => {
                sm.step = 3;
                let peak = sm.peak;
                eprintln!("[smoke] device output peak after the pad hit: {peak:.4}");
                sm.peak = 0.0;
                self.toggle_step(0, 0, 0.25);
                self.toggle_step(2, 2, 0.25);
                self.toggle_play();
            }
            3 if t > 4.0 => {
                let peak = sm.peak;
                let song = std::env::var("MDAW_SMOKE").as_deref() == Ok("song");
                sm.peak = 0.0;
                sm.step = if song { 5 } else { 4 };
                if let Driver::Device(h) = &self.svc.driver {
                    eprintln!("[smoke] device {:?}", h.info());
                    eprintln!("[smoke] stats {:?}", h.stats());
                }
                eprintln!(
                    "[smoke] started={} transport={:?} position={:.2} pattern output peak={peak:.4} midi devices={:?}",
                    self.start == StartPhase::Done,
                    self.transport(),
                    self.svc.engine.get_position(),
                    self.svc
                        .midi
                        .host
                        .devices()
                        .iter()
                        .map(|d| d.name.clone())
                        .collect::<Vec<_>>()
                );
                self.toggle_play();
                if song {
                    // Phase 8: the Song view plays the rock-song template.
                    let v: serde_json::Value =
                        serde_json::from_str(crate::scenes_song::ROCK_SONG).expect("template");
                    let arr: mdaw_model::Arrangement =
                        serde_json::from_value(v["arrangement"].clone()).expect("template");
                    let _ = self.svc.engine.edit(|d| d.load_from_template(&arr));
                    self.view = crate::app::DawView::Song;
                    self.song_play();
                    eprintln!("[smoke] song view: rock-song playing");
                } else {
                    tx.quit();
                }
            }
            5 if t > 8.0 => {
                sm.step = 6;
                let peak = sm.peak;
                eprintln!(
                    "[smoke] song transport={:?} position={:.2} tracks={} song output peak={peak:.4}",
                    self.song_state(),
                    self.svc.engine.get_position(),
                    self.tracks().len()
                );
                self.song_stop();
                tx.quit();
            }
            _ => {}
        }
        tx.wake_after(std::time::Duration::from_millis(16));
    }
}
