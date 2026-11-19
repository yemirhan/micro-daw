//! Phase 10 review finding 5: one app process per data folder.
//!
//! Every setting, the autosave and the recovery checkpoint live in one file
//! (`local-storage.json`) that each process rewrites whole, so two processes on the same data
//! folder would overwrite each other's work. Before it opens anything there, a window run takes
//! an exclusive advisory lock (`flock` on Unix, `LockFileEx` on Windows, through
//! `std::fs::File::try_lock`) on `<data dir>/instance.lock` and holds it until it exits (the OS
//! releases it on any exit, a crash included). A second launch can't take it: it shows
//! "Micro DAW is already running" and quits without touching the data folder.
//!
//! macOS normally routes a second launch of the bundle to the running app; the lock covers
//! `open -n`, a second copy of the bundle, the bare binary, and Windows / Linux.

use std::fs::{File, OpenOptions};
use std::path::Path;

/// The lock file's name in the data folder.
pub const LOCK_FILE: &str = "instance.lock";

/// The held lock (dropping it releases it).
#[derive(Debug)]
pub struct InstanceLock {
    _file: File,
}

/// Why the lock wasn't taken.
#[derive(Debug)]
pub enum LockError {
    /// Another process holds it.
    Busy,
    /// The lock file couldn't be opened (the data folder isn't writable).
    Io(std::io::Error),
}

/// Takes the data folder's lock (creating the folder and the file as needed).
pub fn acquire(data_dir: &Path) -> Result<InstanceLock, LockError> {
    std::fs::create_dir_all(data_dir).map_err(LockError::Io)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(data_dir.join(LOCK_FILE))
        .map_err(LockError::Io)?;
    match file.try_lock() {
        Ok(()) => Ok(InstanceLock { _file: file }),
        Err(std::fs::TryLockError::WouldBlock) => Err(LockError::Busy),
        Err(std::fs::TryLockError::Error(e)) => Err(LockError::Io(e)),
    }
}

/// The message a second instance shows.
pub const BUSY_MESSAGE: &str = "Micro DAW is already running. Switch to its window (it may be \
    in the Dock or behind other windows); a second copy would overwrite its unsaved work.";

/// The second instance's window: the message and a Quit button.
pub mod busy {
    use rune::{AppSpec, Cx, El, Tx, col, text};

    use crate::theme::{R_MD, t, ui};

    pub struct Busy;

    fn view(_s: &Busy, _cx: &mut Cx<Busy>) -> El {
        let th = t();
        col()
            .key("root")
            .w_full()
            .h_full()
            .fill(th.surface0)
            .padding(24.0)
            .gap(16.0)
            .justify_center()
            .child(
                text("Micro DAW is already running")
                    .font(ui(15.0, 600.0))
                    .color(th.fg),
            )
            .child(
                text(super::BUSY_MESSAGE)
                    .key("busy-message")
                    .role("alert")
                    .font(ui(13.0, 400.0))
                    .color(th.muted_fg)
                    .wrap(),
            )
            .child(
                rune::row()
                    .key("busy-quit")
                    .role("button")
                    .label("Quit")
                    .h(28.0)
                    .px(12.0)
                    .center()
                    .radius(R_MD)
                    .fill(th.primary)
                    .hover(th.primary.alpha(0.88))
                    .child(text("Quit").font(ui(13.0, 600.0)).color(th.primary_fg))
                    .on_press_tx(|_s: &mut Busy, tx: &mut Tx| tx.quit()),
            )
    }

    /// A small window with [`super::BUSY_MESSAGE`].
    pub fn spec() -> AppSpec<Busy> {
        AppSpec {
            fonts: crate::theme::FONTS,
            families: crate::theme::FAMILIES,
            ..AppSpec::new("Micro DAW", (460.0, 200.0), || Busy, view)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_lock_on_the_same_folder_is_refused_until_the_first_is_dropped() {
        let dir = std::env::temp_dir().join(format!("mdaw-lock-{}", uuid::Uuid::new_v4()));
        let first = acquire(&dir).expect("first instance");
        assert!(matches!(acquire(&dir), Err(LockError::Busy)));
        drop(first);
        let again = acquire(&dir).expect("free again once the first exits");
        drop(again);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
