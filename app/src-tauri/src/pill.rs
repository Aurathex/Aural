//! The floating listening pill window. It must never take focus (typing would move
//! away from the user's app), never intercept clicks, and appear above the taskbar of
//! the monitor the user is working on.

use aural_core::settings::PillPosition;
use tauri::WebviewWindow;

/// Logical size and gap above the taskbar, in DIPs.
const WIDTH: i32 = 176;
const HEIGHT: i32 = 36;
const GAP: i32 = 48;

#[cfg(windows)]
mod win {
    use super::*;
    use windows::Win32::Foundation::{HWND, POINT};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, MONITORINFO,
        MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY,
    };
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, ShowWindow,
        GWL_EXSTYLE, HWND_TOPMOST, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, WS_EX_NOACTIVATE,
        WS_EX_TOOLWINDOW,
    };

    fn hwnd(w: &WebviewWindow) -> Option<HWND> {
        w.hwnd().ok().map(|h| HWND(h.0 as *mut _))
    }

    pub fn init(w: &WebviewWindow) {
        let _ = w.set_ignore_cursor_events(true);
        if let Some(h) = hwnd(w) {
            // SAFETY: valid top-level HWND owned by this process.
            unsafe {
                let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
                SetWindowLongPtrW(
                    h,
                    GWL_EXSTYLE,
                    ex | (WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0) as isize,
                );
            }
        }
    }

    pub fn show(w: &WebviewWindow, position: PillPosition) {
        let Some(h) = hwnd(w) else { return };
        // SAFETY: plain Win32 queries and a positioning call on our own window.
        unsafe {
            let fg = GetForegroundWindow();
            let monitor = if fg.0.is_null() {
                MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY)
            } else {
                MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST)
            };
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if !GetMonitorInfoW(monitor, &mut info).as_bool() {
                return;
            }
            let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
            let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
            let scale = |v: i32| v * dpi_x as i32 / 96;
            let work = info.rcWork;
            let (w_px, h_px) = (scale(WIDTH), scale(HEIGHT));
            let x = work.left + (work.right - work.left - w_px) / 2;
            let y = match position {
                PillPosition::Bottom => work.bottom - h_px - scale(GAP),
                PillPosition::Top => work.top + scale(GAP),
            };
            let _ = SetWindowPos(
                h,
                Some(HWND_TOPMOST),
                x,
                y,
                w_px,
                h_px,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
    }

    pub fn hide(w: &WebviewWindow) {
        if let Some(h) = hwnd(w) {
            // SAFETY: hiding our own window.
            let _ = unsafe { ShowWindow(h, SW_HIDE) };
        }
    }
}

#[cfg(windows)]
pub use win::{hide, init, show};
