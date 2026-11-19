//! Durable file replacement for everything the Rust app writes (project files, the settings
//! store, exports, recovery audio). Phase 10 review finding 10.
//!
//! [`write_atomic`] replaces `path` with `bytes` so that a crash or power cut leaves either the
//! old file or the new one, never a truncated mix:
//!
//! 1. A symlink is followed: the link's **target** is replaced and the link is kept (a rename
//!    over the link would have turned it into a regular file).
//! 2. The bytes go to a temp file next to the target (`.<name>.tmp-<pid>-<n>`), which takes
//!    the existing file's permissions, and are flushed to disk (`sync_all`).
//! 3. The temp file is renamed over the target, then the directory is synced so the rename
//!    itself survives a power cut.
//!
//! When the temp file can't be created because the directory isn't writable but the file is,
//! the file is rewritten in place (truncate + write + sync): not atomic, but the only way to
//! save there (Electron's `fs.writeFile` always writes in place). Temp files left behind by an
//! earlier crash (older than [`STALE_TMP_AGE`]) are removed the first time a file is written in
//! a process.

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Temp files older than this are leftovers of a crashed write.
pub const STALE_TMP_AGE: Duration = Duration::from_secs(10 * 60);

static SEQ: AtomicU64 = AtomicU64::new(0);

/// The file a write to `path` must replace: `path` itself, or the end of its symlink chain
/// (relative links resolve against the link's folder; at most 32 hops).
pub fn resolve_target(path: &Path) -> io::Result<PathBuf> {
    let mut p = path.to_path_buf();
    for _ in 0..32 {
        match fs::symlink_metadata(&p) {
            Ok(m) if m.file_type().is_symlink() => {
                let link = fs::read_link(&p)?;
                p = if link.is_absolute() {
                    link
                } else {
                    p.parent().unwrap_or(Path::new("")).join(link)
                };
            }
            _ => return Ok(p),
        }
    }
    Err(io::Error::other("too many levels of symbolic links"))
}

fn tmp_prefix(target: &Path) -> String {
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    format!(".{name}.tmp-")
}

/// Syncs a directory (makes a rename in it durable). A no-op where directories can't be
/// opened (Windows).
pub fn sync_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        File::open(dir)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        Ok(())
    }
}

/// Replaces `path` with `bytes` durably (see the module docs).
pub fn write_atomic(path: impl AsRef<Path>, bytes: &[u8]) -> io::Result<()> {
    let target = resolve_target(path.as_ref())?;
    let dir = target
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let prefix = tmp_prefix(&target);
    clean_stale_once(&dir, &prefix);
    let tmp = dir.join(format!(
        "{prefix}{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let perms = fs::metadata(&target).ok().map(|m| m.permissions());
    let mut f = match OpenOptions::new().write(true).create_new(true).open(&tmp) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied && target.is_file() => {
            // A folder we can't create files in, holding a file we may write: in place.
            return write_in_place(&target, bytes);
        }
        Err(e) => return Err(e),
    };
    let written = (|| {
        if let Some(p) = &perms {
            fs::set_permissions(&tmp, p.clone())?;
        }
        f.write_all(bytes)?;
        f.sync_all()
    })();
    drop(f);
    if let Err(e) = written.and_then(|_| fs::rename(&tmp, &target)) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    // The rename is durable once the folder is synced; a folder that can't be synced (some
    // network file systems) still has the complete file.
    let _ = sync_dir(&dir);
    Ok(())
}

fn write_in_place(target: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut f = OpenOptions::new().write(true).truncate(true).open(target)?;
    f.write_all(bytes)?;
    f.sync_all()
}

/// Removes `<prefix>*` files in `dir` older than [`STALE_TMP_AGE`], once per (dir, prefix) per
/// process.
fn clean_stale_once(dir: &Path, prefix: &str) {
    static DONE: Mutex<Option<HashSet<(PathBuf, String)>>> = Mutex::new(None);
    {
        let mut done = DONE.lock().unwrap_or_else(|e| e.into_inner());
        if !done
            .get_or_insert_with(HashSet::new)
            .insert((dir.to_path_buf(), prefix.to_owned()))
        {
            return;
        }
    }
    remove_stale_tmp(dir, prefix, STALE_TMP_AGE);
}

/// Removes `<prefix>*` files in `dir` last modified more than `age` ago. Returns how many.
pub fn remove_stale_tmp(dir: &Path, prefix: &str, age: Duration) -> usize {
    let Ok(rd) = fs::read_dir(dir) else {
        return 0;
    };
    let mut n = 0;
    for e in rd.flatten() {
        let name = e.file_name();
        if !name.to_string_lossy().starts_with(prefix) {
            continue;
        }
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|d| d >= age);
        if old && fs::remove_file(e.path()).is_ok() {
            n += 1;
        }
    }
    n
}

