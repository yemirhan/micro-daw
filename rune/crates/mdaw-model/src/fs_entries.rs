//! Port of `src/shared/fsEntries.ts`: which files the sample browser lists (audio extensions,
//! hidden dotfiles), the `FsDirEntry` shape built by main's `fs:list-dir`, and the listing
//! order (folders first, then `Intl.Collator(undefined, { sensitivity: 'base', numeric: true })`
//! with a raw `<` tie-break).
//!
//! ## Collation
//! [`collator_compare`] reimplements that collator (ICU root collation, en-US) at the primary
//! level: case- and accent-insensitive, punctuation and symbols significant (ordered before
//! digits, digits before letters, like ICU), digit runs compared by numeric value (leading
//! zeros ignored), C0/C1 controls and the soft hyphen ignorable, and ICU's expansions in
//! Latin-1 / Latin Extended-A/B (`ß`=ss, `æ`=ae, `œ`=oe, `ĳ`=ij, `ǆ`=dz, …).
//!
//! The weight table for U+0000..U+024F was generated from Node's ICU
//! (`Intl.Collator(undefined,{sensitivity:'base',numeric:true})`, see
//! `rune/parity/ts/fs_entries.golden.ts` for the parity check), so every name made of those
//! characters sorts exactly like the Electron app.
//!
//! Limits: characters above U+024F are compatibility-decomposed (NFKD), combining marks are
//! ignored, letters are lower-cased, and anything still outside the table sorts after all
//! Latin letters by code point (ICU would interleave Greek, Cyrillic, CJK, … by script).
//! Digit runs use ASCII `0-9` only.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

use crate::packs::js;

/// Audio file extensions the browser lists (lower-case, no dot).
pub const AUDIO_EXTENSIONS: [&str; 7] = ["wav", "aif", "aiff", "mp3", "flac", "ogg", "m4a"];

/// Lower-case extension without the dot ("Kick.WAV" → "wav"); "" when there is none.
pub fn ext_of(name: &str) -> String {
    match name.rfind('.') {
        Some(dot) if dot > 0 && dot != name.len() - 1 => name[dot + 1..].to_lowercase(),
        _ => String::new(),
    }
}

pub fn is_audio_file_name(name: &str) -> bool {
    AUDIO_EXTENSIONS.contains(&ext_of(name).as_str())
}

/// Hidden entries (dotfiles such as .DS_Store, ._foo.wav AppleDouble files).
pub fn is_hidden_name(name: &str) -> bool {
    name.starts_with('.')
}

/// `FsDirEntry` (src/types/electron.ts): one row of `fs:list-dir`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FsDirEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    /// Lower-case extension ('' for folders).
    pub ext: String,
}

/// Anything with a name and a folder flag (`NamedEntry`).
pub trait NamedEntry {
    fn name(&self) -> &str;
    fn is_dir(&self) -> bool;
}

impl NamedEntry for FsDirEntry {
    fn name(&self) -> &str {
        &self.name
    }
    fn is_dir(&self) -> bool {
        self.is_dir
    }
}

impl NamedEntry for (String, bool) {
    fn name(&self) -> &str {
        &self.0
    }
    fn is_dir(&self) -> bool {
        self.1
    }
}

impl NamedEntry for (&str, bool) {
    fn name(&self) -> &str {
        self.0
    }
    fn is_dir(&self) -> bool {
        self.1
    }
}

/// Case-insensitive comparison: folders first, then files, natural number order.
pub fn compare_entries<E: NamedEntry + ?Sized>(a: &E, b: &E) -> Ordering {
    if a.is_dir() != b.is_dir() {
        return if a.is_dir() {
            Ordering::Less
        } else {
            Ordering::Greater
        };
    }
    match collator_compare(a.name(), b.name()) {
        Ordering::Equal => js::utf16_cmp(a.name(), b.name()),
        o => o,
    }
}

/// A new list sorted folders-first, then by name (stable, like `Array.prototype.sort`).
pub fn sort_entries<E: NamedEntry + Clone>(entries: &[E]) -> Vec<E> {
    let mut out = entries.to_vec();
    out.sort_by(|a, b| compare_entries(a, b));
    out
}

