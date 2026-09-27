//! What is installed. A model counts as installed only when its receipt matches the
//! catalog entry and every file is present at the expected size; the receipt is
//! written last on install and deleted first on removal.

use crate::catalog::{split_variant_id, Backend, Catalog, ModelEntry, ModelFile, Runtime};
use crate::recommend::{compatible, recommend, HardwareProfile};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub(crate) const RECEIPT: &str = "installed.json";

#[derive(Debug, Clone)]
pub struct ModelStore {
    root: PathBuf,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Receipt {
    id: String,
    files: Vec<ModelFile>,
}

impl ModelStore {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn dir(&self, id: &str) -> PathBuf {
        self.root.join(id)
    }

    /// Receipt matches the catalog entry exactly and every file has its expected size.
    /// (Hashes were verified at download time; re-hashing gigabytes at startup would
    /// make the app slow to open.)
    pub fn is_installed(&self, entry: &ModelEntry) -> bool {
        let dir = self.dir(&entry.id);
        let Ok(text) = std::fs::read_to_string(dir.join(RECEIPT)) else {
            return false;
        };
        let Ok(receipt) = serde_json::from_str::<Receipt>(&text) else {
            return false;
        };
        receipt.id == entry.id
            && receipt.files == entry.files
            && entry.files.iter().all(|f| {
                std::fs::metadata(dir.join(&f.name)).is_ok_and(|m| m.is_file() && m.len() == f.size)
            })
    }

    pub(crate) fn write_receipt(&self, entry: &ModelEntry) -> Result<()> {
        let receipt = Receipt {
            id: entry.id.clone(),
            files: entry.files.clone(),
        };
        let path = self.dir(&entry.id).join(RECEIPT);
        std::fs::write(&path, serde_json::to_vec_pretty(&receipt)?)
            .with_context(|| format!("writing {}", path.display()))
    }

