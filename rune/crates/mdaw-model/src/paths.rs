//! Pure path helpers shared by the whole app (POSIX and Windows separators).
//! Port of `src/shared/paths.ts`.
//!
//! JS string indices are UTF-16 code units; every index used here is the position of an ASCII
//! separator or dot, so byte indices give identical results.

/// `separatorOf`: '\\' for Windows-style paths (backslash and no slash), otherwise '/'.
pub fn separator_of(p: &str) -> char {
    if p.contains('\\') && !p.contains('/') {
        '\\'
    } else {
        '/'
    }
}

fn is_sep(c: char) -> bool {
    c == '/' || c == '\\'
}

/// `/^[A-Za-z]:[\\/]$/`
fn is_drive_root(p: &str) -> bool {
    let b = p.as_bytes();
    b.len() == 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'/' || b[2] == b'\\')
}

/// `trimTrailingSep`: removes trailing separators, keeping filesystem roots ("/", "C:\\") intact.
pub fn trim_trailing_sep(p: &str) -> String {
    let mut out = p;
    // `out.length > 1` (UTF-16 units): a string ending in an ASCII separator with more than one
    // unit has more than one byte too, and vice versa.
    while out.len() > 1 && out.ends_with(is_sep) && !is_drive_root(out) {
        out = &out[..out.len() - 1];
    }
    out.to_owned()
}

/// `basename`: last path segment ("/a/b/c" → "c").
pub fn basename(p: &str) -> String {
    let trimmed = trim_trailing_sep(p);
    match trimmed.rfind(is_sep) {
        Some(idx) => {
            let base = &trimmed[idx + 1..];
            if base.is_empty() {
                trimmed.clone()
            } else {
                base.to_owned()
            }
        }
        None => trimmed,
    }
}

/// `joinPath(dir, name)`.
pub fn join_path(dir: &str, name: &str) -> String {
    let sep = separator_of(dir);
    let base = trim_trailing_sep(dir);
    if base.ends_with(is_sep) {
        format!("{base}{name}")
    } else {
        format!("{base}{sep}{name}")
    }
}

/// `displayName`: file name without extension ("Kick 01.wav" → "Kick 01"; ".hidden" stays).
pub fn display_name(file_name: &str) -> String {
    match file_name.rfind('.') {
        Some(dot) if dot > 0 => file_name[..dot].to_owned(),
        _ => file_name.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separators_and_trimming() {
        assert_eq!(separator_of("/a/b"), '/');
        assert_eq!(separator_of("C:\\a\\b"), '\\');
        assert_eq!(separator_of("C:\\a/b"), '/');
        assert_eq!(trim_trailing_sep("/u/packs///"), "/u/packs");
        assert_eq!(trim_trailing_sep("/"), "/");
        assert_eq!(trim_trailing_sep("C:\\"), "C:\\");
        assert_eq!(trim_trailing_sep("C:\\\\"), "C:\\");
        assert_eq!(trim_trailing_sep("//"), "/");
    }

    #[test]
    fn basename_join_display() {
        assert_eq!(basename("/a/b/c"), "c");
        assert_eq!(basename("/a/b/c/"), "c");
        assert_eq!(basename("C:\\x\\Kick.wav"), "Kick.wav");
        assert_eq!(basename("/"), "/");
        assert_eq!(basename("plain"), "plain");
        assert_eq!(join_path("/a/", "b"), "/a/b");
        assert_eq!(join_path("C:\\a", "b"), "C:\\a\\b");
        assert_eq!(join_path("/", "b"), "/b");
        assert_eq!(join_path("C:\\", "b"), "C:\\b");
        assert_eq!(display_name("Kick 01.wav"), "Kick 01");
        assert_eq!(display_name(".hidden"), ".hidden");
        assert_eq!(display_name("a.b.c"), "a.b");
        assert_eq!(display_name("noext"), "noext");
    }
}
