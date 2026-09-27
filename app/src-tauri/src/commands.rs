//! Commands the settings window can invoke. The webview has no other access to the
//! system: every action goes through one of these, and each re-validates its input.

use crate::state::{lock, App, AppStateDto, Control};
use aural_core::settings::Settings;
use aural_core::uninstall;
use aural_models::DownloadError;
use aural_platform::chord::{validate, Chord, Verdict};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tauri::{Emitter, State};

type Res<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[tauri::command]
pub fn get_state(app: State<'_, Arc<App>>) -> AppStateDto {
    app.snapshot()
}

#[tauri::command]
pub fn dismiss_notice(app: State<'_, Arc<App>>) -> AppStateDto {
    *lock(&app.notice) = None;
    app.snapshot()
}

#[derive(Serialize)]
pub struct HotkeyCheck {
    display: String,
    verdict: Verdict,
}

#[tauri::command]
pub fn check_hotkey(keys: Vec<String>) -> Res<HotkeyCheck> {
    let chord = Chord::parse(&keys).map_err(err)?;
    Ok(HotkeyCheck {
        display: chord.display(),
        verdict: validate(&chord),
    })
}

/// While the hotkey recorder is open the current hotkey must not trigger dictation.
#[tauri::command]
pub fn pause_hotkey(app: State<'_, Arc<App>>, paused: bool) {
    if let Some(h) = lock(&app.hook).as_ref() {
        h.set_paused(paused);
    }
}

#[tauri::command]
pub fn save_settings(app: State<'_, Arc<App>>, settings: Settings) -> Res<AppStateDto> {
    let chord = Chord::parse(&settings.hotkey.keys).map_err(err)?;
    if let Verdict::Reject(why) = validate(&chord) {
        return Err(why);
    }
    let old = app.settings();
    if let Some(id) = &settings.stt.active_variant {
        if app.catalog.variant(id).is_none() {
            return Err(format!("unknown model variant {id}"));
        }
    }
    if old.startup.launch_at_login != settings.startup.launch_at_login {
        let exe = std::env::current_exe().map_err(err)?;
        aural_platform::autostart::set_enabled(
            aural_platform::autostart::VALUE_NAME,
            &exe,
            settings.startup.launch_at_login,
        )
        .map_err(err)?;
    }
    if old.hotkey.keys != settings.hotkey.keys {
        if let Some(h) = lock(&app.hook).as_ref() {
            h.set_chord(chord);
        }
    }
    if old.hotkey.mode != settings.hotkey.mode {
        app.send(Control::SetMode(settings.hotkey.mode));
    }
    let model_changed = old.stt.active_variant != settings.stt.active_variant;
    app.save_settings(settings).map_err(err)?;
    if model_changed {
        app.reload_engine();
    }
    Ok(app.snapshot())
}

#[derive(Clone, Serialize)]
struct ProgressEvent {
    id: String,
    downloaded: u64,
    total: u64,
}

#[tauri::command]
pub fn download_model(app: State<'_, Arc<App>>, id: String) -> Res<AppStateDto> {
    let entry = app
        .catalog
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown model {id}"))?;
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut downloads = lock(&app.downloads);
        if downloads.contains_key(&id) {
            return Ok(app.snapshot());
        }
        downloads.insert(id.clone(), (0, entry.total_size()));
        lock(&app.cancels).insert(id.clone(), cancel.clone());
    }
    let app2 = Arc::clone(&app);
    std::thread::spawn(move || {
        let mut last = Instant::now();
        let result = aural_models::download(&entry, &app2.store, &cancel, &mut |p| {
            lock(&app2.downloads).insert(entry.id.clone(), (p.downloaded, p.total));
            if last.elapsed().as_millis() >= 200 || p.downloaded == p.total {
                last = Instant::now();
                let _ = app2.handle.emit_to(
                    "main",
                    "model-progress",
                    ProgressEvent {
                        id: entry.id.clone(),
                        downloaded: p.downloaded,
                        total: p.total,
                    },
                );
            }
        });
        lock(&app2.downloads).remove(&entry.id);
        lock(&app2.cancels).remove(&entry.id);
        match result {
            Ok(()) => {
                crate::hwtest::measure_download(&app2, &entry);
                // The first installed model becomes the active one.
                let mut s = app2.settings();
                let active_ok = s
                    .stt
                    .active_model
                    .as_ref()
                    .and_then(|a| app2.catalog.get(a))
                    .is_some_and(|m| app2.store.is_installed(m));
                if !active_ok {
                    // The variant that suits this PC best, not always the processor.
                    let e = crate::hwtest::evaluation(&app2);
                    s.stt.active_model = Some(entry.id.clone());
                    s.stt.active_variant = Some(crate::hwtest::variant_to_activate(
                        &entry, &e.labels, &e.results,
                    ));
                    if app2.save_settings(s).is_ok() {
                        app2.reload_engine();
                    }
                }
            }
            Err(DownloadError::Cancelled) => {}
            Err(_) => app2.set_notice(format!(
                "{} couldn't be downloaded. Check your internet connection and free disk space, then try again.",
                entry.name
            )),
        }
        app2.broadcast();
    });
    Ok(app.snapshot())
}

#[tauri::command]
pub fn cancel_download(app: State<'_, Arc<App>>, id: String) {
    if let Some(flag) = lock(&app.cancels).get(&id) {
        flag.store(true, Ordering::Relaxed);
    }
}