    /// Remove a model. The active model cannot be removed (switch first), and neither
    /// can one that is downloading: the download would recreate its folder and receipt.
    pub fn remove(
        &self,
        entry: &ModelEntry,
        active: Option<&str>,
        downloading: bool,
    ) -> Result<()> {
        if downloading {
            bail!(
                "{} is downloading; cancel the download before removing it",
                entry.name
            );
        }
        if active == Some(entry.id.as_str()) {
            bail!(
                "{} is the active model; choose another model before removing it",
                entry.name
            );
        }
        let dir = self.dir(&entry.id);
        match std::fs::remove_file(dir.join(RECEIPT)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).context("removing install receipt"),
        }
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e).with_context(|| format!("deleting {}", dir.display())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModelState {
    Available,
    Downloading { downloaded: u64, total: u64 },
    Installed,
    Active,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelStatus {
    pub id: String,
    pub name: String,
    pub engine: aural_engines::Engine,
    pub description: String,
    pub size_bytes: u64,
    pub min_ram_mb: u64,
    pub state: ModelState,
    pub removable: bool,
    pub compatible: bool,
    pub recommended: bool,
    pub license_id: String,
    pub attribution: String,
    /// Variant ids ("<model>@<backend>"), in catalog order.
    pub variants: Vec<String>,
    pub runtime: Runtime,
    pub precision: String,
}

/// The variant to use (`<model>@<backend>`): the chosen one if its model is installed
/// and the catalog still has that variant; otherwise the same model on the CPU; otherwise
/// the recommended installed model on the CPU; otherwise any installed model on the CPU;
/// otherwise none. Used at startup so a lost or stale choice doesn't hide a model that
/// is already on disk.
pub fn usable_variant(
    catalog: &Catalog,
    store: &ModelStore,
    chosen: Option<&str>,
    hw: &HardwareProfile,
) -> Option<String> {
    let installed = |id: &str| catalog.get(id).is_some_and(|m| store.is_installed(m));
    let cpu = |id: &str| {
        catalog
            .get(id)
            .filter(|m| m.variant(Backend::Cpu).is_some())
            .map(|m| m.variant_id(Backend::Cpu))
    };
    if let Some(c) = chosen {
        if catalog.variant(c).is_some_and(|(m, _)| installed(&m.id)) {
            return Some(c.to_owned());
        }
        if let Some((model, _)) = split_variant_id(c).filter(|(m, _)| installed(m)) {
            if let Some(v) = cpu(model) {
                return Some(v);
            }
        }
    }
    if let Some(r) = recommend(catalog, hw).filter(|r| installed(r)) {
        if let Some(v) = cpu(r) {
            return Some(v);
        }
    }
    catalog
        .models
        .iter()
        .filter(|m| !m.probe && store.is_installed(m))
        .find_map(|m| cpu(&m.id))
}

/// Status of every catalog model for the Models page.
pub fn statuses(
    catalog: &Catalog,
    store: &ModelStore,
    active: Option<&str>,
    downloading: &HashMap<String, (u64, u64)>,
    hw: &HardwareProfile,
) -> Vec<ModelStatus> {
    let recommended = recommend(catalog, hw);
    catalog
        .models
        .iter()
        // Test models are for the hardware test only.
        .filter(|m| !m.probe)
        .map(|m| {
            let installed = store.is_installed(m);
            let state = if let Some(&(downloaded, total)) = downloading.get(&m.id) {
                ModelState::Downloading { downloaded, total }
            } else if installed && active == Some(m.id.as_str()) {
                ModelState::Active
            } else if installed {
                ModelState::Installed
            } else {
                ModelState::Available
            };
            ModelStatus {
                id: m.id.clone(),
                name: m.name.clone(),
                engine: m.engine,
                description: m.description.clone(),
                size_bytes: m.total_size(),
                min_ram_mb: m.min_ram_mb(),
                state,
                removable: state == ModelState::Installed,
                compatible: compatible(m, hw),
                recommended: recommended == Some(m.id.as_str()),
                license_id: m.license.id.clone(),
                attribution: m.license.attribution.clone(),
                variants: m.variants.iter().map(|v| m.variant_id(v.backend)).collect(),
                runtime: m.runtime,
                precision: m.precision.clone(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;
    use crate::recommend::HardwareProfile;
    use std::collections::HashMap;

    fn catalog() -> Catalog {
        Catalog::builtin()
    }

    /// Fake an install without downloading: sized placeholder files + receipt.
    fn fake_install(store: &ModelStore, entry: &crate::ModelEntry) {
        let dir = store.dir(&entry.id);
        std::fs::create_dir_all(&dir).unwrap();
        for f in &entry.files {
            let file = std::fs::File::create(dir.join(&f.name)).unwrap();
            file.set_len(f.size).unwrap();
        }
        store.write_receipt(entry).unwrap();
    }

    const HW: HardwareProfile = HardwareProfile {
        cpu_name: String::new(),
        total_ram_mb: 16_000,
        free_ram_mb: 8_000,
        logical_cores: 16,
        physical_cores: 8,
        avx2: true,
        avx512: false,
        gpus: Vec::new(),
    };

    #[test]
    fn the_chosen_variant_is_kept_when_its_model_is_installed() {
        let root = tempfile::tempdir().unwrap();
        let store = ModelStore::new(root.path());
        let c = catalog();
        fake_install(&store, c.get("whisper-base.en-q8").unwrap());
        fake_install(&store, c.get("parakeet-tdt-0.6b-v2-int8").unwrap());
        assert_eq!(
            usable_variant(&c, &store, Some("whisper-base.en-q8@cpu"), &HW).as_deref(),
            Some("whisper-base.en-q8@cpu")
        );
    }

    #[test]
    fn an_installed_model_is_used_when_the_choice_was_lost() {
        // Settings reset (or the chosen model deleted by hand) while a model is on disk:
        // use it instead of telling the user to download one.
        let root = tempfile::tempdir().unwrap();
        let store = ModelStore::new(root.path());
        let c = catalog();
        fake_install(&store, c.get("whisper-base.en-q8").unwrap());
        assert_eq!(
            usable_variant(&c, &store, None, &HW).as_deref(),
            Some("whisper-base.en-q8@cpu")
        );
        assert_eq!(
            usable_variant(&c, &store, Some("parakeet-tdt-0.6b-v2-int8@cpu"), &HW).as_deref(),
            Some("whisper-base.en-q8@cpu"),
            "chosen model no longer installed"
        );
    }

    #[test]
    fn a_variant_the_catalog_no_longer_has_falls_back_to_the_cpu() {
        let root = tempfile::tempdir().unwrap();
        let store = ModelStore::new(root.path());
        let c = catalog();
        fake_install(&store, c.get("parakeet-tdt-0.6b-v2-int8").unwrap());
        assert_eq!(
            usable_variant(&c, &store, Some("parakeet-tdt-0.6b-v2-int8@vulkan"), &HW).as_deref(),
            Some("parakeet-tdt-0.6b-v2-int8@cpu")
        );
    }

    #[test]
    fn the_recommended_model_wins_among_installed_ones() {
        let root = tempfile::tempdir().unwrap();
        let store = ModelStore::new(root.path());
        let c = catalog();
        for m in &c.models {
            fake_install(&store, m);
        }
        let recommended = crate::recommend::recommend(&c, &HW).map(|m| format!("{m}@cpu"));
        assert!(recommended.is_some());
        assert_eq!(usable_variant(&c, &store, None, &HW), recommended);
    }

    #[test]
    fn nothing_installed_means_no_model() {
        let root = tempfile::tempdir().unwrap();
        let store = ModelStore::new(root.path());
        assert_eq!(
            usable_variant(&catalog(), &store, Some("whisper-base.en-q8@cpu"), &HW),
            None
        );
    }
    #[test]
    fn a_model_that_is_downloading_cannot_be_removed() {
        let root = tempfile::tempdir().unwrap();
        let store = ModelStore::new(root.path());
        let m = catalog().get("whisper-base.en-q8").unwrap().clone();
        fake_install(&store, &m);
        let err = store.remove(&m, None, true).unwrap_err().to_string();
        assert!(err.contains("downloading"), "{err}");
        assert!(store.is_installed(&m), "files must be left alone");
        store.remove(&m, None, false).unwrap();
        assert!(!store.is_installed(&m));
    }

    #[test]
    fn nothing_installed_in_an_empty_store() {
        let root = tempfile::tempdir().unwrap();
        let store = ModelStore::new(root.path());
        for m in &catalog().models {
            assert!(!store.is_installed(m));
        }
    }

    #[test]
    fn receipt_plus_files_means_installed() {
        let root = tempfile::tempdir().unwrap();
        let store = ModelStore::new(root.path());
        let m = catalog().get("whisper-base.en-q8").unwrap().clone();
        fake_install(&store, &m);
        assert!(store.is_installed(&m));
    }

    #[test]
    fn files_without_receipt_are_not_installed() {
        let root = tempfile::tempdir().unwrap();
        let store = ModelStore::new(root.path());
        let m = catalog().get("whisper-base.en-q8").unwrap().clone();
        fake_install(&store, &m);
        std::fs::remove_file(store.dir(&m.id).join(RECEIPT)).unwrap();
        assert!(!store.is_installed(&m));
    }

    #[test]
    fn truncated_file_is_not_installed() {
        let root = tempfile::tempdir().unwrap();
        let store = ModelStore::new(root.path());
        let m = catalog().get("whisper-base.en-q8").unwrap().clone();
        fake_install(&store, &m);
        let f = std::fs::OpenOptions::new()
            .write(true)
            .open(store.dir(&m.id).join(&m.files[0].name))
            .unwrap();
        f.set_len(10).unwrap();
        assert!(!store.is_installed(&m));
    }

    #[test]
    fn receipt_for_a_different_catalog_version_is_not_installed() {
        let root = tempfile::tempdir().unwrap();
        let store = ModelStore::new(root.path());
        let mut m = catalog().get("whisper-base.en-q8").unwrap().clone();
        fake_install(&store, &m);
        m.files[0].sha256 = "0".repeat(64);
        assert!(!store.is_installed(&m));
    }

    #[test]
    fn active_model_cannot_be_removed() {
        let root = tempfile::tempdir().unwrap();
        let store = ModelStore::new(root.path());
        let m = catalog().get("whisper-base.en-q8").unwrap().clone();
        fake_install(&store, &m);
        assert!(store.remove(&m, Some(&m.id), false).is_err());
        assert!(store.is_installed(&m));
    }

    #[test]
    fn remove_deletes_the_model_folder() {
        let root = tempfile::tempdir().unwrap();
        let store = ModelStore::new(root.path());
        let m = catalog().get("whisper-base.en-q8").unwrap().clone();
        fake_install(&store, &m);
        store
            .remove(&m, Some("parakeet-tdt-0.6b-v2-int8"), false)
            .unwrap();
        assert!(!store.dir(&m.id).exists());
        assert!(!store.is_installed(&m));
    }

    #[test]
    fn statuses_report_state_removability_and_recommendation() {
        let root = tempfile::tempdir().unwrap();
        let store = ModelStore::new(root.path());
        let c = catalog();
        let base = c.get("whisper-base.en-q8").unwrap().clone();
        let small = c.get("whisper-small.en-q8").unwrap().clone();
        fake_install(&store, &base);
        fake_install(&store, &small);
        let mut downloading = HashMap::new();
        downloading.insert("parakeet-tdt-0.6b-v2-int8".to_string(), (100u64, 1000u64));
        let st = statuses(&c, &store, Some("whisper-base.en-q8"), &downloading, &HW);
        let get = |id: &str| st.iter().find(|s| s.id == id).unwrap();
        assert_eq!(get("whisper-base.en-q8").state, ModelState::Active);
        assert!(!get("whisper-base.en-q8").removable);
        assert_eq!(get("whisper-small.en-q8").state, ModelState::Installed);
        assert!(get("whisper-small.en-q8").removable);
        assert_eq!(
            get("parakeet-tdt-0.6b-v2-int8").state,
            ModelState::Downloading {
                downloaded: 100,
                total: 1000
            }
        );
        assert!(get("parakeet-tdt-0.6b-v2-int8").recommended);
        assert!(!get("whisper-base.en-q8").recommended);
    }

    #[test]
    fn statuses_list_variants_and_technical_facts() {
        let root = tempfile::tempdir().unwrap();
        let store = ModelStore::new(root.path());
        let c = Catalog::builtin();
        let st = statuses(&c, &store, None, &HashMap::new(), &HW);
        let pk = st
            .iter()
            .find(|s| s.id == "parakeet-tdt-0.6b-v2-int8")
            .unwrap();
        assert_eq!(
            pk.variants,
            [
                "parakeet-tdt-0.6b-v2-int8@cpu",
                "parakeet-tdt-0.6b-v2-int8@directml"
            ]
        );
        assert_eq!((pk.runtime, pk.precision.as_str()), (Runtime::Onnx, "int8"));
    }

    #[test]
    fn a_test_model_is_never_chosen_for_dictation() {
        let root = tempfile::tempdir().unwrap();
        let store = ModelStore::new(root.path());
        let c = Catalog::builtin();
        fake_install(&store, c.get("whisper-tiny.en-q8").unwrap());
        assert_eq!(usable_variant(&c, &store, None, &HW), None);
    }

    #[test]
    fn test_models_are_not_listed() {
        let root = tempfile::tempdir().unwrap();
        let store = ModelStore::new(root.path());
        let c = Catalog::builtin();
        let st = statuses(&c, &store, None, &HashMap::new(), &HW);
        assert!(!st.is_empty());
        for s in &st {
            assert!(!c.get(&s.id).unwrap().probe, "{} is a test model", s.id);
        }
    }

    #[test]
    fn active_model_that_is_missing_is_reported_available_not_active() {
        let root = tempfile::tempdir().unwrap();
        let store = ModelStore::new(root.path());
        let st = statuses(
            &catalog(),
            &store,
            Some("whisper-base.en-q8"),
            &HashMap::new(),
            &HW,
        );
        let s = st.iter().find(|s| s.id == "whisper-base.en-q8").unwrap();
        assert_eq!(s.state, ModelState::Available);
    }
}
