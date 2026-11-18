use rune::{AppSpec, Cx, El, col, text};
#[derive(Default)]
struct State;
fn view(_s: &State, _: &mut Cx<State>) -> El {
    col().w_full().h_full().center().child(text("micro daw"))
}
fn main() {
    // first thing: the process's first CoreMIDI client lives on a pumped run loop, so external
    // MIDI hot-plug is seen (mdaw-midi `notify.rs`); a no-op off macOS
    mdaw_midi::notify::ensure_started();
    let _ = cpal::default_host();
    let _ = midir::MidiInput::new("probe");
    rune::run(AppSpec::new(
        "micro daw",
        (1440.0, 900.0),
        State::default,
        view,
    ));
}
