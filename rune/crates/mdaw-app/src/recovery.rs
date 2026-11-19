//! Phase 10 review findings 1–4 and 6: the recovery checkpoint.
//!
//! Rune can't cancel a quit, and an app can crash, so what the next launch restores must be
//! right however the process ended. Every autosave and every project transition (open, save,
//! save as, new, a failed write) writes **one checkpoint**: the arrangement
//! (`micro-daw-arrangement`, as the Electron app autosaves it) and its session record
//! ([`SESSION_KEY`]) in a single settings-store write ([`SettingsStore::set_items`]), so the two
//! can never disagree on disk. The record holds:
//!
//! | field | |
//! |---|---|
//! | `v` | 2 |
//! | `filePath`, `createdAt` | the project's file (null = Untitled) |
//! | `fileModifiedAt` | the `modifiedAt` the file had when this app last read or wrote it |
//! | `dirty` | the arrangement has changes the file doesn't |
//! | `arrangementHash` | FNV-1a 64 of the arrangement JSON written with it |
//! | `gen`, `savedAt` | the store generation and time of the checkpoint |
//! | `audio` | the four live-FX parameters a project file restores (reverb, chorus, cutoff, resonance) |
//! | `assets` | `[{sampleId, path, name}]`: generated / processed samples on the timeline, written as WAVs into `<data dir>/recovery/` |
//!
//! **Restore** ([`App::restore_session`]): the arrangement comes back from the autosave as
//! before. The record is trusted only when its hash matches that arrangement; the path only when
//! the file on disk still has `fileModifiedAt` (another app or another save may have replaced
//! it). Anything else restores the work as **Untitled and unsaved**, with a note, so ⌘S can
//! never write it over a file it didn't come from. Dirty is restored with or without a path.
//!
//! **Acknowledgement** (finding 4): the autosave counts as saved only once the store reports its
//! generation on disk; a failed write keeps the work pending, shows "Autosave failed" and is
//! retried every [`AUTOSAVE_RETRY`] seconds. The store keeps the previous file as a second slot.
//!
//! Not recovered: changes made within the last [`crate::services::AUTOSAVE_DEBOUNCE`] before a
//! crash, undo history, the 16 live-FX parameters a project file doesn't restore either,
//! generated / processed samples that are only in the Samples library (not on the timeline).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use mdaw_dsp::buffer::AudioBuffer;
use mdaw_model::settings::{ARRANGEMENT_STORAGE_KEY, SettingsStore};
use serde_json::{Value, json};

use crate::app::App;
use crate::services::RunMode;

/// The checkpoint's session record (Rust app only).
pub const SESSION_KEY: &str = "micro-daw-rune:session";
/// Seconds between retries of a failed autosave.
pub const AUTOSAVE_RETRY: f64 = 2.0;

/// A generated / processed sample's recovery WAV.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Asset {
    Writing,
    Written { path: String, name: String },
    Failed(String),
}

/// FNV-1a 64 (the checkpoint's arrangement hash; not cryptographic, it detects a mismatch).
pub fn fnv64(s: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// A 32-bit float WAV of `b` (lossless for the recovery copy).
pub fn wav_f32(b: &AudioBuffer) -> Vec<u8> {
    let ch = b.channels.len().max(1) as u16;
    let frames = b.channels.first().map_or(0, Vec::len);
    let data_len = frames * ch as usize * 4;
    let mut out = Vec::with_capacity(44 + data_len);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&3u16.to_le_bytes()); // IEEE float
    out.extend_from_slice(&ch.to_le_bytes());
    out.extend_from_slice(&b.sample_rate.to_le_bytes());
    out.extend_from_slice(&(b.sample_rate * ch as u32 * 4).to_le_bytes());
    out.extend_from_slice(&(ch * 4).to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_len as u32).to_le_bytes());
    for i in 0..frames {
        for c in &b.channels {
            out.extend_from_slice(&c.get(i).copied().unwrap_or(0.0).to_le_bytes());
        }
    }
    out
}

/// A file name from a sample name (the recovered sample is named after it).
fn file_stem(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || " -_.()".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let s = s.trim().trim_start_matches('.').to_owned();
    if s.is_empty() { "Sample".into() } else { s }
}

/// The `modifiedAt` of the project file at `path` (None: missing or unreadable).
pub fn file_modified_at(path: &str) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    let v: Value = serde_json::from_str(&raw).ok()?;
    v.get("modifiedAt")?.as_str().map(str::to_owned)
}

impl App {
    /// The checkpoint is written in this run (window runs, and tests that turn saving on).
    fn checkpoints_on(&self) -> bool {
        self.svc.mode == RunMode::Window || self.svc.save_headless
    }

