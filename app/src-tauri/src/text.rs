//! The text pipeline and local data in the app: what gets typed for a transcript, and
//! what is kept afterwards (history, statistics). The rules themselves live in
//! `aural_text`; this module applies the user's settings and the app in front.

use crate::state::{lock, App};
use aural_core::settings::{CleanupMode, Settings};
use aural_text::profile::{self, Effective};
use aural_text::Words;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Polished {
    pub text: String,
    /// Cleanup (light or AI) changed the text.
    pub cleaned: bool,
    /// The model's rewrite was used.
    pub rewritten: bool,
    pub effective: Effective,
}

/// Dictionary and cleanup for `raw`, under the settings for `app` (the program in front;
/// "" when unknown). `rewrite` asks the text model; its answer is used only if it passes
/// `aural_text`'s check, otherwise the light result is typed.
pub fn polish(
    settings: &Settings,
    words: &Words,
    app: &str,
    raw: &str,
    rewrite: impl FnOnce(&str) -> Option<String>,
) -> Polished {
    let effective = profile::resolve(settings, app);
    let prepared =
        aural_text::prepare(raw, &words.entries, effective.dictionary, effective.cleanup);
    let dictionary_only =
        aural_text::prepare(raw, &words.entries, effective.dictionary, CleanupMode::Off);
    let mut rewritten = false;
    let text = if effective.cleanup == CleanupMode::Ai && !prepared.trim().is_empty() {
        match rewrite(&prepared).map(|a| {
            aural_text::accept_rewrite(&prepared, &a, &words.entries, effective.dictionary)
        }) {
            Some(Ok(t)) => {
                rewritten = true;
                t
            }
            _ => prepared,
        }
    } else {
        prepared
    };
    Polished {
        cleaned: text != dictionary_only,
        text,
        rewritten,
        effective,
    }
}

/// Facts about one dictation, gathered while it happened.
#[derive(Debug, Clone, Default)]
pub struct Session {
    pub app: String,
    pub audio_ms: u64,
    pub live: bool,
    pub cleaned: bool,
    pub history: bool,
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The app name statistics may keep: none for an app left out of history.
pub fn stats_app(s: &Session) -> &str {
    if s.history {
        &s.app
    } else {
        ""
    }
}

/// Keep a finished dictation in history and statistics, as the settings allow.
pub fn record(app: &Arc<App>, raw: &str, text: &str, s: &Session) {
    let settings = app.settings();
    let variant = settings.stt.active_variant.clone().unwrap_or_default();
    let words = aural_text::history::word_count(text);
    if s.history && settings.history.enabled {
        let mut h = lock(&app.history);
        let _ = h.add(aural_text::history::Entry {
            id: 0,
            at: now(),
            text: text.to_owned(),
            raw: (raw.trim() != text.trim()).then(|| raw.to_owned()),
            corrected: None,
            app: s.app.clone(),
            variant: variant.clone(),
            audio_ms: s.audio_ms,
            words,
        });
        let _ = h.prune(settings.history.keep_days, now());
    }
    if settings.history.stats {
        let mut st = lock(&app.stats);
        st.record(&aural_text::stats::Record {
            at: now(),
            words: u64::from(words),
            audio_ms: s.audio_ms,
            variant: &variant,
            app: stats_app(s),
            live: s.live,
            cleaned: s.cleaned,
        });
        let _ = st.save(&app.paths.stats_file());
    }
}

/// Load or unload the AI cleanup model to match the settings (off the UI thread).
pub fn reload_text_engine(app: &Arc<App>) {
    let s = app.settings();
    let wanted = s.text.cleanup == CleanupMode::Ai
        || s.apps.iter().any(|p| p.cleanup == Some(CleanupMode::Ai));
    let model = s
        .text
        .ai_model
        .clone()
        .or_else(|| app.text_catalog.models.first().map(|m| m.id.clone()))
        .and_then(|id| app.text_catalog.get(&id).cloned())
        .filter(|m| aural_models::text::is_installed(&app.paths.text_models_dir(), m));
    match (wanted, model) {
        (true, Some(m)) => {
            if !crate::text_engine::needs_load(&app.text_engine.status(), &m.id) {
                return;
            }
            let app = app.clone();
            std::thread::spawn(move || {
                app.text_engine.load(
                    &m.id,
                    aural_models::text::dir(&app.paths.text_models_dir(), &m.id),
                );
                app.broadcast();
            });
        }
        _ => app.text_engine.unload(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aural_core::settings::AppProfile;
    use aural_text::dictionary::Entry;

    fn words() -> Words {
        Words {
            entries: vec![Entry {
                write: "Aurathex".into(),
                heard: vec!["aura thex".into()],
            }],
            ..Default::default()
        }
    }

    fn settings(mode: CleanupMode) -> Settings {
        let mut s = Settings::default();
        s.text.cleanup = mode;
        s
    }

    #[test]
    fn an_app_left_out_of_history_is_not_named_in_statistics() {
        let private = Session {
            app: "keepassxc.exe".into(),
            history: false,
            ..Default::default()
        };
        assert_eq!(stats_app(&private), "");
        let open = Session {
            app: "slack.exe".into(),
            history: true,
            ..Default::default()
        };
        assert_eq!(stats_app(&open), "slack.exe");
    }

    #[test]
    fn off_types_the_models_words_with_the_dictionary() {
        let p = polish(
            &settings(CleanupMode::Off),
            &words(),
            "",
            "um aura thex",
            |_| panic!("no model call"),
        );
        assert_eq!(p.text, "um Aurathex");
        assert!(!p.cleaned && !p.rewritten);
    }

    #[test]
    fn light_cleanup_needs_no_model() {
        let p = polish(
            &settings(CleanupMode::Light),
            &words(),
            "",
            "um, aura thex rocks",
            |_| panic!("no model call"),
        );
        assert_eq!(p.text, "Aurathex rocks");
        assert!(p.cleaned);
    }

    #[test]
    fn an_accepted_rewrite_is_typed() {
        let p = polish(
            &settings(CleanupMode::Ai),
            &words(),
            "",
            "the meeting is at 3pm",
            |t| {
                assert_eq!(t, "The meeting is at 3pm");
                Some("The meeting is at 3pm.".into())
            },
        );
        assert_eq!(p.text, "The meeting is at 3pm.");
        assert!(p.rewritten && p.cleaned);
    }

    #[test]
    fn a_rejected_or_missing_rewrite_falls_back_to_light_cleanup() {
        let changed = polish(
            &settings(CleanupMode::Ai),
            &words(),
            "",
            "the meeting is at 3pm",
            |_| Some("The meeting is at 4pm.".into()),
        );
        assert_eq!(changed.text, "The meeting is at 3pm");
        assert!(!changed.rewritten);
        let none = polish(&settings(CleanupMode::Ai), &words(), "", "um hi", |_| None);
        assert_eq!(none.text, "Hi");
    }

    #[test]
    fn the_app_in_front_can_change_the_cleanup() {
        let mut s = settings(CleanupMode::Light);
        s.apps.push(AppProfile {
            app: "WindowsTerminal.exe".into(),
            cleanup: Some(CleanupMode::Off),
            dictionary: Some(false),
            ..Default::default()
        });
        let p = polish(
            &s,
            &words(),
            "windowsterminal.exe",
            "um git status aura thex",
            |_| None,
        );
        assert_eq!(p.text, "um git status aura thex");
        let p = polish(
            &s,
            &words(),
            "notepad.exe",
            "um git status aura thex",
            |_| None,
        );
        assert_eq!(p.text, "Git status Aurathex");
    }
}
