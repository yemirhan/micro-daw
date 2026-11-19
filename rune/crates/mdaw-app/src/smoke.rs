//! `MDAW_SMOKE=1 micro-daw`: a scripted run of the real window app (the phase-7 smoke check):
//! starts the session, hits pad A1 through the MIDI router and the real output device, plays
//! the pattern, prints the device, its callback count and the master meter's peak, then quits.
//! `MDAW_SMOKE=song` then opens the rock-song template in the Song view and plays it (phase 8);
//! `MDAW_SMOKE=views` goes through the phase-9 pages and writes a WAV and a MIDI export;
//! `MDAW_SMOKE=shell` drives the phase-10 close guard in the real window;
//! `MDAW_SMOKE=io` arms an audio track on the real input, hosts the first installed effect and
//! instrument plugins (their editor windows open), records a take and exports with them.

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
                let shell = std::env::var("MDAW_SMOKE").as_deref() == Ok("shell");
                let io = std::env::var("MDAW_SMOKE").as_deref() == Ok("io");
                sm.peak = 0.0;
                sm.step = if song {
                    5
                } else if views {
                    10
                } else if shell {
                    20
                } else if io {
                    30
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
                } else if !views && !shell && !io {
                    tx.quit();
                }
            }
            // Track I/O (`MDAW_SMOKE=io`): the real input, real plugins and their windows.
            30 if t > 4.5 => {
                sm.step = 31;
                self.view = crate::app::DawView::Song;
                self.add_song_track(mdaw_model::InstrumentType::Audio);
                let audio = self
                    .tracks()
                    .last()
                    .map(|t| t.id.clone())
                    .unwrap_or_default();
                self.toggle_arm(&audio);
                self.toggle_fx(&audio);
                // `MDAW_SMOKE_FX` / `MDAW_SMOKE_INSTRUMENT`: part of a plugin's name (else the
                // first installed one).
                let pick = |s: &App, kind: mdaw_plugin_host::PluginKind, var: &str| {
                    let want = std::env::var(var).unwrap_or_default().to_lowercase();
                    let m = s.svc.plugins.0.borrow();
                    m.of_kind(kind)
                        .iter()
                        .position(|p| p.name.to_lowercase().contains(&want))
                        .unwrap_or(0)
                };
                let fx = pick(self, mdaw_plugin_host::PluginKind::Effect, "MDAW_SMOKE_FX");
                self.add_track_plugin(&audio, fx);
                self.add_song_track(mdaw_model::InstrumentType::Synth);
                let synth = self
                    .tracks()
                    .last()
                    .map(|t| t.id.clone())
                    .unwrap_or_default();
                let inst = pick(
                    self,
                    mdaw_plugin_host::PluginKind::Instrument,
                    "MDAW_SMOKE_INSTRUMENT",
                );
                self.set_track_plugin_instrument(&synth, Some(inst));
                self.toggle_arm(&synth);
                let m = self.svc.plugins.0.borrow();
                let slots = self.svc.engine.doc().plugin_slots();
                for (_, p) in &slots {
                    eprintln!(
                        "[smoke] io: plugin {} loaded={} editor open={} error={:?}",
                        p.name,
                        m.is_loaded(&p.id),
                        m.editor_open(&p.id),
                        m.error(&p.id)
                    );
                }
            }
            31 if t > 6.5 => {
                sm.step = 32;
                eprintln!(
                    "[smoke] io: input {:?} error {:?}, engine input channels {}, keys → slots {:?}",
                    self.svc.audio_io.info,
                    self.svc.audio_io.error,
                    self.svc.engine.input_channels(),
                    self.svc.engine.synth_targets().armed()
                );
                self.song_record();
                eprintln!("[smoke] io: recording, state {:?}", self.song_state());
            }
            32 if t > 8.5 => {
                sm.step = 33;
                self.song_stop();
            }
            33 if t > 9.0 => {
                sm.step = 34;
                let audio = self
                    .tracks()
                    .iter()
                    .find(|t| t.instrument.kind == mdaw_model::InstrumentType::Audio)
                    .cloned();
                let region = audio.as_ref().and_then(|t| t.regions.last());
                eprintln!(
                    "[smoke] io: take region {:?}, record latency {:.1} ms, incomplete {}",
                    region.map(|r| (
                        r.name.clone(),
                        r.start_beat,
                        r.length_beats,
                        r.audio.as_ref().map(|a| a.sample_path.clone())
                    )),
                    self.svc.engine.record_latency() * 1000.0,
                    self.svc.engine.is_recording_incomplete()
                );
                let changed = self.sync_plugin_states();
                eprintln!(
                    "[smoke] io: plugin states read (changed {changed}): {:?}",
                    self.svc
                        .engine
                        .doc()
                        .plugin_slots()
                        .iter()
                        .map(|(_, p)| (p.name.clone(), p.state.as_ref().map(String::len)))
                        .collect::<Vec<_>>()
                );
                let dir = crate::services::data_dir();
                self.export_to(crate::views::ExportKind::Wav, dir.join("smoke-io.wav"));
            }
            34 if t > 10.0 && self.p9.export.running.is_none() => {
                sm.step = 35;
                let wav = crate::services::data_dir().join("smoke-io.wav");
                eprintln!(
                    "[smoke] io: export bytes={:?} error={:?}",
                    std::fs::metadata(&wav).map(|m| m.len()).ok(),
                    self.p9.export.error
                );
                // An undo of the take and a redo keep the plugins loaded.
                self.undo();
                self.redo();
                let m = self.svc.plugins.0.borrow();
                let loaded = self
                    .svc
                    .engine
                    .doc()
                    .plugin_slots()
                    .iter()
                    .filter(|(_, p)| m.is_loaded(&p.id))
                    .count();
                eprintln!("[smoke] io: plugins loaded after undo / redo: {loaded}");
                drop(m);
                tx.quit();
            }
            // Phase 9 (`MDAW_SMOKE=views`): every page, a designed sound played and sent to a
            // pad, a kit preview, the palette and dialogs, then WAV + MIDI exports written to
            // the data dir.
            10 if t > 4.5 => {
                sm.step = 11;
                self.switch_mode(crate::app::AppMode::Samples);
                self.pick_template(mdaw_model::sound_design::Template::Kick808, true);
                eprintln!("[smoke] samples: playing the Kick 808 template");
            }
            11 if t > 5.5 => {
                sm.step = 12;
                self.edit_recipe(|r| r.drive = 0.4);
                self.play_sound();
                eprintln!("[smoke] samples: drive 40 %, playing again");
            }
            12 if t > 7.0 => {
                sm.step = 13;
                sm.peak = 0.0;
                self.send_sound_to_pad();
                eprintln!(
                    "[smoke] samples: {:?}",
                    crate::designer::all_sounds(self)
                        .iter()
                        .map(|d| d.name.clone())
                        .collect::<Vec<_>>()
                );
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
            // Phase 10 (`MDAW_SMOKE=shell`): the close guard in the real window (Rune cannot veto a
            // close: the window comes back with the prompt), Don't Save, reopen, quit.
            20 if t > 4.5 => {
                sm.step = 21;
                eprintln!(
                    "[smoke] shell: title={:?} import note={:?} packs dir={}",
                    self.project.title,
                    self.shell.import_note.as_ref().map(|n| n.message()),
                    self.svc.packs_dir
                );
                let bpm = self.svc.engine.doc().get_bpm();
                self.set_bpm(bpm + 1.0);
                eprintln!(
                    "[smoke] shell: edited, dirty={} → closing the window",
                    self.dirty()
                );
                tx.close_window();
            }
            21 if t > 6.0 => {
                sm.step = 22;
                eprintln!(
                    "[smoke] shell: after close: prompt={:?} window visible={:?} title={:?}",
                    self.project.unsaved,
                    self.project.handle.as_ref().map(|h| h.is_visible()),
                    self.project.title
                );
                self.answer_unsaved(Some(false), tx);
                eprintln!("[smoke] shell: Don't Save");
            }
            22 if t > 7.5 => {
                sm.step = 23;
                eprintln!(
                    "[smoke] shell: closes let through={} window visible={:?}; reopening",
                    self.project.window_closes,
                    self.project.handle.as_ref().map(|h| h.is_visible())
                );
                if let Some(h) = &self.project.handle {
                    h.open();
                }
            }
            23 if t > 9.0 => {
                sm.step = 24;
                eprintln!(
                    "[smoke] shell: reopened, window visible={:?}; quitting",
                    self.project.handle.as_ref().map(|h| h.is_visible())
                );
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
