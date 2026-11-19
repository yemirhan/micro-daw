//! Tests for settings.rs: the store (persistence, atomic write), ported vitest cases that touch
//! localStorage (KitStore in DrumEngine.test.ts, masterBus.test.ts, SampleBrowser.test.ts) and
//! the golden comparison against rune/parity/fixtures/settings.json.

use serde_json::{Value, json};

use super::*;
use crate::types::{PadSample, PadSettings};

fn store_with(key: &str, value: Option<&str>) -> SettingsStore {
    let mut s = SettingsStore::in_memory();
    if let Some(v) = value {
        s.set_item(key, v).unwrap();
    }
    s
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "mdaw-settings-{tag}-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Test stand-in for `normalizeKit` (kit.rs owns the real one): id / name fallbacks + 16 pads.
fn test_normalize_kit(v: &Value) -> KitState {
    let s = |k: &str| {
        v.get(k)
            .and_then(Value::as_str)
            .filter(|x| !x.is_empty())
            .map(str::to_owned)
    };
    let pads: Vec<PadSettings> = (0..16)
        .map(|i| {
            v.get("pads")
                .and_then(|p| p.get(i))
                .and_then(|p| serde_json::from_value::<PadSettings>(p.clone()).ok())
                .unwrap_or_default()
        })
        .collect();
    KitState {
        id: s("id").unwrap_or_else(|| "<uuid>".into()),
        name: s("name").unwrap_or_else(|| "Kit".into()),
        pads,
        extra: Default::default(),
    }
}

#[test]
fn store_persists_atomically_and_reloads() {
    let dir = temp_dir("persist");
    {
        let mut s = SettingsStore::open(&dir);
        assert!(s.is_empty());
        s.set_item("a", "1").unwrap();
        s.set_item("b", "two").unwrap();
        s.remove_item("a").unwrap();
        s.remove_item("missing").unwrap();
    }
    let s = SettingsStore::open(&dir);
    assert_eq!(s.get_item("a"), None);
    assert_eq!(s.get_item("b"), Some("two"));
    assert_eq!(s.keys().collect::<Vec<_>>(), ["b"]);
    // No temp files left behind.
    let names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, [SettingsStore::FILE_NAME]);
    std::fs::write(dir.join(SettingsStore::FILE_NAME), "{corrupt").unwrap();
    assert!(SettingsStore::open(&dir).is_empty());
    std::fs::write(dir.join(SettingsStore::FILE_NAME), r#"{"x":"1","y":2}"#).unwrap();
    let s = SettingsStore::open(&dir);
    assert_eq!((s.get_item("x"), s.get_item("y")), (Some("1"), None));
    let mut s = SettingsStore::open(&dir);
    s.clear().unwrap();
    assert!(SettingsStore::open(&dir).is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn every_key_is_listed_once() {
    let mut keys = ALL_KEYS.to_vec();
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), ALL_KEYS.len());
}

// --- DrumEngine.test.ts: kitStore -------------------------------------------

#[test]
fn kit_store_saves_lists_replaces_and_removes_kits_and_strips_runtime_sample_ids() {
    let mut s = SettingsStore::in_memory();
    let mut kit = KitState {
        id: "mine".into(),
        name: "Mine".into(),
        pads: vec![PadSettings::default(); 16],
        extra: Default::default(),
    };
    kit.pads[0].sample = Some(PadSample {
        name: "hit".into(),
        source: PadSampleSource::File {
            path: "/a/hit.wav".into(),
            sample_id: Some("abc".into()),
        },
    });
    save_kit(&mut s, &kit, test_normalize_kit).unwrap();
    let list = read_kits(&s, test_normalize_kit);
    assert_eq!(list.len(), 1);
    assert_eq!(
        list[0].pads[0].sample.as_ref().unwrap().source,
        PadSampleSource::File {
            path: "/a/hit.wav".into(),
            sample_id: None
        }
    );
    save_kit(
        &mut s,
        &KitState {
            name: "Renamed".into(),
            ..kit.clone()
        },
        test_normalize_kit,
    )
    .unwrap();
    assert_eq!(
        read_kits(&s, test_normalize_kit)
            .iter()
            .map(|k| k.name.as_str())
            .collect::<Vec<_>>(),
        ["Renamed"]
    );
    assert!(remove_kit(&mut s, "mine", test_normalize_kit).unwrap());
    assert!(read_kits(&s, test_normalize_kit).is_empty());
    assert!(!remove_kit(&mut s, "mine", test_normalize_kit).unwrap());
}

#[test]
fn kit_store_survives_corrupt_storage() {
    let s = store_with(KITS_KEY, Some("{nope"));
    assert!(read_kits(&s, test_normalize_kit).is_empty());
}

// --- masterBus.test.ts ---------------------------------------------------------

#[test]
fn master_volume_is_persisted_as_a_js_number_string() {
    let mut s = SettingsStore::in_memory();
    assert_eq!(read_master_volume(&s), DEFAULT_VOLUME);
    assert_eq!(write_master_volume(&mut s, -20.0).unwrap(), -20.0);
    assert_eq!(s.get_item(MASTER_VOLUME_KEY), Some("-20"));
    assert_eq!(read_master_volume(&s), -20.0);
    write_master_volume(&mut s, -99.0).unwrap();
    assert_eq!(s.get_item(MASTER_VOLUME_KEY), Some("-40"));
    write_master_volume(&mut s, f64::NAN).unwrap();
    assert_eq!(s.get_item(MASTER_VOLUME_KEY), Some("-12"));
    write_master_volume(&mut s, -12.5).unwrap();
    assert_eq!(s.get_item(MASTER_VOLUME_KEY), Some("-12.5"));
}

// --- SampleBrowser.test.ts -------------------------------------------------------

#[test]
fn browser_roots_and_last_path_round_trip() {
    let mut s = SettingsStore::in_memory();
    write_browser_roots(&mut s, &["/s".to_owned()]).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(s.get_item(BROWSER_ROOTS_KEY).unwrap()).unwrap(),
        json!(["/s"])
    );
    write_browser_last_path(&mut s, Some("/s")).unwrap();
    let roots = read_browser_roots(&s);
    assert_eq!(read_browser_last_path(&s, &roots).as_deref(), Some("/s"));
    write_browser_last_path(&mut s, None).unwrap();
    assert_eq!(s.get_item(BROWSER_LAST_PATH_KEY), None);
}

