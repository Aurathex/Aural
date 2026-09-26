//! Putting text into the focused app. `plan` is pure (which strategy, which paste
//! chord, how to transform the text); `execute` does the Win32 work.

use serde::Serialize;

/// The window that will receive the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetInfo {
    /// HWND as an integer; 0 when nothing has focus (lock screen, secure desktop).
    pub hwnd: isize,
    /// Executable name, e.g. "WindowsTerminal.exe".
    pub process: String,
    /// Window class name.
    pub class: String,
    /// The target runs elevated (admin). Windows blocks input from normal apps (UIPI).
    pub elevated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum PasteChord {
    CtrlV,
    CtrlShiftV,
    ShiftInsert,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Reason {
    FocusChanged,
    Elevated,
    NoTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Strategy {
    /// Put the text on the clipboard, press the paste chord, restore the clipboard.
    Paste(PasteChord),
    /// Type the text as Unicode key events (used when the clipboard holds something we
    /// could not restore, like an image).
    Type,
    /// Leave the text on the clipboard for the user to paste.
    ClipboardOnly(Reason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InsertPlan {
    pub strategy: Strategy,
    /// Terminals execute on newline, so newlines become spaces there.
    pub single_line: bool,
}

impl InsertPlan {
    pub fn apply(&self, text: &str) -> String {
        let t = text.trim();
        if self.single_line {
            t.split_whitespace().collect::<Vec<_>>().join(" ")
        } else {
            t.to_owned()
        }
    }
}

fn is_terminal(t: &TargetInfo) -> Option<PasteChord> {
    let p = t.process.to_ascii_lowercase();
    match p.as_str() {
        "windowsterminal.exe" => Some(PasteChord::CtrlShiftV),
        "mintty.exe" => Some(PasteChord::ShiftInsert),
        "conhost.exe" | "openconsole.exe" | "alacritty.exe" | "wezterm-gui.exe" | "cmd.exe"
        | "powershell.exe" | "pwsh.exe" => Some(PasteChord::CtrlV),
        _ if t.class == "ConsoleWindowClass" => Some(PasteChord::CtrlV),
        _ => None,
    }
}

/// Decide how to deliver text. `release_hwnd` is the window that had focus when the
/// user released the hotkey; if focus moved since, typing would land in the wrong
/// place, so the text goes to the clipboard instead.
pub fn plan(
    target: &TargetInfo,
    release_hwnd: isize,
    clipboard_restorable: bool,
    self_elevated: bool,
) -> InsertPlan {
    let terminal = is_terminal(target);
    let strategy = if target.hwnd == 0 {
        Strategy::ClipboardOnly(Reason::NoTarget)
    } else if target.hwnd != release_hwnd {
        Strategy::ClipboardOnly(Reason::FocusChanged)
    } else if target.elevated && !self_elevated {
        Strategy::ClipboardOnly(Reason::Elevated)
    } else if !clipboard_restorable {
        Strategy::Type
    } else {
        Strategy::Paste(terminal.unwrap_or(PasteChord::CtrlV))
    };
    InsertPlan {
        strategy,
        single_line: terminal.is_some(),
    }
}

/// UTF-16 code units, as sent with KEYEVENTF_UNICODE (surrogate pairs included).
pub fn utf16_units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

#[cfg(windows)]
pub use win::*;

#[cfg(windows)]
mod win {
    use super::*;
    use anyhow::{bail, Context, Result};
    use std::time::{Duration, Instant};
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, GlobalFree, HANDLE, HGLOBAL, HWND};
    use windows::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData,
        GetClipboardSequenceNumber, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows::Win32::System::Threading::{
        GetCurrentProcess, OpenProcess, OpenProcessToken, QueryFullProcessImageNameW,
        PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
        KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VIRTUAL_KEY,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetForegroundWindow, GetWindowThreadProcessId,
    };

    const CF_UNICODETEXT: u32 = 13;
    const CF_TEXT: u32 = 1;
    const CF_OEMTEXT: u32 = 7;
    const CF_LOCALE: u32 = 16;
    /// Unassigned virtual key: pressing it stops Win/Alt releases from opening menus.
    pub const MASK_VK: u16 = 0xE8;
    const MODIFIERS: [u16; 8] = [0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0x5B, 0x5C];

    pub fn foreground_window() -> isize {
        // SAFETY: no preconditions.
        unsafe { GetForegroundWindow() }.0 as isize
    }

    fn process_path(pid: u32) -> Option<String> {
        // SAFETY: handle closed below; buffer sized to its length.
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(
                h,
                PROCESS_NAME_WIN32,
                PWSTR(buf.as_mut_ptr()),
                &mut len,
            );
            let _ = CloseHandle(h);
            ok.ok()?;
            Some(String::from_utf16_lossy(&buf[..len as usize]))
        }
    }

    fn token_elevated(process: HANDLE) -> Option<bool> {
        // SAFETY: token handle closed below; TOKEN_ELEVATION is the documented size.
        unsafe {
            let mut token = HANDLE::default();
            OpenProcessToken(process, TOKEN_QUERY, &mut token).ok()?;
            let mut elev = TOKEN_ELEVATION::default();
            let mut ret = 0u32;
            let ok = GetTokenInformation(
                token,
                TokenElevation,
                Some(&mut elev as *mut _ as *mut _),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut ret,
            );
            let _ = CloseHandle(token);
            ok.ok()?;
            Some(elev.TokenIsElevated != 0)
        }
    }

    pub fn self_elevated() -> bool {
        // SAFETY: pseudo-handle, no close needed.
        token_elevated(unsafe { GetCurrentProcess() }).unwrap_or(false)
    }

    fn pid_elevated(pid: u32) -> bool {
        // SAFETY: handle closed below.
        unsafe {
            let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                // Cannot even query it: treat as protected/elevated.
                return true;
            };
            // Access denied opening an elevated process's token from a normal process.
            let e = token_elevated(h).unwrap_or(true);
            let _ = CloseHandle(h);
            e
        }
    }

    pub fn foreground_target() -> TargetInfo {
        let hwnd = foreground_window();
        if hwnd == 0 {
            return TargetInfo {
                hwnd: 0,
                process: String::new(),
                class: String::new(),
                elevated: false,
            };
        }
        let h = HWND(hwnd as *mut _);
        let mut pid = 0u32;
        // SAFETY: valid out pointer.
        unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
        let mut class = [0u16; 256];
        // SAFETY: buffer sized to its length.
        let n = unsafe { GetClassNameW(h, &mut class) } as usize;
        let process = process_path(pid)
            .and_then(|p| p.rsplit('\\').next().map(str::to_owned))
            .unwrap_or_default();
        TargetInfo {
            hwnd,
            process,
            class: String::from_utf16_lossy(&class[..n]),
            elevated: pid_elevated(pid),
        }
    }

    struct ClipboardGuard;
    impl Drop for ClipboardGuard {
        fn drop(&mut self) {
            // SAFETY: only constructed after a successful OpenClipboard.
            let _ = unsafe { CloseClipboard() };
        }
    }

    /// Clipboard managers can hold the clipboard briefly; retry for up to ~500 ms.
    fn open_clipboard() -> Result<ClipboardGuard> {
        let deadline = Instant::now() + Duration::from_millis(500);
        loop {
            // SAFETY: no owner window needed for our use.
            if unsafe { OpenClipboard(None) }.is_ok() {
                return Ok(ClipboardGuard);
            }
            if Instant::now() > deadline {
                bail!("the clipboard is busy");
            }
            std::thread::sleep(Duration::from_millis(15));
        }
    }

    /// What was on the clipboard before we used it.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct ClipboardSnapshot {
        pub text: Option<String>,
        /// False when it held formats we cannot put back (images, files, rich data).
        pub restorable: bool,
    }

    pub fn snapshot_clipboard() -> Result<ClipboardSnapshot> {
        let _g = open_clipboard()?;
        let mut restorable = true;
        let mut fmt = 0u32;
        loop {
            // SAFETY: clipboard is open.
            fmt = unsafe { EnumClipboardFormats(fmt) };
            if fmt == 0 {
                break;
            }
            if !matches!(fmt, CF_UNICODETEXT | CF_TEXT | CF_OEMTEXT | CF_LOCALE) {
                restorable = false;
            }
        }
        Ok(ClipboardSnapshot {
            text: read_text_locked(),
            restorable,
        })
    }

    fn read_text_locked() -> Option<String> {
        // SAFETY: clipboard is open; the handle is owned by the clipboard and only
        // locked for the duration of the copy.
        unsafe {
            let h = GetClipboardData(CF_UNICODETEXT).ok()?;
            let g = HGLOBAL(h.0);
            let p = GlobalLock(g) as *const u16;
            if p.is_null() {
                return None;
            }
            let mut len = 0usize;
            while *p.add(len) != 0 {
                len += 1;
            }
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, len));
            let _ = GlobalUnlock(g);
            Some(s)
        }
    }

    pub fn read_clipboard_text() -> Option<String> {
        let _g = open_clipboard().ok()?;
        read_text_locked()
    }

    fn global_from(bytes: &[u8]) -> Result<HGLOBAL> {
        // SAFETY: allocate, copy within bounds, unlock; ownership passes to the
        // clipboard on successful SetClipboardData.
        unsafe {
            let g = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1))?;
            let p = GlobalLock(g) as *mut u8;
            if p.is_null() {
                let _ = GlobalFree(Some(g));
                bail!("GlobalLock failed");
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
            let _ = GlobalUnlock(g);
            Ok(g)
        }
    }

    fn register(name: &str) -> u32 {
        let w: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        // SAFETY: NUL-terminated wide string.
        unsafe { RegisterClipboardFormatW(PCWSTR(w.as_ptr())) }
    }

    /// Put text on the clipboard. With `private`, also mark it so Windows clipboard
    /// history (Win+V), cloud clipboard and clipboard monitors skip it.
    pub fn set_clipboard_text(text: &str, private: bool) -> Result<u32> {
        write_clipboard_text(text, private)?;
        // Read after CloseClipboard: Windows bumps the sequence number when the
        // clipboard closes, so a value read while it is open never matches later.
        // SAFETY: no preconditions.
        Ok(unsafe { GetClipboardSequenceNumber() })
    }

    fn write_clipboard_text(text: &str, private: bool) -> Result<()> {
        let _g = open_clipboard()?;
        // SAFETY: clipboard is open.
        unsafe { EmptyClipboard() }.context("emptying clipboard")?;
        let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
        let bytes: Vec<u8> = wide.iter().flat_map(|u| u.to_le_bytes()).collect();
        let g = global_from(&bytes)?;
        // SAFETY: clipboard open; `g` ownership transfers on success.
        unsafe { SetClipboardData(CF_UNICODETEXT, Some(HANDLE(g.0))) }
            .context("setting clipboard text")?;
        if private {
            let zero = 0u32.to_le_bytes();
            for (name, data) in [
                ("ExcludeClipboardContentFromMonitorProcessing", &zero[..]),
                ("CanIncludeInClipboardHistory", &zero[..]),
                ("CanUploadToCloudClipboard", &zero[..]),
            ] {
                let fmt = register(name);
                if fmt != 0 {
                    if let Ok(g) = global_from(data) {
                        // SAFETY: as above.
                        let _ = unsafe { SetClipboardData(fmt, Some(HANDLE(g.0))) };
                    }
                }
            }
        }
        Ok(())
    }

    fn key(vk: u16, up: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    wScan: 0,
                    dwFlags: if up {
                        KEYEVENTF_KEYUP
                    } else {
                        KEYBD_EVENT_FLAGS(0)
                    },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    fn unicode(unit: u16, up: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: unit,
                    dwFlags: if up {
                        KEYEVENTF_UNICODE | KEYEVENTF_KEYUP
                    } else {
                        KEYEVENTF_UNICODE
                    },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    fn send(inputs: &[INPUT]) -> Result<()> {
        // SAFETY: slice of fully initialised INPUTs.
        let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
        if sent as usize != inputs.len() {
            bail!("Windows accepted {sent} of {} key events", inputs.len());
        }
        Ok(())
    }

    fn held_modifiers() -> Vec<u16> {
        MODIFIERS
            .iter()
            .copied()
            // SAFETY: no preconditions.
            .filter(|&vk| unsafe { GetAsyncKeyState(vk as i32) } as u16 & 0x8000 != 0)
            .collect()
    }

    /// Make sure no modifier is logically down, or Ctrl+V becomes Ctrl+Win+V. Waits up
    /// to 300 ms for the user to let go, then releases stragglers synthetically and
    /// presses the mask key so a Win/Alt release cannot open Start or a menu bar.
    pub fn release_modifiers() -> Result<()> {
        let deadline = Instant::now() + Duration::from_millis(300);
        while !held_modifiers().is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        let held = held_modifiers();
        if !held.is_empty() {
            let mut inputs = vec![key(MASK_VK, false), key(MASK_VK, true)];
            inputs.extend(held.iter().map(|&vk| key(vk, true)));
            send(&inputs)?;
        }
        Ok(())
    }

    pub fn send_mask_key() -> Result<()> {
        send(&[key(MASK_VK, false), key(MASK_VK, true)])
    }

    fn paste_inputs(chord: PasteChord) -> Vec<INPUT> {
        let (mods, k): (&[u16], u16) = match chord {
            PasteChord::CtrlV => (&[0xA2], 0x56),
            PasteChord::CtrlShiftV => (&[0xA2, 0xA0], 0x56),
            PasteChord::ShiftInsert => (&[0xA0], 0x2D),
        };
        let mut v: Vec<INPUT> = mods.iter().map(|&m| key(m, false)).collect();
        v.push(key(k, false));
        v.push(key(k, true));
        v.extend(mods.iter().rev().map(|&m| key(m, true)));
        v
    }

    pub fn type_text(text: &str) -> Result<()> {
        let mut inputs = Vec::new();
        for u in utf16_units(text) {
            inputs.push(unicode(u, false));
            inputs.push(unicode(u, true));
        }
        // One call, so the user's own typing cannot interleave.
        send(&inputs)
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
    pub enum InsertOutcome {
        Inserted,
        /// Text left on the clipboard for the user; carries why.
        CopiedOnly(Reason),
    }

    /// Deliver `text` to the focused app. The pasted text is never left in clipboard
    /// history, and the user's previous clipboard text is restored afterwards.
    pub fn insert(text: &str, release_hwnd: isize) -> Result<InsertOutcome> {
        let target = foreground_target();
        let snapshot = snapshot_clipboard().unwrap_or(ClipboardSnapshot {
            text: None,
            restorable: false,
        });
        let p = plan(&target, release_hwnd, snapshot.restorable, self_elevated());
        let text = p.apply(text);
        if text.is_empty() {
            return Ok(InsertOutcome::Inserted);
        }
        match p.strategy {
            Strategy::ClipboardOnly(reason) => {
                set_clipboard_text(&text, false)?;
                Ok(InsertOutcome::CopiedOnly(reason))
            }
            Strategy::Type => {
                release_modifiers()?;
                type_text(&text)?;
                Ok(InsertOutcome::Inserted)
            }
            Strategy::Paste(chord) => {
                let ours = set_clipboard_text(&text, true)?;
                release_modifiers()?;
                send(&paste_inputs(chord))?;
                // The target reads the clipboard asynchronously; give it time before
                // restoring, and only restore if nobody else changed it meanwhile.
                std::thread::sleep(Duration::from_millis(250));
                // SAFETY: no preconditions.
                if unsafe { GetClipboardSequenceNumber() } == ours {
                    match snapshot.text {
                        Some(prev) => {
                            set_clipboard_text(&prev, false)?;
                        }
                        None => {
                            let _g = open_clipboard()?;
                            // SAFETY: clipboard open.
                            let _ = unsafe { EmptyClipboard() };
                        }
                    }
                }
                Ok(InsertOutcome::Inserted)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(process: &str, class: &str) -> TargetInfo {
        TargetInfo {
            hwnd: 42,
            process: process.into(),
            class: class.into(),
            elevated: false,
        }
    }

    #[test]
    fn normal_app_gets_ctrl_v_paste() {
        let p = plan(&target("notepad.exe", "Notepad"), 42, true, false);
        assert_eq!(p.strategy, Strategy::Paste(PasteChord::CtrlV));
        assert_eq!(p.apply("Hello world."), "Hello world.");
    }

    #[test]
    fn windows_terminal_uses_ctrl_shift_v_and_never_sends_newlines() {
        let p = plan(
            &target("WindowsTerminal.exe", "CASCADIA_HOSTING_WINDOW_CLASS"),
            42,
            true,
            false,
        );
        assert_eq!(p.strategy, Strategy::Paste(PasteChord::CtrlShiftV));
        assert_eq!(p.apply("rm -rf build\nnpm test\n"), "rm -rf build npm test");
    }

    #[test]
    fn classic_console_and_mintty_are_terminals() {
        let conhost = plan(
            &target("conhost.exe", "ConsoleWindowClass"),
            42,
            true,
            false,
        );
        assert_eq!(conhost.strategy, Strategy::Paste(PasteChord::CtrlV));
        assert_eq!(conhost.apply("a\nb"), "a b");
        let mintty = plan(&target("mintty.exe", "mintty"), 42, true, false);
        assert_eq!(mintty.strategy, Strategy::Paste(PasteChord::ShiftInsert));
    }

    #[test]
    fn focus_changed_falls_back_to_clipboard_only() {
        let p = plan(&target("notepad.exe", "Notepad"), 7, true, false);
        assert_eq!(p.strategy, Strategy::ClipboardOnly(Reason::FocusChanged));
    }

    #[test]
    fn elevated_target_is_clipboard_only_unless_we_are_elevated() {
        let mut t = target("regedit.exe", "RegEdit_RegEdit");
        t.elevated = true;
        assert_eq!(
            plan(&t, 42, true, false).strategy,
            Strategy::ClipboardOnly(Reason::Elevated)
        );
        assert_eq!(
            plan(&t, 42, true, true).strategy,
            Strategy::Paste(PasteChord::CtrlV)
        );
    }

    #[test]
    fn nonrestorable_clipboard_switches_to_typing() {
        let p = plan(&target("notepad.exe", "Notepad"), 42, false, false);
        assert_eq!(p.strategy, Strategy::Type);
    }

    #[test]
    fn no_foreground_window_is_clipboard_only() {
        let mut t = target("", "");
        t.hwnd = 0;
        assert_eq!(
            plan(&t, 0, true, false).strategy,
            Strategy::ClipboardOnly(Reason::NoTarget)
        );
    }

    #[test]
    fn text_is_trimmed_everywhere() {
        let p = plan(&target("notepad.exe", "Notepad"), 42, true, false);
        assert_eq!(p.apply("  hi there \n"), "hi there");
    }

    #[test]
    fn unicode_typing_encodes_surrogate_pairs() {
        let units = utf16_units("a👋é");
        assert_eq!(units, vec![0x61, 0xD83D, 0xDC4B, 0xE9]);
    }
}
