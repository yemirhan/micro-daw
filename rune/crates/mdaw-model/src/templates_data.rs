//! The project templates' JSON (`ALL_TEMPLATES`, `src/data/templates/index.ts` order), kept in
//! the Rust tree (`mdaw-model/assets/templates/`) so the Rust app builds without the Electron
//! sources (phase 10 review finding 19). The files are byte-for-byte copies of
//! `src/data/templates/*.json`; `templates_match_the_electron_copies` checks that while `src/`
//! still exists.

/// `(id, file contents)` in `ALL_TEMPLATES` order.
pub const TEMPLATE_FILES: [(&str, &str); 6] = [
    (
        "blank-4track",
        include_str!("../assets/templates/blank-4track.json"),
    ),
    (
        "rock-song",
        include_str!("../assets/templates/rock-song.json"),
    ),
    (
        "hip-hop-beat",
        include_str!("../assets/templates/hip-hop-beat.json"),
    ),
    (
        "jazz-trio",
        include_str!("../assets/templates/jazz-trio.json"),
    ),
    (
        "edm-drop",
        include_str!("../assets/templates/edm-drop.json"),
    ),
    (
        "ambient-pad",
        include_str!("../assets/templates/ambient-pad.json"),
    ),
];

/// The template file `id` (None when unknown).
pub fn template_file(id: &str) -> Option<&'static str> {
    TEMPLATE_FILES
        .iter()
        .find(|(i, _)| *i == id)
        .map(|(_, s)| *s)
}

#[cfg(test)]
mod tests {
    /// The Rust copies equal the Electron app's files. Skipped (with a message) when `src/` is
    /// absent: the Rust tree no longer needs it.
    #[test]
    fn templates_match_the_electron_copies() {
        let src =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../src/data/templates");
        if !src.is_dir() {
            eprintln!(
                "src/data/templates absent: nothing to compare (the Rust tree is self-contained)"
            );
            return;
        }
        for (id, body) in super::TEMPLATE_FILES {
            let ts = std::fs::read_to_string(src.join(format!("{id}.json"))).unwrap();
            assert_eq!(ts, body, "{id}.json differs from src/data/templates");
        }
    }
}