    /// Writes the checkpoint now (see the module docs). Returns false when this run doesn't
    /// write one (then the current version counts as saved, as before).
    pub fn checkpoint_now(&mut self) -> bool {
        let version = self.svc.engine.doc().state_version();
        if !self.checkpoints_on() {
            self.svc.autosave_skipped(version);
            return false;
        }
        self.write_recovery_assets();
        let arr = self.svc.engine.doc().save_to_local_storage();
        let record = self.session_record(&arr);
        let r = self
            .svc
            .settings
            .borrow_mut()
            .set_items(vec![(ARRANGEMENT_STORAGE_KEY, arr), (SESSION_KEY, record)]);
        let generation = self.svc.settings.borrow().generation();
        self.svc.autosave_submitted(generation, version);
        if r.is_err() || self.io.is_sync() {
            self.svc.autosave_ack(self.now);
        }
        true
    }

    fn session_record(&self, arr: &str) -> String {
        let fx = self.svc.engine.live_effect_params();
        let assets: Vec<Value> = self
            .timeline_memory_samples()
            .into_iter()
            .filter_map(|id| match self.recovery_assets.get(&id) {
                Some(Asset::Written { path, name }) => {
                    Some(json!({"sampleId": id, "path": path, "name": name}))
                }
                _ => None,
            })
            .collect();
        json!({
            "v": 2,
            "gen": self.svc.settings.borrow().generation() + 1,
            "filePath": self.project.file_path,
            "createdAt": self.project.created_at,
            "fileModifiedAt": self.project.file_modified_at,
            "dirty": self.dirty(),
            "arrangementHash": fnv64(arr),
            "savedAt": crate::project::iso_now(),
            "audio": {
                "reverbWet": fx.reverb_wet,
                "chorusDepth": fx.chorus_depth,
                "filterCutoff": fx.filter_cutoff,
                "filterResonance": fx.filter_resonance,
            },
            "assets": assets,
        })
        .to_string()
    }

    /// Ids of in-memory samples (no file: generated / processed) the timeline plays.
    fn timeline_memory_samples(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .svc
            .engine
            .doc()
            .get_tracks()
            .iter()
            .flat_map(|t| t.regions.iter())
            .filter_map(|r| r.audio.as_ref())
            .filter(|a| a.sample_path.is_empty() && !a.sample_id.is_empty())
            .map(|a| a.sample_id.clone())
            .collect();
        ids.sort();
        ids.dedup();
        ids
    }

    /// Writes a WAV for each timeline sample that has no file and no recovery copy yet (on the
    /// IO worker); a finished write asks for another checkpoint, which then lists it.
    fn write_recovery_assets(&mut self) {
        let Some(dir) = self.recovery_dir.clone() else {
            return;
        };
        for id in self.timeline_memory_samples() {
            if self.recovery_assets.contains_key(&id) {
                continue;
            }
            let Some(buf) = self.p9.samples.buffer(&id) else {
                continue;
            };
            let name = self
                .p9
                .samples
                .get(&id)
                .map_or_else(|| "Sample".to_owned(), |s| s.name.clone());
            self.recovery_assets.insert(id.clone(), Asset::Writing);
            let path = dir.join(&id).join(format!("{}.wav", file_stem(&name)));
            self.io_submit(move || {
                let r = path
                    .parent()
                    .map_or(Ok(()), std::fs::create_dir_all)
                    .and_then(|_| mdaw_model::fsio::write_atomic(&path, &wav_f32(&buf)))
                    .map_err(|e| e.to_string());
                let path = path.to_string_lossy().into_owned();
                Box::new(move |s: &mut App| {
                    let state = match r {
                        Ok(()) => Asset::Written { path, name },
                        Err(e) => Asset::Failed(e),
                    };
                    s.recovery_assets.insert(id, state);
                    s.svc.request_autosave();
                })
            });
        }
    }

    /// `flush` of `useArrangementPersistence` (window close / quit): a pending checkpoint is
    /// written now and, in a window run, waited for (at most 3 s).
    pub fn flush_autosave(&mut self) {
        if self.svc.save_pending() {
            self.checkpoint_now();
        }
        self.svc
            .settings
            .borrow()
            .wait_idle(std::time::Duration::from_secs(3));
        self.svc.autosave_ack(self.now);
    }

    /// The autosave's frame: acknowledges a durable checkpoint (or reports a failed one), and
    /// writes the next one when due (300 ms after a change, or a retry). True when the view
    /// must re-run (the error note changed).
    pub fn autosave_tick(&mut self, now: f64) -> bool {
        let before = self.svc.autosave_error.clone();
        self.svc.autosave_ack(now);
        if self.svc.autosave_due(now) {
            self.checkpoint_now();
            self.svc.autosave_ack(now);
        }
        self.svc.autosave_error != before
    }

