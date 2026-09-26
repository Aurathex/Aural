//! User settings: one JSON file, written atomically, tolerant of missing fields and
//! recoverable when corrupt.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub schema_version: u32,
    pub hotkey: HotkeySettings,
    pub audio: AudioSettings,
    pub stt: SttSettings,
    pub startup: StartupSettings,
    pub ui: UiSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            hotkey: HotkeySettings::default(),
            audio: AudioSettings::default(),
            stt: SttSettings::default(),
            startup: StartupSettings::default(),
            ui: UiSettings::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HotkeyMode {
    /// Hold to talk, release to transcribe.
    PushToTalk,
    /// Press to start, press again to stop.
    Toggle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HotkeySettings {
    /// Key names as shown to the user, e.g. ["Ctrl", "Win"] or ["RightCtrl"].
    pub keys: Vec<String>,
    pub mode: HotkeyMode,
}

impl Default for HotkeySettings {
    fn default() -> Self {
        Self {
            keys: vec!["Ctrl".into(), "Win".into()],
            mode: HotkeyMode::PushToTalk,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioSettings {
    /// Input device name; `None` follows the Windows default microphone.
    pub device: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SttSettings {
    /// Catalog id of the model used for dictation.
    pub active_model: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StartupSettings {
    pub launch_at_login: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PillPosition {
    Bottom,
    Top,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiSettings {
    pub pill_position: PillPosition,
    /// Short start/stop listening sounds.
    pub sounds: bool,
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            pill_position: PillPosition::Bottom,
            sounds: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded {
    pub settings: Settings,
    /// Set when the file was unreadable and has been replaced by defaults.
    pub notice: Option<String>,
}

pub fn load(path: &Path) -> Result<Loaded> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Loaded {
                settings: Settings::default(),
                notice: None,
            })
        }
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    match serde_json::from_str::<Settings>(&text) {
        Ok(settings) => Ok(Loaded {
            settings,
            notice: None,
        }),
        Err(parse_err) => {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            let backup = path.with_file_name(format!("settings.corrupt-{stamp}.json"));
            std::fs::rename(path, &backup)
                .with_context(|| format!("backing up corrupt {}", path.display()))?;
            Ok(Loaded {
                settings: Settings::default(),
                notice: Some(format!(
                    "Settings were unreadable ({parse_err}); defaults restored. The old file was kept as {}.",
                    backup.display()
                )),
            })
        }
    }
}

/// Atomic write: temp file in the same directory, then rename over the target.
pub fn save(path: &Path, settings: &Settings) -> Result<()> {
    let dir = path.parent().context("settings path has no parent")?;
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(settings)?)
        .with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_push_to_talk_ctrl_win_and_no_autostart() {
        let s = Settings::default();
        assert_eq!(s.hotkey.keys, vec!["Ctrl".to_string(), "Win".to_string()]);
        assert_eq!(s.hotkey.mode, HotkeyMode::PushToTalk);
        assert!(!s.startup.launch_at_login);
        assert_eq!(s.audio.device, None);
        assert_eq!(s.stt.active_model, None);
        assert_eq!(s.schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn listening_sounds_are_on_by_default_including_for_older_settings_files() {
        assert!(Settings::default().ui.sounds);
        let old: Settings =
            serde_json::from_str(r#"{"schema_version":1,"ui":{"pill_position":"top"}}"#).unwrap();
        assert!(old.ui.sounds);
        assert_eq!(old.ui.pill_position, PillPosition::Top);
    }

    #[test]
    fn missing_file_loads_defaults_without_notice() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = load(&dir.path().join("settings.json")).unwrap();
        assert_eq!(loaded.settings, Settings::default());
        assert_eq!(loaded.notice, None);
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("settings.json");
        let mut s = Settings::default();
        s.hotkey.mode = HotkeyMode::Toggle;
        s.audio.device = Some("Microphone (USB)".into());
        s.startup.launch_at_login = true;
        save(&path, &s).unwrap();
        assert_eq!(load(&path).unwrap().settings, s);
    }

    #[test]
    fn partial_file_fills_missing_fields_with_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"hotkey":{"mode":"toggle"}}"#).unwrap();
        let s = load(&path).unwrap().settings;
        assert_eq!(s.hotkey.mode, HotkeyMode::Toggle);
        assert_eq!(s.hotkey.keys, Settings::default().hotkey.keys);
    }

    #[test]
    fn corrupt_file_is_backed_up_and_defaults_loaded_with_notice() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "{ not json").unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.settings, Settings::default());
        assert!(loaded.notice.is_some());
        let backups: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("settings.corrupt-")
            })
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(
            std::fs::read_to_string(backups[0].path()).unwrap(),
            "{ not json"
        );
    }

    #[test]
    fn save_leaves_no_temp_file_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        save(&path, &Settings::default()).unwrap();
        save(&path, &Settings::default()).unwrap();
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["settings.json".to_string()]);
    }
}
