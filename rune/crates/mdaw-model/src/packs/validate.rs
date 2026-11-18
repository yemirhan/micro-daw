//! Port of `src/shared/packValidate.ts`: catalog/manifest schema, the https host allowlist,
//! commit-pinned file URLs and pack-relative path safety. Validators take `serde_json::Value`
//! exactly like the TS takes `unknown`; error messages are verbatim.

use std::collections::HashSet;
use std::fmt;

use serde_json::{Map, Value};
use url::Url;

use super::{
    InstalledPack, PACK_ALLOWED_HOSTS, PackCatalog, PackFile, PackKit, PackKitPad, PackLicense,
    PackManifest,
};
use crate::js;
use crate::types::PAD_COUNT;

/// Max bytes of one downloaded pack file.
pub const MAX_FILE_BYTES: i64 = 50 * 1024 * 1024;
/// Max bytes of one whole pack.
pub const MAX_PACK_BYTES: i64 = 500 * 1024 * 1024;
/// Max bytes of one remote preview.
pub const MAX_PREVIEW_BYTES: u64 = 10 * 1024 * 1024;
/// Max bytes of the remote catalog JSON.
pub const MAX_CATALOG_BYTES: u64 = 5 * 1024 * 1024;
/// Sanity cap on files per pack.
pub const MAX_PACK_FILES: usize = 5000;
/// Max HTTP redirects followed per request.
pub const MAX_REDIRECTS: u32 = 5;
/// Name of the metadata file written into every installed pack folder.
pub const PACK_META_FILE: &str = "pack.json";
/// Host of every pack file URL (commit-pinned raw GitHub content).
pub const PACK_FILE_HOST: &str = "raw.githubusercontent.com";
/// Max length of a file's display label.
pub const MAX_LABEL_LENGTH: usize = 80;

/// File extensions a pack may contain (audio + licence/readme text).
const ALLOWED_EXTENSIONS: [&str; 10] = [
    "wav", "wave", "aif", "aiff", "flac", "mp3", "ogg", "m4a", "txt", "md",
];

/// `PackValidationError` (`name === 'PackValidationError'`); `Display` is the message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackValidationError(pub String);

