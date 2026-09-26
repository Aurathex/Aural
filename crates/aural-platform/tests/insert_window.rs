//! End-to-end insertion into a real Win32 Edit control on the desktop. It briefly takes
//! focus and uses the clipboard (the user's clipboard text is restored afterwards).
//! Run with `cargo test -p aural-platform --test insert_window -- --ignored --test-threads=1`.
#![cfg(windows)]

use aural_platform::insert::{
    insert, read_clipboard_text, set_clipboard_text, InsertOutcome, Reason,
};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::WindowsAndMessaging::*;

struct TestWindow {
    hwnd: isize,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl TestWindow {
    fn open() -> Self {
        let (tx, rx) = mpsc::channel();
        let thread = std::thread::spawn(move || unsafe {
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("EDIT"),
                w!(""),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE | WINDOW_STYLE(0x0004 /* ES_MULTILINE */),
                100,
                100,
                480,
                200,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            tx.send(hwnd.0 as isize).unwrap();
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        });
        Self {
            hwnd: rx.recv().unwrap(),
            thread: Some(thread),
        }
    }

    fn h(&self) -> HWND {
        HWND(self.hwnd as *mut _)
    }

    /// Windows restricts focus stealing; attaching to the current foreground thread's
    /// input queue is the documented-behaviour way for a test to take focus.
    fn focus(&self) -> bool {
        unsafe {
            let fg = GetForegroundWindow();
            let fg_thread = GetWindowThreadProcessId(fg, None);
            let me = GetCurrentThreadId();
            let _ = AttachThreadInput(me, fg_thread, true);
            let _ = ShowWindow(self.h(), SW_RESTORE);
            let _ = BringWindowToTop(self.h());
            let ok = SetForegroundWindow(self.h()).as_bool();
            let _ = AttachThreadInput(me, fg_thread, false);
            let deadline = Instant::now() + Duration::from_secs(2);
            while GetForegroundWindow().0 as isize != self.hwnd && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            ok && GetForegroundWindow().0 as isize == self.hwnd
        }
    }

    fn text(&self) -> String {
        let mut buf = vec![0u16; 1024];
        let n = unsafe {
            SendMessageW(
                self.h(),
                WM_GETTEXT,
                Some(WPARAM(buf.len())),
                Some(LPARAM(buf.as_mut_ptr() as isize)),
            )
        };
        String::from_utf16_lossy(&buf[..n.0 as usize])
    }
}

impl Drop for TestWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = PostMessageW(Some(self.h()), WM_CLOSE, WPARAM(0), LPARAM(0));
            let tid = GetWindowThreadProcessId(self.h(), None);
            let _ = windows::Win32::UI::WindowsAndMessaging::PostThreadMessageW(
                tid,
                WM_QUIT,
                WPARAM(0),
                LPARAM(0),
            );
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct ClipboardRestore(Option<String>);
impl Drop for ClipboardRestore {
    fn drop(&mut self) {
        if let Some(t) = &self.0 {
            let _ = set_clipboard_text(t, false);
        }
    }
}

/// One window for both scenarios: Windows' focus-steal protection makes taking focus a
/// second time (after the first test window closes) unreliable from a test process.
#[test]
#[ignore]
fn inserts_into_a_real_edit_control() {
    let _restore = ClipboardRestore(read_clipboard_text());
    let win = TestWindow::open();
    assert!(win.focus(), "test window could not take focus");

    // 1. Focus moved between hotkey release and insertion: never type, copy instead.
    let out = insert("not for this window", win.hwnd + 1).unwrap();
    assert_eq!(out, InsertOutcome::CopiedOnly(Reason::FocusChanged));
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(win.text(), "");
    assert_eq!(
        read_clipboard_text().as_deref(),
        Some("not for this window")
    );

    // 2. Normal paste: trimmed text arrives (emoji and accents intact) and the
    //    previous clipboard text comes back.
    set_clipboard_text("SENTINEL", false).unwrap();
    let out = insert("  Hello from Aural 👋 café.  ", win.hwnd).unwrap();
    assert_eq!(out, InsertOutcome::Inserted);
    let deadline = Instant::now() + Duration::from_secs(2);
    while win.text().is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(win.text(), "Hello from Aural 👋 café.");
    assert_eq!(
        read_clipboard_text().as_deref(),
        Some("SENTINEL"),
        "clipboard not restored"
    );
}