// --- writers -------------------------------------------------------------------------

#[test]
fn writers_store_what_the_ts_stores() {
    let mut s = SettingsStore::in_memory();
    write_browser_open(&mut s, false).unwrap();
    assert_eq!(s.get_item(BROWSER_OPEN_KEY), Some("false"));
    assert!(!read_browser_open(&s));
    write_dock_pad_bank(&mut s, PadBank::B).unwrap();
    assert_eq!(read_dock_pad_bank(&s), PadBank::B);
    write_beat_key_hints(&mut s, true).unwrap();
    assert_eq!(s.get_item(BEAT_KEY_HINTS_KEY), Some("on"));
    write_beat_pad_layout(&mut s, "2x8").unwrap();
    assert_eq!(read_beat_pad_layout(&s), "2x8");
    write_beat_step_grid(&mut s, "1/32").unwrap();
    assert_eq!(read_beat_step_grid(&s), "1/32");
    write_browser_auto_preview(&mut s, false).unwrap();
    assert_eq!(s.get_item(BROWSER_AUTO_PREVIEW_KEY), Some("0"));
    assert!(!read_browser_auto_preview(&s));
    write_browser_auto_preview(&mut s, true).unwrap();
    assert!(read_browser_auto_preview(&s));
    write_browser_width(&mut s, 300.0).unwrap();
    assert_eq!(s.get_item(BROWSER_WIDTH_KEY), Some("300"));
    write_piano_roll_prefs(
        &mut s,
        &PianoRollPrefs {
            snap: 0.125,
            draw_length: 2.0,
            follow: false,
        },
    )
    .unwrap();
    assert_eq!(
        s.get_item(PIANO_ROLL_KEY),
        Some(r#"{"snap":0.125,"drawLength":2,"follow":false}"#)
    );
    assert_eq!(
        read_piano_roll_prefs(&s),
        PianoRollPrefs {
            snap: 0.125,
            draw_length: 2.0,
            follow: false
        }
    );
    write_app_settings(&mut s, &default_app_settings()).unwrap();
    assert_eq!(
        s.get_item(SETTINGS_STORAGE_KEY),
        Some(
            r#"{"general":{"autoCheckUpdates":true},"audio":{"defaultMasterVolume":-12,"bufferSizeHint":256},"midi":{"autoConnectLastDevice":true}}"#
        )
    );
    let next = update_app_settings(&mut s, &json!({ "audio": { "bufferSizeHint": 512 } })).unwrap();
    assert_eq!(
        next["audio"],
        json!({ "defaultMasterVolume": -12, "bufferSizeHint": 512 })
    );
    assert_eq!(read_app_settings(&s).audio.buffer_size_hint, 512);
    write_controller_raw(&mut s, "{\"version\":1}").unwrap();
    assert_eq!(read_controller_raw(&s), Some("{\"version\":1}"));
    write_arrangement(&mut s, &json!({ "tracks": [] })).unwrap();
    assert_eq!(read_arrangement(&s), Some(json!({ "tracks": [] })));
}

#[test]
fn sample_library_entries() {
    let mut s = SettingsStore::in_memory();
    let e = |path: &str| SerializedSampleEntry {
        sample_id: "id".into(),
        sample_path: path.into(),
        sample_name: "n".into(),
        trim: None,
        added_at: 5.0,
    };
    write_sample_library(&mut s, &[e("/a.wav"), e("")]).unwrap();
    assert_eq!(
        s.get_item(SAMPLE_LIBRARY_STORAGE_KEY),
        Some(r#"[{"sampleId":"id","samplePath":"/a.wav","sampleName":"n","addedAt":5}]"#)
    );
    assert_eq!(read_sample_library(&s).len(), 1);
    s.set_item(SAMPLE_LIBRARY_STORAGE_KEY, r#"[{"sampleId":"a"},null]"#)
        .unwrap();
    assert!(read_sample_library(&s).is_empty()); // the TS loop throws on null: nothing loads
    s.set_item(SAMPLE_LIBRARY_STORAGE_KEY, r#"{"a":1}"#)
        .unwrap();
    assert!(read_sample_library(&s).is_empty());
    s.set_item(SAMPLE_LIBRARY_STORAGE_KEY, r#"[1,"x"]"#)
        .unwrap();
    assert_eq!(read_sample_library(&s).len(), 2);
}

#[test]
fn recent_projects_most_recent_first_deduped_max_10() {
    let mut list = vec![];
    for i in 0..12 {
        list = add_recent_project(&list, &format!("/p/song{i}.mdaw"), "t");
    }
    assert_eq!(list.len(), 10);
    assert_eq!(list[0].name, "song11");
    list = add_recent_project(&list, "/p/song5.mdaw", "t2");
    assert_eq!(list.len(), 10);
    assert_eq!(
        (list[0].name.as_str(), list[0].modified_at.as_str()),
        ("song5", "t2")
    );
    assert_eq!(
        list.iter()
            .filter(|p| p.file_path == "/p/song5.mdaw")
            .count(),
        1
    );
    assert_eq!(add_recent_project(&[], "/p/", "t")[0].name, "Untitled");
    // Electron bug fixed in both apps (phase 10): split on `\` too.
    assert_eq!(add_recent_project(&[], "C:\\x\\a.mdaw", "t")[0].name, "a");
    assert_eq!(add_recent_project(&[], "/p/.mdaw", "t")[0].name, "Untitled");
    let mut s = SettingsStore::in_memory();
    write_recent_projects(&mut s, &list).unwrap();
    assert_eq!(read_recent_projects(&s), list);
}

// --- golden ---------------------------------------------------------------------------

#[test]
fn golden_settings_readers_match_typescript() {
    let fx: Value =
        serde_json::from_str(include_str!("../../../parity/fixtures/settings.json")).unwrap();
    let mut n = 0;
    let rows = |k: &str| fx[k].as_array().unwrap().clone();
    let check = |k: &str, key: &str, read: &dyn Fn(&SettingsStore) -> Value, n: &mut usize| {
        for row in rows(k) {
            let s = store_with(key, row[0].as_str());
            assert_eq!(read(&s), row[1], "{k} {}", row[0]);
            *n += 1;
        }
    };
    let num = |x: f64| crate::js::number_value(x);
    check(
        "browserOpen",
        BROWSER_OPEN_KEY,
        &|s| json!(read_browser_open(s)),
        &mut n,
    );
    check(
        "bank",
        DOCK_PAD_BANK_KEY,
        &|s| {
            json!(if read_dock_pad_bank(s) == PadBank::B {
                "B"
            } else {
                "A"
            })
        },
        &mut n,
    );
    check(
        "layout",
        BEAT_PAD_LAYOUT_KEY,
        &|s| json!(read_beat_pad_layout(s)),
        &mut n,
    );
    check(
        "grid",
        BEAT_STEP_GRID_KEY,
        &|s| json!(read_beat_step_grid(s)),
        &mut n,
    );
    check(
        "keys",
        BEAT_KEY_HINTS_KEY,
        &|s| json!(read_beat_key_hints(s)),
        &mut n,
    );
    check(
        "pianoRoll",
        PIANO_ROLL_KEY,
        &|s| {
            let p = read_piano_roll_prefs(s);
            json!({ "snap": num(p.snap), "drawLength": num(p.draw_length), "follow": p.follow })
        },
        &mut n,
    );
    check(
        "settings",
        SETTINGS_STORAGE_KEY,
        &|s| read_app_settings_value(s),
        &mut n,
    );
    check(
        "volume",
        MASTER_VOLUME_KEY,
        &|s| num(read_master_volume(s)),
        &mut n,
    );
    check(
        "width",
        BROWSER_WIDTH_KEY,
        &|s| num(read_browser_width(s)),
        &mut n,
    );
    check(
        "roots",
        BROWSER_ROOTS_KEY,
        &|s| json!(read_browser_roots(s)),
        &mut n,
    );
    check(
        "packNames",
        PACK_NAMES_KEY,
        &|s| {
            json!(
                read_pack_names(s)
                    .into_iter()
                    .map(|(k, v)| vec![k, v])
                    .collect::<Vec<_>>()
            )
        },
        &mut n,
    );
    check(
        "kits",
        KITS_KEY,
        &|s| {
            json!(
                read_kits(s, test_normalize_kit)
                    .iter()
                    .map(|k| json!({ "id": k.id, "name": k.name, "pads": k.pads.len() }))
                    .collect::<Vec<_>>()
            )
        },
        &mut n,
    );
    check(
        "recent",
        RECENT_PROJECTS_KEY,
        &|s| read_recent_projects_raw(s).unwrap_or_else(|| json!("<untouched>")),
        &mut n,
    );
    for row in rows("lastPaths") {
        let roots: Vec<String> = serde_json::from_value(row[0].clone()).unwrap();
        let s = store_with(BROWSER_LAST_PATH_KEY, row[1].as_str());
        assert_eq!(
            json!(read_browser_last_path(&s, &roots)),
            row[2],
            "lastPath {} {}",
            row[0],
            row[1]
        );
        n += 1;
    }
    // Key order of the settings object (JS spreads put index keys first).
    let s = store_with(
        SETTINGS_STORAGE_KEY,
        Some(r#"{"audio":{"bufferSizeHint":"big","extra":{"a":1},"10":1,"2":2}}"#),
    );
    let keys: Vec<String> = read_app_settings_value(&s)["audio"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    assert_eq!(
        keys,
        ["2", "10", "defaultMasterVolume", "bufferSizeHint", "extra"]
    );
    eprintln!("settings golden comparisons: {n}");
    assert!(n > 250);
}
