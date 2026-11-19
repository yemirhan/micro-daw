//! The in-app pack store ("Packs"): shared types (`src/types/packs.ts`), the pure validator
//! (`src/shared/packValidate.ts`, re-exported by `src/main/packs/validate.ts`), the Packs page
//! model (`src/components/packs/packsModel.ts`, `remoteKit.ts`, the pure half of
//! `kitPreview.ts`, `useKitSources.ts`), the pure parts of `src/services/PackService.ts`, and
//! the pure main-process helpers (`src/main/packs/pausedState.ts`, `sender.ts`).
//!
//! No network or file IO lives here (the downloader / installer is a later crate).
//!
//! Numbers: the TS types use `number`; integral fields (`size`, `version`, `pad`) are `i64`
//! here. The validator only lets integers through, so this is lossless for every value
//! JavaScript can represent exactly; absurd integral values beyond ±2^63 saturate.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// JS semantics helpers (moved to [`crate::js`]; kept reachable as `packs::js`).
pub use crate::js;
pub mod kit_preview;
pub mod model;
pub mod sender;
pub mod service;
pub mod validate;

pub use model::*;
pub use service::*;
pub use validate::*;

#[cfg(test)]
mod tests;

/// `PackLicense`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PackLicense {
    #[serde(rename = "CC0-1.0")]
    Cc0,
    #[serde(rename = "public-domain")]
    PublicDomain,
    #[serde(rename = "CC-BY-4.0")]
    CcBy4,
}

impl PackLicense {
    pub const ALL: [PackLicense; 3] = [Self::Cc0, Self::PublicDomain, Self::CcBy4];

    pub fn id(self) -> &'static str {
        match self {
            Self::Cc0 => "CC0-1.0",
            Self::PublicDomain => "public-domain",
            Self::CcBy4 => "CC-BY-4.0",
        }
    }

    pub fn from_id(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.id() == s)
    }
}

/// One downloadable audio file in a pack (`PackFile`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackFile {
    /// Path inside the installed pack folder, POSIX, no leading slash.
    pub path: String,
    /// Commit-pinned raw.githubusercontent.com URL.
    pub url: String,
    /// Exact byte count.
    pub size: i64,
    /// Lower-case hex sha256.
    pub sha256: String,
    /// Optional display label (≤ 80 chars).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// `PackKitPad`: pad index 0..15 → a file in the pack.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackKitPad {
    pub pad: i64,
    pub file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// `Partial<Omit<PadSettings, 'sample'>>`: plain JSON scalars only (clamped by normalizePad).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<Map<String, Value>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackKit {
    pub id: String,
    pub name: String,
    pub pads: Vec<PackKitPad>,
}

/// `PackManifest`. Field order is the order `validateManifest` builds the object in
/// (`attribution`, when present, is appended last), so a re-serialized manifest matches the
/// TS `JSON.stringify` output byte for byte.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackManifest {
    pub id: String,
    pub name: String,
    pub tagline: String,
    pub description: String,
    pub tags: Vec<String>,
    pub author: String,
    pub source: String,
    pub license: PackLicense,
    pub color: String,
    pub files: Vec<PackFile>,
    pub kits: Vec<PackKit>,
    pub version: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribution: Option<String>,
}

/// `PackCatalog` (`version` is always 1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackCatalog {
    pub version: u32,
    pub updated_at: String,
    pub packs: Vec<PackManifest>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledPack {
    pub id: String,
    pub version: i64,
    /// Absolute folder path of the installed pack.
    pub dir: String,
    pub installed_at: String,
    pub manifest: PackManifest,
}

/// `PackInstallState`, tagged by `state` like the TS union.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum PackInstallState {
    Idle,
    Downloading {
        #[serde(rename = "receivedBytes")]
        received_bytes: u64,
        #[serde(rename = "totalBytes")]
        total_bytes: Option<u64>,
        #[serde(rename = "fileIndex")]
        file_index: u64,
        #[serde(rename = "fileCount")]
        file_count: u64,
    },
    Installed,
    Error {
        message: String,
    },
}

/// Hosts the pack store may download from (catalog, pack files, previews, every redirect hop).
pub const PACK_ALLOWED_HOSTS: [&str; 1] = ["raw.githubusercontent.com"];

/// Remote catalog (falls back to the copy bundled with the app).
pub const PACK_CATALOG_URL: &str =
    "https://raw.githubusercontent.com/yemirhan/micro-daw/main/packs/catalog.json";

/// `INSTALL_PAUSED_MESSAGE` (src/main/packs/pausedState.ts): the `error` message `packs:states`
/// reports for an interrupted install whose verified files were kept (card shows "Resume").
pub const INSTALL_PAUSED_MESSAGE: &str = "Download paused";
