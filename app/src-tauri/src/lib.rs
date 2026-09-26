//! Aural desktop app: tray, settings window, listening pill, and the dictation loop.

pub mod commands;
pub mod dictation;
pub mod engine;
pub mod io;
pub mod pill;
pub mod shell;
pub mod state;
pub mod tray;

use crate::dictation::{Dictation, Input};
use crate::state::{lock, App, Control};
use aural_core::paths::AppPaths;
use aural_core::settings;
use aural_models::{Catalog, HardwareProfile, ModelStore};
use aural_platform::chord::Chord;
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::{Manager, WindowEvent};

fn start(handle: &tauri::AppHandle) -> anyhow::Result<()> {
    let paths = AppPaths::from_env()?;
    std::fs::create_dir_all(paths.models_dir())?;
    std::fs::create_dir_all(&paths.config_dir)?;
    let loaded = settings::load(&paths.settings_file())?;
    let mut s = loaded.settings;
    let mut notice = loaded.notice;

    let chord = match Chord::parse(&s.hotkey.keys) {
        Ok(c) => c,
        Err(e) => {
            notice = Some(format!(
                "Your hotkey could not be read ({e}); Ctrl + Win restored."
            ));
            s.hotkey.keys = settings::Settings::default().hotkey.keys;
            Chord::parse(&s.hotkey.keys)?
        }
    };

    let app = Arc::new(App {
        handle: handle.clone(),
        store: ModelStore::new(&paths.models_dir()),
        paths,
        catalog: Catalog::builtin(),
        hw: HardwareProfile::detect(),
        settings: Mutex::new(s.clone()),
        notice: Mutex::new(notice),
        downloads: Mutex::new(HashMap::new()),
        cancels: Mutex::new(HashMap::new()),
        engine: Default::default(),
        hook: Mutex::new(None),
        control: Mutex::new(None),
        mic_test: Mutex::new(None),
        last_transcript: Mutex::new(None),
        tray_paste: Mutex::new(None),
        pill_hidden: AtomicBool::new(true),
    });
    handle.manage(app.clone());

    // Keep "start with Windows" pointing at this executable (e.g. after an update).
    if s.startup.launch_at_login {
        if let Ok(exe) = std::env::current_exe() {
            let _ = aural_platform::autostart::set_enabled(
                aural_platform::autostart::VALUE_NAME,
                &exe,
                true,
            );
        }
    }

    if let Some(pill) = handle.get_webview_window("pill") {
        pill::init(&pill);
    }
    tray::build(&app, &chord.display())?;

    // Dictation thread: hotkey events and timers in, effects out.
    let (tx, rx) = mpsc::channel::<Control>();
    *lock(&app.control) = Some(tx.clone());
    let io = io::AppIo::new(app.clone());
    let mode = s.hotkey.mode;
    std::thread::Builder::new()
        .name("aural-dictation".into())
        .spawn(move || {
            let mut d = Dictation::new(io, mode);
            while let Ok(msg) = rx.recv() {
                match msg {
                    Control::Input(input) => d.handle(input),
                    Control::SetMode(m) => d.set_mode(m),
                }
            }
        })?;

    // Hotkey hook → dictation thread, stamped with a monotonic clock.
    let (hk_tx, hk_rx) = mpsc::channel();
    let epoch = Instant::now();
    std::thread::Builder::new()
        .name("aural-hotkey-forward".into())
        .spawn(move || {
            while let Ok(event) = hk_rx.recv() {
                let t_ms = epoch.elapsed().as_millis() as u64;
                if tx
                    .send(Control::Input(Input::Hotkey { event, t_ms }))
                    .is_err()
                {
                    break;
                }
            }
        })?;
    match aural_platform::hook::spawn(chord, hk_tx) {
        Ok(h) => *lock(&app.hook) = Some(h),
        Err(e) => app.set_notice(format!("The global hotkey could not be installed: {e}")),
    }

    app.reload_engine();

    let autostarted = std::env::args().any(|a| a == "--autostart");
    if !autostarted {
        tray::show_settings(handle);
    }
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_settings(app);
        }))
        .setup(|app| {
            start(app.handle()).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the settings window keeps Aural running in the tray.
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                    if let Some(app) = window.try_state::<Arc<App>>() {
                        if let Some(cap) = lock(&app.mic_test).take() {
                            let _ = cap.stop();
                        }
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::dismiss_notice,
            commands::check_hotkey,
            commands::pause_hotkey,
            commands::save_settings,
            commands::download_model,
            commands::cancel_download,
            commands::remove_model,
            commands::use_model,
            commands::mic_test_start,
            commands::mic_test_stop,
            commands::open_mic_privacy,
            commands::open_data_folder,
            commands::delete_aural,
            commands::quit,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Aural");
}
