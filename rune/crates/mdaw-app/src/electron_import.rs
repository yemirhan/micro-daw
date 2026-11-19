//! Phase 10: the one-time import of the Electron app's settings (plan §5 item 5).
//!
//! The Electron app keeps all its persistent state (inventory §4.6) in the renderer's
//! `localStorage`, which Chromium stores as LevelDB in `<userData>/Local Storage/leveldb`
//! (`userData` = `<config dir>/micro-daw`: `~/Library/Application Support/micro-daw` on macOS,
//! `%APPDATA%\micro-daw` on Windows, `~/.config/micro-daw` on Linux). On the Rust app's first
//! launch with this code ([`IMPORT_MARK_KEY`] absent), every key of
//! [`mdaw_model::settings::ALL_KEYS`] found there is copied into the Rust settings store,
//! unless the store already has that key (a Rust value is never overwritten). The packs folder is
//! **shared, not copied**: when `<userData>/packs` exists the Rust app uses it from then on
//! ([`PACKS_DIR_KEY`]); both apps follow `docs/rune-port/packs-dir-protocol.md`, so they can
//! install into it side by side.
//!
//! The LevelDB folder is copied to a temporary folder first (Electron may be running and holds
//! its `LOCK`; opening the original could also compact or rewrite it) and read from the copy.
//!
//! **Robustness** (phase 10 review finding 8): the mark is written as `in-progress` (with the
//! attempt number) *before* anything is read, so an import that crashes the app is not retried
//! forever; the copy is removed by a drop guard whatever happens; every file of the copy is
//! verified first ([`crate::leveldb_check`]: log and table checksums, a record cut off by a
//! concurrent write is *transient* and re-copied, up to [`COPY_ATTEMPTS`] times); the read runs
//! on its own thread, so a panic in `rusty-leveldb` or a hang (bounded by [`READ_TIMEOUT`])
//! becomes an error; a non-empty database that reads as empty is an error. `done` is written
//! only after a validated read. A failed import runs again at the next launch, at most
//! [`MAX_ATTEMPTS`] launches in all; its note offers "Retry import" ([`import_retry`]).
//!
//! Chromium's layout (`components/services/storage/dom_storage/local_storage_impl.cc`):
//! `VERSION`, `META:<origin>`, `METAACCESS:<origin>`, and one record per item with the key
//! `_` + origin + `\0` + key and the value, both strings stored with a format byte: `\x00` =
//! UTF-16LE, `\x01` = Latin-1. The packaged app's origin is `file://`; a dev build's is the Vite
//! server (`http://localhost:5173`). The import takes `file://` when it has micro-daw keys, else
//! the origin with the most of them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mdaw_model::settings::{ALL_KEYS, SettingsStore};
use serde_json::json;

/// Set once the import ran (or found nothing): `{at, status, origin?, imported, kept, packsDir?}`.
pub const IMPORT_MARK_KEY: &str = "micro-daw-rune:electron-import";
/// The packs folder the Rust app uses (absent: `<data dir>/packs`).
pub const PACKS_DIR_KEY: &str = "micro-daw-rune:packs-dir";

/// The Electron app's `userData`: `$MDAW_ELECTRON_PROFILE` (empty = none), else
/// `<config dir>/micro-daw`.
pub fn electron_profile_dir() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("MDAW_ELECTRON_PROFILE") {
        return (!p.is_empty()).then(|| PathBuf::from(p));
    }
    dirs::config_dir().map(|d| d.join("micro-daw"))
}

/// One localStorage item of one origin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageItem {
    pub origin: String,
    pub key: String,
    pub value: String,
}

/// A Chromium localStorage string: format byte, then UTF-16LE or Latin-1.
pub fn decode_string(b: &[u8]) -> Option<String> {
    let (&format, rest) = b.split_first()?;
    match format {
        0 => {
            if rest.len() % 2 != 0 {
                return None;
            }
            let units: Vec<u16> = rest
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes(*c))
                .collect();
            String::from_utf16(&units).ok()
        }
        1 => Some(rest.iter().map(|&c| c as char).collect()),
        _ => None,
    }
}

/// Launches that try the import before it gives up (each writes its attempt number first).
pub const MAX_ATTEMPTS: u64 = 3;
/// Copies of the database tried within one attempt when one is caught mid-write.
pub const COPY_ATTEMPTS: usize = 3;
/// The longest a read may take.
pub const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// The temporary copy, removed when dropped (also on an early return or a panic).
struct TempCopy(PathBuf);

impl Drop for TempCopy {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn copy_db(leveldb_dir: &Path) -> Result<TempCopy, String> {
    let tmp =
        TempCopy(std::env::temp_dir().join(format!("mdaw-electron-ls-{}", uuid::Uuid::new_v4())));
    std::fs::create_dir_all(&tmp.0).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(leveldb_dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        if name == "LOCK" || !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        std::fs::copy(entry.path(), tmp.0.join(&name)).map_err(|e| e.to_string())?;
    }
    Ok(tmp)
}

/// Reads every item with `rusty-leveldb` on a thread of its own (a panic or a hang becomes an
/// error).
fn read_copy(dir: &Path) -> Result<Vec<StorageItem>, String> {
    quiet_import_panics();
    let dir = dir.to_path_buf();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name(IMPORT_THREAD.into())
        .spawn(move || {
            let _ = tx.send(read_db(&dir));
        })
        .map_err(|e| e.to_string())?;
    match rx.recv_timeout(READ_TIMEOUT) {
        Ok(r) => r,
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            Err("the profile's database is damaged (the reader failed)".into())
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            Err("reading the profile's database timed out".into())
        }
    }
}

