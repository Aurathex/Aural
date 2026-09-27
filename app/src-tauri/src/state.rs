//! Shared application state and the snapshot the settings window renders.

use crate::engine::{EngineHost, EngineStatus};
use crate::hwtest::{HwTest, HwTestStatus};
use aural_audio::capture::{CaptureHandle, InputDevice};
use aural_core::paths::AppPaths;
use aural_core::settings::{self, Settings};
use aural_models::{
    statuses, Catalog, HardwareProfile, Label, ModelStatus, ModelStore, VariantResult,
};
use aural_platform::consent::MicConsent;
use aural_platform::hook::HookHandle;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use tauri::menu::MenuItem;
use tauri::{AppHandle, Emitter, Wry};

/// Messages for the dictation thread.
pub enum Control {
    Input(crate::dictation::Input),
    SetMode(settings::HotkeyMode),
}

pub struct App {
    pub handle: AppHandle,
    pub paths: AppPaths,
    pub catalog: Catalog,
    pub store: ModelStore,
    pub hw: HardwareProfile,
    pub settings: Mutex<Settings>,
    pub notice: Mutex<Option<String>>,
    pub downloads: Mutex<HashMap<String, (u64, u64)>>,
    pub cancels: Mutex<HashMap<String, Arc<AtomicBool>>>,
    pub engine: EngineHost,
    pub hook: Mutex<Option<HookHandle>>,
    pub control: Mutex<Option<Sender<Control>>>,
    pub mic_test: Mutex<Option<CaptureHandle>>,
    pub last_transcript: Mutex<Option<String>>,
    pub tray_paste: Mutex<Option<MenuItem<Wry>>>,
    /// Latest pill state, so a delayed hide never hides a newer session.
    pub pill_hidden: AtomicBool,
    pub hwtest: HwTest,
    /// Your dictionary and learned suggestions (dictionary.json).
    pub words: Mutex<aural_text::Words>,
    pub history: Mutex<aural_text::history::History>,
    pub stats: Mutex<aural_text::stats::Stats>,
    pub text_catalog: aural_models::text::TextCatalog,
    pub text_engine: crate::text_engine::TextHost,
    pub text_downloads: Mutex<HashMap<String, (u64, u64)>>,
}

#[derive(Clone, Serialize)]
pub struct AppStateDto {
    pub version: String,
    pub settings: Settings,
    pub hotkey_display: String,
    pub models: Vec<ModelStatus>,
    pub devices: Vec<InputDevice>,
    pub hardware: HardwareProfile,
    pub engine: EngineStatus,
    pub mic_consent: MicConsent,
    pub autostart: bool,
    pub data_dir: String,
    pub notice: Option<String>,
    pub last_transcript_available: bool,
    /// Hardware Test: measured or estimated numbers per variant id, and the labels.
    pub results: Vec<VariantResult>,
    pub labels: BTreeMap<String, Vec<Label>>,
    pub hardware_test: HwTestStatus,
    /// Whether words show while you speak with the model in use.
    pub live: crate::live::LiveStatus,
    /// Dictionary and learned suggestions.
    pub words: aural_text::Words,
    pub text_models: Vec<TextModelStatus>,
    pub text_engine: crate::text_engine::TextStatus,
    /// Apps you have dictated into (for per-app settings), most used first.
    pub recent_apps: Vec<String>,
    pub history_count: usize,
}

#[derive(Clone, Serialize)]
pub struct TextModelStatus {
    pub id: String,
    pub name: String,
    pub description: String,
    pub size_bytes: u64,
    pub min_ram_mb: u64,
    pub license_id: String,
    pub attribution: String,
    /// `available`, `downloading` or `installed`.
    pub state: &'static str,
    pub downloaded: u64,
}

