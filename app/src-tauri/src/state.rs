//! Shared application state and the snapshot the settings window renders.

use crate::engine::{EngineHost, EngineStatus};
use aural_audio::capture::{CaptureHandle, InputDevice};
use aural_core::paths::AppPaths;
use aural_core::settings::{self, Settings};
use aural_models::{statuses, Catalog, HardwareProfile, ModelStatus, ModelStore};
use aural_platform::consent::MicConsent;
use aural_platform::hook::HookHandle;
use serde::Serialize;
use std::collections::HashMap;
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
}

#[derive(Serialize)]
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
        AppStateDto {
            version: env!("CARGO_PKG_VERSION").into(),
            hotkey_display,
            models,
            devices: aural_audio::capture::input_devices().unwrap_or_default(),
            hardware: self.hw,
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
            settings: s,
        }
    }

    pub fn broadcast(&self) {
        let _ = self.handle.emit_to("main", "state-changed", self.snapshot());
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
            let active = app.settings().stt.active_model;
            match active.and_then(|id| app.catalog.get(&id).cloned()) {
                Some(entry) if app.store.is_installed(&entry) => {
                    app.engine.mark_loading(&entry.id);
                    app.broadcast();
                    app.engine.load(&entry, &app.store);
                }
                _ => app.engine.unload(),
            }
            app.broadcast();
        });
    }
}