/// The temp-file prefix [`write_atomic`] uses for `path` (tests, cleanup).
pub fn tmp_prefix_for(path: &Path) -> String {
    tmp_prefix(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "mdaw-fsio-{tag}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn replaces_the_file_and_leaves_no_temp() {
        let d = dir("plain");
        let f = d.join("a.json");
        write_atomic(&f, b"one").unwrap();
        write_atomic(&f, b"two").unwrap();
        assert_eq!(fs::read(&f).unwrap(), b"two");
        let names: Vec<_> = fs::read_dir(&d).unwrap().flatten().collect();
        assert_eq!(names.len(), 1, "no temp file left");
    }

    #[cfg(unix)]
    #[test]
    fn follows_symlinks_and_keeps_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let d = dir("link");
        let real = d.join("real.mdaw");
        fs::write(&real, b"old").unwrap();
        fs::set_permissions(&real, fs::Permissions::from_mode(0o640)).unwrap();
        let link = d.join("link.mdaw");
        std::os::unix::fs::symlink("real.mdaw", &link).unwrap();
        write_atomic(&link, b"new").unwrap();
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink(),
            "the link stays a link"
        );
        assert_eq!(fs::read(&real).unwrap(), b"new");
        assert_eq!(
            fs::metadata(&real).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_unwritable_folder_falls_back_to_writing_in_place() {
        use std::os::unix::fs::PermissionsExt;
        let d = dir("ro");
        let f = d.join("song.mdaw");
        fs::write(&f, b"old").unwrap();
        fs::set_permissions(&d, fs::Permissions::from_mode(0o555)).unwrap();
        let r = write_atomic(&f, b"saved in place");
        fs::set_permissions(&d, fs::Permissions::from_mode(0o755)).unwrap();
        if running_as_root() {
            return; // root ignores the folder's mode
        }
        r.unwrap();
        assert_eq!(fs::read(&f).unwrap(), b"saved in place");
        // A missing file in an unwritable folder is an error, not a silent no-op.
        fs::set_permissions(&d, fs::Permissions::from_mode(0o555)).unwrap();
        let r = write_atomic(d.join("new.mdaw"), b"x");
        fs::set_permissions(&d, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(r.is_err());
    }

    #[cfg(unix)]
    fn running_as_root() -> bool {
        std::env::var("USER").as_deref() == Ok("root")
    }

    #[test]
    fn stale_temp_files_are_removed() {
        let d = dir("stale");
        let f = d.join("x.json");
        let prefix = tmp_prefix_for(&f);
        let stale = d.join(format!("{prefix}999-0"));
        fs::write(&stale, b"half").unwrap();
        assert_eq!(remove_stale_tmp(&d, &prefix, Duration::from_secs(3600)), 0);
        assert_eq!(remove_stale_tmp(&d, &prefix, Duration::ZERO), 1);
        assert!(!stale.exists());
    }

    #[test]
    fn a_missing_folder_is_an_error() {
        let d = dir("missing").join("nope");
        assert!(write_atomic(d.join("a.mdaw"), b"x").is_err());
    }
}
