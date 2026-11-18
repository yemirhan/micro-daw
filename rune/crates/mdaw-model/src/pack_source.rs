//! Pack sample references: `{ kind: 'pack', packId, file }` resolved against the packs folder.
//! Port of `src/shared/packSource.ts`.
//!
//! The pack id / pack path validation it needs (`isValidPackId`, `isSafePackPath` from
//! `src/shared/packValidate.ts`) comes from the pack store validator ([`crate::packs::validate`]).

use crate::js;
use crate::packs::validate::{is_safe_pack_path, is_valid_pack_id};
use crate::paths::{separator_of, trim_trailing_sep};

/// Command-line switch carrying the packs folder from main to the preload.
pub const PACKS_DIR_ARG: &str = "--mdaw-packs-dir=";

/// A pack sound reference (`{ packId, file }`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackRef {
    pub pack_id: String,
    pub file: String,
}

/// `packsDirFromArgv`: the packs folder from a renderer's argv, or None.
pub fn packs_dir_from_argv<S: AsRef<str>>(argv: Option<&[S]>) -> Option<String> {
    let arg = argv?.iter().map(AsRef::as_ref).find(|a| a.starts_with(PACKS_DIR_ARG))?;
    let dir = &arg[PACKS_DIR_ARG.len()..];
    if dir.is_empty() { None } else { Some(dir.to_owned()) }
}

/// `packDirPrefix`: `<packsDir>/<packId>/` (trailing separator); None for an unsafe id.
pub fn pack_dir_prefix(packs_dir: &str, pack_id: &str) -> Option<String> {
    if packs_dir.is_empty() || !is_valid_pack_id(pack_id) {
        return None;
    }
    let sep = separator_of(packs_dir);
    Some(format!("{}{sep}{pack_id}{sep}", trim_trailing_sep(packs_dir)))
}

/// `resolvePackFilePath`: absolute path of `file` in pack `packId` (separators follow `packsDir`).
pub fn resolve_pack_file_path(packs_dir: &str, pack_id: &str, file: &str) -> Option<String> {
    let prefix = pack_dir_prefix(packs_dir, pack_id)?;
    if !is_safe_pack_path(file) {
        return None;
    }
    Some(if separator_of(packs_dir) == '\\' { prefix + &file.replace('/', "\\") } else { prefix + file })
}

/// `packRefFromPath`: inverse of `resolve_pack_file_path` for migrating old absolute paths.
pub fn pack_ref_from_path(packs_dir: Option<&str>, abs_path: &str) -> Option<PackRef> {
    let packs_dir = packs_dir.filter(|d| !d.is_empty())?;
    if abs_path.is_empty() {
        return None;
    }
    let root = trim_trailing_sep(packs_dir);
    if js::len(abs_path) <= js::len(&root) + 1 || !abs_path.starts_with(root.as_str()) {
        return None;
    }
    let rest = &abs_path[root.len()..];
    let sep_char = rest.chars().next()?;
    if sep_char != '/' && sep_char != '\\' {
        return None;
    }
    let segs: Vec<&str> = rest[1..].split(['\\', '/']).collect();
    if segs.len() < 2 {
        return None;
    }
    let pack_id = segs[0];
    let file = segs[1..].join("/");
    if !is_valid_pack_id(pack_id) || !is_safe_pack_path(&file) {
        return None;
    }
    Some(PackRef { pack_id: pack_id.to_owned(), file })
}

#[cfg(test)]
#[path = "pack_source_tests.rs"]
mod tests;
