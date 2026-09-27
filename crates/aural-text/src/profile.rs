//! Per-app behaviour. A profile names an app by its program file ("slack.exe") and
//! overrides only what it sets; everything else, and every app without a profile
//! (including when Aural can't tell which app is in front), uses the general settings.

use aural_core::settings::{AppProfile, CleanupMode, Settings};
use serde::Serialize;

/// What applies to one dictation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Effective {
    pub cleanup: CleanupMode,
    pub dictionary: bool,
    pub live: bool,
    pub history: bool,
}

/// "Slack.EXE", "slack.exe" and "slack" all name the same app.
pub fn app_key(name: &str) -> String {
    let n = name.trim().to_lowercase();
    let n = n.rsplit(['\\', '/']).next().unwrap_or(&n).to_owned();
    n.strip_suffix(".exe").map(str::to_owned).unwrap_or(n)
}

pub fn find<'a>(profiles: &'a [AppProfile], app: &str) -> Option<&'a AppProfile> {
    let key = app_key(app);
    if key.is_empty() {
        return None;
    }
    profiles.iter().find(|p| app_key(&p.app) == key)
}

pub fn resolve(settings: &Settings, app: &str) -> Effective {
    let p = find(&settings.apps, app);
    Effective {
        cleanup: p.and_then(|p| p.cleanup).unwrap_or(settings.text.cleanup),
        dictionary: p
            .and_then(|p| p.dictionary)
            .unwrap_or(settings.text.dictionary),
        live: p.and_then(|p| p.live).unwrap_or(settings.live.enabled),
        history: p
            .and_then(|p| p.history)
            .unwrap_or(settings.history.enabled),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> Settings {
        let mut s = Settings::default();
        s.text.cleanup = CleanupMode::Light;
        s.apps = vec![
            AppProfile {
                app: "Code.exe".into(),
                cleanup: Some(CleanupMode::Off),
                ..Default::default()
            },
            AppProfile {
                app: "KeePassXC.exe".into(),
                history: Some(false),
                live: Some(false),
                ..Default::default()
            },
        ];
        s
    }

    #[test]
    fn a_profile_overrides_only_what_it_sets() {
        let s = settings();
        let code = resolve(&s, "code.exe");
        assert_eq!(code.cleanup, CleanupMode::Off);
        assert!(code.history && code.dictionary && code.live);
        let kp = resolve(&s, "C:\\Program Files\\KeePassXC\\KeePassXC.EXE");
        assert_eq!(kp.cleanup, CleanupMode::Light);
        assert!(!kp.history && !kp.live);
    }

    #[test]
    fn unknown_or_undetected_apps_use_the_general_settings() {
        let s = settings();
        for app in ["notepad.exe", "", "   "] {
            let e = resolve(&s, app);
            assert_eq!(e.cleanup, CleanupMode::Light, "{app:?}");
            assert!(e.history && e.dictionary && e.live);
        }
    }

    #[test]
    fn app_names_compare_without_case_path_or_extension() {
        assert_eq!(app_key("C:\\x\\Slack.EXE"), "slack");
        assert_eq!(app_key("slack"), "slack");
        assert!(find(&settings().apps, "CODE").is_some());
    }
}
