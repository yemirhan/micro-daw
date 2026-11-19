//! Phase 10 review finding 5: two processes on one data folder. The test holds the lock, runs
//! the real binary (`--instance-probe`) against the same folder, then releases it.

use std::process::Command;

fn probe(dir: &std::path::Path) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_micro-daw"))
        .arg("--instance-probe")
        .env("MDAW_DATA_DIR", dir)
        .output()
        .expect("runs the app binary");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).trim().to_owned(),
    )
}

#[test]
fn a_second_process_on_the_same_data_folder_is_refused() {
    let dir = std::env::temp_dir().join(format!("mdaw-instance-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let lock = mdaw_app::instance::acquire(&dir).expect("this process takes the lock");
    assert_eq!(probe(&dir), (3, "busy".to_owned()));
    // A store this process wrote stays untouched by the refused one.
    drop(lock);
    assert_eq!(probe(&dir), (0, "acquired".to_owned()));
    let _ = std::fs::remove_dir_all(&dir);
}