/// What a directory entry is (after following symlinks; a dangling link is `Other`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Dir,
    File,
    Other,
}

/// `Invalid directory path` rule of `fs:list-dir` (the absolute-path check is the caller's).
pub fn check_list_dir_path(dir: &str) -> Result<(), &'static str> {
    if dir.is_empty() || js::len(dir) > 4096 || dir.contains('\0') {
        return Err("Invalid directory path");
    }
    Ok(())
}

/// The pure half of `fs:list-dir`: hidden names dropped, folders kept, files kept when they are
/// audio, `path` = `dir` + `sep` + name, sorted with [`sort_entries`].
pub fn list_entries<I, S>(dir: &str, sep: char, raw: I) -> Vec<FsDirEntry>
where
    I: IntoIterator<Item = (S, EntryKind)>,
    S: Into<String>,
{
    let base = dir.trim_end_matches(sep);
    let mut entries = Vec::new();
    for (name, kind) in raw {
        let name: String = name.into();
        if is_hidden_name(&name) {
            continue;
        }
        let path = format!("{base}{sep}{name}");
        match kind {
            EntryKind::Dir => entries.push(FsDirEntry {
                name,
                path,
                is_dir: true,
                ext: String::new(),
            }),
            EntryKind::File if is_audio_file_name(&name) => {
                let ext = ext_of(&name);
                entries.push(FsDirEntry {
                    name,
                    path,
                    is_dir: false,
                    ext,
                });
            }
            _ => {}
        }
    }
    sort_entries(&entries)
}

// ---------------------------------------------------------------------------
// Collation
// ---------------------------------------------------------------------------

/// Expansion marker in [`PRIMARY`].
const EXPANDS: u16 = 0xffff;
/// Weight of `'0'` (every number sorts here; numbers compare by value among themselves).
const DIGIT_WEIGHT: u32 = 63;
/// Weight of U+02BC MODIFIER LETTER APOSTROPHE (the first half of `ŉ`).
const APOSTROPHE_WEIGHT: u32 = 176;
/// Characters outside the table sort after every table weight.
const OUTSIDE_BASE: u32 = 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Elem {
    W(u32),
    /// Digit run without leading zeros ("" = 0).
    Num(String),
}

fn expansion(c: char) -> Option<&'static str> {
    Some(match c as u32 {
        0xc6 | 0xe6 | 0x1e2 | 0x1e3 | 0x1fc | 0x1fd => "ae",
        0xdf => "ss",
        0x132 | 0x133 => "ij",
        0x149 => "\u{2bc}n",
        0x152 | 0x153 => "oe",
        0x18d => "zw",
        0x1be => "ts",
        0x1c4..=0x1c6 | 0x1f1..=0x1f3 => "dz",
        0x1c7..=0x1c9 => "lj",
        0x1ca..=0x1cc => "nj",
        0x238 => "db",
        0x239 => "qp",
        _ => return None,
    })
}

/// Pushes the weights of one non-digit character (0 weights are ignorable and skipped).
fn push_char(c: char, out: &mut Vec<u32>) {
    let cp = c as u32;
    if cp == 0x2bc {
        out.push(APOSTROPHE_WEIGHT);
        return;
    }
    if (cp as usize) < PRIMARY.len() {
        let w = PRIMARY[cp as usize];
        if w == EXPANDS {
            for e in expansion(c).unwrap_or("").chars() {
                push_char(e, out);
            }
        } else if w != 0 {
            out.push(w as u32);
        }
        return;
    }
    // Outside the table: decompose (compatibility, so ligatures / full-width forms fold),
    // drop combining marks, lower-case.
    let decomposed: Vec<char> = c.to_string().nfkd().collect();
    if decomposed.len() > 1 || decomposed[0] != c {
        for d in decomposed {
            if !is_combining(d) {
                push_char(d, out);
            }
        }
        return;
    }
    if is_combining(c) {
        return;
    }
    let lower = c.to_lowercase().next().unwrap_or(c);
    out.push(OUTSIDE_BASE + lower as u32);
}

fn is_combining(c: char) -> bool {
    matches!(c as u32, 0x300..=0x36f | 0x1ab0..=0x1aff | 0x1dc0..=0x1dff | 0x20d0..=0x20ff | 0xfe20..=0xfe2f)
}

