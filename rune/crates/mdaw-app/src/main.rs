fn main() {
    // First thing: the process's first CoreMIDI client lives on a pumped run loop, so external
    // MIDI hot-plug is seen (mdaw-midi `notify.rs`); a no-op off macOS.
    mdaw_midi::notify::ensure_started();
    let args: Vec<String> = std::env::args().collect();
    // `--instance-probe`: takes the data folder's lock, says whether it got it, exits (the
    // single-instance test runs the binary with it).
    if args.iter().any(|a| a == "--instance-probe") {
        let code = match mdaw_app::instance::acquire(&mdaw_app::services::data_dir()) {
            Ok(_) => {
                println!("acquired");
                0
            }
            Err(mdaw_app::instance::LockError::Busy) => {
                println!("busy");
                3
            }
            Err(mdaw_app::instance::LockError::Io(e)) => {
                println!("error: {e}");
                4
            }
        };
        std::process::exit(code);
    }
    let headless = args.iter().any(|a| {
        matches!(
            a.as_str(),
            "--snap" | "--tree" | "--script" | "--frames" | "--video" | "--trace" | "--list-scenes"
        )
    });
    // A window run owns its data folder (review finding 5): a second one shows a message and
    // quits without opening the store.
    let _lock = if headless {
        None
    } else {
        match mdaw_app::instance::acquire(&mdaw_app::services::data_dir()) {
            Ok(l) => Some(l),
            Err(mdaw_app::instance::LockError::Busy) => {
                eprintln!("{}", mdaw_app::instance::BUSY_MESSAGE);
                rune::App::new(mdaw_app::instance::busy::spec()).run();
                return;
            }
            // An unwritable data folder: run anyway (the store reports its own write errors).
            Err(mdaw_app::instance::LockError::Io(e)) => {
                eprintln!("micro-daw: could not take the instance lock: {e}");
                None
            }
        }
    };
    mdaw_app::app_with_hooks(mdaw_app::spec(headless), !headless).run();
}
