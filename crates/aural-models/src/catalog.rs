//! The model catalog ships inside the app (no runtime fetch, so no silent network use).
//! Every file is pinned to an immutable URL and a SHA-256.

use anyhow::{bail, Context, Result};
pub use aural_engines::{Backend, Engine};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

const BUILTIN: &str = include_str!("../../../manifests/catalog.v2.json");

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Catalog {
    pub version: u32,
    pub models: Vec<ModelEntry>,
}

/// The worker family that runs a model's files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Runtime {
    /// ONNX Runtime, in the `aural-stt-onnx` worker.
    Onnx,
    /// whisper.cpp / ggml, in the `aural-stt-ggml` worker.
    Ggml,
}

/// One downloadable file set: a model's weights at one precision. Its id is also its
/// folder name, unchanged from catalog v1 for the models v0.1 shipped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelEntry {
    pub id: String,
    pub name: String,
    /// "parakeet" | "whisper" | "moonshine" | …
    pub family: String,
    pub engine: Engine,
    pub runtime: Runtime,
    /// "int8" | "fp16" | "fp32" | "q5_0" | "q8_0" | …
    pub precision: String,
    /// The engine can produce partial results while audio is still coming in.
    pub streaming: bool,
    /// Used only by the hardware test to calibrate this PC; hidden from the Models page.
    pub probe: bool,
    pub description: String,
    pub languages: Vec<String>,
    pub license: ModelLicense,
    pub source: String,
    pub files: Vec<ModelFile>,
    /// The backends these files can run on. Each is one compatibility unit:
    /// Model + Runtime + Backend + Precision, with id `<model id>@<backend>`.
    pub variants: Vec<Variant>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Variant {
    pub backend: Backend,
    pub min_ram_mb: u64,
    pub min_vram_mb: u64,
    /// Measured on the reference PC (docs/benchmarks); used to estimate other PCs.
    pub reference: Option<Reference>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reference {
    pub wer: f64,
    pub p50_ms: u64,
    pub rtf: f64,
    pub load_ms: u64,
    pub ram_mb: u64,
    pub vram_mb: u64,
}

fn backend_name(b: Backend) -> String {
    serde_json::to_value(b)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// Split `<model id>@<backend>` into its parts; `None` if it isn't a variant id.
pub fn split_variant_id(id: &str) -> Option<(&str, Backend)> {
    let (model, backend) = id.rsplit_once('@')?;
    let backend: Backend =
        serde_json::from_value(serde_json::Value::String(backend.into())).ok()?;
    (!model.is_empty()).then_some((model, backend))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelLicense {
    pub id: String,
    pub url: String,
    pub attribution: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelFile {
    pub name: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

impl ModelEntry {
    pub fn total_size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }

    pub fn variant_id(&self, backend: Backend) -> String {
        format!("{}@{}", self.id, backend_name(backend))
    }

    pub fn variant(&self, backend: Backend) -> Option<&Variant> {
        self.variants.iter().find(|v| v.backend == backend)
    }

    /// Smallest RAM any variant needs (the model's CPU requirement, in practice).
    pub fn min_ram_mb(&self) -> u64 {
        self.variants
            .iter()
            .map(|v| v.min_ram_mb)
            .min()
            .unwrap_or(0)
    }
}

/// The runtime an engine's files are read by.
fn runtime_of(engine: Engine) -> Runtime {
    match engine {
        Engine::Parakeet | Engine::Moonshine => Runtime::Onnx,
        Engine::Whisper => Runtime::Ggml,
    }
}

impl Catalog {
    /// The catalog compiled into this build. It is validated by tests, so a malformed
    /// catalog can never ship.
    pub fn builtin() -> Self {
        Self::parse(BUILTIN).expect("built-in catalog is validated by tests")
    }

    pub fn parse(json: &str) -> Result<Self> {
        let c: Catalog = serde_json::from_str(json).context("parsing model catalog")?;
        c.validate()?;
        Ok(c)
    }

    pub fn get(&self, id: &str) -> Option<&ModelEntry> {
        self.models.iter().find(|m| m.id == id)
    }

    /// Look up a variant by its `<model id>@<backend>` id.
    pub fn variant(&self, id: &str) -> Option<(&ModelEntry, &Variant)> {
        let (model, backend) = split_variant_id(id)?;
        let m = self.get(model)?;
        Some((m, m.variant(backend)?))
    }

    pub fn validate(&self) -> Result<()> {
        let mut ids = HashSet::new();
        for m in &self.models {
            if m.variants.is_empty() {
                bail!("model {} has no variants", m.id);
            }
            let mut backends = HashSet::new();
            for v in &m.variants {
                if !backends.insert(v.backend) {
                    bail!("model {}: duplicate {:?} variant", m.id, v.backend);
                }
            }
            if runtime_of(m.engine) != m.runtime {
                bail!(
                    "model {}: engine {:?} does not run on the {:?} runtime",
                    m.id,
                    m.engine,
                    m.runtime
                );
            }
            if !is_safe_id(&m.id) {
                bail!(
                    "model id {:?} must be lowercase letters, digits, '.', '-' or '_'",
                    m.id
                );
            }
            if !ids.insert(&m.id) {
                bail!("duplicate model id {:?}", m.id);
            }
            if m.files.is_empty() {
                bail!("model {} has no files", m.id);
            }
            for f in &m.files {
                if !is_safe_file_name(&f.name) {
                    bail!("model {}: unsafe file name {:?}", m.id, f.name);
                }
                if !f.url.starts_with("https://") {
                    bail!("model {}: {} must be an https URL", m.id, f.url);
                }
                if f.sha256.len() != 64
                    || !f
                        .sha256
                        .chars()
                        .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
                {
                    bail!("model {}: {} has a malformed sha256", m.id, f.name);
                }
                if f.size == 0 {
                    bail!("model {}: {} has zero size", m.id, f.name);
                }
            }
        }
        Ok(())
    }
}

fn is_safe_id(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('.')
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-' | '_'))
}

/// A plain file name that stays inside the model folder: no separators, no drive
/// prefix, no `.`/`..`.
fn is_safe_file_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':'])
        && !name.starts_with("..")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(id: &str, file_name: &str, url: &str, sha: &str) -> String {
        format!(
            r#"{{"version":2,"models":[{{"id":"{id}","name":"N","family":"whisper","engine":"whisper",
            "runtime":"ggml","precision":"q8_0","streaming":false,"probe":false,"description":"d",
            "languages":["en"],
            "license":{{"id":"MIT","url":"https://x","attribution":"a"}},"source":"https://x",
            "files":[{{"name":"{file_name}","url":"{url}","sha256":"{sha}","size":3}}],
            "variants":[{{"backend":"cpu","min_ram_mb":512,"min_vram_mb":0,"reference":null}}]}}]}}"#
        )
    }
    const SHA: &str = "a4d4a0768075e13cfd7e19df3ae2dbc4a68d37d36a7dad45e8410c9a34f8c87e";

    #[test]
    fn builtin_catalog_is_v2_and_valid() {
        let c = Catalog::builtin();
        assert_eq!(c.version, 2);
        c.validate().unwrap();
    }

    #[test]
    fn expected_models_present() {
        let c = Catalog::builtin();
        for (id, backends) in [
            ("parakeet-tdt-0.6b-v2-int8", &["cpu", "directml"][..]),
            ("parakeet-tdt-0.6b-v3-int8", &["cpu", "directml"]),
            ("moonshine-base-int8", &["cpu"]),
            ("whisper-large-v3-turbo-q5", &["cpu", "vulkan"]),
            ("whisper-distil-large-v3.5", &["cpu", "vulkan"]),
            ("whisper-small.en-q8", &["cpu", "vulkan"]),
            ("whisper-base.en-q8", &["cpu", "vulkan"]),
            ("whisper-tiny.en-q8", &["cpu", "vulkan"]),
            ("moonshine-tiny-int8", &["cpu"]),
        ] {
            let m = c.get(id).unwrap_or_else(|| panic!("{id} missing"));
            let have: Vec<String> = m.variants.iter().map(|v| backend_name(v.backend)).collect();
            assert_eq!(have, backends, "{id}");
            assert!(!m.license.attribution.is_empty(), "{id}");
        }
    }

    #[test]
    fn probe_models_are_small() {
        let c = Catalog::builtin();
        let probes: Vec<&ModelEntry> = c.models.iter().filter(|m| m.probe).collect();
        let ids: Vec<&str> = probes.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["whisper-tiny.en-q8", "moonshine-tiny-int8"]);
        // One per runtime, so each worker can be calibrated.
        assert_ne!(probes[0].runtime, probes[1].runtime);
        for p in &probes {
            assert!(
                p.total_size() <= 60 * 1024 * 1024,
                "{}: {}",
                p.id,
                p.total_size()
            );
        }
    }

    #[test]
    fn moonshine_is_never_offered_on_the_graphics_card() {
        // It produces garbage there (see onnx_moonshine.rs).
        for m in Catalog::builtin()
            .models
            .iter()
            .filter(|m| m.family == "moonshine")
        {
            assert!(
                m.variants.iter().all(|v| v.backend == Backend::Cpu),
                "{}",
                m.id
            );
        }
    }

    #[test]
    fn v01_model_ids_are_unchanged() {
        let c = Catalog::builtin();
        for id in [
            "parakeet-tdt-0.6b-v2-int8",
            "whisper-small.en-q8",
            "whisper-base.en-q8",
        ] {
            assert!(
                c.get(id).is_some(),
                "{id} missing: v0.1 installs would lose their model"
            );
        }
    }

    #[test]
    fn variant_ids_round_trip() {
        let c = Catalog::builtin();
        let m = c.get("parakeet-tdt-0.6b-v2-int8").unwrap();
        let id = m.variant_id(Backend::Cpu);
        assert_eq!(id, "parakeet-tdt-0.6b-v2-int8@cpu");
        assert_eq!(
            split_variant_id(&id),
            Some(("parakeet-tdt-0.6b-v2-int8", Backend::Cpu))
        );
        let (model, variant) = c.variant(&id).unwrap();
        assert_eq!(
            (model.id.as_str(), variant.backend),
            ("parakeet-tdt-0.6b-v2-int8", Backend::Cpu)
        );
        assert!(
            c.variant("parakeet-tdt-0.6b-v2-int8@vulkan").is_none(),
            "parakeet has no vulkan variant"
        );
        assert_eq!(split_variant_id("no-at-sign"), None);
        assert_eq!(split_variant_id("m@not-a-backend"), None);
    }

    #[test]
    fn validation_rejects_bad_catalogs() {
        let good = Catalog::builtin();
        let mut c = good.clone();
        c.models[0].variants.clear();
        assert!(c
            .validate()
            .unwrap_err()
            .to_string()
            .contains("no variants"));
        let mut c = good.clone();
        let dup = c.models[0].variants[0].clone();
        c.models[0].variants.push(dup);
        assert!(c.validate().unwrap_err().to_string().contains("duplicate"));
        let mut c = good.clone();
        c.models[0].runtime = Runtime::Ggml; // parakeet engine on the ggml runtime
        assert!(c.validate().unwrap_err().to_string().contains("runtime"));
    }

    #[test]
    fn builtin_catalog_is_valid_and_has_the_default_model() {
        let c = Catalog::builtin();
        assert!(c.get("parakeet-tdt-0.6b-v2-int8").is_some());
        assert!(c.models.len() >= 3);
        let p = c.get("parakeet-tdt-0.6b-v2-int8").unwrap();
        assert_eq!(p.engine, aural_engines::Engine::Parakeet);
        assert_eq!(p.total_size(), 652184014 + 8998286 + 139764 + 9384);
    }

    #[test]
    fn builtin_urls_are_pinned_to_a_commit() {
        for m in &Catalog::builtin().models {
            for f in &m.files {
                assert!(
                    f.url.contains("/resolve/") && !f.url.contains("/resolve/main/"),
                    "{}",
                    f.url
                );
            }
        }
    }

    #[test]
    fn accepts_a_well_formed_entry() {
        assert!(Catalog::parse(&one("m", "w.bin", "https://h/w.bin", SHA)).is_ok());
    }

    #[test]
    fn rejects_duplicate_ids() {
        let a = one("m", "w.bin", "https://h/w.bin", SHA);
        let v: serde_json::Value = serde_json::from_str(&a).unwrap();
        let mut both = v.clone();
        let m = v["models"][0].clone();
        both["models"].as_array_mut().unwrap().push(m);
        assert!(Catalog::parse(&both.to_string()).is_err());
    }

    #[test]
    fn rejects_non_https_urls() {
        assert!(Catalog::parse(&one("m", "w.bin", "http://h/w.bin", SHA)).is_err());
    }

    #[test]
    fn rejects_malformed_sha256() {
        assert!(Catalog::parse(&one("m", "w.bin", "https://h/w.bin", "abc")).is_err());
        let upper = SHA.to_uppercase();
        assert!(Catalog::parse(&one("m", "w.bin", "https://h/w.bin", &upper)).is_err());
    }

    #[test]
    fn rejects_file_names_that_escape_the_model_folder() {
        for bad in [
            "../evil.dll",
            "sub/w.bin",
            "sub\\\\w.bin",
            "..",
            "C:w.bin",
            "",
        ] {
            assert!(
                Catalog::parse(&one("m", bad, "https://h/w.bin", SHA)).is_err(),
                "{bad:?} accepted"
            );
        }
    }

    #[test]
    fn rejects_ids_that_are_not_folder_safe() {
        assert!(Catalog::parse(&one("../m", "w.bin", "https://h/w.bin", SHA)).is_err());
        assert!(Catalog::parse(&one("M X", "w.bin", "https://h/w.bin", SHA)).is_err());
    }
}
