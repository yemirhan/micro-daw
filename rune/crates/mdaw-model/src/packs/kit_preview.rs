//! The pure half of `src/components/packs/kitPreview.ts` (drum-role detection from pad names,
//! the 2-bar demo groove, which pads sound at a time) and `remoteKit.ts`'s `remoteKitPlan`
//! (a catalog kit as a KitState plus the remote URL of each pad). The players themselves are
//! audio code (a later crate).

use std::collections::HashMap;

use serde::Serialize;

use super::PackManifest;
use super::model::pack_file_label;
use super::service::{fallback_sample_name, normalize_pad_settings};
use crate::types::{KitState, PAD_COUNT, PadIndex, PadSample, PadSampleSource, PadSettings};

/// `DrumRole`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DrumRole {
    Kick,
    Snare,
    Clap,
    ClosedHat,
    OpenHat,
    Perc,
}

/// ASCII case-insensitive substring search (JS non-unicode `i` regexps fold ASCII only here).
fn find_ci(hay: &[char], needle: &str, from: usize) -> Option<usize> {
    let n: Vec<char> = needle.chars().collect();
    (from..=hay.len().saturating_sub(n.len())).find(|&i| {
        i + n.len() <= hay.len()
            && hay[i..i + n.len()]
                .iter()
                .zip(&n)
                .all(|(a, b)| a.to_ascii_lowercase() == *b)
    })
}

fn has_ci(hay: &[char], needle: &str) -> bool {
    find_ci(hay, needle, 0).is_some()
}

/// `a\s*-?\s*b` (case-insensitive), e.g. `hi\s*-?\s*hat`, `bass\s*drum` (no `-` allowed there).
fn has_split(hay: &[char], a: &str, b: &str, dash: bool) -> bool {
    let mut from = 0;
    while let Some(i) = find_ci(hay, a, from) {
        let mut j = i + a.len();
        while j < hay.len() && super::js::is_js_ws(hay[j]) {
            j += 1;
        }
        if dash && hay.get(j) == Some(&'-') {
            j += 1;
            while j < hay.len() && super::js::is_js_ws(hay[j]) {
                j += 1;
            }
        }
        if find_ci(hay, b, j) == Some(j) {
            return true;
        }
        from = i + 1;
    }
    false
}

/// `tok(alts)`: `(^|[^a-z])(alt1|alt2|…)([^a-z]|$)` (case-insensitive): an abbreviation as its
/// own token ("BD01", "808 bd", "sd-2"; letters may not touch it).
fn has_token(hay: &[char], alts: &[&str]) -> bool {
    (0..hay.len()).any(|i| {
        (i == 0 || !hay[i - 1].is_ascii_alphabetic())
            && alts.iter().any(|a| {
                let end = i + a.len();
                find_ci(hay, a, i) == Some(i)
                    && (end == hay.len() || !hay[end].is_ascii_alphabetic())
            })
    })
}

/// Role of a pad from its sample name (None = unknown). Order matters: open hat before closed
/// hat (both say "hat").
pub fn classify_pad_name(name: &str) -> Option<DrumRole> {
    // name.replace(/[_.]+/g, ' ')
    let n = replace_underscores_dots(name);
    let any = |words: &[&str]| words.iter().any(|w| has_ci(&n, w));
    if any(&["open"]) || has_token(&n, &["oh", "ohh"]) {
        return Some(DrumRole::OpenHat);
    }
    if has_split(&n, "hi", "hat", true)
        || any(&["closed", "pedal"])
        || has_token(&n, &["ch", "chh", "phh", "hh", "hat"])
    {
        return Some(DrumRole::ClosedHat);
    }
    if any(&["kick"]) || has_split(&n, "bass", "drum", false) || has_token(&n, &["bd", "kik", "kk"])
    {
        return Some(DrumRole::Kick);
    }
    if any(&["snare"]) || has_token(&n, &["sd", "snr", "sn"]) {
        return Some(DrumRole::Snare);
    }
    if any(&["clap"]) || has_token(&n, &["cp"]) {
        return Some(DrumRole::Clap);
    }
    if any(&[
        "cowbell", "clave", "shaker", "maraca", "conga", "bongo", "tom", "rim", "tamb", "perc",
        "cabasa", "agogo",
    ]) || has_token(&n, &["rs", "cb", "cl", "ma", "lt", "mt", "ht"])
    {
        return Some(DrumRole::Perc);
    }
    None
}

