//! Text models for AI cleanup: their own small catalog (so they never appear among the
//! speech models or in the hardware check), downloaded only when the user asks, into
//! `text-models\<id>\` with the same resumable, SHA-256-checked download as speech models.

use crate::catalog::{ModelFile, ModelLicense};
use crate::download::{fetch_file, DownloadError, DownloadOptions, Progress};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

const RECEIPT: &str = "installed.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextModelEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub license: ModelLicense,
    pub source: String,
    pub min_ram_mb: u64,
    pub files: Vec<ModelFile>,
}

impl TextModelEntry {
    pub fn total_size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct TextCatalog {
    pub version: u32,
    pub models: Vec<TextModelEntry>,
}

impl TextCatalog {
    pub fn builtin() -> Self {
        let c: TextCatalog =
            serde_json::from_str(include_str!("../../../manifests/text-models.json"))
                .expect("built-in text model catalog parses");
        c.validate().expect("built-in text model catalog is valid");
        c
    }

    pub fn validate(&self) -> Result<()> {
        for m in &self.models {
            if m.files.is_empty() {
                bail!("{} has no files", m.id);
            }
            for f in &m.files {
                if f.sha256.len() != 64 || !f.url.starts_with("https://huggingface.co/") {
                    bail!(
                        "{}: {} needs a pinned Hugging Face URL and SHA-256",
                        m.id,
                        f.name
                    );
                }
                if !f.url.contains("/resolve/") || f.url.contains("/resolve/main/") {
                    bail!("{}: {} must be pinned to a commit", m.id, f.name);
                }
            }
        }
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&TextModelEntry> {
        self.models.iter().find(|m| m.id == id)
    }
}

#[derive(Serialize, Deserialize, PartialEq)]
struct Receipt {
    id: String,
    files: Vec<ModelFile>,
}

pub fn dir(root: &Path, id: &str) -> PathBuf {
    root.join(id)
}

pub fn is_installed(root: &Path, m: &TextModelEntry) -> bool {
    let d = dir(root, &m.id);
    let Ok(text) = std::fs::read_to_string(d.join(RECEIPT)) else {
        return false;
    };
    serde_json::from_str::<Receipt>(&text).is_ok_and(|r| r.id == m.id && r.files == m.files)
        && m.files.iter().all(|f| {
            std::fs::metadata(d.join(&f.name)).is_ok_and(|x| x.is_file() && x.len() == f.size)
        })
}

pub fn download(
    m: &TextModelEntry,
    root: &Path,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(Progress),
) -> Result<(), DownloadError> {
    let d = dir(root, &m.id);
    std::fs::create_dir_all(&d).map_err(|e| DownloadError::Io(e.to_string()))?;
    let total = m.total_size();
    let mut before = 0u64;
    for f in &m.files {
        fetch_file(
            f,
            &d,
            cancel,
            &mut |n| {
                on_progress(Progress {
                    downloaded: before + n,
                    total,
                })
            },
            &DownloadOptions::default(),
        )?;
        before += f.size;
    }
    let receipt = Receipt {
        id: m.id.clone(),
        files: m.files.clone(),
    };
    std::fs::write(
        d.join(RECEIPT),
        serde_json::to_vec_pretty(&receipt).map_err(|e| DownloadError::Io(e.to_string()))?,
    )
    .map_err(|e| DownloadError::Io(e.to_string()))?;
    on_progress(Progress {
        downloaded: total,
        total,
    });
    Ok(())
}

/// Delete a text model (receipt first, so a half-deleted model never counts as installed).
pub fn remove(root: &Path, m: &TextModelEntry) -> Result<()> {
    let d = dir(root, &m.id);
    let _ = std::fs::remove_file(d.join(RECEIPT));
    match std::fs::remove_dir_all(&d) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err(e).with_context(|| format!("removing {}", d.display()))
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_text_catalog_is_pinned_and_permissively_licensed() {
        let c = TextCatalog::builtin();
        assert!(!c.models.is_empty());
        for m in &c.models {
            assert!(
                ["Apache-2.0", "MIT"].contains(&m.license.id.as_str()),
                "{}",
                m.id
            );
            assert!(m.license.attribution.contains("huggingface.co"));
            assert!(m.files.iter().any(|f| f.name == "model.onnx"));
            assert!(m.files.iter().any(|f| f.name == "tokenizer.json"));
        }
    }

    #[test]
    fn unpinned_urls_are_refused() {
        let mut c = TextCatalog::builtin();
        c.models[0].files[0].url = "https://huggingface.co/x/y/resolve/main/config.json".into();
        assert!(c.validate().is_err());
    }

    #[test]
    fn installed_means_receipt_and_every_file_at_its_size() {
        let d = tempfile::tempdir().unwrap();
        let mut m = TextCatalog::builtin().models[0].clone();
        m.files.truncate(1);
        m.files[0].size = 3;
        assert!(!is_installed(d.path(), &m));
        let md = dir(d.path(), &m.id);
        std::fs::create_dir_all(&md).unwrap();
        std::fs::write(md.join(&m.files[0].name), b"abc").unwrap();
        assert!(!is_installed(d.path(), &m), "no receipt yet");
        std::fs::write(
            md.join(RECEIPT),
            serde_json::to_vec(&Receipt {
                id: m.id.clone(),
                files: m.files.clone(),
            })
            .unwrap(),
        )
        .unwrap();
        assert!(is_installed(d.path(), &m));
        remove(d.path(), &m).unwrap();
        assert!(!is_installed(d.path(), &m));
        assert!(!md.exists());
    }
}
