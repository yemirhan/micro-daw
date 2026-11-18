fn main() {
    // First thing: the process's first CoreMIDI client lives on a pumped run loop, so external
    // MIDI hot-plug is seen (mdaw-midi `notify.rs`); a no-op off macOS.
    mdaw_midi::notify::ensure_started();
    let headless = std::env::args().any(|a| {
        matches!(
            a.as_str(),
            "--snap" | "--tree" | "--script" | "--frames" | "--video" | "--trace" | "--list-scenes"
        )
    });
    rune::App::new(mdaw_app::spec(headless)).run();
}
