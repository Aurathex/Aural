//! The model catalog ships inside the app (no runtime fetch, so no silent network use).
//! Every file is pinned to an immutable URL and a SHA-256.

use anyhow::{bail, Context, Result};
use aural_engines::Engine;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

const BUILTIN: &str = include_str!("../../../manifests/catalog.v1.json");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Catalog {
    pub version: u32,
    pub models: Vec<ModelEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelEntry {
    pub id: String,
    pub name: String,
    pub engine: Engine,
    pub description: String,
    pub languages: Vec<String>,
    pub min_ram_mb: u64,
    /// "best" | "good" | "basic" — shown as a label only.
    pub accuracy: String,
    /// "fast" | "moderate" — shown as a label only.
    pub speed: String,
    pub license: ModelLicense,
    pub source: String,
    pub files: Vec<ModelFile>,
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

    fn validate(&self) -> Result<()> {
        let mut ids = HashSet::new();
        for m in &self.models {
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
            r#"{{"version":1,"models":[{{"id":"{id}","name":"N","engine":"whisper","description":"d",
            "languages":["en"],"min_ram_mb":512,"accuracy":"basic","speed":"fast",
            "license":{{"id":"MIT","url":"https://x","attribution":"a"}},"source":"https://x",
            "files":[{{"name":"{file_name}","url":"{url}","sha256":"{sha}","size":3}}]}}]}}"#
        )
    }
    const SHA: &str = "a4d4a0768075e13cfd7e19df3ae2dbc4a68d37d36a7dad45e8410c9a34f8c87e";

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
