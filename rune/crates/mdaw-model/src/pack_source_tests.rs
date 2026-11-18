//! Ported from src/shared/packSource.test.ts (pack sample references, P1).

use super::*;

fn r(pack_id: &str, file: &str) -> Option<PackRef> {
    Some(PackRef {
        pack_id: pack_id.into(),
        file: file.into(),
    })
}

#[test]
fn resolves_pack_id_and_path_against_the_packs_folder() {
    assert_eq!(
        resolve_pack_file_path("/u/packs", "tr808-fischer", "bass-drum/bd0010.wav").as_deref(),
        Some("/u/packs/tr808-fischer/bass-drum/bd0010.wav")
    );
    assert_eq!(
        resolve_pack_file_path("/u/packs/", "p", "a.wav").as_deref(),
        Some("/u/packs/p/a.wav")
    );
    assert_eq!(
        resolve_pack_file_path("C:\\Users\\me\\packs", "p", "kicks/bd.wav").as_deref(),
        Some("C:\\Users\\me\\packs\\p\\kicks\\bd.wav")
    );
    assert_eq!(
        pack_dir_prefix("/u/packs", "p").as_deref(),
        Some("/u/packs/p/")
    );
}

#[test]
fn refuses_unsafe_ids_and_paths() {
    assert_eq!(resolve_pack_file_path("/u/packs", "../x", "a.wav"), None);
    assert_eq!(
        resolve_pack_file_path("/u/packs", "p", "../../etc/passwd.wav"),
        None
    );
    assert_eq!(resolve_pack_file_path("/u/packs", "p", "/abs.wav"), None);
    assert_eq!(resolve_pack_file_path("", "p", "a.wav"), None);
}

#[test]
fn migrates_absolute_paths_inside_the_packs_folder_and_only_those() {
    assert_eq!(
        pack_ref_from_path(
            Some("/u/packs"),
            "/u/packs/tr808-fischer/bass-drum/bd0010.wav"
        ),
        r("tr808-fischer", "bass-drum/bd0010.wav")
    );
    assert_eq!(
        pack_ref_from_path(Some("C:\\u\\packs"), "C:\\u\\packs\\p\\kicks\\bd.wav"),
        r("p", "kicks/bd.wav")
    );
    assert_eq!(
        pack_ref_from_path(Some("/u/packs"), "/u/packs-other/p/a.wav"),
        None
    );
    assert_eq!(pack_ref_from_path(Some("/u/packs"), "/u/packs/p"), None);
    assert_eq!(
        pack_ref_from_path(Some("/u/packs"), "/u/packs/.partial-p/a.wav"),
        None
    );
    assert_eq!(
        pack_ref_from_path(Some("/u/packs"), "/u/packs/p/../q/a.wav"),
        None
    );
    assert_eq!(
        pack_ref_from_path(Some("/u/packs"), "/Users/me/kick.wav"),
        None
    );
    assert_eq!(pack_ref_from_path(None, "/u/packs/p/a.wav"), None);
}

#[test]
fn reads_the_packs_folder_from_the_renderer_argv() {
    let arg = format!("{PACKS_DIR_ARG}/u/packs");
    assert_eq!(
        packs_dir_from_argv(Some(&["electron", arg.as_str()][..])).as_deref(),
        Some("/u/packs")
    );
    assert_eq!(packs_dir_from_argv(Some(&["electron"][..])), None);
    assert_eq!(packs_dir_from_argv(Some(&[PACKS_DIR_ARG][..])), None);
    assert_eq!(packs_dir_from_argv::<&str>(None), None);
}

#[test]
fn pack_path_safety_edge_cases() {
    for ok in [
        "a.wav",
        "kicks/bd.WAV",
        "a/b/c/d/e/f/g/h.flac",
        "readme.md",
        "con1x.wav",
        "x.aiff",
    ] {
        assert!(is_safe_pack_path(ok), "{ok}");
    }
    for bad in [
        "",
        "/a.wav",
        "a\\b.wav",
        "a//b.wav",
        "./a.wav",
        "a/../b.wav",
        ".hidden.wav",
        "a.wav.",
        "a .wav/",
        "con.wav",
        "COM1.wav",
        "lpt9",
        "nul",
        "pack.json",
        "a/b/c/d/e/f/g/h/i.wav",
        "a.exe",
        "noext",
        "a:b.wav",
        "a\u{1}.wav",
    ] {
        assert!(!is_safe_pack_path(bad), "{bad:?}");
    }
    assert!(is_safe_pack_path("sub/pack.json.wav"));
    assert!(!is_safe_pack_path(&format!("{}.wav", "x".repeat(130))));
    assert!(is_valid_pack_id("tr808-fischer"));
    for bad in ["", "-a", "a-", "a--b", "A", "a_b", &"a".repeat(65)] {
        assert!(!is_valid_pack_id(bad), "{bad}");
    }
}
