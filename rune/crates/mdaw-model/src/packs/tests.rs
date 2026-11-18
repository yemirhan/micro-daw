//! Ported vitest cases (src/main/packs/validate.test.ts, src/test/packCatalog.test.ts (app-side
//! cases), src/components/packs/packsModel.test.ts, remoteKit.test.ts, kitPreview.test.ts (pure
//! cases), src/services/PackService.test.ts (pure helpers + the pure analogues of the pack-name
//! / status cases), src/main/packs/sender.test.ts) plus the golden comparisons against
//! rune/parity/fixtures/packs_*.json.

use serde_json::{Value, json};

use super::kit_preview::*;
use super::sender::*;
use super::*;
use crate::types::{
    ClassicMachineId, FactoryVoiceId, KitState, PadSample, PadSampleSource, PadSettings,
};

const COMMIT: &str = "c0ffee0000000000000000000000000000000000";
const SHA_KICK: &str = "0db10f2c2f332cd27cf1407fa16c686337b2b23f46125d6e17740dbfc6df427e";
const SHA_SNARE: &str = "4ef396f332bea419aedd7ff9ce8585c449babcbbed5c5f3a5ab6522c688b91a4";
const SHA_A: &str = "ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb";
const SHA_B: &str = "3e23e8160039594a33894f6564e1b1348bbd7a0088d42c4acb73eeaed59c009d";
const SHA_X: &str = "2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881";

fn raw_test_url(path: &str) -> String {
    format!("https://raw.githubusercontent.com/o/r/{COMMIT}/{path}")
}

/// testUtils.packFile(path, body)
fn pack_file(path: &str, body: &str) -> Value {
    let sha = match body {
        "kick" => SHA_KICK,
        "snare" => SHA_SNARE,
        "a" => SHA_A,
        "b" => SHA_B,
        _ => SHA_X,
    };
    json!({ "path": path, "url": raw_test_url(path), "size": body.len(), "sha256": sha })
}

/// testUtils.manifest(overrides)
fn manifest_with(overrides: Value) -> Value {
    let mut m = json!({
        "id": "test-pack",
        "name": "Test Pack",
        "tagline": "808-style",
        "description": "A test pack",
        "tags": ["808"],
        "author": "Someone",
        "source": "https://github.com/example/repo",
        "license": "CC0-1.0",
        "color": "#f80",
        "files": [
            { "path": "kicks/bd.wav", "url": raw_test_url("bd.wav"), "size": 4, "sha256": SHA_KICK },
            { "path": "sd.wav", "url": raw_test_url("sd.wav"), "size": 5, "sha256": SHA_SNARE },
        ],
        "kits": [{ "id": "main", "name": "Main", "pads": [{ "pad": 0, "file": "kicks/bd.wav", "name": "Kick" }, { "pad": 1, "file": "sd.wav" }] }],
        "version": 1,
    });
    for (k, v) in overrides.as_object().unwrap() {
        m[k] = v.clone();
    }
    m
}

fn manifest_v() -> Value {
    manifest_with(json!({}))
}

fn manifest() -> PackManifest {
    validate_manifest(&manifest_v()).unwrap()
}

fn manifest_o(overrides: Value) -> PackManifest {
    serde_json::from_value(manifest_with(overrides)).unwrap()
}

fn err_of(v: &Value) -> String {
    validate_manifest(v).unwrap_err().0
}

fn installed(m: PackManifest, dir: &str) -> InstalledPack {
    InstalledPack {
        id: m.id.clone(),
        version: m.version,
        dir: dir.into(),
        installed_at: "2026-11-17T00:00:00Z".into(),
        manifest: m,
    }
}

// ---------------------------------------------------------------------------
// validate.test.ts
// ---------------------------------------------------------------------------

#[test]
fn is_allowed_url_accepts_https_on_the_allowlisted_host_only() {
    assert!(is_allowed_url(
        "https://raw.githubusercontent.com/o/r/sha/a.wav"
    ));
    assert!(is_allowed_url("https://RAW.githubusercontent.com:443/a"));
    assert!(!is_allowed_url("https://github.com/o/r/raw/main/a.wav"));
    assert!(!is_allowed_url("https://objects.githubusercontent.com/x"));
    assert!(!is_allowed_url("https://codeload.github.com/x"));
}

#[test]
fn is_allowed_url_rejects_other_schemes_hosts_ports_and_credentials() {
    assert!(!is_allowed_url("http://raw.githubusercontent.com/a"));
    assert!(!is_allowed_url("https://evil.com/a"));
    assert!(!is_allowed_url(
        "https://raw.githubusercontent.com.evil.com/a"
    ));
    assert!(!is_allowed_url(
        "https://evil.com@raw.githubusercontent.com/a"
    ));
    assert!(!is_allowed_url("https://github.com:8443/a"));
    assert!(!is_allowed_url("file:///etc/passwd"));
    assert!(!is_allowed_url("not a url"));
    assert!(!is_allowed_url_value(&json!(42)));
}

#[test]
fn is_safe_pack_path_accepts() {
    for p in [
        "kick.wav",
        "kicks/bd 01.wav",
        "a/b/c/Snare-1.WAV",
        "LICENSE.txt",
        "hats/ch.flac",
    ] {
        assert!(is_safe_pack_path(p), "{p}");
    }
}

#[test]
fn is_safe_pack_path_rejects() {
    for p in [
        "",
        "/etc/x.wav",
        "../x.wav",
        "a/../../x.wav",
        "a/./x.wav",
        "a//x.wav",
        "a\\..\\x.wav",
        "C:/x.wav",
        "c:x.wav",
        ".hidden.wav",
        "a/.git/x.wav",
        "x.exe",
        "kick",
        "pack.json",
        "con.wav",
        "a/nul",
        "trailing.wav ",
        "x\u{0}.wav",
        "a/b/c/d/e/f/g/h/i.wav",
    ] {
        assert!(!is_safe_pack_path(p), "{p:?}");
    }
}

#[test]
fn validate_manifest_accepts_a_valid_manifest() {
    let m = manifest();
    assert_eq!(m.id, "test-pack");
    assert_eq!(m.kits[0].pads.len(), 2);
}

#[test]
fn validate_manifest_rejects_bad_ids_licences_hosts_and_traversal() {
    assert!(!is_valid_pack_id("Bad_Id"));
    assert!(err_of(&manifest_with(json!({ "id": "../x" }))).contains("pack id"));
    assert!(err_of(&manifest_with(json!({ "license": "GPL" }))).contains("licence"));
    let mut f = pack_file("a.wav", "a");
    f["url"] = json!("https://evil.com/a.wav");
    assert!(err_of(&manifest_with(json!({ "files": [f], "kits": [] }))).contains("not allowed"));
    let mut f = pack_file("../a.wav", "a");
    f["url"] = json!(raw_test_url("a.wav"));
    assert!(err_of(&manifest_with(json!({ "files": [f], "kits": [] }))).contains("Unsafe"));
}

#[test]
fn validate_manifest_rejects_duplicate_paths_and_clashes() {
    assert!(
        err_of(&manifest_with(
            json!({ "files": [pack_file("A.wav", "a"), pack_file("a.wav", "a")], "kits": [] })
        ))
        .contains("Duplicate")
    );
    assert!(
        err_of(&manifest_with(
            json!({ "files": [pack_file("a.wav", "a"), pack_file("a.wav/b.wav", "b")], "kits": [] })
        ))
        .contains("clash")
    );
}

#[test]
fn validate_manifest_enforces_size_caps_and_sha256_format() {
    let mut f = pack_file("a.wav", "a");
    f["size"] = json!(MAX_FILE_BYTES + 1);
    assert!(err_of(&manifest_with(json!({ "files": [f], "kits": [] }))).contains("too large"));
    let many: Vec<Value> = (0..11)
        .map(|i| {
            let mut f = pack_file(&format!("f{i}.wav"), "x");
            f["size"] = json!(MAX_FILE_BYTES);
            f
        })
        .collect();
    assert!(
        err_of(&manifest_with(json!({ "files": many, "kits": [] }))).contains("Pack too large")
    );
    let mut f = pack_file("a.wav", "a");
    f["sha256"] = json!("XYZ");
    assert!(err_of(&manifest_with(json!({ "files": [f], "kits": [] }))).contains("sha256"));
}

