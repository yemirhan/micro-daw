//! `MDAW_SMOKE=1 micro-daw`: a scripted run of the real window app (the phase-7 smoke check):
//! starts the session, hits pad A1 through the MIDI router and the real output device, plays
//! the pattern, prints the device, its callback count and the master meter's peak, then quits.
//! `MDAW_SMOKE=song` then opens the rock-song template in the Song view and plays it (phase 8);
//! `MDAW_SMOKE=views` goes through the phase-9 pages and writes a WAV and a MIDI export.

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
                let views = std::env::var("MDAW_SMOKE").as_deref() == Ok("views");
                sm.peak = 0.0;
                sm.step = if song {
                    5
                } else if views {
                    10
                } else {
                    4
                };
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
                } else if !views {
                    tx.quit();
                }
            }
            // Phase 9 (`MDAW_SMOKE=views`): every page, a generated + processed sample, a kit
            // preview, the palette and dialogs, then WAV + MIDI exports written to the data dir.
            10 if t > 4.5 => {
                sm.step = 11;
                self.switch_mode(crate::app::AppMode::Samples);
                self.generate_sample();
                eprintln!("[smoke] samples: generating a 440 Hz sine");
            }
            11 if t > 5.5 => {
                sm.step = 12;
                let n = self.p9.samples.entries.len();
                eprintln!(
                    "[smoke] samples: {n} in the library, selected {:?}",
                    self.p9
                        .samples
                        .selected_entry()
                        .map(|e| e.sample.name.clone())
                );
                self.p9.process.reverb.enabled = true;
                self.p9.process.filter.enabled = true;
                self.process_sample();
            }
            12 if t > 7.0 => {
                sm.step = 13;
                eprintln!(
                    "[smoke] samples: {:?}",
                    self.p9
                        .samples
                        .entries
                        .iter()
                        .map(|e| format!("{} {:.2}s", e.sample.name, e.sample.duration_seconds))
                        .collect::<Vec<_>>()
                );
                sm.peak = 0.0;
                self.switch_mode(crate::app::AppMode::Packs);
                self.preview_kit("builtin:tr808", "tr808");
                eprintln!("[smoke] packs: previewing the 808 groove");
            }
            13 if t > 9.5 => {
                sm.step = 14;
                let peak = sm.peak;
                eprintln!(
                    "[smoke] packs: kit preview playing={:?} output peak={peak:.4}",
                    self.p9.kit_preview.playing,
                );
                self.stop_kit_preview();
                for m in [
                    crate::app::AppMode::Dev,
                    crate::app::AppMode::Settings,
                    crate::app::AppMode::Daw,
                ] {
                    self.switch_mode(m);
                }
                self.open_dialog(crate::views::Dialog::Palette);
                self.close_dialog();
                let v: serde_json::Value =
                    serde_json::from_str(crate::scenes_song::ROCK_SONG).expect("template");
                let arr: mdaw_model::Arrangement =
                    serde_json::from_value(v["arrangement"].clone()).expect("template");
                let _ = self.svc.engine.edit(|d| d.load_from_template(&arr));
                self.view = crate::app::DawView::Song;
                self.open_dialog(crate::views::Dialog::Export);
                let dir = crate::services::data_dir();
                self.export_to(crate::views::ExportKind::Wav, dir.join("smoke-export.wav"));
                eprintln!("[smoke] export: rendering the rock-song template to WAV");
            }
            14 if t > 12.0 && self.p9.export.running.is_none() => {
                sm.step = 15;
                let dir = crate::services::data_dir();
                let wav = dir.join("smoke-export.wav");
                eprintln!(
                    "[smoke] export: {:?} written={:?} bytes={:?} error={:?}",
                    wav,
                    self.p9.export.written,
                    std::fs::metadata(&wav).map(|m| m.len()).ok(),
                    self.p9.export.error
                );
                self.export_to(crate::views::ExportKind::Midi, dir.join("smoke-export.mid"));
            }
            15 if t > 12.5 && self.p9.export.running.is_none() => {
                sm.step = 16;
                let mid = crate::services::data_dir().join("smoke-export.mid");
                eprintln!(
                    "[smoke] export: {:?} bytes={:?}",
                    mid,
                    std::fs::metadata(&mid).map(|m| m.len()).ok()
                );
                self.close_dialog();
                tx.quit();
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
