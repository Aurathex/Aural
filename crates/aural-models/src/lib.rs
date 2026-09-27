//! Model management: the built-in catalog, what is installed, verified downloads and
//! which model suits this PC. Large weights never live in the repo; they are fetched on
//! request into `%LOCALAPPDATA%\com.aurathex.aural\models\<id>\`.

pub mod catalog;
pub mod download;
pub mod recommend;
pub mod store;

pub use aural_stt_protocol::bench::{Stability, VariantResult};
pub use catalog::{Catalog, ModelEntry, ModelFile};
pub use download::{download, download_with, DownloadError, DownloadOptions, Progress};
pub use recommend::{compatible, recommend, HardwareProfile};
pub use store::{statuses, ModelState, ModelStatus, ModelStore};
