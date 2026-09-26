//! Model management: the built-in catalog, what is installed, verified downloads and
//! which model suits this PC. Large weights never live in the repo; they are fetched on
//! request into `%LOCALAPPDATA%\Aural\models\<id>\`.

pub mod catalog;
pub mod download;
pub mod recommend;
pub mod store;

pub use catalog::{Catalog, ModelEntry, ModelFile};
pub use download::{download, DownloadError, Progress};
pub use recommend::{compatible, recommend, HardwareProfile};
pub use store::{statuses, ModelState, ModelStatus, ModelStore};