#[test]
fn s2_requires_size_sha256_and_a_commit_pinned_raw_url() {
    let f = pack_file("a.wav", "a");
    let without = |k: &str| {
        let mut c = f.clone();
        c.as_object_mut().unwrap().remove(k);
        c
    };
    assert!(
        err_of(&manifest_with(
            json!({ "files": [without("size")], "kits": [] })
        ))
        .contains("size")
    );
    assert!(
        err_of(&manifest_with(
            json!({ "files": [without("sha256")], "kits": [] })
        ))
        .contains("sha256")
    );
    let mut z = f.clone();
    z["size"] = json!(0);
    assert!(err_of(&manifest_with(json!({ "files": [z], "kits": [] }))).contains("size"));
    let mut b = f.clone();
    b["url"] = json!("https://raw.githubusercontent.com/o/r/main/a.wav");
    assert!(err_of(&manifest_with(json!({ "files": [b], "kits": [] }))).contains("pinned"));
    let ok =
        validate_manifest(&manifest_with(json!({ "files": [f.clone()], "kits": [] }))).unwrap();
    assert_eq!(serde_json::to_value(&ok.files[0]).unwrap(), f);
}

#[test]
fn s2_s4_source_must_be_github_and_color_hex_or_oklch() {
    assert!(
        err_of(&manifest_with(
            json!({ "source": "https://evil.example/repo" })
        ))
        .contains("github")
    );
    assert!(
        err_of(&manifest_with(json!({ "source": "http://github.com/o/r" }))).contains("github")
    );
    assert!(
        err_of(&manifest_with(
            json!({ "color": "red; background: url(https://evil.example/x)" })
        ))
        .contains("color")
    );
    assert!(err_of(&manifest_with(json!({ "color": "var(--x)" }))).contains("color"));
    assert_eq!(
        validate_manifest(&manifest_with(json!({ "color": "oklch(0.68 0.19 35)" })))
            .unwrap()
            .color,
        "oklch(0.68 0.19 35)"
    );
}

#[test]
fn p4_keeps_a_valid_file_label_and_rejects_bad_ones() {
    let mut f = pack_file("a.wav", "a");
    f["label"] = json!("Kick · tone 0 · decay 10");
    let m = validate_manifest(&manifest_with(json!({ "files": [f.clone()], "kits": [] }))).unwrap();
    assert_eq!(
        m.files[0].label.as_deref(),
        Some("Kick · tone 0 · decay 10")
    );
    f["label"] = json!("x".repeat(81));
    assert!(err_of(&manifest_with(json!({ "files": [f.clone()], "kits": [] }))).contains("label"));
    f["label"] = json!(3);
    assert!(err_of(&manifest_with(json!({ "files": [f], "kits": [] }))).contains("label"));
}

#[test]
fn validates_kits_against_files_and_pad_range() {
    assert!(err_of(&manifest_with(json!({ "kits": [{ "id": "k", "name": "K", "pads": [{ "pad": 16, "file": "sd.wav" }] }] }))).contains("pad index"));
    assert!(err_of(&manifest_with(json!({ "kits": [{ "id": "k", "name": "K", "pads": [{ "pad": 0, "file": "nope.wav" }] }] }))).contains("unknown file"));
    assert!(
        err_of(&manifest_with(json!({ "kits": [{ "id": "k", "name": "K", "pads": [{ "pad": 0, "file": "sd.wav" }, { "pad": 0, "file": "sd.wav" }] }] })))
            .contains("twice")
    );
}

#[test]
fn keeps_only_scalar_pad_settings() {
    let m = validate_manifest(&manifest_with(json!({
        "kits": [{ "id": "k", "name": "K", "pads": [{ "pad": 0, "file": "sd.wav", "settings": { "chokeGroup": 1, "sample": { "evil": true }, "nested": { "a": 1 } } }] }]
    })))
    .unwrap();
    assert_eq!(
        Value::Object(m.kits[0].pads[0].settings.clone().unwrap()),
        json!({ "chokeGroup": 1 })
    );
}

#[test]
fn requires_attribution_for_cc_by() {
    assert!(err_of(&manifest_with(json!({ "license": "CC-BY-4.0" }))).contains("attribution"));
    assert_eq!(
        validate_manifest(&manifest_with(
            json!({ "license": "CC-BY-4.0", "attribution": "by X" })
        ))
        .unwrap()
        .attribution
        .as_deref(),
        Some("by X")
    );
}

#[test]
fn validate_catalog_drops_invalid_and_duplicate_packs_but_keeps_the_rest() {
    let r = validate_catalog(&json!({
        "version": 1,
        "updatedAt": "2026-01-01",
        "packs": [manifest_v(), manifest_v(), manifest_with(json!({ "id": "other", "license": "nope" })), manifest_with(json!({ "id": "good-two" }))],
    }))
    .unwrap();
    assert_eq!(
        r.catalog
            .packs
            .iter()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>(),
        ["test-pack", "good-two"]
    );
    assert_eq!(r.errors.len(), 2);
}

#[test]
fn validate_catalog_throws_on_an_invalid_envelope() {
    assert!(validate_catalog(&Value::Null).is_err());
    assert!(
        validate_catalog(&json!({ "version": 2, "updatedAt": "x", "packs": [] }))
            .unwrap_err()
            .0
            .contains("version")
    );
    assert!(
        validate_catalog(&json!({ "version": 1, "updatedAt": "x" }))
            .unwrap_err()
            .0
            .contains("packs")
    );
}

#[test]
fn validate_installed_uses_the_folder_it_was_read_from() {
    let pack = validate_installed(&json!({ "id": "test-pack", "version": 1, "dir": "/evil", "installedAt": "now", "manifest": manifest_v() }), "/real/test-pack").unwrap();
    assert_eq!(pack.dir, "/real/test-pack");
    assert!(
        validate_installed(&json!({ "id": "other", "manifest": manifest_v() }), "/x")
            .unwrap_err()
            .0
            .contains("mismatch")
    );
}

#[test]
fn declared_pack_bytes_sums_declared_sizes() {
    assert_eq!(declared_pack_bytes(&manifest()), 9);
}

#[test]
fn is_pinned_pack_url_cases() {
    assert!(is_pinned_pack_url(&raw_test_url("a/b%20c.wav")));
    assert!(!is_pinned_pack_url(&format!(
        "https://raw.githubusercontent.com/o/r/{COMMIT}"
    )));
    assert!(!is_pinned_pack_url(
        "https://raw.githubusercontent.com/o/r/main/a.wav"
    ));
    assert!(!is_pinned_pack_url(&format!(
        "https://raw.githubusercontent.com/o/r/{}/a.wav",
        COMMIT.to_uppercase()
    )));
    assert!(!is_pinned_pack_url(&format!(
        "{}?x=1",
        raw_test_url("a.wav")
    )));
    assert!(!is_pinned_pack_url(&format!("{}#x", raw_test_url("a.wav"))));
    assert!(!is_pinned_pack_url(&format!(
        "https://raw.githubusercontent.com/o/r/{COMMIT}/a/%2e%2e/b.wav"
    )));
    assert!(!is_pinned_pack_url(&format!(
        "https://github.com/o/r/{COMMIT}/a.wav"
    )));
    assert!(!is_pinned_pack_url_value(&json!(42)));
}

#[test]
fn is_github_source_url_cases() {
    assert!(is_github_source_url("https://github.com/o/r/tree/abc/dir"));
    assert!(!is_github_source_url("https://github.com.evil.example/o/r"));
    assert!(!is_github_source_url("https://user@github.com/o/r"));
    assert!(!is_github_source_url("javascript:alert(1)"));
}

#[test]
fn colors_accepted_and_rejected() {
    for c in [
        "#f80",
        "#ff8800",
        "#ff8800cc",
        "oklch(0.68 0.19 35)",
        "oklch(68% 0.19 35deg / 0.5)",
    ] {
        assert!(is_safe_pack_color(c), "{c}");
    }
    for c in [
        "red",
        "#ggg",
        "rgb(1,2,3)",
        "oklch(0.6 0.1 30); color: red",
        "oklch(var(--x) 0.1 30)",
        "url(x)",
        "oklch(0.6 0.1)",
        "",
    ] {
        assert!(!is_safe_pack_color(c), "{c:?}");
    }
}

fn cat(updated_at: &str, packs: Vec<PackManifest>) -> PackCatalog {
    PackCatalog {
        version: 1,
        updated_at: updated_at.into(),
        packs,
    }
}

#[test]
fn merge_keeps_the_bundled_catalog_when_the_remote_one_is_not_newer_or_undated() {
    let bundled = cat("2026-11-17", vec![manifest()]);
    let v9 = || vec![manifest_o(json!({ "version": 9 }))];
    assert_eq!(
        merge_catalogs(&bundled, &cat("2026-11-16", v9()), &[]).source,
        CatalogSource::Bundled
    );
    assert_eq!(
        merge_catalogs(&bundled, &cat("2026-11-17", v9()), &[]).source,
        CatalogSource::Bundled
    );
    assert_eq!(
        merge_catalogs(&bundled, &cat("not a date", v9()), &[]).source,
        CatalogSource::Bundled
    );
    assert_eq!(
        merge_catalogs(&bundled, &cat("2026-12-01", vec![]), &[]).source,
        CatalogSource::Bundled
    );
}

