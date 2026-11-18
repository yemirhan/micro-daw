//! File-system fault injection (the Electron suite spies on `fs.rename` / `fs.open` /
//! `fs.writeFile` / `FileHandle.close`, `installer.test.ts` "installer fault injection (L-f)").
//! The installer and downloader call these hooks right before the matching operation; the
//! default is a no-op. Tests only.

use std::io;
use std::path::Path;

/// The file-system operations a hook can fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsOp {
    /// `fs.rename(a, b)`.
    Rename,
    /// `fs.open(a, 'wx')`: a download's destination file.
    Open,
    /// `fs.writeFile(a)`: the staging `pack.json`.
    WriteFile,
}

pub trait FsFaults: Send + Sync {
    /// Called before `op` on `a` (and `b` for a rename); an error fails the operation.
    fn before(&self, op: FsOp, a: &Path, b: Option<&Path>) -> io::Result<()> {
        let _ = (op, a, b);
        Ok(())
    }
    /// Called before a download's file is closed (a slow close delays the cleanup of an aborted
    /// download: the B2 race).
    fn before_close(&self, path: &Path) {
        let _ = path;
    }
}

/// No faults.
pub struct NoFaults;

impl FsFaults for NoFaults {}
