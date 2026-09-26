//! System tray icon: open settings, paste the last transcript, quit.

use crate::state::{lock, App};
use std::sync::Arc;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

pub fn show_settings(handle: &AppHandle) {
    if let Some(w) = handle.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

pub fn build(app: &Arc<App>, hotkey: &str) -> tauri::Result<()> {
    let h = &app.handle;
    let open = MenuItem::with_id(h, "open", "Open Aural", true, None::<&str>)?;
    let paste = MenuItem::with_id(h, "paste", "Copy last transcript", false, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(h)?;
    let quit = MenuItem::with_id(h, "quit", "Quit Aural", true, None::<&str>)?;
    let menu = Menu::with_items(h, &[&open, &paste, &sep, &quit])?;
    *lock(&app.tray_paste) = Some(paste);

    let state = app.clone();
    TrayIconBuilder::with_id("aural")
        .icon(h.default_window_icon().cloned().ok_or(tauri::Error::InvalidIcon(
            std::io::Error::other("missing app icon"),
        ))?)
        .tooltip(format!("Aural — hold {hotkey} to dictate"))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |handle, event| match event.id().as_ref() {
            "open" => show_settings(handle),
            "paste" => {
                if let Some(text) = lock(&state.last_transcript).clone() {
                    let _ = aural_platform::insert::set_clipboard_text(&text, false);
                }
            }
            "quit" => handle.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_settings(tray.app_handle());
            }
        })
        .build(h)?;
    Ok(())
}
