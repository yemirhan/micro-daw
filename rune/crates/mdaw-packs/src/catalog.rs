//! Pack catalog loading (`src/main/packs/catalog.ts`): the remote `PACK_CATALOG_URL` with a
//! timeout, validated and merged over the copy bundled into the app (`packs/catalog.json`,
//! compiled in like the Vite main build does).

use std::time::Duration;

use mdaw_model::packs::{
    CatalogSource, EPOCH_ISO, MAX_CATALOG_BYTES, MergedCatalog, PACK_CATALOG_URL, PackCatalog,
    merge_catalogs, validate_catalog,
};

use crate::download::{DownloadOptions, download_to_buffer};
use crate::http::Connector;
use crate::signal::Signal;

/// `CATALOG_TIMEOUT_MS`: total and idle.
pub const CATALOG_TIMEOUT: Duration = Duration::from_millis(8000);

/// `packs/catalog.json` as shipped with this build.
pub const BUNDLED_CATALOG_JSON: &str = include_str!("../../../../packs/catalog.json");

/// `bundledCatalog(raw)`: validated; empty when the bundled file is broken.
pub fn bundled_catalog_from(raw: &str) -> PackCatalog {
    let parsed = serde_json::from_str::<serde_json::Value>(raw)
        .map_err(|e| e.to_string())
        .and_then(|v| validate_catalog(&v).map_err(|e| e.0));
    match parsed {
        Ok(v) => {
            if !v.errors.is_empty() {
                eprintln!("[packs] bundled catalog: dropped packs {:?}", v.errors);
            }
            v.catalog
        }
        Err(err) => {
            eprintln!("[packs] bundled catalog invalid {err}");
            PackCatalog {
                version: 1,
                updated_at: EPOCH_ISO.into(),
                packs: vec![],
            }
        }
    }
}

/// The catalog shipped with the app.
pub fn bundled_catalog() -> PackCatalog {
    bundled_catalog_from(BUNDLED_CATALOG_JSON)
}

/// `parseCatalog`: parse (UTF-8, invalid bytes replaced like `TextDecoder`) + validate. Err when
/// the JSON or the envelope is invalid.
pub fn parse_catalog(bytes: &[u8]) -> Result<(PackCatalog, Vec<String>), String> {
    let text = String::from_utf8_lossy(bytes);
    let raw: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let v = validate_catalog(&raw).map_err(|e| e.0)?;
    if !v.errors.is_empty() {
        eprintln!("[packs] remote catalog: dropped packs {:?}", v.errors);
    }
    Ok((v.catalog, v.errors))
}

/// `fetchCatalog` options.
pub struct FetchCatalogOptions<'a> {
    pub url: &'a str,
    pub connector: &'a dyn Connector,
    pub timeout: Duration,
    pub bundled: PackCatalog,
}

impl<'a> FetchCatalogOptions<'a> {
    pub fn new(connector: &'a dyn Connector) -> Self {
        Self {
            url: PACK_CATALOG_URL,
            connector,
            timeout: CATALOG_TIMEOUT,
            bundled: bundled_catalog(),
        }
    }
}

/// `fetchCatalog`: the remote catalog merged over the bundled one; any failure (offline,
/// invalid, too big, timeout) gives the bundled copy.
pub fn fetch_catalog(opts: FetchCatalogOptions) -> MergedCatalog {
    let signal = Signal::with_timeout(opts.timeout);
    let mut d = DownloadOptions::new(MAX_CATALOG_BYTES, opts.connector);
    d.signal = Some(&signal);
    d.idle_timeout = Some(opts.timeout);
    let res = download_to_buffer(opts.url, &d)
        .map_err(|e| e.message())
        .and_then(|bytes| parse_catalog(&bytes));
    match res {
        Ok((remote, errors)) => merge_catalogs(&opts.bundled, &remote, &errors),
        Err(err) => {
            eprintln!("[packs] remote catalog unavailable, using bundled copy: {err}");
            MergedCatalog {
                catalog: opts.bundled,
                source: CatalogSource::Bundled,
            }
        }
    }
}
