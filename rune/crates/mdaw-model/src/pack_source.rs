//! Pack sample references: `{ kind: 'pack', packId, file }` resolved against the packs folder.
//! Port of `src/shared/packSource.ts`.
//!
//! The pack id / pack path validation it needs (`isValidPackId`, `isSafePackPath` from
//! `src/shared/packValidate.ts`) is ported here as minimal private-to-crate helpers; the pack
//! store module (`packs.rs`) owns the full validator.

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

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// `packRefFromPath`: inverse of `resolve_pack_file_path` for migrating old absolute paths.
pub fn pack_ref_from_path(packs_dir: Option<&str>, abs_path: &str) -> Option<PackRef> {
    let packs_dir = packs_dir.filter(|d| !d.is_empty())?;
    if abs_path.is_empty() {
        return None;
    }
    let root = trim_trailing_sep(packs_dir);
    if utf16_len(abs_path) <= utf16_len(&root) + 1 || !abs_path.starts_with(root.as_str()) {
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

// ---------------------------------------------------------------------------
// Minimal port of the two packValidate.ts checks used above.
// ---------------------------------------------------------------------------

/// `isValidPackId`: `/^[a-z0-9]+(?:-[a-z0-9]+)*$/`, 1..64 chars.
pub(crate) fn is_valid_pack_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .split('-')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
}

const ALLOWED_EXTENSIONS: [&str; 10] = ["wav", "wave", "aif", "aiff", "flac", "mp3", "ogg", "m4a", "txt", "md"];
const PACK_META_FILE: &str = "pack.json";

/// `WIN_RESERVED = /^(con|prn|aux|nul|com[0-9]|lpt[0-9])(\..*)?$/i`
fn is_win_reserved(seg: &str) -> bool {
    let lower = seg.to_ascii_lowercase();
    let stem_len = if ["con", "prn", "aux", "nul"].iter().any(|s| lower.starts_with(s)) {
        3
    } else if (lower.starts_with("com") || lower.starts_with("lpt"))
        && lower.as_bytes().get(3).is_some_and(u8::is_ascii_digit)
    {
        4
    } else {
        return false;
    };
    let rest = &seg[stem_len..];
    // `.` in a JS regex does not match line terminators.
    rest.is_empty() || (rest.starts_with('.') && !rest.contains(['\n', '\r', '\u{2028}', '\u{2029}']))
}

/// `isSafePackPath` (= `safePackPathSegments` does not throw).
pub(crate) fn is_safe_pack_path(rel: &str) -> bool {
    let len = utf16_len(rel);
    if len == 0 || len > 512 {
        return false;
    }
    if rel.chars().any(|c| (c as u32) <= 0x1f || c == '\u{7f}' || "\\:*?\"<>|".contains(c)) {
        return false;
    }
    if rel.starts_with('/') {
        return false;
    }
    let segments: Vec<&str> = rel.split('/').collect();
    for seg in &segments {
        if seg.is_empty() || *seg == "." || *seg == ".." || utf16_len(seg) > 128 || seg.starts_with('.') {
            return false;
        }
        if seg.ends_with(['.', ' ']) || is_win_reserved(seg) {
            return false;
        }
    }
    if segments.len() > 8 {
        return false;
    }
    let name = segments[segments.len() - 1];
    if segments.len() == 1 && name.to_lowercase() == PACK_META_FILE {
        return false;
    }
    let ext = match name.rfind('.') {
        Some(dot) if dot > 0 => name[dot + 1..].to_lowercase(),
        _ => String::new(),
    };
    ALLOWED_EXTENSIONS.contains(&ext.as_str())
}

#[cfg(test)]
#[path = "pack_source_tests.rs"]
mod tests;