fn replace_underscores_dots(name: &str) -> Vec<char> {
    let mut out: Vec<char> = Vec::new();
    let mut in_run = false;
    for c in name.chars() {
        if c == '_' || c == '.' {
            if !in_run {
                out.push(' ');
            }
            in_run = true;
        } else {
            out.push(c);
            in_run = false;
        }
    }
    out
}

/// First pad for each role (`kitRoles`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KitRoles {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kick: Option<PadIndex>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snare: Option<PadIndex>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clap: Option<PadIndex>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closed_hat: Option<PadIndex>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_hat: Option<PadIndex>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub perc: Option<PadIndex>,
}

impl KitRoles {
    fn slot(&mut self, role: DrumRole) -> &mut Option<PadIndex> {
        match role {
            DrumRole::Kick => &mut self.kick,
            DrumRole::Snare => &mut self.snare,
            DrumRole::Clap => &mut self.clap,
            DrumRole::ClosedHat => &mut self.closed_hat,
            DrumRole::OpenHat => &mut self.open_hat,
            DrumRole::Perc => &mut self.perc,
        }
    }
    fn values(&self) -> Vec<PadIndex> {
        [
            self.kick,
            self.snare,
            self.clap,
            self.closed_hat,
            self.open_hat,
            self.perc,
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

/// First pad for each role; falls back to the usual layout (0 kick, 1 snare, 2 closed hat,
/// 3 open hat, 4 clap).
pub fn kit_roles(kit: &KitState) -> KitRoles {
    let mut roles = KitRoles::default();
    for (i, p) in kit.pads.iter().enumerate() {
        let Some(sample) = &p.sample else { continue };
        if let Some(role) = classify_pad_name(&sample.name) {
            let slot = roles.slot(role);
            if slot.is_none() {
                *slot = Some(i);
            }
        }
    }
    let has = |i: usize| kit.pads.get(i).is_some_and(|p| p.sample.is_some());
    let mut used = roles.values();
    for (role, pad) in [
        (DrumRole::Kick, 0),
        (DrumRole::Snare, 1),
        (DrumRole::ClosedHat, 2),
        (DrumRole::OpenHat, 3),
        (DrumRole::Clap, 4),
    ] {
        if roles.slot(role).is_none() && has(pad) && !used.contains(&pad) {
            *roles.slot(role) = Some(pad);
            used.push(pad);
        }
    }
    roles
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct DemoHit {
    /// 16th-note step from the start, 0..31.
    pub step: u32,
    pub pad: PadIndex,
    pub velocity: f64,
}

pub const DEMO_STEPS: u32 = 32;
pub const DEMO_BPM: f64 = 96.0;

/// A 2-bar groove for the kit (16th steps). Empty when the kit has no sounds.
pub fn demo_pattern(kit: &KitState) -> Vec<DemoHit> {
    let r = kit_roles(kit);
    let mut hits = Vec::new();
    let mut add = |pad: Option<PadIndex>, steps: &[u32], velocity: f64| {
        if let Some(pad) = pad {
            hits.extend(steps.iter().map(|&step| DemoHit {
                step,
                pad,
                velocity,
            }));
        }
    };
    add(r.kick, &[0, 6, 10, 16, 22, 26, 27], 0.95);
    add(r.snare.or(r.clap), &[4, 12, 20, 28], 0.9);
    if r.snare.is_some() {
        add(r.clap, &[28], 0.7);
    }
    let hat_steps: Vec<u32> = (0..16)
        .map(|i| i * 2)
        .filter(|s| *s != 14 && *s != 30)
        .collect();
    add(r.closed_hat, &hat_steps, 0.6);
    add(r.closed_hat, &[7, 23], 0.35);
    add(r.open_hat, &[14, 30], 0.7);
    add(r.perc, &[11, 25], 0.55);
    if hits.is_empty() {
        // No recognisable roles: walk the first sounding pads.
        let pads: Vec<usize> = kit
            .pads
            .iter()
            .enumerate()
            .filter(|(_, p)| p.sample.is_some())
            .map(|(i, _)| i)
            .take(8)
            .collect();
        hits.extend(pads.into_iter().enumerate().map(|(k, pad)| DemoHit {
            step: k as u32 * 4,
            pad,
            velocity: 0.85,
        }));
    }
    hits.sort_by(|a, b| a.step.cmp(&b.step).then(a.pad.cmp(&b.pad)));
    hits
}

/// Seconds per 16th step.
pub fn step_seconds(bpm: f64) -> f64 {
    60.0 / bpm / 4.0
}

/// The running preview, for UIs that light pads in time with it.
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewSession {
    pub id: String,
    /// Clock time of step 0.
    pub t0: f64,
    pub step_dur: f64,
    pub hits: Vec<DemoHit>,
}

/// Pads sounding at clock time `now` (a hit lights its pad for one step), in hit order.
pub fn pads_at(session: &PreviewSession, now: f64) -> Vec<PadIndex> {
    let step = ((now - session.t0) / session.step_dur).floor();
    let mut out = Vec::new();
    if step < 0.0 || step.is_nan() {
        return out;
    }
    for h in &session.hits {
        if h.step as f64 == step && !out.contains(&h.pad) {
            out.push(h.pad);
        }
    }
    out
}

/// `RemoteKitPlan`: a not-installed catalog kit, sounds by URL.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteKitPlan {
    pub kit: KitState,
    /// Remote URL per pad (None = empty pad).
    pub urls: Vec<Option<String>>,
}

/// `remoteKitPlan(manifest, kitId)`: pad names / settings plus each pad's remote URL. Pads play
/// a `file` source `remote:<url>` (never loaded: the preview plays the remote buffer).
pub fn remote_kit_plan(manifest: &PackManifest, kit_id: &str) -> Option<RemoteKitPlan> {
    let kit = manifest.kits.iter().find(|k| k.id == kit_id)?;
    let files: HashMap<&str, &super::PackFile> = manifest
        .files
        .iter()
        .map(|f| (f.path.as_str(), f))
        .collect();
    let mut pads: Vec<PadSettings> = (0..PAD_COUNT).map(|_| PadSettings::default()).collect();
    let mut urls: Vec<Option<String>> = vec![None; PAD_COUNT];
    for p in &kit.pads {
        if p.pad < 0 || p.pad >= PAD_COUNT as i64 {
            continue;
        }
        let Some(file) = files.get(p.file.as_str()) else {
            continue;
        };
        let i = p.pad as usize;
        urls[i] = Some(file.url.clone());
        let path = format!("remote:{}", file.url);
        let name = match p.name.as_deref() {
            Some(n) if !n.is_empty() => n.to_owned(),
            _ => pack_file_label(file),
        };
        let name = if name.is_empty() {
            fallback_sample_name(&path)
        } else {
            name
        };
        let sample = PadSample {
            source: PadSampleSource::File {
                path,
                sample_id: None,
            },
            name,
        };
        pads[i] = normalize_pad_settings(p.settings.as_ref(), Some(sample));
    }
    Some(RemoteKitPlan {
        kit: KitState {
            id: format!("preview:{}:{}", manifest.id, kit.id),
            name: kit.name.clone(),
            pads,
        },
        urls,
    })
}
