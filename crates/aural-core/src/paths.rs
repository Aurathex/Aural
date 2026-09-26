//! Where Aural keeps its files. Everything lives under two per-user roots so that
//! "Delete Aural" knows exactly what to remove.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Folder name for both roots: the app identifier. Not "Aural": the per-user installer
/// puts the program itself in %LOCALAPPDATA%\Aural, and data must never share a folder
/// with the install (Delete Aural and the uninstaller each remove one of them).
pub const DIR_NAME: &str = "com.aurathex.aural";

/// `data_dir` (%LOCALAPPDATA%\com.aurathex.aural): models and logs — machine-local, large.
/// `config_dir` (%APPDATA%\com.aurathex.aural): settings.json — small, roams with the profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
}

impl AppPaths {
    /// Test/sandbox layout: both roots under one directory.
    pub fn under(root: &Path) -> Self {
        Self {
            data_dir: root.join("data"),
            config_dir: root.join("config"),
        }
    }

    pub fn from_env() -> Result<Self> {
        Self::from_env_with(|k| std::env::var(k).ok())
    }

    /// `AURAL_DATA_DIR` redirects everything (tests, portable use); otherwise the
    /// standard per-user folders.
    pub fn from_env_with(get: impl Fn(&str) -> Option<String>) -> Result<Self> {
        if let Some(root) = get("AURAL_DATA_DIR").filter(|s| !s.is_empty()) {
            return Ok(Self::under(Path::new(&root)));
        }
        let local = get("LOCALAPPDATA").context("LOCALAPPDATA is not set")?;
        let roaming = get("APPDATA").context("APPDATA is not set")?;
        Ok(Self {
            data_dir: Path::new(&local).join(DIR_NAME),
            config_dir: Path::new(&roaming).join(DIR_NAME),
        })
    }

    pub fn models_dir(&self) -> PathBuf {
        self.data_dir.join("models")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }

    pub fn settings_file(&self) -> PathBuf {
        self.config_dir.join("settings.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn override_root_puts_everything_under_it() {
        let p = AppPaths::under(Path::new("C:/tmp/aural-test"));
        assert_eq!(p.data_dir, Path::new("C:/tmp/aural-test/data"));
        assert_eq!(p.config_dir, Path::new("C:/tmp/aural-test/config"));
        assert_eq!(p.models_dir(), Path::new("C:/tmp/aural-test/data/models"));
        assert_eq!(p.logs_dir(), Path::new("C:/tmp/aural-test/data/logs"));
        assert_eq!(
            p.settings_file(),
            Path::new("C:/tmp/aural-test/config/settings.json")
        );
    }

    #[test]
    fn default_roots_are_the_per_user_identifier_folders() {
        let p = AppPaths::from_env_with(|k| match k {
            "LOCALAPPDATA" => Some("C:/Users/u/AppData/Local".into()),
            "APPDATA" => Some("C:/Users/u/AppData/Roaming".into()),
            _ => None,
        })
        .unwrap();
        // Not %LOCALAPPDATA%\Aural: that is where the per-user installer puts the app.
        assert_eq!(
            p.data_dir,
            Path::new("C:/Users/u/AppData/Local/com.aurathex.aural")
        );
        assert_eq!(
            p.config_dir,
            Path::new("C:/Users/u/AppData/Roaming/com.aurathex.aural")
        );
    }

    #[test]
    fn aural_data_dir_env_overrides_both_roots() {
        let p = AppPaths::from_env_with(|k| match k {
            "AURAL_DATA_DIR" => Some("D:/sandbox".into()),
            _ => Some("C:/should-not-be-used".into()),
        })
        .unwrap();
        assert_eq!(p.data_dir, Path::new("D:/sandbox/data"));
        assert_eq!(p.config_dir, Path::new("D:/sandbox/config"));
    }

    #[test]
    fn missing_localappdata_is_an_error() {
        assert!(AppPaths::from_env_with(|_| None).is_err());
    }
}