#[test]
fn merge_per_pack_older_remote_or_dropped_entry_keeps_bundled() {
    let bundled = cat(
        "2026-11-17",
        vec![
            manifest_o(json!({ "version": 3 })),
            manifest_o(json!({ "id": "other", "version": 1 })),
        ],
    );
    let remote = cat(
        "2026-12-01",
        vec![
            manifest_o(json!({ "version": 2 })),
            manifest_o(json!({ "id": "new-pack" })),
        ],
    );
    let merged = merge_catalogs(
        &bundled,
        &remote,
        &["Pack other: Invalid sha256 for a.wav".into()],
    );
    assert_eq!(merged.source, CatalogSource::Remote);
    let ids: Vec<String> = merged
        .catalog
        .packs
        .iter()
        .map(|p| format!("{}@{}", p.id, p.version))
        .collect();
    assert_eq!(ids, ["test-pack@3", "new-pack@1", "other@1"]);
}

#[test]
fn merge_a_cleanly_removed_pack_stays_removed() {
    let bundled = cat(
        "2026-11-17",
        vec![manifest(), manifest_o(json!({ "id": "withdrawn" }))],
    );
    let merged = merge_catalogs(
        &bundled,
        &cat("2026-12-01", vec![manifest_o(json!({ "version": 2 }))]),
        &[],
    );
    assert_eq!(
        merged
            .catalog
            .packs
            .iter()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>(),
        ["test-pack"]
    );
    assert_eq!(merged.catalog.packs[0].version, 2);
}

#[test]
fn find_catalog_file_only_catalog_urls() {
    let c = cat("2026-11-17", vec![manifest()]);
    assert_eq!(
        find_catalog_file(&c, &c.packs[0].files[1].url),
        Some(&c.packs[0].files[1])
    );
    assert_eq!(find_catalog_file(&c, &raw_test_url("other.wav")), None);
}

// ---------------------------------------------------------------------------
// packCatalog.test.ts (app-side cases; the curation-script checks stay in Node)
// ---------------------------------------------------------------------------

fn real_catalog() -> PackCatalog {
    let raw: Value =
        serde_json::from_str(include_str!("../../../../../packs/catalog.json")).unwrap();
    validate_catalog(&raw).unwrap().catalog
}

#[test]
fn catalog_passes_the_app_validator_without_dropping_any_pack() {
    let raw: Value =
        serde_json::from_str(include_str!("../../../../../packs/catalog.json")).unwrap();
    let r = validate_catalog(&raw).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let raw_ids: Vec<&str> = raw["packs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        r.catalog
            .packs
            .iter()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>(),
        raw_ids
    );
    assert!(r.catalog.packs.len() >= 4);
}

#[test]
fn l1_the_withdrawn_tr8_pack_is_not_offered() {
    assert!(!real_catalog().packs.iter().any(|p| p.id == "tr8-mckaudio"));
}

#[test]
fn l2_sonic_pi_ships_its_license_and_uzu_no_mot4i_files() {
    let c = real_catalog();
    let sonic = c.packs.iter().find(|p| p.id == "sonic-pi-drums").unwrap();
    let lic = sonic.files.iter().find(|f| f.path == "LICENSE.md").unwrap();
    let tail = lic.url.rsplit('/').nth(1).unwrap();
    assert!(
        lic.url.ends_with("/LICENSE.md")
            && tail.len() == 40
            && tail.bytes().all(|b| b.is_ascii_hexdigit())
    );
    let uzu = c.packs.iter().find(|p| p.id == "uzu-drumkit").unwrap();
    assert!(uzu.files.iter().all(|f| {
        let u = f.url.to_lowercase();
        !u.contains("mot4i") && !u.contains("14_sd") && !u.contains("10_tb")
    }));
}

#[test]
fn p4_every_fischer_808_sound_has_a_friendly_label() {
    let c = real_catalog();
    let fischer = c.packs.iter().find(|p| p.id == "tr808-fischer").unwrap();
    assert!(
        fischer
            .files
            .iter()
            .filter(|f| f.path.ends_with(".wav"))
            .all(|f| f.label.as_deref().is_some_and(|l| !l.is_empty()))
    );
    let label = |p: &str| {
        fischer
            .files
            .iter()
            .find(|f| f.path == p)
            .and_then(|f| f.label.clone())
    };
    assert_eq!(
        label("bass-drum/bd0010.wav").as_deref(),
        Some("Kick · tone 0 · decay 10")
    );
    assert_eq!(
        label("snare/sd7525.wav").as_deref(),
        Some("Snare · tone 75 · snappy 25")
    );
}

#[test]
fn ships_each_pack_with_its_upstream_licence_or_readme_text() {
    for p in real_catalog().packs {
        assert!(
            p.files
                .iter()
                .any(|f| f.path == "LICENSE.txt" || f.path == "README.md"),
            "{}",
            p.id
        );
    }
}

#[test]
fn puts_hi_hats_in_one_choke_group_in_every_kit() {
    let is_hat = |name: &str| {
        ["Closed", "Open", "Pedal", "Tick"].iter().any(|w| {
            let needle = format!("{w} Hat");
            name.match_indices(&needle).any(|(i, _)| {
                let before = name[..i].chars().last();
                let after = name[i + needle.len()..].chars().next();
                before.is_none_or(|c| !c.is_alphanumeric() && c != '_')
                    && after.is_none_or(|c| !c.is_alphanumeric() && c != '_')
            })
        })
    };
    for p in real_catalog().packs {
        for kit in &p.kits {
            for pad in kit
                .pads
                .iter()
                .filter(|pad| is_hat(pad.name.as_deref().unwrap_or("")))
            {
                let choke = pad
                    .settings
                    .as_ref()
                    .and_then(|s| s.get("chokeGroup"))
                    .and_then(Value::as_f64);
                assert_eq!(choke, Some(1.0), "{}/{}/{:?}", p.id, kit.id, pad.name);
            }
        }
    }
}

