//! What happens to dictated text between the speech model and the app it goes to, and
//! the local data kept about it. Everything here is pure Rust with no OS calls, and
//! nothing leaves the PC.
//!
//! - [`dictionary`]: your words and names, spelled your way.
//! - [`cleanup`]: light, rule-based tidying (no model).
//! - [`rewrite`]: the instructions and the safety check for AI cleanup.
//! - [`profile`]: per-app settings.
//! - [`learn`]: suggestions learned from your own corrections.
//! - [`history`], [`stats`]: searchable history and statistics.

pub mod cleanup;
pub mod dictionary;
pub mod history;
pub mod learn;
pub mod profile;
pub mod protect;
pub mod rewrite;
pub mod stats;

use anyhow::{Context, Result};
use aural_core::settings::CleanupMode;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// `dictionary.json`: your entries and what Aural learned from your corrections.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Words {
    pub entries: Vec<dictionary::Entry>,
    pub learned: learn::Learned,
}

impl Words {
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(t) => {
                serde_json::from_str(&t).with_context(|| format!("reading {}", path.display()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
}

/// The text before any model rewrite: dictionary, then light cleanup when cleanup is on
/// (AI cleanup starts from, and falls back to, this light result).
pub fn prepare(
    raw: &str,
    entries: &[dictionary::Entry],
    use_dictionary: bool,
    mode: CleanupMode,
) -> String {
    let text = if use_dictionary {
        dictionary::apply(entries, raw)
    } else {
        raw.to_owned()
    };
    match mode {
        CleanupMode::Off => text,
        CleanupMode::Light | CleanupMode::Ai => cleanup::light(&text),
    }
}

/// Use a model's rewrite of `prepared` if it passes the check (dictionary spellings are
/// applied again, in case the model changed them); otherwise keep `prepared`.
pub fn accept_rewrite(
    prepared: &str,
    answer: &str,
    entries: &[dictionary::Entry],
    use_dictionary: bool,
) -> Result<String, rewrite::Rejected> {
    let ok = rewrite::check(prepared, answer)?;
    Ok(if use_dictionary {
        dictionary::apply(entries, &ok)
    } else {
        ok
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dictionary::Entry;

    fn dict() -> Vec<Entry> {
        vec![Entry {
            write: "Aurathex".into(),
            heard: vec!["aura thex".into()],
        }]
    }

    #[test]
    fn off_means_the_models_words_plus_the_dictionary() {
        assert_eq!(
            prepare("um email aura thex", &dict(), true, CleanupMode::Off),
            "um email Aurathex"
        );
        assert_eq!(
            prepare("um email aura thex", &dict(), false, CleanupMode::Off),
            "um email aura thex"
        );
    }

    #[test]
    fn light_cleanup_runs_after_the_dictionary() {
        assert_eq!(
            prepare("um, email aura thex", &dict(), true, CleanupMode::Light),
            "Email Aurathex"
        );
    }

    #[test]
    fn a_rewrite_keeps_dictionary_spellings() {
        let got = accept_rewrite(
            "Email Aurathex today",
            "Email aurathex today.",
            &dict(),
            true,
        );
        assert_eq!(got.unwrap(), "Email Aurathex today.");
    }

    #[test]
    fn words_file_round_trips_and_a_missing_file_is_empty() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("dictionary.json");
        assert_eq!(Words::load(&p).unwrap(), Words::default());
        let mut w = Words {
            entries: dict(),
            ..Default::default()
        };
        w.learned.observe("call kate", "call Cate");
        w.save(&p).unwrap();
        assert_eq!(Words::load(&p).unwrap(), w);
    }
}