fn elems(s: &str, numeric: bool) -> Vec<Elem> {
    let mut out = Vec::new();
    let mut ws = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if numeric && c.is_ascii_digit() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            let run: String = chars[start..i].iter().collect();
            out.push(Elem::Num(run.trim_start_matches('0').to_owned()));
            continue;
        }
        ws.clear();
        push_char(c, &mut ws);
        out.extend(ws.iter().map(|w| Elem::W(*w)));
        i += 1;
    }
    out
}

fn cmp_elem(a: &Elem, b: &Elem) -> Ordering {
    match (a, b) {
        (Elem::W(x), Elem::W(y)) => x.cmp(y),
        (Elem::Num(x), Elem::Num(y)) => x.len().cmp(&y.len()).then_with(|| x.cmp(y)),
        (Elem::Num(_), Elem::W(y)) => DIGIT_WEIGHT.cmp(y).then(Ordering::Less),
        (Elem::W(x), Elem::Num(_)) => x.cmp(&DIGIT_WEIGHT).then(Ordering::Greater),
    }
}

fn primary_cmp(a: &str, b: &str, numeric: bool) -> Ordering {
    let (ea, eb) = (elems(a, numeric), elems(b, numeric));
    for (x, y) in ea.iter().zip(eb.iter()) {
        let o = cmp_elem(x, y);
        if o != Ordering::Equal {
            return o;
        }
    }
    ea.len().cmp(&eb.len())
}

/// `new Intl.Collator(undefined, { sensitivity: 'base', numeric: true }).compare(a, b)`
/// (see the module doc for the exact scope).
pub fn collator_compare(a: &str, b: &str) -> Ordering {
    primary_cmp(a, b, true)
}

/// `a.localeCompare(b)` (default options: tertiary strength, no numeric ordering).
/// Approximation: the primary level is exact for the table's characters; ties are broken by
/// accents (unaccented first, then by combining-mark code points), then by case (lower case
/// first, as ICU), else equal.
pub fn locale_compare(a: &str, b: &str) -> Ordering {
    primary_cmp(a, b, false)
        .then_with(|| {
            let marks = |s: &str| -> Vec<char> { s.nfd().filter(|c| is_combining(*c)).collect() };
            marks(a).cmp(&marks(b))
        })
        .then_with(|| {
            let case = |s: &str| -> Vec<bool> {
                s.chars()
                    .filter(|c| c.is_alphabetic())
                    .map(char::is_uppercase)
                    .collect()
            };
            case(a).cmp(&case(b))
        })
}