const IMPORT_THREAD: &str = "mdaw-electron-import";

/// A panic on the import's reader thread is reported as an error, not printed as a crash.
fn quiet_import_panics() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if std::thread::current().name() != Some(IMPORT_THREAD) {
                previous(info);
            }
        }));
    });
}

fn read_db(dir: &Path) -> Result<Vec<StorageItem>, String> {
    let opts = rusty_leveldb::Options {
        create_if_missing: false,
        ..Default::default()
    };
    let mut db = rusty_leveldb::DB::open(dir, opts).map_err(|e| e.to_string())?;
    let mut it = db.new_iter().map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    use rusty_leveldb::LdbIterator;
    while let Some((k, v)) = it.next() {
        let Some(rest) = k.strip_prefix(b"_") else {
            continue;
        };
        let Some(sep) = rest.iter().position(|&b| b == 0) else {
            continue;
        };
        let origin = String::from_utf8_lossy(&rest[..sep]).into_owned();
        let (Some(key), Some(value)) = (decode_string(&rest[sep + 1..]), decode_string(&v)) else {
            continue;
        };
        out.push(StorageItem { origin, key, value });
    }
    Ok(out)
}

/// Every item of the localStorage LevelDB at `leveldb_dir`, read from a verified temporary
/// copy (see the module docs).
pub fn read_local_storage(leveldb_dir: &Path) -> Result<Vec<StorageItem>, String> {
    if !leveldb_dir.is_dir() {
        return Err(format!("{} is not a folder", leveldb_dir.display()));
    }
    let mut last = String::new();
    for attempt in 0..COPY_ATTEMPTS {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
        let copy = copy_db(leveldb_dir)?;
        let has_data = match crate::leveldb_check::check_dir(&copy.0) {
            Ok(d) => d,
            Err(crate::leveldb_check::CheckError::Transient(e)) => {
                last = format!("the profile was being written ({e})");
                continue;
            }
            Err(crate::leveldb_check::CheckError::Corrupt(e)) => {
                return Err(format!("the profile's database is damaged ({e})"));
            }
        };
        let items = read_copy(&copy.0)?;
        if items.is_empty() && has_data {
            return Err("the profile's database has data but none of it could be read".into());
        }
        return Ok(items);
    }
    Err(last)
}

/// The origin whose items are the app's: `file://` when it has any of [`ALL_KEYS`], else the
/// origin with the most of them.
pub fn pick_origin(items: &[StorageItem]) -> Option<String> {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for i in items.iter().filter(|i| ALL_KEYS.contains(&i.key.as_str())) {
        *counts.entry(&i.origin).or_default() += 1;
    }
    if counts.contains_key("file://") {
        return Some("file://".into());
    }
    counts
        .into_iter()
        .max_by_key(|(_, n)| *n)
        .map(|(o, _)| o.to_owned())
}

/// What the import did (the note the app shows once).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportSummary {
    pub profile: String,
    pub origin: Option<String>,
    /// Keys copied into the Rust store.
    pub imported: Vec<String>,
    /// Keys the Electron profile had that the Rust store already had (kept).
    pub kept: Vec<String>,
    /// The shared packs folder, when there was one.
    pub packs_dir: Option<String>,
    /// Reading the profile failed.
    pub error: Option<String>,
}

impl ImportSummary {
    /// The note's sentence.
    pub fn message(&self) -> String {
        if let Some(e) = &self.error {
            return format!("Couldn't import the settings of the Electron app: {e}");
        }
        let n = self.imported.len();
        let mut m = format!(
            "Imported {n} setting{} from the Electron app",
            if n == 1 { "" } else { "s" }
        );
        if !self.kept.is_empty() {
            m.push_str(&format!(" ({} kept from this app)", self.kept.len()));
        }
        if self.packs_dir.is_some() {
            m.push_str("; its packs folder is shared");
        }
        m.push('.');
        m
    }
}

fn mark(store: &mut SettingsStore, v: serde_json::Value) {
    let _ = store.set_item(IMPORT_MARK_KEY, v.to_string());
}

