//! Selected-text editing is a separate, explicitly reviewed local operation.
use crate::state::{lock, App};
use aural_platform::insert::{self, SelectionSnapshot};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::{Manager, State};

#[derive(Clone, Default, Serialize)]
pub struct Preview {
    pub id: u64,
    pub phase: String,
    pub original: String,
    pub target: String,
    pub suggestion: String,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct Touchup {
    inner: Mutex<Session>,
    shortcut: Mutex<Option<std::sync::mpsc::Sender<bool>>>,
}
#[derive(Default)]
struct Session {
    preview: Preview,
    selection: Option<SelectionSnapshot>,
}

pub fn validate_dictation_chord(chord: &aural_platform::chord::Chord) -> Result<(), String> {
    let names = chord.names();
    // A modifier-only subset would also start dictation when touch-up is invoked.
    if names
        .iter()
        .all(|k| matches!(k.as_str(), "Ctrl" | "Alt" | "RightCtrl" | "RightAlt" | "T"))
    {
        return Err("Ctrl + Alt + T is reserved for selected-text touch-up. Choose a different dictation shortcut.".into());
    }
    Ok(())
}

fn show(app: &App) {
    crate::tray::show_settings(&app.handle);
    app.broadcast();
}

fn begin(app: &Arc<App>) {
    if !app.settings().text.touchup {
        return;
    }
    crate::text::reload_text_engine(app);
    let existing_phase = lock(&app.touchup.inner).preview.phase.clone();
    if matches!(existing_phase.as_str(), "working" | "review") {
        show(app);
        return;
    }
    let id = {
        let mut s = lock(&app.touchup.inner);
        if matches!(s.preview.phase.as_str(), "working" | "applying" | "review") {
            return;
        }
        let id = s.preview.id.wrapping_add(1);
        s.preview = Preview {
            id,
            phase: "working".into(),
            ..Default::default()
        };
        s.selection = None;
        id
    };
    // Wait for the real shortcut keys to be released before synthesizing Copy.
    // Otherwise holding T could repeat into the selection after modifier release.
    let foreground = insert::foreground_window();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while shortcut_keys_down() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    if shortcut_keys_down() || foreground != insert::foreground_window() {
        finish_error(
            app,
            id,
            "Release the shortcut without changing apps, then try again. Original text kept."
                .into(),
        );
        show(app);
        return;
    }
    // Capture before showing Aural, which would change the foreground selection.
    let captured = insert::capture_selection()
        .map_err(|e| e.to_string())
        .and_then(|selection| {
            validate_selection(&selection.text)?;
            Ok(selection)
        });
    let selection = match captured {
        Ok(selection) => selection,
        Err(e) => {
            finish_error(app, id, e);
            show(app);
            return;
        }
    };
    {
        let mut s = lock(&app.touchup.inner);
        if s.preview.id != id {
            return;
        }
        s.preview.original = selection.text.clone();
        s.preview.target = selection.target_caption().to_owned();
        s.selection = Some(selection.clone());
    }
    show(app);
    let result = wait_for_helper(app, id).and_then(|()| app.text_engine.touchup(&selection.text));
    {
        let mut s = lock(&app.touchup.inner);
        if s.preview.id != id || s.preview.phase != "working" {
            return;
        }
        match result {
            Ok(suggestion) => {
                s.preview.suggestion = suggestion;
                s.preview.phase = "review".into();
            }
            Err(e) => {
                s.preview.error = Some(e);
                s.preview.phase = "error".into();
            }
        }
    }
    app.broadcast();
}

/// Wait off the UI thread so one shortcut also recovers a timed-out worker.
fn wait_for_helper(app: &Arc<App>, id: u64) -> Result<(), String> {
    let settings = app.settings();
    let model = settings
        .text
        .ai_model
        .as_deref()
        .and_then(|id| app.text_catalog.get(id))
        .or_else(|| {
            if settings.text.ai_model.is_none() {
                app.text_catalog.models.first()
            } else {
                None
            }
        })
        .filter(|model| aural_models::text::is_installed(&app.paths.text_models_dir(), model))
        .ok_or("Download the writing helper in Writing settings before using AI touch-up.")?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(185);
    loop {
        {
            let session = lock(&app.touchup.inner);
            if session.preview.id != id || session.preview.phase != "working" {
                return Err("Touch-up cancelled.".into());
            }
        }
        if !app.settings().text.touchup {
            return Err("AI touch-up was disabled. Original text kept.".into());
        }
        if helper_ready(&app.text_engine.status(), &model.id)? {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            return Err("The writing helper did not become ready. Original text kept.".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}

fn helper_ready(status: &crate::text_engine::TextStatus, wanted: &str) -> Result<bool, String> {
    use crate::text_engine::TextStatus;
    match status {
        TextStatus::Ready { model, .. } => Ok(model == wanted),
        TextStatus::Error { model, message } if model == wanted => Err(format!(
            "The writing helper could not start: {message}. Original text kept."
        )),
        _ => Ok(false),
    }
}

fn shortcut_keys_down() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    [0x11, 0x12, 0x54]
        .iter()
        .any(|vk| unsafe { GetAsyncKeyState(*vk) } < 0)
}

fn validate_selection(text: &str) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("Select the writing you want to touch up, then press Ctrl + Alt + T.".into());
    }
    if text.chars().count() > 3000 {
        return Err("Select up to 3,000 characters at a time. Original text kept.".into());
    }
    Ok(())
}

fn finish_error(app: &App, id: u64, error: String) {
    let mut s = lock(&app.touchup.inner);
    if s.preview.id == id {
        s.preview.phase = "error".into();
        s.preview.error = Some(error);
    }
}

pub fn preview(app: &App) -> Preview {
    lock(&app.touchup.inner).preview.clone()
}

#[tauri::command]
pub fn get_touchup(app: State<'_, Arc<App>>) -> Preview {
    lock(&app.touchup.inner).preview.clone()
}

#[tauri::command]
pub fn cancel_touchup(app: State<'_, Arc<App>>, id: u64) {
    let mut s = lock(&app.touchup.inner);
    if s.preview.id == id && s.preview.phase != "applying" {
        let next = s.preview.id.wrapping_add(1);
        *s = Session::default();
        s.preview.id = next;
    }
}

#[tauri::command]
pub async fn apply_touchup(app: State<'_, Arc<App>>, id: u64) -> Result<Preview, String> {
    let app = Arc::clone(&app);
    let (selection, suggestion) = {
        let mut s = lock(&app.touchup.inner);
        if s.preview.id != id || s.preview.phase != "review" {
            return Err(
                "This preview is no longer available. Select the text and try again.".into(),
            );
        }
        let selection = s
            .selection
            .clone()
            .ok_or("The original selection is unavailable")?;
        let suggestion = s.preview.suggestion.clone();
        s.preview.phase = "applying".into();
        (selection, suggestion)
    };
    app.broadcast();
    let hwnd = match app
        .handle
        .get_webview_window("main")
        .and_then(|w| w.hwnd().ok())
    {
        Some(hwnd) => hwnd.0 as isize,
        None => {
            finish_error(
                &app,
                id,
                "The review window is unavailable. Original text kept.".into(),
            );
            app.broadcast();
            return Ok(preview(&app));
        }
    };
    let result = tauri::async_runtime::spawn_blocking(move || {
        insert::replace_selection(&selection, &suggestion, hwnd)
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|r| r.map_err(|e| e.to_string()));
    let preview = {
        let mut s = lock(&app.touchup.inner);
        // Never retry automatically: a sent paste may already have replaced the selection.
        s.selection = None;
        match result {
            Ok(outcome) => {
                s.preview.phase = "done".into();
                s.preview.error = Some(match outcome {
                    insert::InsertOutcome::Inserted => "Replacement sent to the original app. Review it there.".into(),
                    insert::InsertOutcome::PasteSent => "Paste sent; delivery is unconfirmed. Check the original app before trying again.".into(),
                    insert::InsertOutcome::CopiedOnly(_) => "The revision was copied. Paste it into the intended selection yourself.".into(),
                });
            }
            Err(e) => {
                s.preview.phase = "error".into();
                s.preview.error = Some(e);
            }
        }
        s.preview.clone()
    };
    app.broadcast();
    if preview.phase == "error" {
        show(&app);
    }
    Ok(preview)
}

/// Update registration on its owning thread; disabled touch-up leaves the shortcut free.
pub fn update_shortcut(app: &App) {
    if let Some(tx) = lock(&app.touchup.shortcut).as_ref() {
        let _ = tx.send(app.settings().text.touchup);
    }
}

/// RegisterHotKey is independent of the dictation hook and refuses occupied shortcuts.
pub fn start_shortcut(app: &Arc<App>) {
    use std::sync::mpsc::{channel, RecvTimeoutError};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{PeekMessageW, MSG, PM_REMOVE, WM_HOTKEY};
    let (tx, rx) = channel();
    *lock(&app.touchup.shortcut) = Some(tx);
    let app = app.clone();
    std::thread::spawn(move || {
        let mut enabled = app.settings().text.touchup;
        let mut registered = false;
        let mut attempted = false;
        loop {
            if enabled && !attempted {
                // SAFETY: thread-owned hotkey; no HWND or pointers outlive this thread.
                match unsafe {
                    RegisterHotKey(
                        None,
                        0x4155,
                        MOD_CONTROL | MOD_ALT | MOD_NOREPEAT,
                        u32::from(b'T'),
                    )
                } {
                    Ok(()) => registered = true,
                    Err(e) => {
                        app.set_notice(format!(
                            "AI touch-up shortcut Ctrl + Alt + T is unavailable: {e}"
                        ));
                        app.broadcast();
                    }
                }
                attempted = true;
            }
            if !enabled && registered {
                let _ = unsafe { UnregisterHotKey(None, 0x4155) };
                registered = false;
            }
            let mut msg = MSG::default();
            while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
                if enabled && msg.message == WM_HOTKEY {
                    let app = app.clone();
                    std::thread::spawn(move || begin(&app));
                }
            }
            match rx.recv_timeout(std::time::Duration::from_millis(25)) {
                Ok(value) => {
                    enabled = value;
                    attempted = registered && enabled;
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        if registered {
            let _ = unsafe { UnregisterHotKey(None, 0x4155) };
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restarting_helper_waits_for_requested_model_and_reports_load_failure() {
        use crate::text_engine::TextStatus;
        assert!(!helper_ready(&TextStatus::Off, "qwen").unwrap());
        assert!(!helper_ready(
            &TextStatus::Loading {
                model: "qwen".into()
            },
            "qwen"
        )
        .unwrap());
        assert!(!helper_ready(
            &TextStatus::Ready {
                model: "old".into(),
                label: "old".into()
            },
            "qwen"
        )
        .unwrap());
        assert!(helper_ready(
            &TextStatus::Ready {
                model: "qwen".into(),
                label: "local".into()
            },
            "qwen"
        )
        .unwrap());
        assert!(helper_ready(
            &TextStatus::Error {
                model: "qwen".into(),
                message: "load failed".into()
            },
            "qwen"
        )
        .is_err());
    }

    #[test]
    fn empty_and_oversized_selections_are_rejected_before_generation() {
        assert!(validate_selection("  ").is_err());
        assert!(validate_selection(&"字".repeat(3001)).is_err());
        assert!(validate_selection(&"字".repeat(3000)).is_ok());
    }
    #[test]
    fn dictation_cannot_share_touchup_or_its_modifier_subset() {
        let chord = |names: &[&str]| {
            aural_platform::chord::Chord::parse(
                &names.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            )
            .unwrap()
        };
        assert!(validate_dictation_chord(&chord(&["Ctrl", "Alt", "T"])).is_err());
        assert!(validate_dictation_chord(&chord(&["Ctrl", "Alt"])).is_err());
        assert!(validate_dictation_chord(&chord(&["Ctrl", "Win"])).is_ok());
    }
}
