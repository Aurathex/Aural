//! Model management: the built-in catalog, what is installed, verified downloads and
//! which model suits this PC. Large weights never live in the repo; they are fetched on
//! request into `%LOCALAPPDATA%\com.aurathex.aural\models\<id>\`.

pub mod catalog;
pub mod download;
pub mod estimate;
pub mod labels;
pub mod recommend;
pub mod results;
pub mod store;
#[cfg(test)]
mod test_fixtures;

pub use aural_stt_protocol::bench::{Stability, VariantResult};
pub use catalog::{Catalog, ModelEntry, ModelFile};
pub use download::{download, download_with, DownloadError, DownloadOptions, Progress};
pub use estimate::{estimate, Calibration};
pub use labels::{labels, Label, Reason};
pub use recommend::{compatible, recommend, HardwareProfile};
pub use results::ResultsStore;
pub use store::{statuses, ModelState, ModelStatus, ModelStore};
