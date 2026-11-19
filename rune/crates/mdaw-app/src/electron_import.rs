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

/// Every item of the localStorage LevelDB at `leveldb_dir` (read from a temporary copy).
pub fn read_local_storage(leveldb_dir: &Path) -> Result<Vec<StorageItem>, String> {
    if !leveldb_dir.is_dir() {
        return Err(format!("{} is not a folder", leveldb_dir.display()));
    }
    let tmp = std::env::temp_dir().join(format!("mdaw-electron-ls-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let result = (|| {
        for entry in std::fs::read_dir(leveldb_dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.file_name();
            if name == "LOCK" || !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                continue;
            }
            std::fs::copy(entry.path(), tmp.join(&name)).map_err(|e| e.to_string())?;
        }
        let opts = rusty_leveldb::Options {
            create_if_missing: false,
            ..Default::default()
        };
        let mut db = rusty_leveldb::DB::open(&tmp, opts).map_err(|e| e.to_string())?;
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
            let (Some(key), Some(value)) = (decode_string(&rest[sep + 1..]), decode_string(&v))
            else {
                continue;
            };
            out.push(StorageItem { origin, key, value });
        }
        Ok(out)
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result
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

/// The one-time import into `store` from the Electron profile `profile`. None when it already
/// ran or there is nothing to import (no profile, no items, no packs); the mark is written in
/// every case so it never runs again.
pub fn import_once(store: &mut SettingsStore, profile: Option<&Path>) -> Option<ImportSummary> {
    if store.get_item(IMPORT_MARK_KEY).is_some() {
        return None;
    }
    let at = mdaw_packs::json::iso_now();
    let Some(profile) = profile.filter(|p| p.is_dir()) else {
        let _ = store.set_item(
            IMPORT_MARK_KEY,
            json!({"at": at, "status": "no-profile"}).to_string(),
        );
        return None;
    };
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
                        let Some(item) = items.iter().find(|i| &i.origin == origin && i.key == key)
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
    let packs = profile.join("packs");
    if packs.is_dir() && store.get_item(PACKS_DIR_KEY).is_none() {
        let p = packs.to_string_lossy().into_owned();
        let _ = store.set_item(PACKS_DIR_KEY, p.clone());
        summary.packs_dir = Some(p);
    }
    let status = if summary.error.is_some() {
        "error"
    } else {
        "done"
    };
    let _ = store.set_item(
        IMPORT_MARK_KEY,
        json!({
            "at": at,
            "status": status,
            "profile": summary.profile,
            "origin": summary.origin,
            "imported": summary.imported,
            "kept": summary.kept,
            "packsDir": summary.packs_dir,
            "error": summary.error,
        })
        .to_string(),
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