/// Primary weights of U+0000..U+024F (0 = ignorable, 0xffff = expansion), generated from ICU.
#[rustfmt::skip]
const PRIMARY: [u16; 592] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4, 5, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    7, 13, 20, 36, 60, 37, 35, 19, 23, 24, 32, 47, 10, 9, 17, 33,
    63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 12, 11, 51, 52, 53, 15,
    31, 79, 81, 85, 88, 93, 98, 100, 105, 107, 111, 114, 116, 120, 121, 126,
    130, 132, 135, 138, 142, 149, 153, 156, 157, 158, 162, 25, 34, 26, 40, 8,
    38, 79, 81, 85, 88, 93, 98, 100, 105, 107, 111, 114, 116, 120, 121, 126,
    130, 132, 135, 138, 142, 149, 153, 156, 157, 158, 162, 27, 55, 28, 57, 0,
    0, 0, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    7, 14, 59, 61, 58, 62, 56, 29, 42, 45, 79, 21, 54, 0, 46, 41,
    44, 48, 76, 77, 39, 181, 30, 18, 43, 73, 126, 22, 75, 74, 78, 16,
    79, 79, 79, 79, 79, 79, 65535, 85, 93, 93, 93, 93, 107, 107, 107, 107,
    88, 121, 126, 126, 126, 126, 126, 50, 126, 149, 149, 149, 149, 158, 169, 65535,
    79, 79, 79, 79, 79, 79, 65535, 85, 93, 93, 93, 93, 107, 107, 107, 107,
    88, 121, 126, 126, 126, 126, 126, 49, 126, 149, 149, 149, 149, 158, 169, 158,
    79, 79, 79, 79, 79, 79, 85, 85, 85, 85, 85, 85, 85, 85, 88, 88,
    88, 88, 93, 93, 93, 93, 93, 93, 93, 93, 93, 93, 100, 100, 100, 100,
    100, 100, 100, 100, 105, 105, 105, 105, 107, 107, 107, 107, 107, 107, 107, 107,
    107, 108, 65535, 65535, 111, 111, 114, 114, 134, 116, 116, 116, 116, 116, 116, 116,
    116, 116, 116, 121, 121, 121, 121, 121, 121, 65535, 125, 125, 126, 126, 126, 126,
    126, 126, 65535, 65535, 135, 135, 135, 135, 135, 135, 138, 138, 138, 138, 138, 138,
    138, 138, 142, 142, 142, 142, 143, 143, 149, 149, 149, 149, 149, 149, 149, 149,
    149, 149, 149, 149, 156, 156, 158, 158, 158, 162, 162, 162, 162, 162, 162, 138,
    82, 83, 84, 84, 174, 174, 127, 87, 87, 89, 90, 91, 91, 65535, 95, 96,
    97, 99, 99, 102, 103, 106, 110, 109, 115, 115, 117, 119, 151, 122, 123, 128,
    126, 126, 104, 104, 131, 131, 136, 172, 172, 140, 141, 145, 146, 146, 147, 149,
    149, 152, 154, 160, 160, 163, 163, 166, 167, 167, 168, 171, 173, 173, 65535, 170,
    177, 178, 179, 180, 65535, 65535, 65535, 65535, 65535, 65535, 65535, 65535, 65535, 79, 79, 107,
    107, 126, 126, 149, 149, 149, 149, 149, 149, 149, 149, 149, 149, 95, 79, 79,
    79, 79, 65535, 65535, 101, 101, 100, 100, 114, 114, 126, 126, 126, 126, 166, 166,
    111, 65535, 65535, 65535, 100, 100, 106, 170, 121, 121, 79, 79, 65535, 65535, 126, 126,
    79, 79, 79, 79, 93, 93, 93, 93, 107, 107, 107, 107, 126, 126, 126, 126,
    135, 135, 135, 135, 149, 149, 149, 149, 138, 138, 142, 142, 161, 161, 105, 105,
    123, 92, 129, 129, 164, 164, 79, 79, 93, 93, 126, 126, 126, 126, 126, 126,
    126, 126, 158, 158, 118, 124, 148, 112, 65535, 65535, 80, 86, 86, 117, 144, 139,
    165, 175, 175, 82, 150, 155, 94, 94, 113, 113, 133, 133, 137, 137, 159, 159,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_and_hidden() {
        assert_eq!(ext_of("Kick.WAV"), "wav");
        assert_eq!(ext_of(".wav"), "");
        assert_eq!(ext_of("kick."), "");
        assert_eq!(ext_of("kick"), "");
        assert_eq!(ext_of("a.b.Mp3"), "mp3");
        assert!(is_audio_file_name("x.aiff"));
        assert!(!is_audio_file_name("x.wave"));
        assert!(!is_audio_file_name("x.txt"));
        assert!(is_hidden_name(".DS_Store"));
        assert!(is_hidden_name("._kick.wav"));
        assert!(!is_hidden_name("kick.wav"));
    }

    #[test]
    fn collation_basics() {
        use Ordering::*;
        let c = collator_compare;
        assert_eq!(c("01", "1"), Equal);
        assert_eq!(c("ß", "ss"), Equal);
        assert_eq!(c("æ", "ae"), Equal);
        assert_eq!(c("a1", "a01"), Equal);
        assert_eq!(c("x", ""), Greater);
        assert_eq!(c("\u{300}a", "a"), Equal);
        assert_eq!(c("a-b", "ab"), Less);
        assert_eq!(c("ø", "o"), Equal);
        assert_eq!(c("1.5", "1.10"), Less);
        assert_eq!(c("a b", "ab"), Less);
        assert_eq!(c("a", "a "), Less);
        assert_eq!(c("a\u{ad}b", "ab"), Equal);
        assert_eq!(c("9", "a"), Less);
        assert_eq!(c("9", "_"), Greater);
        assert_eq!(c("10", "9a"), Greater);
        assert_eq!(c("ŉ", "\u{2bc}n"), Equal);
        assert_eq!(c("Kick 2", "kick 10"), Less);
        assert_eq!(c("Été", "ete"), Equal);
        assert_eq!(c("ı", "i"), Greater);
        assert_eq!(c("þ", "th"), Greater);
    }

    #[test]
    fn apostrophe_weight_sits_between_its_neighbours() {
        // ɂ < ʼ < ǀ in ICU.
        assert!(u32::from(PRIMARY[0x242]) < APOSTROPHE_WEIGHT);
        assert!(APOSTROPHE_WEIGHT < u32::from(PRIMARY[0x1c0]));
        assert_eq!(u32::from(PRIMARY['0' as usize]), DIGIT_WEIGHT);
    }

    #[test]
    fn sorts_folders_first_then_naturally() {
        let names = [
            ("b.wav", false),
            ("Kicks", true),
            ("a10.wav", false),
            ("a2.wav", false),
            ("A2.wav", false),
            ("snares", true),
        ];
        let sorted: Vec<&str> = sort_entries(&names).iter().map(|e| e.0).collect();
        assert_eq!(
            sorted,
            ["Kicks", "snares", "A2.wav", "a2.wav", "a10.wav", "b.wav"]
        );
    }

    #[test]
    fn builds_list_dir_rows() {
        let rows = list_entries(
            "/s/",
            '/',
            [
                ("Hat.wav", EntryKind::File),
                (".DS_Store", EntryKind::File),
                ("Kicks", EntryKind::Dir),
                ("notes.txt", EntryKind::File),
                ("link", EntryKind::Other),
            ],
        );
        assert_eq!(
            rows,
            vec![
                FsDirEntry {
                    name: "Kicks".into(),
                    path: "/s/Kicks".into(),
                    is_dir: true,
                    ext: String::new()
                },
                FsDirEntry {
                    name: "Hat.wav".into(),
                    path: "/s/Hat.wav".into(),
                    is_dir: false,
                    ext: "wav".into()
                },
            ]
        );
        assert_eq!(check_list_dir_path(""), Err("Invalid directory path"));
        assert_eq!(check_list_dir_path("/a\0b"), Err("Invalid directory path"));
        assert_eq!(check_list_dir_path("/a"), Ok(()));
    }

    #[test]
    fn golden_sort_matches_typescript() {
        let fx: serde_json::Value =
            serde_json::from_str(include_str!("../../../parity/fixtures/fs_entries.json")).unwrap();
        let mut n = 0;
        for case in fx["sorts"].as_array().unwrap() {
            let input: Vec<(String, bool)> = case["input"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| {
                    (
                        e["name"].as_str().unwrap().to_owned(),
                        e["isDir"].as_bool().unwrap(),
                    )
                })
                .collect();
            let expected: Vec<&str> = case["sorted"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect();
            let got: Vec<String> = sort_entries(&input).into_iter().map(|e| e.0).collect();
            assert_eq!(got, expected, "sort case {n}");
            n += 1;
        }
        for pair in fx["pairs"].as_array().unwrap() {
            let (a, b, want) = (
                pair[0].as_str().unwrap(),
                pair[1].as_str().unwrap(),
                pair[2].as_i64().unwrap(),
            );
            let got = match collator_compare(a, b) {
                Ordering::Less => -1,
                Ordering::Equal => 0,
                Ordering::Greater => 1,
            };
            assert_eq!(got, want.signum(), "collator({a:?}, {b:?})");
            n += 1;
        }
        for row in fx["names"].as_array().unwrap() {
            let name = row["name"].as_str().unwrap();
            assert_eq!(ext_of(name), row["ext"].as_str().unwrap(), "{name}");
            assert_eq!(
                is_audio_file_name(name),
                row["audio"].as_bool().unwrap(),
                "{name}"
            );
            assert_eq!(
                is_hidden_name(name),
                row["hidden"].as_bool().unwrap(),
                "{name}"
            );
            n += 1;
        }
        assert!(n > 100);
    }
}