pub fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl App {
    pub fn settings(&self) -> Settings {
        lock(&self.settings).clone()
    }

    pub fn set_notice(&self, msg: impl Into<String>) {
        *lock(&self.notice) = Some(msg.into());
    }

    /// Read-modify-write under one lock, so an automatic change can't overwrite a save
    /// made at the same moment.
    pub fn update_settings(&self, f: impl FnOnce(&mut Settings)) -> anyhow::Result<()> {
        let mut guard = lock(&self.settings);
        let mut s = guard.clone();
        f(&mut s);
        settings::save(&self.paths.settings_file(), &s)?;
        *guard = s;
        Ok(())
    }

    pub fn save_settings(&self, s: Settings) -> anyhow::Result<()> {
        settings::save(&self.paths.settings_file(), &s)?;
        *lock(&self.settings) = s;
        Ok(())
    }

    pub fn snapshot(&self) -> AppStateDto {
        let s = self.settings();
        let hotkey_display = aural_platform::chord::Chord::parse(&s.hotkey.keys)
            .map(|c| c.display())
            .unwrap_or_else(|_| s.hotkey.keys.join(" + "));
        let downloads = lock(&self.downloads).clone();
        let models = statuses(
            &self.catalog,
            &self.store,
            s.stt.active_model.as_deref(),
            &downloads,
            &self.hw,
        );
        let exe = std::env::current_exe().unwrap_or_default();
        let eval = crate::hwtest::evaluation(self);
        AppStateDto {
            version: env!("CARGO_PKG_VERSION").into(),
            hotkey_display,
            models,
            devices: aural_audio::capture::input_devices().unwrap_or_default(),
            hardware: self.hw.clone(),
            engine: self.engine.status(),
            mic_consent: aural_platform::consent::mic_consent(),
            autostart: aural_platform::autostart::is_enabled(
                aural_platform::autostart::VALUE_NAME,
                &exe,
            )
            .unwrap_or(false),
            data_dir: self.paths.data_dir.display().to_string(),
            notice: lock(&self.notice).clone(),
            last_transcript_available: lock(&self.last_transcript).is_some(),
            results: eval.results,
            labels: eval.labels,
            hardware_test: crate::hwtest::status(self),
            live: crate::live::for_app(self),
            words: lock(&self.words).clone(),
            text_models: self.text_models(),
            text_engine: self.text_engine.status(),
            recent_apps: self.recent_apps(),
            history_count: lock(&self.history).entries().len(),
            settings: s,
        }
    }

    fn text_models(&self) -> Vec<TextModelStatus> {
        let downloads = lock(&self.text_downloads).clone();
        let root = self.paths.text_models_dir();
        self.text_catalog
            .models
            .iter()
            .map(|m| {
                let dl = downloads.get(&m.id);
                TextModelStatus {
                    id: m.id.clone(),
                    name: m.name.clone(),
                    description: m.description.clone(),
                    size_bytes: m.total_size(),
                    min_ram_mb: m.min_ram_mb,
                    license_id: m.license.id.clone(),
                    attribution: m.license.attribution.clone(),
                    state: if dl.is_some() {
                        "downloading"
                    } else if aural_models::text::is_installed(&root, m) {
                        "installed"
                    } else {
                        "available"
                    },
                    downloaded: dl.map_or(0, |d| d.0),
                }
            })
            .collect()
    }

    /// Apps seen in statistics and history, plus those with settings; most used first.
    fn recent_apps(&self) -> Vec<String> {
        let mut apps: Vec<(String, u64)> = lock(&self.stats)
            .apps
            .iter()
            .filter(|(a, _)| a.as_str() != "unknown")
            .map(|(a, n)| (a.clone(), *n))
            .collect();
        for e in lock(&self.history).entries() {
            if !e.app.is_empty() && !apps.iter().any(|(a, _)| a.eq_ignore_ascii_case(&e.app)) {
                apps.push((e.app.to_lowercase(), 0));
            }
        }
        apps.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        apps.into_iter().map(|(a, _)| a).take(30).collect()
    }

    pub fn broadcast(&self) {
        let _ = self
            .handle
            .emit_to("main", "state-changed", self.snapshot());
    }

    pub fn send(&self, c: Control) {
        if let Some(tx) = lock(&self.control).as_ref() {
            let _ = tx.send(c);
        }
    }

    /// Load the active model in the background (no-op when none is installed).
    pub fn reload_engine(self: &Arc<Self>) {
        let app = self.clone();
        std::thread::spawn(move || {
            let active = app.settings().stt.active_variant;
            let chosen = active
                .as_deref()
                .and_then(|id| app.catalog.variant(id))
                .map(|(entry, v)| (entry.clone(), v.backend));
            match chosen {
                Some((entry, backend)) if app.store.is_installed(&entry) => {
                    app.engine.mark_loading(&entry.id);
                    app.broadcast();
                    let failed = app.engine.load(&entry, backend, &app.store);
                    // A graphics card that can't start the model must not leave the user
                    // without dictation: fall back to the processor and say so.
                    if let Some(message) = failed {
                        if backend != aural_engines::Backend::Cpu {
                            let variant = entry.variant_id(backend);
                            crate::hwtest::after_load_failure(&app, &variant, &message);
                            return;
                        }
                    }
                }
                _ => app.engine.unload(),
            }
            app.broadcast();
        });
    }
}