#[test]
fn turns_every_kit_into_a_full_kit_state_of_pack_pads() {
    for m in real_catalog().packs {
        let inst = installed(m.clone(), &format!("/packs/{}", m.id));
        for kit in &m.kits {
            let state = kit_from_installed(&inst, &kit.id).unwrap();
            assert_eq!(state.pads.len(), 16);
            for pad in &state.pads {
                assert!(
                    matches!(&pad.sample, Some(PadSample { source: PadSampleSource::Pack { pack_id, .. }, .. }) if *pack_id == m.id)
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// PackService.test.ts (pure helpers)
// ---------------------------------------------------------------------------

#[test]
fn kit_from_pack_builds_a_16_pad_kit_with_portable_sources_and_clamped_settings() {
    let mut m = manifest();
    m.kits[0].pads[0].settings = Some(
        json!({ "chokeGroup": 1, "gainDb": 99, "mode": "gate" })
            .as_object()
            .unwrap()
            .clone(),
    );
    let kit = kit_from_installed(&installed(m, "/data/packs/test-pack"), "main").unwrap();
    assert_eq!(kit.id, pack_kit_id("test-pack", "main"));
    assert_eq!(kit.name, "Main");
    assert_eq!(kit.pads.len(), 16);
    assert_eq!(
        kit.pads[0].sample,
        Some(PadSample {
            source: PadSampleSource::Pack {
                pack_id: "test-pack".into(),
                file: "kicks/bd.wav".into()
            },
            name: "Kick".into()
        })
    );
    assert!(!serde_json::to_string(&kit).unwrap().contains("/data/packs"));
    assert_eq!(
        (
            kit.pads[0].choke_group,
            kit.pads[0].gain_db,
            kit.pads[0].mode
        ),
        (1, 12.0, crate::types::PadPlayMode::Gate)
    );
    assert_eq!(kit.pads[1].sample.as_ref().unwrap().name, "sd");
    assert_eq!(kit.pads[2].sample, None);
}

#[test]
fn pack_file_path_uses_windows_separators_and_unknown_kits_are_none() {
    assert_eq!(
        pack_file_path("C:\\Users\\me\\packs\\p", "kicks/bd.wav"),
        "C:\\Users\\me\\packs\\p\\kicks\\bd.wav"
    );
    assert_eq!(
        pack_file_path("/data/packs/p/", "kicks/bd.wav"),
        "/data/packs/p/kicks/bd.wav"
    );
    assert!(kit_from_installed(&installed(manifest(), "/x"), "nope").is_none());
}

#[test]
fn kit_from_pack_skips_pads_pointing_at_unsafe_or_unknown_files() {
    let mut m = manifest();
    m.kits[0].pads.push(PackKitPad {
        pad: 5,
        file: "../../etc/passwd.wav".into(),
        name: None,
        settings: None,
    });
    m.kits[0].pads.push(PackKitPad {
        pad: 6,
        file: "missing.wav".into(),
        name: None,
        settings: None,
    });
    let kit = kit_from_installed(&installed(m, "/x"), "main").unwrap();
    assert!(kit.pads[5].sample.is_none());
    assert!(kit.pads[6].sample.is_none());
}

#[test]
fn kit_from_pack_names_unnamed_pads_from_the_file_label() {
    let mut m = manifest();
    m.files[1].label = Some("Snare · tone 0 · snappy 10".into());
    let kit = kit_from_pack(&m.id, &m, "main").unwrap();
    assert_eq!(
        kit.pads[1].sample.as_ref().unwrap().name,
        "Snare · tone 0 · snappy 10"
    );
    assert_eq!(
        service_pack_file_label("bass-drum/bd0010.wav", None),
        "bd0010"
    );
}

#[test]
fn finds_the_packs_a_project_uses_incl_legacy_absolute_paths() {
    let kit = kit_from_installed(&installed(manifest(), "/data/packs/test-pack"), "main").unwrap();
    let mut legacy = kit.clone();
    legacy.pads[0].sample = Some(PadSample {
        source: PadSampleSource::File {
            path: "/data/packs/old-pack/a.wav".into(),
            sample_id: None,
        },
        name: "a".into(),
    });
    let mut ids: Vec<String> =
        pack_ids_used_by_kits(&[Some(&kit), None, Some(&legacy)], Some("/data/packs"))
            .into_iter()
            .collect();
    ids.sort();
    assert_eq!(ids, ["old-pack", "test-pack"]);
    assert!(!pack_ids_used_by_kits(&[Some(&legacy)], None).contains("old-pack"));
}

#[test]
fn cleans_ipc_error_messages() {
    assert_eq!(
        ipc_error_message("Error invoking remote method 'packs:install': Error: HTTP 404"),
        "HTTP 404"
    );
    assert_eq!(
        ipc_error_message(
            "Error invoking remote method 'packs:install': PackValidationError: Pack x: bad"
        ),
        "Pack x: bad"
    );
    assert_eq!(
        ipc_error_message("Error invoking remote method 'packs:install': plain"),
        "plain"
    );
    assert_eq!(
        ipc_error_message("Error invoking remote method 'x': "),
        "Unknown error"
    );
    assert_eq!(ipc_error_message(""), "Unknown error");
    assert_eq!(ipc_error_message("Timed out"), "Timed out");
}

#[test]
fn names_withdrawn_packs_from_withdrawn_json() {
    let name = pack_name("tr8-mckaudio", None, &[], &vec![]);
    assert!(name.contains("TR-8 Recordings"));
    assert_eq!(
        pack_name("never-heard-of", None, &[], &vec![]),
        "never-heard-of"
    );
}

#[test]
fn remembers_names_seen_in_the_catalog_after_the_pack_is_withdrawn() {
    let first = remember_names(&vec![], [("soon-gone", "Soon Gone Kit")]).unwrap();
    let stored = serialize_remembered_names(&first);
    let remembered = parse_remembered_names(Some(&stored));
    let catalog = cat("y", vec![manifest()]);
    let state = PackStoreState {
        catalog: Some(catalog.clone()),
        installed_stamps: Some(Default::default()),
        ..Default::default()
    };
    assert!(state.is_unavailable("soon-gone"));
    assert_eq!(
        pack_name("soon-gone", Some(&catalog), &[], &remembered),
        "Soon Gone Kit"
    );
    // Unchanged names write nothing.
    assert_eq!(
        remember_names(&remembered, [("soon-gone", "Soon Gone Kit")]),
        None
    );
}

#[test]
fn remembered_names_keep_the_most_recent_200_in_js_key_order() {
    let mut names: RememberedNames = vec![];
    for i in 0..205 {
        names = remember_names(&names, [(format!("p-{i}").as_str(), "N")]).unwrap();
    }
    assert_eq!(names.len(), 200);
    assert_eq!(names[0].0, "p-5");
    // JS quirk kept: array-index-like ids sort first in an object, so on a full map the
    // trim (oldest = first keys) drops a just-remembered numeric id right away.
    let names = remember_names(&names, [("808", "Eight")]).unwrap();
    assert!(!names.iter().any(|(k, _)| k == "808"));
    assert_eq!(names.len(), 200);
    let small = remember_names(&vec![("a".into(), "A".into())], [("808", "Eight")]).unwrap();
    assert_eq!(
        small,
        vec![("808".into(), "Eight".into()), ("a".into(), "A".into())]
    );
    assert_eq!(
        parse_remembered_names(Some("{\"b\":\"B\",\"a\":\"\",\"c\":5,\"10\":\"T\"}")),
        vec![("10".into(), "T".into()), ("b".into(), "B".into())]
    );
    assert_eq!(parse_remembered_names(Some("[1]")), vec![]);
    assert_eq!(parse_remembered_names(Some("{nope")), vec![]);
    assert_eq!(parse_remembered_names(None), vec![]);
}

#[test]
fn l_c_a_pack_the_loaded_catalog_does_not_list_is_unavailable_not_missing() {
    let mut s = PackStoreState::default();
    assert_eq!(s.pack_status("gone-pack"), PackRefStatus::Unknown);
    s.invalidate_changed(&[]);
    assert_eq!(s.pack_status("gone-pack"), PackRefStatus::Missing); // catalog not loaded yet
    s.catalog = Some(cat("x", vec![manifest()]));
    assert_eq!(s.pack_status("gone-pack"), PackRefStatus::Unavailable);
    assert!(s.is_unavailable("gone-pack"));
    assert_eq!(s.pack_status("test-pack"), PackRefStatus::Missing);
    s.installed = vec![installed(
        manifest_o(json!({ "id": "gone-pack" })),
        "/data/packs/gone-pack",
    )];
    assert_eq!(s.pack_status("gone-pack"), PackRefStatus::Installed);
    s.begin_install("test-pack");
    assert_eq!(s.pack_status("test-pack"), PackRefStatus::Installing);
}

#[test]
fn install_state_reductions() {
    let mut s = PackStoreState::default();
    s.begin_install("p");
    assert!(matches!(
        s.install_state("p"),
        PackInstallState::Downloading {
            file_count: 0,
            total_bytes: None,
            ..
        }
    ));
    assert!(!s.apply_progress(
        "p",
        PackInstallState::Downloading {
            received_bytes: 5,
            total_bytes: Some(10),
            file_index: 1,
            file_count: 2
        }
    ));
    s.install_failed("p", "Install cancelled");
    assert_eq!(s.install_state("p"), PackInstallState::Idle);
    s.install_failed("p", "HTTP 404");
    assert_eq!(
        s.install_state("p"),
        PackInstallState::Error {
            message: "HTTP 404".into()
        }
    );
    s.install_failed("p", "later");
    assert_eq!(
        s.install_state("p"),
        PackInstallState::Error {
            message: "HTTP 404".into()
        }
    );
    assert!(s.apply_progress("p", PackInstallState::Installed));
    s.installed = vec![installed(manifest(), "/x")];
    assert_eq!(s.install_state("test-pack"), PackInstallState::Installed);
    assert!(s.has_update(&manifest_o(json!({ "version": 2 }))));
    assert!(!s.has_update(&manifest()));
}

#[test]
fn invalidate_changed_reports_updated_and_removed_packs() {
    let mut s = PackStoreState::default();
    let a = installed(manifest(), "/a");
    let b = installed(manifest_o(json!({ "id": "b" })), "/b");
    assert!(s.invalidate_changed(&[a.clone(), b.clone()]).is_empty()); // baseline
    let mut a2 = a.clone();
    a2.installed_at = "2026-12-01T00:00:00Z".into();
    assert_eq!(s.invalidate_changed(&[a2.clone()]), ["test-pack", "b"]);
    assert!(s.invalidate_changed(&[a2]).is_empty());
}

// ---------------------------------------------------------------------------
// packsModel.test.ts
// ---------------------------------------------------------------------------

const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn fake_machine() -> ClassicMachineInfo {
    serde_json::from_value(json!({
        "id": "tr808", "name": "808-style", "tagline": "Booming analog kick, snappy snare, metallic hats", "color": "oklch(0.70 0.18 45)",
        "voices": [{ "id": "kick", "label": "Kick", "gm": 36 }, { "id": "snare", "label": "Snare", "gm": 38 }, { "id": "ch", "label": "Closed Hat", "gm": 42 }, { "id": "oh", "label": "Open Hat", "gm": 46 }],
        "pads": ["kick", "snare", "ch", "oh"],
    }))
    .unwrap()
}

fn fake_manifest_v(over: Value) -> Value {
    let mut m = json!({
        "id": "tr808-fischer", "name": "TR-808 recordings", "tagline": "Every voice of a real 808, multisampled",
        "description": "Recordings of a TR-808 by Michael Fischer.", "tags": ["808", "analog", "hip-hop"], "author": "Michael Fischer",
        "source": "https://github.com/tidalcycles/sounds-tr808-fischer", "license": "CC0-1.0", "color": "oklch(0.7 0.15 30)",
        "files": [
            { "path": "bd/BD0000.wav", "url": "https://raw.githubusercontent.com/x/y/sha/bd/BD0000.wav", "size": 1048576, "sha256": SHA, "label": "Kick · tone 0 · decay 0" },
            { "path": "sd/SD0000.wav", "url": "https://raw.githubusercontent.com/x/y/sha/sd/SD0000.wav", "size": 524288, "sha256": SHA },
            { "path": "ch/CH.wav", "url": "https://raw.githubusercontent.com/x/y/sha/ch/CH.wav", "size": 0, "sha256": SHA },
        ],
        "kits": [{ "id": "main", "name": "808 Basic", "pads": [
            { "pad": 0, "file": "bd/BD0000.wav", "name": "Kick" },
            { "pad": 1, "file": "sd/SD0000.wav", "name": "Snare" },
            { "pad": 2, "file": "ch/CH.wav", "name": "Closed Hat", "settings": { "chokeGroup": 1 } },
        ] }],
        "version": 1,
    });
    for (k, v) in over.as_object().unwrap() {
        m[k] = v.clone();
    }
    m
}

fn fake_manifest(over: Value) -> PackManifest {
    serde_json::from_value(fake_manifest_v(over)).unwrap()
}

fn fake_installed(m: PackManifest) -> InstalledPack {
    let dir = format!("/data/packs/{}", m.id);
    InstalledPack {
        id: m.id.clone(),
        version: m.version,
        dir,
        installed_at: "2026-11-01T00:00:00Z".into(),
        manifest: m,
    }
}

fn m909() -> PackManifest {
    fake_manifest(
        json!({ "id": "tr909-x", "name": "909 Hits", "tagline": "Punchy techno", "tags": ["909", "techno", "Analog"], "author": "Jane", "license": "CC-BY-4.0" }),
    )
}

#[test]
fn builds_built_in_items_from_classic_machines() {
    let item = builtin_item(&fake_machine());
    assert!(
        matches!(&item, PackItem::Builtin { id, name, sound_count: 4, .. } if id == "builtin:tr808" && name == "808-style")
    );
    assert!(item.tags().iter().any(|t| t == "808"));
    assert!(item.tags().iter().any(|t| t == "built-in"));
}

#[test]
fn sums_known_file_sizes() {
    let m808 = fake_manifest(json!({}));
    assert_eq!(pack_size_bytes(&m808.files), Some(1024 * 1024 + 512 * 1024));
    // TS: a file without a size → null; every PackFile has a size here, so no files → None.
    assert_eq!(pack_size_bytes(&[]), None);
    assert_eq!(format_bytes(Some(1.5 * 1024.0 * 1024.0)), "1.5 MB");
    assert_eq!(format_bytes(Some(2048.0)), "2 KB");
    assert_eq!(format_bytes(None), "");
}

#[test]
fn lists_catalog_packs_and_keeps_installed_packs_the_catalog_no_longer_has() {
    let m808 = fake_manifest(json!({}));
    let orphan = fake_installed(fake_manifest(json!({ "id": "gone", "name": "Gone" })));
    let items = remote_items(
        Some(&[m808.clone(), m909(), m808.clone()]),
        &[fake_installed(m808), orphan.clone()],
    );
    assert_eq!(
        items.iter().map(PackItem::id).collect::<Vec<_>>(),
        ["tr808-fischer", "tr909-x", "gone"]
    );
    assert_eq!(
        remote_items(None, &[orphan])
            .iter()
            .map(PackItem::id)
            .collect::<Vec<_>>(),
        ["gone"]
    );
}

#[test]
fn searches_name_tags_description_author_and_licence() {
    let a = remote_item(&fake_manifest(json!({})));
    let b = remote_item(&m909());
    assert!(matches_query(&a, "fischer"));
    assert!(matches_query(&a, "808 hip"));
    assert!(!matches_query(&a, "808 techno"));
    assert!(matches_query(&b, "cc-by"));
    let ids = |v: Vec<PackItem>| v.iter().map(|i| i.id().to_owned()).collect::<Vec<_>>();
    assert_eq!(
        ids(filter_items(&[a.clone(), b.clone()], "", Some("analog"))),
        ["tr808-fischer", "tr909-x"]
    );
    assert_eq!(
        ids(filter_items(
            &[a.clone(), b.clone()],
            "punchy",
            Some("ANALOG")
        )),
        ["tr909-x"]
    );
    assert_eq!(filter_items(&[a, b], "  ", None).len(), 2);
}

#[test]
fn ranks_tags_by_count_case_insensitively() {
    let tags = top_tags(
        &[
            remote_item(&fake_manifest(json!({}))),
            remote_item(&m909()),
            builtin_item(&fake_machine()),
        ],
        3,
    );
    assert_eq!(
        tags[0],
        TagCount {
            tag: "808".into(),
            count: 2
        }
    );
    assert_eq!(
        tags[1],
        TagCount {
            tag: "analog".into(),
            count: 2
        }
    );
    assert_eq!(tags.len(), 3);
}

#[test]
fn groups_sounds_by_folder_root_first_natural_order() {
    let f = |p: &str| PackFile {
        path: p.into(),
        url: "u".into(),
        size: 0,
        sha256: SHA.into(),
        label: None,
    };
    let groups = group_files_by_folder(&[
        f("sd/SD10.wav"),
        f("sd/SD2.wav"),
        f("root.wav"),
        f("bd/BD.wav"),
    ]);
    assert_eq!(
        groups.iter().map(|g| g.folder.as_str()).collect::<Vec<_>>(),
        ["", "bd", "sd"]
    );
    assert_eq!(
        groups[2]
            .files
            .iter()
            .map(|f| f.path.as_str())
            .collect::<Vec<_>>(),
        ["sd/SD2.wav", "sd/SD10.wav"]
    );
    assert_eq!(sound_label("sd/Snare 01.wav"), "Snare 01");
}

#[test]
fn derives_the_install_button_state() {
    let item = remote_item(&fake_manifest(json!({})));
    assert_eq!(
        install_button_state(&builtin_item(&fake_machine()), None, false),
        InstallButtonState::Builtin
    );
    assert_eq!(
        install_button_state(&item, Some(&PackInstallState::Idle), false),
        InstallButtonState::Get
    );
    assert_eq!(
        install_button_state(&item, Some(&PackInstallState::Idle), true),
        InstallButtonState::Installed
    );
    assert_eq!(
        install_button_state(
            &item,
            Some(&PackInstallState::Error {
                message: "Timed out".into()
            }),
            false
        ),
        InstallButtonState::Error {
            message: "Timed out".into()
        }
    );
    let d = |r, t, i, c| PackInstallState::Downloading {
        received_bytes: r,
        total_bytes: t,
        file_index: i,
        file_count: c,
    };
    assert_eq!(
        install_button_state(&item, Some(&d(50, Some(200), 0, 3)), false),
        InstallButtonState::Downloading {
            progress: Some(0.25),
            label: "25%".into()
        }
    );
    assert!(
        matches!(install_button_state(&item, Some(&d(0, None, 1, 4)), false), InstallButtonState::Downloading { progress: Some(p), .. } if p == 0.25)
    );
    assert_eq!(download_progress(&d(0, None, 0, 0)), None);
    assert_eq!(
        pack_button_state(Some(&d(0, None, 0, 0)), false),
        InstallButtonState::Downloading {
            progress: None,
            label: "…".into()
        }
    );
}

#[test]
fn lights_the_pads_a_kit_fills() {
    let lit = lit_pads(&remote_item(&fake_manifest(json!({}))));
    assert_eq!(lit[..4], [true, true, true, false]);
    assert_eq!(
        lit_pads(&builtin_item(&fake_machine()))
            .iter()
            .filter(|b| **b)
            .count(),
        4
    );
}

fn pad_with(source: Option<PadSampleSource>) -> PadSettings {
    PadSettings {
        sample: source.map(|source| PadSample {
            source,
            name: "x".into(),
        }),
        ..PadSettings::default()
    }
}

fn kit_of_pads(pads: Vec<PadSettings>) -> KitState {
    KitState {
        id: "k".into(),
        name: "K".into(),
        pads,
    }
}

#[test]
fn finds_pads_that_use_a_pack() {
    let dir = "/data/packs/tr808-fischer";
    let pack = |id: &str, f: &str| {
        Some(PadSampleSource::Pack {
            pack_id: id.into(),
            file: f.into(),
        })
    };
    let file = |p: &str| {
        Some(PadSampleSource::File {
            path: p.into(),
            sample_id: None,
        })
    };
    let k1 = kit_of_pads(vec![
        pad_with(None),
        pad_with(pack("tr808-fischer", "bd/a.wav")),
    ]);
    assert!(kits_use_pack(&[Some(&k1)], "tr808-fischer", None));
    let k2 = kit_of_pads(vec![pad_with(file(&format!("{dir}/bd/a.wav")))]);
    assert!(kits_use_pack(&[Some(&k2)], "tr808-fischer", Some(dir)));
    let k3 = kit_of_pads(vec![pad_with(file(&format!("{dir}-other/bd/a.wav")))]);
    assert!(!kits_use_pack(&[Some(&k3)], "tr808-fischer", Some(dir)));
    let k4 = kit_of_pads(vec![pad_with(pack("other", "x.wav"))]);
    assert!(!kits_use_pack(
        &[Some(&k4), None],
        "tr808-fischer",
        Some(dir)
    ));
    let k5 = kit_of_pads(vec![pad_with(file("C:\\packs\\p\\a.wav"))]);
    assert!(kits_use_pack(&[Some(&k5)], "p", Some("C:\\packs\\p")));
}

#[test]
fn reads_the_pack_id_of_a_pack_source() {
    assert_eq!(
        pack_id_of_source(Some(&PadSampleSource::Pack {
            pack_id: "a".into(),
            file: "x".into()
        })),
        Some("a")
    );
    assert_eq!(
        pack_id_of_source(Some(&PadSampleSource::File {
            path: "/x".into(),
            sample_id: None
        })),
        None
    );
    assert_eq!(pack_id_of_source(None), None);
}

#[test]
fn labels_a_sound_by_its_catalog_label_else_its_file_name() {
    let f = |label: Option<&str>| PackFile {
        path: "bd/bd0010.wav".into(),
        url: "u".into(),
        size: 0,
        sha256: SHA.into(),
        label: label.map(str::to_owned),
    };
    assert_eq!(
        model::pack_file_label(&f(Some("Kick · tone 0 · decay 10"))),
        "Kick · tone 0 · decay 10"
    );
    assert_eq!(model::pack_file_label(&f(Some("  "))), "bd0010");
    assert_eq!(model::pack_file_label(&f(None)), "bd0010");
}

#[test]
fn n7_a_kept_staging_folder_shows_resume_other_errors_retry() {
    let paused = PackInstallState::Error {
        message: INSTALL_PAUSED_MESSAGE.into(),
    };
    assert!(is_paused_install(Some(&paused)));
    assert_eq!(
        pack_button_state(Some(&paused), false),
        InstallButtonState::Resume
    );
    assert_eq!(
        pack_button_state(Some(&paused), true),
        InstallButtonState::Resume
    );
    assert_eq!(
        pack_button_state(
            Some(&PackInstallState::Error {
                message: "HTTP 404".into()
            }),
            false
        ),
        InstallButtonState::Error {
            message: "HTTP 404".into()
        }
    );
    assert!(!is_paused_install(Some(&PackInstallState::Idle)));
}

// ---------------------------------------------------------------------------
// remoteKit.test.ts / useKitSources
// ---------------------------------------------------------------------------

#[test]
fn remote_kit_plan_maps_a_catalog_kit_to_names_settings_and_urls() {
    let m = fake_manifest(json!({}));
    let plan = remote_kit_plan(&m, "main").unwrap();
    assert_eq!(plan.kit.name, "808 Basic");
    let names: Vec<Option<&str>> = plan.kit.pads[..4]
        .iter()
        .map(|p| p.sample.as_ref().map(|s| s.name.as_str()))
        .collect();
    assert_eq!(
        names,
        [Some("Kick"), Some("Snare"), Some("Closed Hat"), None]
    );
    assert_eq!(plan.kit.pads[2].choke_group, 1);
    assert_eq!(
        plan.urls[..4],
        [
            Some(m.files[0].url.clone()),
            Some(m.files[1].url.clone()),
            Some(m.files[2].url.clone()),
            None
        ]
    );
    assert_eq!(plan.urls.len(), 16);
}

#[test]
fn remote_kit_plan_skips_unknown_files_and_kits() {
    let m = fake_manifest(json!({}));
    let bad = fake_manifest(
        json!({ "kits": [{ "id": "k", "name": "K", "pads": [{ "pad": 0, "file": "nope.wav" }, { "pad": 99, "file": m.files[0].path }] }] }),
    );
    let plan = remote_kit_plan(&bad, "k").unwrap();
    assert!(plan.urls.iter().all(Option::is_none));
    assert!(plan.kit.pads.iter().all(|p| p.sample.is_none()));
    assert!(remote_kit_plan(&m, "missing").is_none());
}

#[test]
fn remote_kit_plan_names_an_unnamed_pad_after_its_file_label() {
    let m = fake_manifest(json!({}));
    let mut files = m.files.clone();
    files[0].label = Some("Kick · tone 0".into());
    let named = fake_manifest(
        json!({ "files": files, "kits": [{ "id": "k", "name": "K", "pads": [{ "pad": 0, "file": files[0].path }] }] }),
    );
    assert_eq!(
        remote_kit_plan(&named, "k").unwrap().kit.pads[0]
            .sample
            .as_ref()
            .unwrap()
            .name,
        "Kick · tone 0"
    );
}

#[test]
fn pack_kit_choices_in_install_order() {
    let a = fake_installed(fake_manifest(json!({})));
    let b = fake_installed(m909());
    let choices = pack_kit_choices(&[a.clone(), b]);
    assert_eq!(
        choices.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
        ["pack:tr808-fischer:main", "pack:tr909-x:main"]
    );
    assert_eq!(choices[0].detail.as_deref(), Some("TR-808 recordings"));
    assert!(kit_from_installed(&a, &choices[0].kit_id).is_some());
}

// ---------------------------------------------------------------------------
// kitPreview.test.ts (pure cases)
// ---------------------------------------------------------------------------

fn kit_of(names: &[Option<&str>]) -> KitState {
    let pads = (0..16)
        .map(|i| match names.get(i).copied().flatten() {
            Some(n) => pad_with(Some(PadSampleSource::File {
                path: format!("/x/{n}.wav"),
                sample_id: None,
            }))
            .with_name(n),
            None => PadSettings::default(),
        })
        .collect();
    kit_of_pads(pads)
}

trait WithName {
    fn with_name(self, n: &str) -> Self;
}
impl WithName for PadSettings {
    fn with_name(mut self, n: &str) -> Self {
        if let Some(s) = &mut self.sample {
            s.name = n.into();
        }
        self
    }
}

/// createFactoryKit(): FACTORY_PAD_VOICES with FACTORY_VOICE_LABELS names.
fn factory_kit() -> KitState {
    use FactoryVoiceId::*;
    let voices = [
        (Kick, "Kick"),
        (Snare, "Snare"),
        (Chh, "Closed HH"),
        (Ohh, "Open HH"),
        (Clap, "Clap"),
        (TomMid, "Mid Tom"),
        (Crash, "Crash"),
        (Ride, "Ride"),
        (KickSub, "Sub Kick"),
        (Rim, "Rim"),
        (Phh, "Pedal HH"),
        (Shaker, "Shaker"),
        (Snap, "Snap"),
        (TomLo, "Low Tom"),
        (TomHi, "High Tom"),
        (Perc, "Cowbell"),
    ];
    kit_of_pads(
        voices
            .iter()
            .map(|(v, n)| PadSettings {
                sample: Some(PadSample {
                    source: PadSampleSource::Factory { voice: *v },
                    name: (*n).into(),
                }),
                ..PadSettings::default()
            })
            .collect(),
    )
}

#[test]
fn classifies_common_drum_names_and_abbreviations() {
    use DrumRole::*;
    for (name, role) in [
        ("Kick", Some(Kick)),
        ("BD0010", Some(Kick)),
        ("808 bd long", Some(Kick)),
        ("SD0025", Some(Snare)),
        ("Clap", Some(Clap)),
        ("Closed HH", Some(ClosedHat)),
        ("CH", Some(ClosedHat)),
        ("Open HH", Some(OpenHat)),
        ("OH_02", Some(OpenHat)),
        ("Cowbell", Some(Perc)),
        ("Crash", None),
    ] {
        assert_eq!(classify_pad_name(name), role, "{name}");
    }
}

#[test]
fn finds_roles_by_name_falling_back_to_the_usual_layout() {
    let r = kit_roles(&factory_kit());
    assert_eq!(
        (r.kick, r.snare, r.closed_hat, r.open_hat, r.clap),
        (Some(0), Some(1), Some(2), Some(3), Some(4))
    );
    let r = kit_roles(&kit_of(&[Some("Snare"), Some("Hat"), Some("Kick")]));
    assert_eq!((r.kick, r.snare, r.closed_hat), (Some(2), Some(0), Some(1)));
    assert_eq!(
        kit_roles(&kit_of(&[Some("Thing A"), Some("Thing B")])),
        KitRoles {
            kick: Some(0),
            snare: Some(1),
            ..Default::default()
        }
    );
}

#[test]
fn builds_a_2_bar_groove_within_range_using_only_pads_with_sounds() {
    let kit = factory_kit();
    let hits = demo_pattern(&kit);
    assert!(hits.len() > 20);
    for h in &hits {
        assert!(h.step < DEMO_STEPS);
        assert!(kit.pads[h.pad].sample.is_some());
    }
    assert_eq!(hits.iter().filter(|h| h.pad == 0 && h.step == 0).count(), 1);
    assert!(!hits.iter().any(|h| h.pad == 2 && h.step == 14));
    assert!(hits.iter().any(|h| h.pad == 3 && h.step == 14));
}

#[test]
fn walks_the_pads_when_no_roles_are_found_and_is_empty_for_an_empty_kit() {
    let hits = demo_pattern(&kit_of(&[
        None,
        None,
        None,
        None,
        None,
        Some("Zap"),
        Some("Zip"),
    ]));
    assert_eq!(
        hits,
        [
            DemoHit {
                step: 0,
                pad: 5,
                velocity: 0.85
            },
            DemoHit {
                step: 4,
                pad: 6,
                velocity: 0.85
            }
        ]
    );
    assert!(demo_pattern(&kit_of(&[])).is_empty());
}

#[test]
fn reports_which_pads_sound_at_a_given_time() {
    let hit = |step, pad| DemoHit {
        step,
        pad,
        velocity: 1.0,
    };
    let session = PreviewSession {
        id: "x".into(),
        t0: 10.0,
        step_dur: 0.1,
        hits: vec![hit(0, 0), hit(2, 1), hit(2, 2)],
    };
    assert!(pads_at(&session, 9.9).is_empty());
    assert_eq!(pads_at(&session, 10.05), [0]);
    let mut p = pads_at(&session, 10.25);
    p.sort();
    assert_eq!(p, [1, 2]);
    assert_eq!(step_seconds(DEMO_BPM), 60.0 / 96.0 / 4.0);
}

// ---------------------------------------------------------------------------
// sender.test.ts
// ---------------------------------------------------------------------------

const DEV: &str = "http://localhost:5173";
const PROD: &str = "file:///Applications/Micro%20DAW.app/Contents/Resources/app.asar/.vite/renderer/main_window/index.html";

#[test]
fn accepts_the_dev_server_origin() {
    assert!(is_trusted_renderer_url(Some("http://localhost:5173/"), DEV));
    assert!(is_trusted_renderer_url(
        Some("http://localhost:5173/index.html#/packs"),
        DEV
    ));
}

#[test]
fn rejects_other_origins_ports_and_protocols() {
    for u in [
        "http://localhost:5174/",
        "https://localhost:5173/",
        "https://evil.example/",
        "file:///tmp/index.html",
    ] {
        assert!(!is_trusted_renderer_url(Some(u), DEV), "{u}");
    }
}

#[test]
fn accepts_only_the_packaged_index_html_for_file_renderers() {
    assert!(is_trusted_renderer_url(Some(PROD), PROD));
    assert!(is_trusted_renderer_url(
        Some(&format!("{PROD}#/beat")),
        PROD
    ));
    assert!(!is_trusted_renderer_url(
        Some("file:///tmp/evil.html"),
        PROD
    ));
    assert!(!is_trusted_renderer_url(
        Some(&PROD.replace("index.html", "other.html")),
        PROD
    ));
    assert!(!is_trusted_renderer_url(
        Some("https://raw.githubusercontent.com/x/index.html"),
        PROD
    ));
}

#[test]
fn rejects_missing_or_malformed_urls() {
    assert!(!is_trusted_renderer_url(None, DEV));
    assert!(!is_trusted_renderer_url(Some(""), DEV));
    assert!(!is_trusted_renderer_url(Some("not a url"), DEV));
    assert!(!is_trusted_renderer_url(Some(DEV), "not a url"));
}

#[test]
fn assert_trusted_sender_passes_the_main_frame() {
    assert_eq!(
        assert_trusted_sender(
            Some(&SenderFrame {
                url: format!("{DEV}/"),
                has_parent: false
            }),
            DEV
        ),
        Ok(())
    );
}

#[test]
fn assert_trusted_sender_throws_for_subframes_foreign_pages_and_missing_frames() {
    assert_eq!(
        assert_trusted_sender(
            Some(&SenderFrame {
                url: format!("{DEV}/"),
                has_parent: true
            }),
            DEV
        ),
        Err("Untrusted IPC sender")
    );
    assert_eq!(
        assert_trusted_sender(
            Some(&SenderFrame {
                url: "https://evil.example/".into(),
                has_parent: false
            }),
            DEV
        ),
        Err("Untrusted IPC sender")
    );
    assert_eq!(
        assert_trusted_sender(None, DEV),
        Err("Untrusted IPC sender")
    );
}

#[test]
fn machine_ids_round_trip() {
    assert_eq!(builtin_item_id(ClassicMachineId::Cr78), "builtin:cr78");
}

// ---------------------------------------------------------------------------
// Golden: rune/parity/fixtures/packs_validate.json
// ---------------------------------------------------------------------------

fn outcome<T: serde::Serialize>(r: Result<T, PackValidationError>) -> Value {
    match r {
        Ok(v) => json!({ "ok": serde_json::to_value(v).unwrap() }),
        Err(e) => json!({ "error": e.0 }),
    }
}

fn catalog_validation_value(r: Result<CatalogValidation, PackValidationError>) -> Value {
    outcome(r.map(|c| json!({ "catalog": c.catalog, "errors": c.errors })))
}

#[test]
fn golden_validate_matches_typescript() {
    let fx: Value = serde_json::from_str(include_str!(
        "../../../../parity/fixtures/packs_validate.json"
    ))
    .unwrap();
    let mut n = 0;

    let raw: Value =
        serde_json::from_str(include_str!("../../../../../packs/catalog.json")).unwrap();
    assert_eq!(
        catalog_validation_value(validate_catalog(&raw)),
        fx["catalog"],
        "real catalog"
    );
    n += 1;

    for case in fx["manifests"].as_array().unwrap() {
        assert_eq!(
            outcome(validate_manifest(&case["input"])),
            case["out"],
            "manifest {}",
            case["input"]
        );
        n += 1;
    }
    for case in fx["envelopes"].as_array().unwrap() {
        assert_eq!(
            catalog_validation_value(validate_catalog(&case["input"])),
            case["out"],
            "catalog {}",
            case["input"]
        );
        n += 1;
    }
    for case in fx["installed"].as_array().unwrap() {
        assert_eq!(
            outcome(validate_installed(&case["input"], "/real/test-pack")),
            case["out"],
            "installed {}",
            case["input"]
        );
        n += 1;
    }
    for row in fx["ids"].as_array().unwrap() {
        assert_eq!(
            Value::Bool(is_valid_pack_id_value(&row[0])),
            row[1],
            "id {}",
            row[0]
        );
        n += 1;
    }
    for row in fx["urls"].as_array().unwrap() {
        let got = json!([
            row[0],
            is_allowed_url_value(&row[0]),
            is_pinned_pack_url_value(&row[0]),
            is_github_source_url_value(&row[0])
        ]);
        assert_eq!(&got, row, "url {}", row[0]);
        n += 1;
    }
    for row in fx["colors"].as_array().unwrap() {
        assert_eq!(
            Value::Bool(is_safe_pack_color_value(&row[0])),
            row[1],
            "color {}",
            row[0]
        );
        n += 1;
    }
    for row in fx["paths"].as_array().unwrap() {
        assert_eq!(
            outcome(safe_pack_path_segments_value(&row[0])),
            row[1],
            "path {}",
            row[0]
        );
        n += 1;
    }
    for case in fx["merges"].as_array().unwrap() {
        let bundled: PackCatalog = serde_json::from_value(case["bundled"].clone()).unwrap();
        let remote: PackCatalog = serde_json::from_value(case["remote"].clone()).unwrap();
        let errors: Vec<String> = serde_json::from_value(case["errors"].clone()).unwrap();
        let m = merge_catalogs(&bundled, &remote, &errors);
        assert_eq!(
            json!({ "catalog": m.catalog, "source": m.source }),
            case["out"],
            "merge {}",
            case["remote"]["updatedAt"]
        );
        n += 1;
    }
    let c: PackCatalog = serde_json::from_value(fx["findCatalog"].clone()).unwrap();
    for row in fx["finds"].as_array().unwrap() {
        let got = row[0]
            .as_str()
            .and_then(|u| find_catalog_file(&c, u))
            .map(|f| serde_json::to_value(f).unwrap())
            .unwrap_or(Value::Null);
        assert_eq!(got, row[1]);
        n += 1;
    }
    let declared: Vec<i64> = std::iter::once(declared_pack_bytes(&manifest()))
        .chain(
            validate_catalog(&raw)
                .unwrap()
                .catalog
                .packs
                .iter()
                .map(declared_pack_bytes),
        )
        .collect();
    assert_eq!(serde_json::to_value(declared).unwrap(), fx["declared"]);
    n += 1;
    eprintln!("packs_validate golden comparisons: {n}");
}

// ---------------------------------------------------------------------------
// Golden: rune/parity/fixtures/packs_model.json
// ---------------------------------------------------------------------------

/// JSON numbers compared like JS numbers (`1.0` and `1` are the same value).
fn js_norm(v: &Value) -> Value {
    match v {
        Value::Number(_) => js::normalize_number(v),
        Value::Array(a) => Value::Array(a.iter().map(js_norm).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, v)| (k.clone(), js_norm(v))).collect()),
        other => other.clone(),
    }
}

fn to_v<T: serde::Serialize>(t: &T) -> Value {
    js_norm(&serde_json::to_value(t).unwrap())
}

#[test]
fn golden_model_matches_typescript() {
    let fx: Value =
        serde_json::from_str(include_str!("../../../../parity/fixtures/packs_model.json")).unwrap();
    let mut n = 0;
    let catalog = real_catalog();
    let machines: Vec<ClassicMachineInfo> = serde_json::from_value(fx["machines"].clone()).unwrap();
    let installed: Vec<InstalledPack> = serde_json::from_value(fx["installed"].clone()).unwrap();
    let items: Vec<PackItem> = machines
        .iter()
        .map(builtin_item)
        .chain(remote_items(Some(&catalog.packs), &installed))
        .collect();

    let summaries: Vec<Value> = items
        .iter()
        .map(|i| {
            json!({
                "id": i.id(), "kind": if i.is_builtin() { "builtin" } else { "remote" }, "name": i.name(), "description": i.description(),
                "tags": i.tags(), "soundCount": i.sound_count(),
                "sizeBytes": match i { PackItem::Remote { size_bytes, .. } => json!(size_bytes), _ => Value::Null },
                "lit": lit_pads(i).to_vec(),
            })
        })
        .collect();
    assert_eq!(Value::Array(summaries), fx["items"]);
    n += 1;

    for f in fx["filters"].as_array().unwrap() {
        let got: Vec<String> =
            filter_items(&items, f["query"].as_str().unwrap(), f["tag"].as_str())
                .iter()
                .map(|i| i.id().to_owned())
                .collect();
        assert_eq!(
            json!(got),
            f["ids"],
            "filter {:?} {:?}",
            f["query"],
            f["tag"]
        );
        n += 1;
    }
    for (limit, want) in [1, 3, 10, 100]
        .into_iter()
        .zip(fx["top"].as_array().unwrap())
    {
        assert_eq!(to_v(&top_tags(&items, limit)), *want, "topTags {limit}");
        n += 1;
    }
    for row in fx["sizes"].as_array().unwrap() {
        let b = match &row[0] {
            Value::Number(x) => x.as_f64(),
            Value::String(s) if s == "NaN" => Some(f64::NAN),
            Value::String(s) if s == "Infinity" => Some(f64::INFINITY),
            _ => None,
        };
        assert_eq!(
            format_bytes(b),
            row[1].as_str().unwrap(),
            "formatBytes {}",
            row[0]
        );
        n += 1;
    }
    for (p, want) in catalog.packs.iter().zip(fx["groups"].as_array().unwrap()) {
        let got: Vec<Value> = group_files_by_folder(&p.files)
            .iter()
            .map(|g| json!({ "folder": g.folder, "files": g.files.iter().map(|f| f.path.clone()).collect::<Vec<_>>() }))
            .collect();
        assert_eq!(Value::Array(got), *want, "groups {}", p.id);
        n += 1;
    }
    let files: Vec<&PackFile> = catalog.packs.iter().flat_map(|p| p.files.iter()).collect();
    for (f, row) in files.iter().zip(fx["labels"].as_array().unwrap()) {
        let got = json!([
            f.path,
            sound_label(&f.path),
            model::pack_file_label(f),
            service_pack_file_label(&f.path, f.label.as_deref())
        ]);
        assert_eq!(&got, row);
        n += 1;
    }
    for row in fx["extraLabels"].as_array().unwrap() {
        let p = row[0].as_str().unwrap();
        assert_eq!(
            json!([p, sound_label(p), service_pack_file_label(p, None)]),
            *row
        );
        n += 1;
    }
    for row in fx["buttons"].as_array().unwrap() {
        let state: Option<PackInstallState> = serde_json::from_value(row[0].clone()).unwrap();
        let got = pack_button_state(state.as_ref(), row[1].as_bool().unwrap());
        assert_eq!(to_v(&got), js_norm(&row[2]), "button {} {}", row[0], row[1]);
        n += 1;
    }
    let kit_ids: Vec<(&PackManifest, &str)> = catalog
        .packs
        .iter()
        .flat_map(|m| m.kits.iter().map(move |k| (m, k.id.as_str())))
        .collect();
    let kits: Vec<KitState> = kit_ids
        .iter()
        .map(|(m, k)| kit_from_pack(&m.id, m, k).unwrap())
        .collect();
    for (kit, want) in kits.iter().zip(fx["kits"].as_array().unwrap()) {
        assert_eq!(
            to_v(kit),
            js_norm(&want["state"]),
            "kitFromPack {}/{}",
            want["pack"],
            want["kit"]
        );
        n += 1;
    }
    for ((m, k), want) in kit_ids.iter().zip(fx["plans"].as_array().unwrap()) {
        let plan = remote_kit_plan(m, k).unwrap();
        assert_eq!(
            json!({ "kit": to_v(&plan.kit), "urls": plan.urls }),
            js_norm(&want["plan"]),
            "remoteKitPlan {}/{k}",
            m.id
        );
        n += 1;
    }
    for (kit, want) in kits.iter().zip(fx["roles"].as_array().unwrap()) {
        assert_eq!(to_v(&kit_roles(kit)), want["roles"], "kitRoles {}", kit.id);
        assert_eq!(
            to_v(&demo_pattern(kit)),
            js_norm(&want["demo"]),
            "demoPattern {}",
            kit.id
        );
        n += 2;
    }
    for row in fx["classified"].as_array().unwrap() {
        let name = row[0].as_str().unwrap();
        assert_eq!(to_v(&classify_pad_name(name)), row[1], "classify {name:?}");
        n += 1;
    }
    for row in fx["ipc"].as_array().unwrap() {
        assert_eq!(
            ipc_error_message(row[0].as_str().unwrap()),
            row[1].as_str().unwrap(),
            "ipc {}",
            row[0]
        );
        n += 1;
    }
    for row in fx["paths"].as_array().unwrap() {
        assert_eq!(
            pack_file_path(row[0].as_str().unwrap(), row[1].as_str().unwrap()),
            row[2].as_str().unwrap()
        );
        n += 1;
    }
    for row in fx["senders"].as_array().unwrap() {
        assert_eq!(
            is_trusted_renderer_url(row[0].as_str(), row[1].as_str().unwrap()),
            row[2].as_bool().unwrap(),
            "sender {} {}",
            row[0],
            row[1]
        );
        n += 1;
    }
    eprintln!("packs_model golden comparisons: {n}");
}