/// The one-time import into `store` from the Electron profile `profile`. None when it already
/// ran (or gave up), or there is nothing to import (no profile, no items, no packs). The mark is
/// written as `in-progress` before reading, then `done` / `no-profile`, or `error` (tried again
/// at the next launch, [`MAX_ATTEMPTS`] in all; the last one is `failed`).
pub fn import_once(store: &mut SettingsStore, profile: Option<&Path>) -> Option<ImportSummary> {
    let prev: Option<serde_json::Value> = store
        .get_item(IMPORT_MARK_KEY)
        .and_then(|m| serde_json::from_str(m).ok());
    let attempts = match &prev {
        None => 0,
        Some(m) => match m.get("status").and_then(|s| s.as_str()) {
            Some("in-progress" | "error") => {
                m.get("attempts").and_then(|a| a.as_u64()).unwrap_or(1)
            }
            // done, no-profile, failed, or a mark this code doesn't know.
            _ => return None,
        },
    };
    if attempts >= MAX_ATTEMPTS {
        // The previous launch died during its last attempt: give up, once, with a note.
        let summary = ImportSummary {
            profile: profile.map(|p| p.display().to_string()).unwrap_or_default(),
            origin: None,
            imported: Vec::new(),
            kept: Vec::new(),
            packs_dir: None,
            error: Some(format!(
                "the import stopped {attempts} times without finishing; it won't run again                  by itself"
            )),
        };
        mark(
            store,
            json!({"at": mdaw_packs::json::iso_now(), "status": "failed", "attempts": attempts,
                   "error": summary.error}),
        );
        return Some(summary);
    }
    run_import(store, profile, attempts + 1)
}

/// "Retry import": runs the import again whatever the mark says (Rust values are still kept).
pub fn import_retry(store: &mut SettingsStore, profile: Option<&Path>) -> Option<ImportSummary> {
    run_import(store, profile, 1)
}

fn run_import(
    store: &mut SettingsStore,
    profile: Option<&Path>,
    attempt: u64,
) -> Option<ImportSummary> {
    let at = mdaw_packs::json::iso_now();
    let Some(profile) = profile.filter(|p| p.is_dir()) else {
        mark(store, json!({"at": at, "status": "no-profile"}));
        return None;
    };
    // First: a crash from here on is counted.
    mark(
        store,
        json!({"at": at, "status": "in-progress", "attempts": attempt,
               "profile": profile.display().to_string()}),
    );
    let mut summary = ImportSummary {
        profile: profile.display().to_string(),
        origin: None,
        imported: Vec::new(),
        kept: Vec::new(),
        packs_dir: None,
        error: None,
    };
    let ls = profile.join("Local Storage").join("leveldb");
    if ls.is_dir() {
        match read_local_storage(&ls) {
            Ok(items) => {
                summary.origin = pick_origin(&items);
                if let Some(origin) = &summary.origin {
                    for key in ALL_KEYS {
                        let Some(item) = items
                            .iter()
                            .rev()
                            .find(|i| &i.origin == origin && i.key == key)
                        else {
                            continue;
                        };
                        if store.get_item(key).is_some() {
                            summary.kept.push(key.to_owned());
                        } else if store.set_item(key, item.value.clone()).is_ok() {
                            summary.imported.push(key.to_owned());
                        }
                    }
                }
            }
            Err(e) => summary.error = Some(e),
        }
    }
    if summary.error.is_none() {
        let packs = profile.join("packs");
        if packs.is_dir() && store.get_item(PACKS_DIR_KEY).is_none() {
            let p = packs.to_string_lossy().into_owned();
            let _ = store.set_item(PACKS_DIR_KEY, p.clone());
            summary.packs_dir = Some(p);
        }
    }
    let status = match (&summary.error, attempt >= MAX_ATTEMPTS) {
        (None, _) => "done",
        (Some(_), false) => "error",
        (Some(_), true) => "failed",
    };
    mark(
        store,
        json!({
            "at": at,
            "status": status,
            "attempts": attempt,
            "profile": summary.profile,
            "origin": summary.origin,
            "imported": summary.imported,
            "kept": summary.kept,
            "packsDir": summary.packs_dir,
            "error": summary.error,
        }),
    );
    let nothing = summary.imported.is_empty()
        && summary.kept.is_empty()
        && summary.packs_dir.is_none()
        && summary.error.is_none();
    (!nothing).then_some(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_decode_both_formats() {
        assert_eq!(decode_string(b"\x01abc\xe9").as_deref(), Some("abcé"));
        let utf16: Vec<u8> = std::iter::once(0u8)
            .chain("Ş/ü".encode_utf16().flat_map(|u| u.to_le_bytes()))
            .collect();
        assert_eq!(decode_string(&utf16).as_deref(), Some("Ş/ü"));
        assert_eq!(decode_string(b"\x00a"), None, "odd UTF-16 length");
        assert_eq!(decode_string(b"\x02a"), None);
        assert_eq!(decode_string(b""), None);
    }

    #[test]
    fn the_file_origin_wins_else_the_busiest() {
        let item = |o: &str, k: &str| StorageItem {
            origin: o.into(),
            key: k.into(),
            value: String::new(),
        };
        let items = vec![
            item("http://localhost:5173", "micro-daw-settings"),
            item("http://localhost:5173", "micro-daw:kits"),
            item("file://", "micro-daw-settings"),
            item("https://other", "unrelated"),
        ];
        assert_eq!(pick_origin(&items).as_deref(), Some("file://"));
        assert_eq!(
            pick_origin(&items[..2]).as_deref(),
            Some("http://localhost:5173")
        );
        assert_eq!(pick_origin(&items[3..]), None);
    }
}
