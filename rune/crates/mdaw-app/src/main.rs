use rune::{AppSpec, Cx, El, col, text};
#[derive(Default)]
struct State;
fn view(_s: &State, _: &mut Cx<State>) -> El {
    col().w_full().h_full().center().child(text("micro daw"))
}
fn main() {
    let _ = cpal::default_host();
    let _ = midir::MidiInput::new("probe");
    rune::run(AppSpec::new(
        "micro daw",
        (1440.0, 900.0),
        State::default,
        view,
    ));
}
