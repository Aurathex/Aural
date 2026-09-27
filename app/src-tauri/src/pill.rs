//! The floating listening pill window. It must never take focus (typing would move
//! away from the user's app), never intercept clicks, and appear above the taskbar of
//! the monitor the user is working on.

use aural_core::settings::PillPosition;
use tauri::WebviewWindow;

/// The pill's own size (matches `.pill` in Pill.svelte), in DIPs.
const PILL_WIDTH: i32 = 176;
const PILL_HEIGHT: i32 = 36;
/// Transparent, click-through room around the pill inside its window, so the entry
/// scale, the error nudge and the exit animation are never cut off by the window edge.
const MARGIN: i32 = 8;
/// Distance between the pill and the taskbar (or the top of the screen).
const GAP: i32 = 48;
/// Live text: a caption above the pill (below it when the pill is at the top). Matches
/// `.caption` in Pill.svelte.
const CAPTION_WIDTH: i32 = 440;
const CAPTION_HEIGHT: i32 = 58;
const CAPTION_GAP: i32 = 8;

/// A screen rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// Where the pill window goes on a monitor's work area (the screen minus the taskbar):
/// centred horizontally, `GAP` from the bottom or top, and always fully inside the
/// work area. With `caption` the window also holds the live-text caption, on the side
/// away from the screen edge, and the pill itself stays where it always is.
pub fn place(work: Rect, dpi: u32, position: PillPosition, caption: bool) -> Rect {
    let scale = |v: i32| v * dpi as i32 / 96;
    let (inner_w, inner_h) = if caption {
        (
            CAPTION_WIDTH.max(PILL_WIDTH),
            PILL_HEIGHT + CAPTION_GAP + CAPTION_HEIGHT,
        )
    } else {
        (PILL_WIDTH, PILL_HEIGHT)
    };
    let (w, h) = (scale(inner_w + 2 * MARGIN), scale(inner_h + 2 * MARGIN));
    let (gap, margin) = (scale(GAP), scale(MARGIN));
    let x = work.left + (work.right - work.left - w) / 2;
    let y = match position {
        // GAP is measured to the pill itself, not to its transparent margin.
        PillPosition::Bottom => work.bottom - gap - h + margin,
        PillPosition::Top => work.top + gap - margin,
    };
    let clamp = |v: i32, lo: i32, hi: i32| v.min(hi).max(lo);
    let x = clamp(x, work.left, work.right - w);
    let y = clamp(y, work.top, work.bottom - h);
    Rect {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    }
}

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

    pub fn show(w: &WebviewWindow, position: PillPosition, caption: bool) {
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
            let work = info.rcWork;
            let r = place(
                Rect {
                    left: work.left,
                    top: work.top,
                    right: work.right,
                    bottom: work.bottom,
                },
                dpi_x,
                position,
                caption,
            );
            let _ = SetWindowPos(
                h,
                Some(HWND_TOPMOST),
                r.left,
                r.top,
                r.right - r.left,
                r.bottom - r.top,
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

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Rect = Rect {
        left: 0,
        top: 0,
        right: 2048,
        bottom: 1232,
    };

    fn inside(r: Rect, work: Rect, margin: i32) -> bool {
        r.left >= work.left + margin
            && r.top >= work.top + margin
            && r.right <= work.right - margin
            && r.bottom <= work.bottom - margin
    }

    #[test]
    fn live_text_caption_sits_above_a_bottom_pill_without_moving_the_pill() {
        let plain = place(WORK, 96, PillPosition::Bottom, false);
        let live = place(WORK, 96, PillPosition::Bottom, true);
        // Same pill spot: the window's bottom edge (pill + margin) is unchanged.
        assert_eq!(live.bottom, plain.bottom);
        assert_eq!(
            live.bottom - live.top,
            PILL_HEIGHT + CAPTION_GAP + CAPTION_HEIGHT + 2 * MARGIN
        );
        assert_eq!(live.right - live.left, CAPTION_WIDTH + 2 * MARGIN);
        assert_eq!((live.left + live.right) / 2, (plain.left + plain.right) / 2);
    }

    #[test]
    fn live_text_caption_sits_below_a_top_pill() {
        let plain = place(WORK, 96, PillPosition::Top, false);
        let live = place(WORK, 96, PillPosition::Top, true);
        assert_eq!(live.top, plain.top);
        assert!(live.bottom > plain.bottom);
    }

    #[test]
    fn live_text_window_stays_on_screen_at_every_scale() {
        for dpi in [96, 120, 144, 192] {
            for pos in [PillPosition::Bottom, PillPosition::Top] {
                let r = place(WORK, dpi, pos, true);
                assert!(inside(r, WORK, 0), "{dpi} {pos:?}: {r:?}");
            }
        }
    }

    #[test]
    fn window_leaves_room_around_the_pill_for_its_animations() {
        let r = place(WORK, 96, PillPosition::Bottom, false);
        assert_eq!(r.right - r.left, PILL_WIDTH + 2 * MARGIN);
        assert_eq!(r.bottom - r.top, PILL_HEIGHT + 2 * MARGIN);
    }

    #[test]
    fn pill_is_centred_and_fully_on_screen_at_every_scale() {
        for dpi in [96, 120, 144, 192] {
            for pos in [PillPosition::Bottom, PillPosition::Top] {
                let r = place(WORK, dpi, pos, false);
                assert!(inside(r, WORK, 0), "{dpi} {pos:?}: {r:?}");
                let centre = (r.left + r.right) / 2;
                assert!((centre - 1024).abs() <= 1, "{dpi}: centre {centre}");
            }
        }
    }

    #[test]
    fn a_monitor_that_does_not_start_at_zero_is_respected() {
        // A second monitor to the left of the primary one, taskbar at its bottom.
        let work = Rect {
            left: -1920,
            top: 0,
            right: 0,
            bottom: 1040,
        };
        let r = place(work, 96, PillPosition::Bottom, false);
        assert!(inside(r, work, 0), "{r:?}");
    }

    #[test]
    fn a_work_area_narrower_than_the_pill_still_keeps_it_on_screen() {
        let work = Rect {
            left: 100,
            top: 0,
            right: 250,
            bottom: 600,
        };
        let r = place(work, 96, PillPosition::Bottom, false);
        assert_eq!(
            r.left, work.left,
            "pinned to the left edge rather than cut off on both"
        );
    }
}
