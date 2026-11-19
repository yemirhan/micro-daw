//! The pack store (≙ `src/main/packs/*` + the IO half of `src/services/PackService.ts`):
//!
//! - [`http`]: a small blocking HTTP/1.1 client over rustls whose every wait can be interrupted;
//! - [`download`]: allowlisted, size-capped, sha256-verified downloads with redirects, idle
//!   timeout and retries (`download.ts`);
//! - [`catalog`]: remote catalog merged over the bundled copy (`catalog.ts`);
//! - [`installer`]: staging, resume, atomic swap, crash recovery, pause on quit, uninstall
//!   (`installer.ts`), same on-disk layout and `pack.json` as the Electron app;
//! - [`preview`]: remote preview fetcher with LRU (`preview.ts`);
//! - [`store`]: the "main process" API (`ipc.ts`, `quit.ts`);
//! - [`service`]: the UI-thread state, previews through the engine and Get & load
//!   (`PackService.ts`).
//!
//! Validation and the pure state reductions live in `mdaw_model::packs`.

pub mod catalog;
pub mod download;
pub mod fault;
pub mod http;
pub mod installer;
pub mod json;
pub mod lock;
pub mod preview;
pub mod service;
pub mod signal;
pub mod store;

pub use installer::{InstallError, InstallHandle, InstallerOptions, PackInstaller};
pub use service::{KitLoadedEvent, PackHost, PackService, PackSnapshot};
pub use signal::{AbortReason, Signal};
pub use store::{PackStore, PackStoreOptions};