#[tauri::command]
pub fn remove_model(app: State<'_, Arc<App>>, id: String) -> Res<AppStateDto> {
    let entry = app
        .catalog
        .get(&id)
        .ok_or_else(|| format!("unknown model {id}"))?;
    let active = app.settings().stt.active_model;
    {
        // Held across the check and the delete so a download can't start in between.
        let downloads = lock(&app.downloads);
        app.store
            .remove(entry, active.as_deref(), downloads.contains_key(&id))
            .map_err(err)?;
    }
    Ok(app.snapshot())
}

/// Use a model variant (`<model id>@<backend>`) for dictation.
#[tauri::command]
pub fn use_variant(app: State<'_, Arc<App>>, id: String) -> Res<AppStateDto> {
    let (entry, _) = app
        .catalog
        .variant(&id)
        .ok_or_else(|| format!("unknown model variant {id}"))?;
    if !app.store.is_installed(entry) {
        return Err(format!("{} is not downloaded yet", entry.name));
    }
    let mut s = app.settings();
    s.stt.active_model = Some(entry.id.clone());
    s.stt.active_variant = Some(id);
    app.save_settings(s).map_err(err)?;
    app.reload_engine();
    Ok(app.snapshot())
}

#[derive(Serialize)]
pub struct HardwareState {
    hardware: aural_models::HardwareProfile,
    results: Vec<aural_models::VariantResult>,
    labels: std::collections::BTreeMap<String, Vec<aural_models::Label>>,
    hardware_test: crate::hwtest::HwTestStatus,
}

#[tauri::command]
pub fn hardware_state(app: State<'_, Arc<App>>) -> HardwareState {
    let e = crate::hwtest::evaluation(&app);
    HardwareState {
        hardware: app.hw.clone(),
        results: e.results,
        labels: e.labels,
        hardware_test: crate::hwtest::status(&app),
    }
}

/// Runs the Hardware Test in the background; `allow_probe_download` is the user's answer
/// to "download two small test models (about 80 MB)?".
#[tauri::command]
pub fn start_hardware_test(app: State<'_, Arc<App>>, allow_probe_download: bool) -> AppStateDto {
    crate::hwtest::start(&app, allow_probe_download);
    app.snapshot()
}

#[tauri::command]
pub fn cancel_hardware_test(app: State<'_, Arc<App>>) {
    crate::hwtest::cancel(&app);
}

#[tauri::command]
pub fn mic_test_start(app: State<'_, Arc<App>>) -> Res<()> {
    let mut slot = lock(&app.mic_test);
    if slot.is_some() {
        return Ok(());
    }
    let device = app.settings().audio.device;
    let handle = app.handle.clone();
    let cap = aural_audio::capture::start(
        device.as_deref(),
        Box::new(move |frame| {
            let _ = handle.emit_to("main", "mic-levels", frame.bands);
        }),
    )
    .map_err(err)?;
    *slot = Some(cap);
    Ok(())
}

#[tauri::command]
pub fn mic_test_stop(app: State<'_, Arc<App>>) {
    if let Some(cap) = lock(&app.mic_test).take() {
        let _ = cap.stop();
    }
}

#[tauri::command]
pub fn open_mic_privacy() {
    crate::shell::open(aural_platform::consent::PRIVACY_SETTINGS_URI);
}

#[tauri::command]
pub fn open_data_folder(app: State<'_, Arc<App>>) {
    let _ = std::fs::create_dir_all(&app.paths.data_dir);
    crate::shell::open(&app.paths.data_dir.display().to_string());
}

/// "Delete Aural". Refuses unless the exact confirmation phrase was typed (the window
/// checks too; this is the authoritative check). Removes the startup entry, quits, and
/// leaves a cleanup step that deletes models, logs and settings once Aural's files are
/// released and then runs the installer's uninstaller silently.
#[tauri::command]
pub fn delete_aural(app: State<'_, Arc<App>>, confirmation: String) -> Res<()> {
    if !uninstall::is_confirmed(&confirmation) {
        return Err(format!(
            "Type {} exactly to delete Aural.",
            uninstall::CONFIRMATION_PHRASE
        ));
    }
    let install_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));
    let registered = aural_platform::install::registered_uninstaller();
    let plan = uninstall::plan(&app.paths, install_dir.as_deref(), registered.as_deref());
    // Validate before touching anything: a refused plan leaves Aural fully working.
    let cleanup = plan.after_exit_command().map_err(err)?;

    // Stop everything that holds files open.
    *lock(&app.hook) = None;
    if let Some(cap) = lock(&app.mic_test).take() {
        let _ = cap.stop();
    }
    app.engine.unload();

    if plan.remove_autostart {
        if let Ok(exe) = std::env::current_exe() {
            let _ = aural_platform::autostart::set_enabled(
                aural_platform::autostart::VALUE_NAME,
                &exe,
                false,
            );
        }
    }
    crate::shell::run_detached(&cleanup).map_err(err)?;
    let _ = app.handle.emit_to("main", "deleted", ());
    let handle = app.handle.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(400));
        handle.exit(0);
    });
    Ok(())
}

#[tauri::command]
pub fn quit(app: State<'_, Arc<App>>) {
    app.handle.exit(0);
}