impl PackValidationError {
    pub const NAME: &'static str = "PackValidationError";
    pub fn message(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PackValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PackValidationError {}

type Res<T> = Result<T, PackValidationError>;

fn err<T>(msg: impl Into<String>) -> Res<T> {
    Err(PackValidationError(msg.into()))
}

// ---------------------------------------------------------------------------
// Predicates
// ---------------------------------------------------------------------------

/// `/^[a-z0-9]+(?:-[a-z0-9]+)*$/`
fn is_kebab(id: &str) -> bool {
    !id.is_empty()
        && id.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

/// kebab-case, 1..64 chars: safe as a folder name on every platform.
pub fn is_valid_pack_id(id: &str) -> bool {
    let n = js::len(id);
    n > 0 && n <= 64 && is_kebab(id)
}

/// `isValidPackId(unknown)`.
pub fn is_valid_pack_id_value(v: &Value) -> bool {
    v.as_str().is_some_and(is_valid_pack_id)
}

fn is_hex_lower(s: &str, n: usize) -> bool {
    s.len() == n
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// `/^[A-Za-z0-9](?:[A-Za-z0-9-]{0,38})$/`
fn is_gh_owner(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 39
        && b[0].is_ascii_alphanumeric()
        && b[1..]
            .iter()
            .all(|c| c.is_ascii_alphanumeric() || *c == b'-')
}

/// `/^[A-Za-z0-9_.-]{1,100}$/`
fn is_gh_repo(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 100
        && b.iter()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b'-'))
}

/// True when `url` is https on one of PACK_ALLOWED_HOSTS (no credentials, default port).
pub fn is_allowed_url(url: &str) -> bool {
    if js::len(url) > 4096 {
        return false;
    }
    let Ok(u) = Url::parse(url) else { return false };
    if u.scheme() != "https" {
        return false;
    }
    if !u.username().is_empty() || u.password().is_some_and(|p| !p.is_empty()) {
        return false;
    }
    // WHATWG drops the default port, so any remaining port is not 443.
    if u.port().is_some() {
        return false;
    }
    let host = u.host_str().unwrap_or("").to_lowercase();
    PACK_ALLOWED_HOSTS.contains(&host.as_str())
}

pub fn is_allowed_url_value(v: &Value) -> bool {
    v.as_str().is_some_and(is_allowed_url)
}

/// True when `url` is `https://raw.githubusercontent.com/<owner>/<repo>/<40-hex>/<path>` in
/// canonical form (no credentials, port, query or fragment, no dot segments).
///
/// JS quirk kept: a bare `?` or `#` (empty search / hash) passes, because `u.search` and
/// `u.hash` are then `''` and `href` keeps the character.
pub fn is_pinned_pack_url(url: &str) -> bool {
    if !is_allowed_url(url) {
        return false;
    }
    let Ok(u) = Url::parse(url) else { return false };
    if u.as_str() != url
        || u.host_str() != Some(PACK_FILE_HOST)
        || u.query().is_some_and(|q| !q.is_empty())
        || u.fragment().is_some_and(|f| !f.is_empty())
    {
        return false;
    }
    let parts: Vec<&str> = u.path().split('/').collect();
    if parts.len() < 5
        || !is_gh_owner(parts[1])
        || !is_gh_repo(parts[2])
        || !is_hex_lower(parts[3], 40)
    {
        return false;
    }
    parts[4..].iter().all(|p| {
        let lower = p.to_ascii_lowercase();
        !p.is_empty()
            && *p != "."
            && *p != ".."
            && !lower.contains("%2f")
            && !lower.contains("%5c")
            && !lower.contains("%2e%2e")
    })
}

pub fn is_pinned_pack_url_value(v: &Value) -> bool {
    v.as_str().is_some_and(is_pinned_pack_url)
}

/// True for https://github.com/... links (the pack's upstream `source`).
pub fn is_github_source_url(url: &str) -> bool {
    if js::len(url) > 2000 {
        return false;
    }
    let Ok(u) = Url::parse(url) else { return false };
    u.scheme() == "https"
        && u.host_str().unwrap_or("").to_lowercase() == "github.com"
        && u.username().is_empty()
        && u.password().is_none_or(str::is_empty)
        && u.port().is_none()
}

pub fn is_github_source_url_value(v: &Value) -> bool {
    v.as_str().is_some_and(is_github_source_url)
}

/// `NUM = [+-]?(?:\d+(?:\.\d*)?|\.\d+)` at `i`; returns the index after it.
fn scan_num(b: &[char], mut i: usize) -> Option<usize> {
    if i < b.len() && (b[i] == '+' || b[i] == '-') {
        i += 1;
    }
    let start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i > start {
        if i < b.len() && b[i] == '.' {
            i += 1;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
        }
        return Some(i);
    }
    if i < b.len() && b[i] == '.' {
        let d = i + 1;
        let mut j = d;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > d {
            return Some(j);
        }
    }
    None
}

fn skip_ws(b: &[char], mut i: usize) -> usize {
    while i < b.len() && js::is_js_ws(b[i]) {
        i += 1;
    }
    i
}

fn eat_ci(b: &[char], i: usize, word: &str) -> Option<usize> {
    let n = word.len();
    if i + n > b.len() {
        return None;
    }
    b[i..i + n]
        .iter()
        .zip(word.chars())
        .all(|(c, w)| c.to_ascii_lowercase() == w)
        .then_some(i + n)
}

/// `^oklch\(\s*NUM%?\s+NUM%?\s+NUM(?:deg)?\s*(?:/\s*NUM%?\s*)?\)$` (case-insensitive).
fn is_oklch(s: &str) -> bool {
    let b: Vec<char> = s.chars().collect();
    let parse = || -> Option<()> {
        let mut i = eat_ci(&b, 0, "oklch(")?;
        i = skip_ws(&b, i);
        for _ in 0..2 {
            i = scan_num(&b, i)?;
            if b.get(i) == Some(&'%') {
                i += 1;
            }
            let j = skip_ws(&b, i);
            if j == i {
                return None;
            }
            i = j;
        }
        i = scan_num(&b, i)?;
        if let Some(j) = eat_ci(&b, i, "deg") {
            i = j;
        }
        i = skip_ws(&b, i);
        if b.get(i) == Some(&'/') {
            i = skip_ws(&b, i + 1);
            i = scan_num(&b, i)?;
            if b.get(i) == Some(&'%') {
                i += 1;
            }
            i = skip_ws(&b, i);
        }
        (b.get(i) == Some(&')') && i + 1 == b.len()).then_some(())
    };
    parse().is_some()
}

/// `#rgb[a]`, `#rrggbb[aa]` (case-insensitive hex).
fn is_hex_color(s: &str) -> bool {
    let Some(hex) = s.strip_prefix('#') else {
        return false;
    };
    matches!(hex.len(), 3 | 4 | 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit())
}

/// A pack accent color: `#rgb[a]`, `#rrggbb[aa]` or `oklch(L C H [/ A])` with plain numbers.
pub fn is_safe_pack_color(color: &str) -> bool {
    js::len(color) <= 64 && (is_hex_color(color) || is_oklch(color))
}

pub fn is_safe_pack_color_value(v: &Value) -> bool {
    v.as_str().is_some_and(is_safe_pack_color)
}

/// `/^(con|prn|aux|nul|com[0-9]|lpt[0-9])(\..*)?$/i` (`.` does not match line terminators).
fn is_win_reserved(seg: &str) -> bool {
    let lower: String = seg.chars().map(|c| c.to_ascii_lowercase()).collect();
    let base_len = if ["con", "prn", "aux", "nul"]
        .iter()
        .any(|p| lower.starts_with(p))
    {
        3
    } else if (lower.starts_with("com") || lower.starts_with("lpt"))
        && lower.as_bytes().get(3).is_some_and(u8::is_ascii_digit)
    {
        4
    } else {
        return false;
    };
    let rest = &lower[base_len..];
    rest.is_empty()
        || (rest.starts_with('.')
            && !rest
                .chars()
                .any(|c| matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')))
}

/// Validates a pack-relative file path ("kicks/bd-01.wav") and returns its segments.
pub fn safe_pack_path_segments(rel: &str) -> Res<Vec<String>> {
    let n = js::len(rel);
    if n == 0 || n > 512 {
        return err("Invalid file path");
    }
    if rel.chars().any(|c| {
        c <= '\u{1f}'
            || c == '\u{7f}'
            || matches!(c, '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
    }) {
        return err(format!("Unsafe characters in file path: {rel}"));
    }
    if rel.starts_with('/') {
        return err(format!("Absolute file path: {rel}"));
    }
    let segments: Vec<String> = rel.split('/').map(str::to_owned).collect();
    for seg in &segments {
        if seg.is_empty() || seg == "." || seg == ".." {
            return err(format!("Unsafe file path: {rel}"));
        }
        if js::len(seg) > 128 {
            return err(format!("File path segment too long: {rel}"));
        }
        if seg.starts_with('.') {
            return err(format!("Hidden file in pack: {rel}"));
        }
        if seg.ends_with('.') || seg.ends_with(' ') || is_win_reserved(seg) {
            return err(format!("Unsafe file name: {rel}"));
        }
    }
    if segments.len() > 8 {
        return err(format!("File path too deep: {rel}"));
    }
    let name = &segments[segments.len() - 1];
    if segments.len() == 1 && name.to_lowercase() == PACK_META_FILE {
        return err(format!("Reserved file name: {rel}"));
    }
    let ext = match name.rfind('.') {
        Some(dot) if dot > 0 => name[dot + 1..].to_lowercase(),
        _ => String::new(),
    };
    if !ALLOWED_EXTENSIONS.contains(&ext.as_str()) {
        return err(format!("File type not allowed: {rel}"));
    }
    Ok(segments)
}

/// `safePackPathSegments(unknown)`: a non-string is `Invalid file path`.
pub fn safe_pack_path_segments_value(v: &Value) -> Res<Vec<String>> {
    match v.as_str() {
        Some(s) => safe_pack_path_segments(s),
        None => err("Invalid file path"),
    }
}

pub fn is_safe_pack_path(rel: &str) -> bool {
    safe_pack_path_segments(rel).is_ok()
}

pub fn is_safe_pack_path_value(v: &Value) -> bool {
    safe_pack_path_segments_value(v).is_ok()
}

// ---------------------------------------------------------------------------
// Schema
// ---------------------------------------------------------------------------

type Obj = Map<String, Value>;

/// `str(o, key, max, required)`.
fn str_field(o: &Obj, key: &str, max: usize, required: bool) -> Res<Option<String>> {
    let v = o.get(key);
    if v.is_none() && !required {
        return Ok(None);
    }
    match v {
        Some(Value::String(s)) if !(required && js::trim(s).is_empty()) && js::len(s) <= max => {
            Ok(Some(s.clone()))
        }
        _ => err(format!("Invalid \"{key}\"")),
    }
}

fn req(o: &Obj, key: &str, max: usize) -> Res<String> {
    Ok(str_field(o, key, max, true)?.unwrap_or_default())
}

fn has_control(s: &str) -> bool {
    s.chars().any(|c| c <= '\u{1f}' || c == '\u{7f}')
}

fn validate_file(raw: &Value) -> Res<PackFile> {
    let Some(raw) = raw.as_object() else {
        return err("Invalid file entry");
    };
    let path = req(raw, "path", 2000)?;
    safe_pack_path_segments(&path)?;
    let url = req(raw, "url", 4096)?;
    if !is_allowed_url(&url) {
        return err(format!("File URL not allowed: {url}"));
    }
    if !is_pinned_pack_url(&url) {
        return err(format!("File URL not pinned to a commit: {url}"));
    }
    let size = match js::integer_of(raw.get("size")) {
        Some(s) if s > 0.0 => s,
        _ => return err(format!("Invalid size for {path}")),
    };
    if size > MAX_FILE_BYTES as f64 {
        return err(format!("File too large: {path}"));
    }
    let sha256 = match raw.get("sha256") {
        Some(Value::String(s)) if is_hex_lower(s, 64) => s.clone(),
        _ => return err(format!("Invalid sha256 for {path}")),
    };
    let mut file = PackFile {
        path,
        url,
        size: size as i64,
        sha256,
        label: None,
    };
    if let Some(label) = raw.get("label") {
        match label {
            Value::String(l)
                if !js::trim(l).is_empty() && js::len(l) <= MAX_LABEL_LENGTH && !has_control(l) =>
            {
                file.label = Some(l.clone());
            }
            _ => return err(format!("Invalid label for {}", file.path)),
        }
    }
    Ok(file)
}

fn validate_kit(raw: &Value, files: &HashSet<&str>) -> Res<PackKit> {
    let Some(raw) = raw.as_object() else {
        return err("Invalid kit");
    };
    let id = req(raw, "id", 64)?;
    let name = req(raw, "name", 200)?;
    let Some(Value::Array(raw_pads)) = raw.get("pads") else {
        return err(format!("Kit {id} has no pads"));
    };
    let mut used = HashSet::new();
    let mut pads = Vec::new();
    for p in raw_pads {
        let Some(p) = p.as_object() else {
            return err(format!("Kit {id}: invalid pad"));
        };
        let pad = match js::integer_of(p.get("pad")) {
            Some(n) if n >= 0.0 && n < PAD_COUNT as f64 => n as i64,
            _ => return err(format!("Kit {id}: invalid pad index")),
        };
        if !used.insert(pad) {
            return err(format!("Kit {id}: pad {pad} assigned twice"));
        }
        let file = req(p, "file", 2000)?;
        if !files.contains(file.as_str()) {
            return err(format!("Kit {id}: unknown file {file}"));
        }
        let mut out = PackKitPad {
            pad,
            file,
            name: None,
            settings: None,
        };
        if let Some(pad_name) = str_field(p, "name", 200, false)?
            && !pad_name.is_empty()
        {
            out.name = Some(pad_name);
        }
        if let Some(settings) = p.get("settings") {
            let Some(settings) = settings.as_object() else {
                return err(format!("Kit {id}: invalid pad settings"));
            };
            // Values are clamped later by normalizePad; only plain JSON scalars pass.
            let mut kept = Map::new();
            for (k, v) in js::order_keys(settings.clone()) {
                if k == "sample" || k == "__proto__" {
                    continue;
                }
                if matches!(
                    v,
                    Value::Number(_) | Value::String(_) | Value::Bool(_) | Value::Null
                ) {
                    kept.insert(k, js::normalize_number(&v));
                }
            }
            out.settings = Some(kept);
        }
        pads.push(out);
    }
    Ok(PackKit { id, name, pads })
}

fn safe_color(v: Option<&Value>) -> Res<String> {
    match v {
        Some(Value::String(c)) if is_safe_pack_color(c) => Ok(c.clone()),
        _ => err("Invalid \"color\" (#hex or oklch() only)"),
    }
}

/// Validates one manifest; the first problem is returned as `PackValidationError`.
pub fn validate_manifest(raw: &Value) -> Res<PackManifest> {
    let Some(raw) = raw.as_object() else {
        return err("Invalid pack");
    };
    let id = match raw.get("id") {
        Some(Value::String(id)) if is_valid_pack_id(id) => id.clone(),
        other => return err(format!("Invalid pack id: {}", js::string_of(other))),
    };
    manifest_body(raw, &id).map_err(|e| PackValidationError(format!("Pack {id}: {}", e.0)))
}

fn manifest_body(raw: &Obj, id: &str) -> Res<PackManifest> {
    let license = match raw.get("license") {
        Some(Value::String(l)) => PackLicense::from_id(l),
        _ => None,
    };
    let Some(license) = license else {
        return err(format!(
            "Unsupported licence: {}",
            js::string_of(raw.get("license"))
        ));
    };
    let source = req(raw, "source", 2000)?;
    if !is_github_source_url(&source) {
        return err("Source must be an https://github.com/ URL");
    }
    let tags: Vec<String> = match raw.get("tags") {
        Some(Value::Array(t))
            if t.iter()
                .all(|t| t.as_str().is_some_and(|s| js::len(s) <= 64)) =>
        {
            t.iter()
                .filter_map(|t| t.as_str().map(str::to_owned))
                .collect()
        }
        _ => return err("Invalid tags"),
    };
    let raw_files = match raw.get("files") {
        Some(Value::Array(f)) if !f.is_empty() => f,
        _ => return err("Pack has no files"),
    };
    if raw_files.len() > MAX_PACK_FILES {
        return err("Too many files");
    }
    let files = raw_files
        .iter()
        .map(validate_file)
        .collect::<Res<Vec<_>>>()?;
    let mut seen = HashSet::new();
    let mut declared = 0f64;
    for f in &files {
        // Case-insensitive: macOS/Windows file systems would merge these.
        if !seen.insert(f.path.to_lowercase()) {
            return err(format!("Duplicate file path: {}", f.path));
        }
        declared += f.size as f64;
    }
    if declared > MAX_PACK_BYTES as f64 {
        return err("Pack too large");
    }
    // A file and a folder must not share a name ("a.wav" and "a.wav/b.wav").
    for f in &files {
        let lower = f.path.to_lowercase();
        let segs: Vec<&str> = lower.split('/').collect();
        for i in 1..segs.len() {
            if seen.contains(&segs[..i].join("/")) {
                return err(format!("File/folder name clash: {}", f.path));
            }
        }
    }
    let file_paths: HashSet<&str> = files.iter().map(|f| f.path.as_str()).collect();
    let Some(Value::Array(raw_kits)) = raw.get("kits") else {
        return err("Invalid kits");
    };
    let kits = raw_kits
        .iter()
        .map(|k| validate_kit(k, &file_paths))
        .collect::<Res<Vec<_>>>()?;
    let mut kit_ids = HashSet::new();
    for k in &kits {
        if !kit_ids.insert(k.id.as_str()) {
            return err(format!("Duplicate kit id: {}", k.id));
        }
    }
    let version = match js::integer_of(raw.get("version")) {
        Some(v) if v >= 0.0 => v as i64,
        _ => return err("Invalid version"),
    };
    let name = req(raw, "name", 200)?;
    let tagline = req(raw, "tagline", 200)?;
    let description = req(raw, "description", 5000)?;
    let author = req(raw, "author", 500)?;
    let color = safe_color(raw.get("color"))?;
    let mut manifest = PackManifest {
        id: id.to_owned(),
        name,
        tagline,
        description,
        tags,
        author,
        source,
        license,
        color,
        files,
        kits,
        version,
        attribution: None,
    };
    let attribution = str_field(raw, "attribution", 2000, false)?.filter(|a| !a.is_empty());
    if license == PackLicense::CcBy4 && attribution.is_none() {
        return err("CC-BY pack needs attribution");
    }
    manifest.attribution = attribution;
    Ok(manifest)
}

/// `CatalogValidation`: the valid catalog plus the messages of the packs that were dropped.
#[derive(Debug, Clone, PartialEq)]
pub struct CatalogValidation {
    pub catalog: PackCatalog,
    pub errors: Vec<String>,
}

/// Validates a catalog. The envelope must be valid (error otherwise); invalid or duplicate
/// packs are dropped and reported in `errors`.
pub fn validate_catalog(raw: &Value) -> Res<CatalogValidation> {
    let Some(raw) = raw.as_object() else {
        return err("Catalog is not an object");
    };
    if raw.get("version").and_then(Value::as_f64) != Some(1.0) {
        return err(format!(
            "Unsupported catalog version: {}",
            js::string_of(raw.get("version"))
        ));
    }
    let updated_at = match raw.get("updatedAt") {
        Some(Value::String(s)) if js::len(s) <= 64 => s.clone(),
        _ => return err("Invalid catalog updatedAt"),
    };
    let Some(Value::Array(raw_packs)) = raw.get("packs") else {
        return err("Catalog has no packs array");
    };
    let mut errors = Vec::new();
    let mut packs = Vec::new();
    let mut ids = HashSet::new();
    for p in raw_packs {
        let r = validate_manifest(p).and_then(|m| {
            if ids.contains(&m.id) {
                return err(format!("Duplicate pack id: {}", m.id));
            }
            Ok(m)
        });
        match r {
            Ok(m) => {
                ids.insert(m.id.clone());
                packs.push(m);
            }
            Err(e) => errors.push(e.0),
        }
    }
    Ok(CatalogValidation {
        catalog: PackCatalog {
            version: 1,
            updated_at,
            packs,
        },
        errors,
    })
}

/// `new Date(0).toISOString()`.
pub const EPOCH_ISO: &str = "1970-01-01T00:00:00.000Z";

/// Validates a stored pack.json; `dir` is replaced by the folder it was read from.
pub fn validate_installed(raw: &Value, dir: &str) -> Res<InstalledPack> {
    let Some(raw) = raw.as_object() else {
        return err("Invalid pack.json");
    };
    let manifest = validate_manifest(raw.get("manifest").unwrap_or(&Value::Null))?;
    if raw.get("id").and_then(Value::as_str) != Some(manifest.id.as_str()) {
        return err("pack.json id mismatch");
    }
    let version = js::integer_of(raw.get("version"))
        .map(|v| v as i64)
        .unwrap_or(manifest.version);
    let installed_at = raw
        .get("installedAt")
        .and_then(Value::as_str)
        .unwrap_or(EPOCH_ISO)
        .to_owned();
    Ok(InstalledPack {
        id: manifest.id.clone(),
        version,
        dir: dir.to_owned(),
        installed_at,
        manifest,
    })
}

/// Sum of declared file sizes.
pub fn declared_pack_bytes(manifest: &PackManifest) -> i64 {
    manifest.files.iter().map(|f| f.size).sum()
}

/// The catalog file with exactly this URL, or None (the preview IPC fetches only these).
pub fn find_catalog_file<'a>(catalog: &'a PackCatalog, url: &str) -> Option<&'a PackFile> {
    catalog
        .packs
        .iter()
        .flat_map(|p| p.files.iter())
        .find(|f| f.url == url)
}

/// Where a merged catalog came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CatalogSource {
    Remote,
    Bundled,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MergedCatalog {
    pub catalog: PackCatalog,
    pub source: CatalogSource,
}

/// Chooses between a validated remote catalog and the bundled one (see the TS doc comment):
/// the remote copy must be strictly newer and non-empty; per pack a lower remote `version` or
/// a pack the remote dropped as invalid keeps the bundled entry; cleanly removed packs stay
/// removed; remote-only packs are added. `Date.parse` limits: see [`js::date_parse`].
pub fn merge_catalogs(
    bundled: &PackCatalog,
    remote: &PackCatalog,
    remote_errors: &[String],
) -> MergedCatalog {
    let rt = js::date_parse(&remote.updated_at);
    let bt = js::date_parse(&bundled.updated_at);
    if !rt.is_finite() || (bt.is_finite() && rt <= bt) {
        return MergedCatalog {
            catalog: bundled.clone(),
            source: CatalogSource::Bundled,
        };
    }
    if remote.packs.is_empty() && !bundled.packs.is_empty() {
        return MergedCatalog {
            catalog: bundled.clone(),
            source: CatalogSource::Bundled,
        };
    }
    // `new Map(...)`: the last bundled entry with an id wins.
    let bundled_by_id = |id: &str| bundled.packs.iter().rev().find(|p| p.id == id);
    let mut packs: Vec<PackManifest> = remote
        .packs
        .iter()
        .map(|p| match bundled_by_id(&p.id) {
            Some(b) if b.version > p.version => b.clone(),
            _ => p.clone(),
        })
        .collect();
    let have: HashSet<String> = packs.iter().map(|p| p.id.clone()).collect();
    for b in &bundled.packs {
        if have.contains(&b.id) {
            continue;
        }
        let prefix = format!("Pack {}:", b.id);
        let dup = format!("Duplicate pack id: {}", b.id);
        if remote_errors
            .iter()
            .any(|e| e.starts_with(&prefix) || *e == dup)
        {
            packs.push(b.clone());
        }
    }
    MergedCatalog {
        catalog: PackCatalog {
            version: 1,
            updated_at: remote.updated_at.clone(),
            packs,
        },
        source: CatalogSource::Remote,
    }
}