    /// Restores the last session from its checkpoint (see the module docs).
    pub fn restore_session(&mut self) {
        if !self.svc.restored_autosave {
            return;
        }
        let (arr, raw) = {
            let st = self.svc.settings.borrow();
            (
                st.get_item(ARRANGEMENT_STORAGE_KEY).map(str::to_owned),
                st.get_item(SESSION_KEY).map(str::to_owned),
            )
        };
        // No record (an arrangement imported from the Electron app): Untitled and clean, as
        // Electron restores its autosave.
        let Some(v) = raw.and_then(|r| serde_json::from_str::<Value>(&r).ok()) else {
            return;
        };
        let matches = arr
            .as_deref()
            .is_some_and(|a| v.get("arrangementHash").and_then(Value::as_str) == Some(&fnv64(a)));
        // The live FX and the timeline's generated samples come back either way (they belong to
        // the arrangement the autosave restored).
        self.restore_audio(&v);
        self.restore_assets(&v);
        if !matches {
            // An older record, or one written apart from this arrangement: the work comes back
            // untitled and unsaved.
            self.clean_version = u64::MAX;
            self.shell.recovery_note = Some(
                "Your last arrangement was recovered as Untitled: it couldn't be matched to a \
                 project file. Use Save As to keep it."
                    .into(),
            );
            return;
        }
        let dirty = v.get("dirty").and_then(Value::as_bool) == Some(true);
        if let Some(c) = v.get("createdAt").and_then(Value::as_str) {
            self.project.created_at = c.to_owned();
        }
        match v.get("filePath").and_then(Value::as_str) {
            None => {
                if dirty {
                    self.clean_version = u64::MAX;
                }
            }
            Some(path) => {
                let expected = v.get("fileModifiedAt").and_then(Value::as_str);
                let on_disk = file_modified_at(path);
                if expected.is_some() && on_disk.as_deref() == expected {
                    self.project.file_path = Some(path.to_owned());
                    self.project.file_modified_at = on_disk;
                    if dirty {
                        self.clean_version = u64::MAX;
                    }
                } else {
                    self.project.created_at = crate::project::iso_now();
                    self.clean_version = u64::MAX;
                    self.shell.recovery_note = Some(format!(
                        "\u{201c}{}\u{201d} changed on disk or is missing since your work was \
                         saved for recovery, so it is shown as Untitled and won't overwrite \
                         the file. Use Save As to keep it.",
                        mdaw_model::migrate::project_name(Some(path))
                    ));
                }
            }
        }
    }

    fn restore_audio(&mut self, v: &Value) {
        use mdaw_engine::fx::live::LiveOp;
        let Some(a) = v.get("audio") else {
            return;
        };
        let num = |k: &str| a.get(k).and_then(Value::as_f64).filter(|x| x.is_finite());
        for (k, op) in [
            ("reverbWet", LiveOp::ReverbWet as fn(f64) -> LiveOp),
            ("chorusDepth", LiveOp::ChorusDepth),
            ("filterCutoff", LiveOp::FilterCutoff),
            ("filterResonance", LiveOp::FilterResonance),
        ] {
            if let Some(x) = num(k) {
                self.svc.engine.live_fx(op(x));
            }
        }
    }

    fn restore_assets(&mut self, v: &Value) {
        let mut keep: Vec<String> = Vec::new();
        let mut load = Vec::new();
        for a in v
            .get("assets")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let (Some(id), Some(path)) = (
                a.get("sampleId").and_then(Value::as_str),
                a.get("path").and_then(Value::as_str),
            ) else {
                continue;
            };
            let name = a.get("name").and_then(Value::as_str).unwrap_or("Sample");
            if !Path::new(path).is_file() {
                continue;
            }
            keep.push(id.to_owned());
            self.recovery_assets.insert(
                id.to_owned(),
                Asset::Written {
                    path: path.to_owned(),
                    name: name.to_owned(),
                },
            );
            load.push((id.to_owned(), path.to_owned()));
        }
        let now = self.now;
        self.p9.samples.ensure_region_samples(&load, now);
        // Copies no checkpoint lists any more.
        if let Some(dir) = &self.recovery_dir
            && let Ok(rd) = std::fs::read_dir(dir)
        {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if !keep.contains(&name) {
                    let _ = std::fs::remove_dir_all(e.path());
                }
            }
        }
    }
}

/// `<data dir>/recovery` (window runs).
pub fn recovery_dir_in(data: &Path) -> PathBuf {
    data.join("recovery")
}

/// The checkpoint record of `store` (tests, smoke).
pub fn session_record_of(store: &SettingsStore) -> Option<Value> {
    serde_json::from_str(store.get_item(SESSION_KEY)?).ok()
}

/// Asset states (re-exported for the app struct).
pub type Assets = HashMap<String, Asset>;
