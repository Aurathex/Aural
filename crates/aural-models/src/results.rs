//! Hardware-test results for this PC (`hardware.json`): one result per variant, the
//! probe calibrations, and the fingerprint of the hardware they were measured on.

use crate::estimate::Calibration;
use crate::recommend::HardwareProfile;
use anyhow::{Context, Result};
use aural_stt_protocol::bench::VariantResult;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct ResultsFile {
    version: u32,
    /// `HardwareProfile::fingerprint` when the results were measured.
    fingerprint: String,
    calibrations: Vec<Calibration>,
    results: BTreeMap<String, VariantResult>,
    /// A full hardware check finished on this hardware (not just a measured download).
    checked: bool,
}

#[derive(Debug, Clone)]
pub struct ResultsStore {
    path: PathBuf,
    file: ResultsFile,
    /// Set when the file was unreadable and has been set aside.
    pub notice: Option<String>,
}

impl ResultsStore {
    /// An empty store that will save to path.
    pub fn empty(path: &Path) -> ResultsStore {
        ResultsStore {
            path: path.to_owned(),
            file: ResultsFile {
                version: 1,
                ..ResultsFile::default()
            },
            notice: None,
        }
    }

    /// A missing file is an empty store; an unreadable one is renamed
    /// `hardware.corrupt-<time>.json` and the store starts empty.
    pub fn load(path: &Path) -> Result<ResultsStore> {
        let mut store = ResultsStore::empty(path);
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(store),
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        match serde_json::from_str::<ResultsFile>(&text) {
            Ok(file) => store.file = file,
            Err(_) => {
                let stamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs());
                let backup = path.with_file_name(format!("hardware.corrupt-{stamp}.json"));
                std::fs::rename(path, &backup)
                    .with_context(|| format!("backing up corrupt {}", path.display()))?;
                store.notice = Some(
                    "Aural couldn't read its saved hardware check, so it will check your PC again."
                        .into(),
                );
            }
        }
        Ok(store)
    }

    /// Atomic write: temp file in the same directory, then rename over the target.
    pub fn save(&self) -> Result<()> {
        let dir = self.path.parent().context("results path has no parent")?;
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(&self.file)?)
            .with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, &self.path)
            .with_context(|| format!("replacing {}", self.path.display()))?;
        Ok(())
    }

    /// Stores a result. An estimate never replaces a measured result; returns whether
    /// the store changed.
    pub fn put(&mut self, result: VariantResult) -> bool {
        if !result.measured
            && self
                .file
                .results
                .get(&result.variant)
                .is_some_and(|old| old.measured)
        {
            return false;
        }
        self.file.results.insert(result.variant.clone(), result);
        true
    }

    pub fn get(&self, variant: &str) -> Option<&VariantResult> {
        self.file.results.get(variant)
    }

    pub fn results(&self) -> impl Iterator<Item = &VariantResult> {
        self.file.results.values()
    }

    /// A full check has finished for this hardware. (Files from before this flag count
    /// as checked when they hold calibrations.)
    pub fn checked(&self) -> bool {
        self.file.checked || !self.file.calibrations.is_empty()
    }

    pub fn mark_checked(&mut self) {
        self.file.checked = true;
    }

    pub fn calibrations(&self) -> &[Calibration] {
        &self.file.calibrations
    }

    pub fn set_calibrations(&mut self, c: Vec<Calibration>) {
        self.file.calibrations = c;
    }

    /// The results were measured on different hardware (new graphics card, driver…).
    pub fn is_stale(&self, hw: &HardwareProfile) -> bool {
        !self.file.fingerprint.is_empty() && self.file.fingerprint != hw.fingerprint()
    }

    /// Ties the store to this hardware before adding a result: a store from other
    /// hardware starts over; one never tied to any keeps what it has.
    pub fn adopt(&mut self, hw: &HardwareProfile) {
        if self.is_stale(hw) {
            self.reset_for(hw);
        } else {
            self.file.fingerprint = hw.fingerprint();
        }
    }

    /// Starts over for this hardware: drops old results and calibrations.
    pub fn reset_for(&mut self, hw: &HardwareProfile) {
        self.file = ResultsFile {
            version: 1,
            fingerprint: hw.fingerprint(),
            ..ResultsFile::default()
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Runtime;
    use aural_engines::Backend;
    use aural_eval::metrics::RunMetrics;
    use aural_stt_protocol::bench::Stability;

    fn result(variant: &str, measured: bool, p50: u64) -> VariantResult {
        VariantResult {
            variant: variant.into(),
            metrics: Some(RunMetrics {
                wer: 0.03,
                words: 396,
                p50_ms: p50,
                p95_ms: p50 * 2,
                rtf: 0.1,
            }),
            load_ms: 900,
            ram_mb: 800,
            vram_mb: None,
            spread: 1.4,
            passes: 3,
            stability: Stability::Stable,
            measured,
            error: None,
        }
    }

    fn hw(driver: &str) -> HardwareProfile {
        HardwareProfile {
            cpu_name: "cpu".into(),
            total_ram_mb: 16_000,
            gpus: vec![aural_platform::gpu::GpuInfo {
                name: "gpu".into(),
                vendor: aural_platform::gpu::Vendor::Nvidia,
                vram_mb: 8_000,
                integrated: false,
                driver: driver.into(),
                luid: 1,
            }],
            ..HardwareProfile::default()
        }
    }

    #[test]
    fn round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hardware.json");
        let mut s = ResultsStore::load(&path).unwrap();
        assert!(s.results().next().is_none());
        s.reset_for(&hw("1"));
        s.set_calibrations(vec![Calibration {
            runtime: Runtime::Ggml,
            backend: Backend::Vulkan,
            probe_p50_ms: 90,
            probe_ref_p50_ms: 60,
        }]);
        assert!(s.put(result("a@cpu", true, 300)));
        s.save().unwrap();
        let back = ResultsStore::load(&path).unwrap();
        assert_eq!(back.get("a@cpu"), Some(&result("a@cpu", true, 300)));
        assert_eq!(back.calibrations(), s.calibrations());
        assert!(!back.is_stale(&hw("1")));
        assert!(back.notice.is_none());
        assert!(!dir.path().join("hardware.json.tmp").exists());
    }

    #[test]
    fn stale_when_fingerprint_changes() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = ResultsStore::load(&dir.path().join("hardware.json")).unwrap();
        // Nothing measured yet: nothing to be stale.
        assert!(!s.is_stale(&hw("1")));
        s.reset_for(&hw("1"));
        s.put(result("a@cpu", true, 300));
        assert!(!s.is_stale(&hw("1")));
        assert!(s.is_stale(&hw("2")));
        s.reset_for(&hw("2"));
        assert!(s.get("a@cpu").is_none());
        assert!(!s.is_stale(&hw("2")));
    }

    #[test]
    fn adopt_stamps_a_new_store_and_clears_a_stale_one() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = ResultsStore::load(&dir.path().join("hardware.json")).unwrap();
        s.put(result("a@cpu", true, 300));
        s.adopt(&hw("1"));
        // Measured before any test on this PC: kept, and now tied to this PC.
        assert!(s.get("a@cpu").is_some());
        assert!(s.is_stale(&hw("2")));
        s.adopt(&hw("2"));
        assert!(s.get("a@cpu").is_none());
        assert!(!s.is_stale(&hw("2")));
    }

    #[test]
    fn corrupt_file_is_backed_up_and_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hardware.json");
        std::fs::write(&path, "{ not json").unwrap();
        let s = ResultsStore::load(&path).unwrap();
        assert!(s.results().next().is_none());
        let notice = s.notice.clone().unwrap();
        // Plain words for the user: no parser message or file path.
        assert!(
            !notice.contains("expected") && !notice.contains(".json"),
            "{notice}"
        );
        assert!(!path.exists());
        let backups: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("hardware.corrupt-")
            })
            .collect();
        assert_eq!(backups.len(), 1);
    }

    #[test]
    fn measured_result_replaces_estimate() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = ResultsStore::load(&dir.path().join("hardware.json")).unwrap();
        assert!(s.put(result("a@cpu", false, 500)));
        assert!(s.put(result("a@cpu", true, 300)));
        assert_eq!(s.get("a@cpu").unwrap().metrics.unwrap().p50_ms, 300);
        // An estimate never overwrites a measurement…
        assert!(!s.put(result("a@cpu", false, 100)));
        assert!(s.get("a@cpu").unwrap().measured);
        // …but a new measurement does.
        assert!(s.put(result("a@cpu", true, 250)));
        assert_eq!(s.get("a@cpu").unwrap().metrics.unwrap().p50_ms, 250);
    }
}
