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
    /// The paste was sent but the app never read the clipboard (it ignores Ctrl+V,
    /// or is too slow, e.g. a frozen remote session).
    PasteIgnored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Strategy {
    /// Put the text on the clipboard, press the paste chord, restore the clipboard.
    Paste(PasteChord),
    /// Type the text as Unicode key events. Last resort, only when the clipboard could
    /// not be saved (busy, or holding data Aural cannot copy): some apps, Windows 11
    /// Notepad among them, drop most of a long burst of typed characters.
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
            // Keep spaces in quoted arguments intact. Line breaks and control keys
            // must never submit a prompt, run a command, or trigger terminal actions.
            t.replace("\r\n", "\n")
                .chars()
                .filter_map(|c| match c {
                    '\n' | '\r' | '\t' | '\u{2028}' | '\u{2029}' => Some(' '),
                    c if c.is_control() => None,
                    c => Some(c),
                })
                .collect()
        } else {
            t.to_owned()
        }
    }
}

fn is_terminal(t: &TargetInfo) -> Option<PasteChord> {
    let p = t.process.to_ascii_lowercase();
    match p.as_str() {
        "windowsterminal.exe"
        | "alacritty.exe"
        | "wezterm-gui.exe"
        | "ghostty.exe"
        | "warp.exe" => Some(PasteChord::CtrlShiftV),
        "mintty.exe" => Some(PasteChord::ShiftInsert),
        "conhost.exe" | "openconsole.exe" | "cmd.exe" | "powershell.exe" | "pwsh.exe" => {
            Some(PasteChord::CtrlV)
        }
        _ if t
            .class
            .eq_ignore_ascii_case("CASCADIA_HOSTING_WINDOW_CLASS") =>
        {
            Some(PasteChord::CtrlShiftV)
        }
        _ if t.class.eq_ignore_ascii_case("ConsoleWindowClass") => Some(PasteChord::CtrlV),
        _ => None,
    }
}

/// How one clipboard format is saved while Aural pastes, so it can be put back after.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveKind {
    /// A memory block: text, HTML, RTF, images (DIB), copied files, apps' own formats.
    Bytes,
    /// An enhanced metafile (drawings from Office and others).
    EnhMetafile,
    /// Windows makes this format from another one on the clipboard; not saved itself.
    Synthesized,
    /// A handle Aural cannot copy (a bitmap or palette without a DIB, owner-drawn or
    /// private data).
    Unsupported,
}

/// How to save `format`, given every format on the clipboard (`present`).
pub fn save_kind(format: u32, present: &[u32]) -> SaveKind {
    const CF_DIB: u32 = 8;
    const CF_UNICODETEXT: u32 = 13;
    const CF_ENHMETAFILE: u32 = 14;
    const CF_DIBV5: u32 = 17;
    let has = |f: u32| present.contains(&f);
    match format {
        // CF_TEXT, CF_OEMTEXT from Unicode text.
        1 | 7 if has(CF_UNICODETEXT) => SaveKind::Synthesized,
        // CF_BITMAP, CF_PALETTE from a DIB.
        2 | 9 if has(CF_DIB) || has(CF_DIBV5) => SaveKind::Synthesized,
        // CF_METAFILEPICT from an enhanced metafile.
        3 if has(CF_ENHMETAFILE) => SaveKind::Synthesized,
        CF_ENHMETAFILE => SaveKind::EnhMetafile,
        // Bitmap, metafile picture and palette handles on their own; owner-display
        // formats; private and GDI-object ranges.
        2 | 3 | 9 | 0x80 | 0x82 | 0x83 | 0x8E | 0x200..=0x3FF => SaveKind::Unsupported,
        _ => SaveKind::Bytes,
    }
}

/// Decide how to deliver text. `release_hwnd` is the window that had focus when the
/// user released the hotkey; if focus moved since, typing would land in the wrong
/// place, so the text goes to the clipboard instead. `clipboard_saved`: the whole
/// clipboard was saved, so pasting can put it back afterwards.
pub fn plan(
    target: &TargetInfo,
    release_hwnd: isize,
    clipboard_saved: bool,
    self_elevated: bool,
) -> InsertPlan {
    plan_with_paste(target, release_hwnd, clipboard_saved, self_elevated, None)
}

/// An explicit shortcut supports embedded terminals without guessing whether an
/// editor or its terminal has focus. Explicit overrides also use single-line text.
pub fn plan_with_paste(
    target: &TargetInfo,
    release_hwnd: isize,
    clipboard_saved: bool,
    self_elevated: bool,
    paste: Option<PasteChord>,
) -> InsertPlan {
    let terminal = paste.or_else(|| is_terminal(target));
    let strategy = if target.hwnd == 0 {
        Strategy::ClipboardOnly(Reason::NoTarget)
    } else if target.hwnd != release_hwnd {
        Strategy::ClipboardOnly(Reason::FocusChanged)
    } else if target.elevated && !self_elevated {
        Strategy::ClipboardOnly(Reason::Elevated)
    } else if !clipboard_saved {
        if terminal.is_some() {
            Strategy::ClipboardOnly(Reason::PasteIgnored)
        } else {
            Strategy::Type
        }
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
    /// How long an app may take to read pasted text (remote desktops can be slow).
    const PASTE_TIMEOUT: Duration = Duration::from_secs(3);
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

    struct ClipboardGuard {
        // Keep a real owner alive until after CloseClipboard. EmptyClipboard with
        // a NULL owner makes subsequent SetClipboardData fail on Windows.
        _owner: Option<OwnerWindow>,
    }
    impl Drop for ClipboardGuard {
        fn drop(&mut self) {
            // SAFETY: only constructed after a successful OpenClipboard.
            let _ = unsafe { CloseClipboard() };
        }
    }

    /// Clipboard managers can hold the clipboard briefly; retry for up to ~500 ms.
    fn open_clipboard() -> Result<ClipboardGuard> {
        let owner = OwnerWindow::create()?;
        let deadline = Instant::now() + Duration::from_millis(500);
        loop {
            // SAFETY: our message-only window stays alive in the guard.
            if unsafe { OpenClipboard(Some(owner.0)) }.is_ok() {
                return Ok(ClipboardGuard {
                    _owner: Some(owner),
                });
            }
            if Instant::now() > deadline {
                bail!("the clipboard is busy");
            }
            // A clipboard watcher may be waiting for this thread to render the
            // previous owner's text. Dispatch it before retrying OpenClipboard.
            owner.pump();
            std::thread::sleep(Duration::from_millis(15));
        }
    }

    /// A clipboard bigger than this is not saved (typing is used instead).
    const MAX_SAVED_BYTES: usize = 64 << 20;
    const CF_ENHMETAFILE: u32 = 14;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Saved {
        format: u32,
        metafile: bool,
        data: Vec<u8>,
    }

    /// Everything that was on the clipboard before Aural used it, in the order the
    /// source app offered it.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct ClipboardSnapshot {
        items: Vec<Saved>,
        /// Every format was saved, so the clipboard can be put back as it was.
        pub complete: bool,
    }

    impl ClipboardSnapshot {
        /// A clipboard holding just `text`.
        pub fn text(text: &str) -> Self {
            let bytes = text
                .encode_utf16()
                .chain(Some(0))
                .flat_map(|u| u.to_le_bytes())
                .collect();
            Self::from_items(vec![(CF_UNICODETEXT, bytes)])
        }

        /// A clipboard holding these memory-block formats.
        pub fn from_items(items: Vec<(u32, Vec<u8>)>) -> Self {
            Self {
                items: items
                    .into_iter()
                    .map(|(format, data)| Saved {
                        format,
                        metafile: false,
                        data,
                    })
                    .collect(),
                complete: true,
            }
        }

        pub fn bytes(&self, format: u32) -> Option<&[u8]> {
            self.items
                .iter()
                .find(|s| s.format == format)
                .map(|s| s.data.as_slice())
        }

        pub fn byte_items(&self) -> impl Iterator<Item = (u32, &[u8])> {
            self.items
                .iter()
                .filter(|s| !s.metafile)
                .map(|s| (s.format, s.data.as_slice()))
        }
    }

    /// Save the whole clipboard: text, HTML, RTF, images, copied files, drawings and
    /// apps' own formats. `complete` is false when some of it could not be saved.
    pub fn snapshot_clipboard() -> Result<ClipboardSnapshot> {
        let _g = open_clipboard()?;
        let mut present = Vec::new();
        let mut fmt = 0u32;
        loop {
            // SAFETY: clipboard is open.
            fmt = unsafe { EnumClipboardFormats(fmt) };
            if fmt == 0 {
                break;
            }
            present.push(fmt);
        }
        let mut items = Vec::new();
        let mut complete = true;
        let mut total = 0usize;
        for &format in &present {
            // A format the source can no longer produce (GetClipboardData fails) could
            // not be pasted by anyone either; it is skipped rather than blocking.
            let saved = match save_kind(format, &present) {
                SaveKind::Synthesized => continue,
                SaveKind::Unsupported => {
                    complete = false;
                    continue;
                }
                SaveKind::Bytes => read_bytes_locked(format).map(|data| Saved {
                    format,
                    metafile: false,
                    data,
                }),
                SaveKind::EnhMetafile => read_metafile_locked().map(|data| Saved {
                    format,
                    metafile: true,
                    data,
                }),
            };
            if let Some(s) = saved {
                total += s.data.len();
                items.push(s);
            }
            if total > MAX_SAVED_BYTES {
                complete = false;
                break;
            }
        }
        Ok(ClipboardSnapshot { items, complete })
    }

    fn read_bytes_locked(format: u32) -> Option<Vec<u8>> {
        use windows::Win32::System::Memory::GlobalSize;
        // SAFETY: clipboard is open; the handle belongs to the clipboard and is only
        // locked while copied. A handle that is not a memory block has size 0.
        unsafe {
            let h = GetClipboardData(format).ok()?;
            let g = HGLOBAL(h.0);
            let size = GlobalSize(g);
            if size == 0 {
                return None;
            }
            let p = GlobalLock(g) as *const u8;
            if p.is_null() {
                return None;
            }
            let data = std::slice::from_raw_parts(p, size).to_vec();
            let _ = GlobalUnlock(g);
            Some(data)
        }
    }

    fn read_metafile_locked() -> Option<Vec<u8>> {
        use windows::Win32::Graphics::Gdi::{GetEnhMetaFileBits, HENHMETAFILE};
        // SAFETY: clipboard is open; the metafile belongs to the clipboard and is only
        // read.
        unsafe {
            let h = HENHMETAFILE(GetClipboardData(CF_ENHMETAFILE).ok()?.0);
            let size = GetEnhMetaFileBits(h, None) as usize;
            if size == 0 {
                return None;
            }
            let mut data = vec![0u8; size];
            (GetEnhMetaFileBits(h, Some(&mut data)) as usize == size).then_some(data)
        }
    }

    /// Put saved formats on the (open, emptied) clipboard.
    fn write_saved_locked(snapshot: &ClipboardSnapshot) -> Result<()> {
        use windows::Win32::Graphics::Gdi::SetEnhMetaFileBits;
        for s in &snapshot.items {
            // SAFETY: clipboard is open; ownership of the handle passes to it.
            unsafe {
                if s.metafile {
                    let h = SetEnhMetaFileBits(&s.data);
                    if !h.is_invalid() {
                        let _ = SetClipboardData(s.format, Some(HANDLE(h.0)));
                    }
                } else {
                    let g = global_from(&s.data)?;
                    SetClipboardData(s.format, Some(HANDLE(g.0)))
                        .with_context(|| format!("restoring clipboard format {}", s.format))?;
                }
            }
        }
        Ok(())
    }

    /// Replace the clipboard with `snapshot`.
    pub fn restore_clipboard(snapshot: &ClipboardSnapshot) -> Result<()> {
        let _g = open_clipboard()?;
        // SAFETY: clipboard is open.
        unsafe { EmptyClipboard() }.context("emptying clipboard")?;
        write_saved_locked(snapshot)
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

    pub fn register_format(name: &str) -> u32 {
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
        write_clipboard_text_locked(text, private)
    }

    fn write_clipboard_text_locked(text: &str, private: bool) -> Result<()> {
        // SAFETY: caller holds the clipboard open.
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
                let fmt = register_format(name);
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

    /// Recover failed delivery only if nobody changed the original clipboard.
    /// The comparison and write share the clipboard lock, preventing a user-copy race.
    pub fn recover_insert_error(error: &InsertError) -> Result<bool> {
        let _g = open_clipboard()?;
        if unsafe { GetClipboardSequenceNumber() } != error.recovery_sequence {
            // An interrupted paste may already have left normalized recovery text.
            // Report availability without rewriting it or any newer user copy.
            return Ok(read_text_locked().as_deref() == Some(error.fallback_text.as_str()));
        }
        write_clipboard_text_locked(&error.fallback_text, false)?;
        Ok(true)
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

    // ---- Delayed rendering -------------------------------------------------------
    //
    // Aural offers the text without data; Windows asks our owner window for it at the
    // moment an app reads the clipboard (WM_RENDERFORMAT). Only an identified target
    // process can acknowledge that read. This is transport evidence, not proof that
    // a particular input control accepted the text.

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum PasteResult {
        /// The target read the text; the previous clipboard was restored if nothing
        /// else had taken the clipboard since.
        Read,
        /// Nothing read it in time; the transcript is left on the clipboard (unless the
        /// user copied something newer), and nothing was restored over it.
        Ignored,
        /// Text was rendered for an unidentified reader. Paste may have happened;
        /// retain recovery text and never automatically retry.
        Unconfirmed,
    }

    thread_local! {
        static PENDING: std::cell::RefCell<Option<Vec<u8>>> = const { std::cell::RefCell::new(None) };
        static PENDING_OWNER: std::cell::Cell<isize> = const { std::cell::Cell::new(0) };
        static RENDERED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
        static TARGET_PID: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
        static TARGET_READ: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
        // Only cleanup permits unrelated readers to finish: at that point the
        // transcript is intentionally being retained for manual clipboard recovery.
        static RECOVERING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    fn render_pending() {
        PENDING.with(|p| {
            if let Some(bytes) = p.borrow().as_ref() {
                if let Ok(g) = global_from(bytes) {
                    // SAFETY: called while the clipboard is open for rendering (inside
                    // WM_RENDERFORMAT, or after OpenClipboard by the owner).
                    if unsafe { SetClipboardData(CF_UNICODETEXT, Some(HANDLE(g.0))) }.is_ok() {
                        RENDERED.with(|r| r.set(true));
                    }
                }
            }
        });
    }

    unsafe extern "system" fn owner_proc(
        hwnd: HWND,
        msg: u32,
        wparam: windows::Win32::Foundation::WPARAM,
        lparam: windows::Win32::Foundation::LPARAM,
    ) -> windows::Win32::Foundation::LRESULT {
        use windows::Win32::Foundation::LRESULT;
        use windows::Win32::System::DataExchange::GetClipboardOwner;
        use windows::Win32::UI::WindowsAndMessaging::{
            DefWindowProcW, WM_RENDERALLFORMATS, WM_RENDERFORMAT,
        };
        // Clipboard utility operations create other owner windows on this same
        // thread. Their destruction must never render an older transaction's text
        // over a newer copy, nor clear that transaction's pending buffer.
        if matches!(msg, WM_RENDERFORMAT | WM_RENDERALLFORMATS)
            && PENDING_OWNER.with(|owner| owner.get()) != hwnd.0 as isize
        {
            return LRESULT(0);
        }
        match msg {
            WM_RENDERFORMAT => {
                if wparam.0 as u32 == CF_UNICODETEXT {
                    // A watcher must not consume the delayed format: subsequent reads
                    // of rendered data would be invisible to this owner window.
                    use windows::Win32::System::DataExchange::GetOpenClipboardWindow;
                    let reader = unsafe { GetOpenClipboardWindow() }.ok();
                    let mut pid = 0;
                    if let Some(reader) = reader {
                        unsafe { GetWindowThreadProcessId(reader, Some(&mut pid)) };
                    }
                    let intended = TARGET_PID.with(|p| p.get()) == pid && pid != 0;
                    if intended || pid == 0 || RECOVERING.with(|r| r.get()) {
                        render_pending();
                        if intended && RENDERED.with(|r| r.get()) {
                            TARGET_READ.with(|r| r.set(true));
                        }
                    }
                }
                LRESULT(0)
            }
            WM_RENDERALLFORMATS => {
                // SAFETY: documented handling: open with our window, render if we still
                // own the clipboard, close.
                unsafe {
                    if OpenClipboard(Some(hwnd)).is_ok() {
                        if GetClipboardOwner().ok() == Some(hwnd) {
                            render_pending();
                        }
                        let _ = CloseClipboard();
                    }
                }
                LRESULT(0)
            }
            // SAFETY: default handling for everything else.
            _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        }
    }

    /// A message-only window that owns the clipboard while Aural pastes.
    struct OwnerWindow(HWND);

    impl OwnerWindow {
        fn create() -> Result<Self> {
            use windows::core::w;
            use windows::Win32::System::LibraryLoader::GetModuleHandleW;
            use windows::Win32::UI::WindowsAndMessaging::{
                CreateWindowExW, RegisterClassW, HWND_MESSAGE, WINDOW_EX_STYLE, WINDOW_STYLE,
                WNDCLASSW,
            };
            static REGISTER: std::sync::Once = std::sync::Once::new();
            // SAFETY: standard class registration / message-only window creation.
            unsafe {
                let instance = GetModuleHandleW(None)?;
                REGISTER.call_once(|| {
                    let class = WNDCLASSW {
                        lpfnWndProc: Some(owner_proc),
                        hInstance: instance.into(),
                        lpszClassName: w!("AuralClipboardOwner"),
                        ..Default::default()
                    };
                    RegisterClassW(&class);
                });
                let hwnd = CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    w!("AuralClipboardOwner"),
                    w!(""),
                    WINDOW_STYLE(0),
                    0,
                    0,
                    0,
                    0,
                    Some(HWND_MESSAGE),
                    None,
                    Some(instance.into()),
                    None,
                )?;
                Ok(Self(hwnd))
            }
        }

        fn open_clipboard(&self) -> Result<ClipboardGuard> {
            let deadline = Instant::now() + Duration::from_millis(500);
            loop {
                // SAFETY: our own window.
                if unsafe { OpenClipboard(Some(self.0)) }.is_ok() {
                    return Ok(ClipboardGuard { _owner: None });
                }
                if Instant::now() > deadline {
                    bail!("the clipboard is busy");
                }
                self.pump();
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        fn owns_clipboard(&self) -> bool {
            use windows::Win32::System::DataExchange::GetClipboardOwner;
            // SAFETY: no preconditions.
            unsafe { GetClipboardOwner() }.ok() == Some(self.0)
        }

        /// Deliver messages sent to this thread (WM_RENDERFORMAT arrives this way).
        fn pump(&self) {
            use windows::Win32::UI::WindowsAndMessaging::{
                DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
            };
            let mut msg = MSG::default();
            // SAFETY: standard message pump on this thread.
            unsafe {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
    }

    impl Drop for OwnerWindow {
        fn drop(&mut self) {
            use windows::Win32::UI::WindowsAndMessaging::DestroyWindow;
            // SAFETY: our own window, destroyed on the thread that created it.
            let _ = unsafe { DestroyWindow(self.0) };
            if PENDING_OWNER.with(|owner| owner.get()) == self.0 .0 as isize {
                PENDING.with(|p| p.borrow_mut().take());
                PENDING_OWNER.with(|owner| owner.set(0));
            }
        }
    }

    /// How long pasted text stays on the clipboard after the paste keys, at least. The
    /// page's paste in a Chromium browser follows the key press by tens of milliseconds.
    const PASTE_HOLD: Duration = Duration::from_millis(400);

    /// Offer `text`, press paste via `press`, and restore `previous` once the target
    /// has read the text. See `PasteResult`.
    pub fn paste_and_restore(
        text: &str,
        previous: &ClipboardSnapshot,
        press: &mut dyn FnMut() -> Result<()>,
        timeout: Duration,
    ) -> Result<PasteResult> {
        // Compatibility helper for clipboard integrations in this process.
        let pid = unsafe { windows::Win32::System::Threading::GetCurrentProcessId() };
        paste_for_process(text, previous, press, timeout, pid)
    }

    fn paste_for_process(
        text: &str,
        previous: &ClipboardSnapshot,
        press: &mut dyn FnMut() -> Result<()>,
        timeout: Duration,
        target_pid: u32,
    ) -> Result<PasteResult> {
        let owner = OwnerWindow::create()?;
        PENDING_OWNER.with(|pending| pending.set(owner.0 .0 as isize));
        TARGET_PID.with(|p| p.set(target_pid));
        TARGET_READ.with(|r| r.set(false));
        RECOVERING.with(|r| r.set(false));
        let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
        PENDING
            .with(|p| *p.borrow_mut() = Some(wide.iter().flat_map(|u| u.to_le_bytes()).collect()));
        RENDERED.with(|r| r.set(false));
        {
            let _g = owner.open_clipboard()?;
            // SAFETY: clipboard open by our window; a null handle means "render on
            // request"; the privacy markers carry real data.
            unsafe {
                EmptyClipboard().context("emptying clipboard")?;
                // For delayed rendering SetClipboardData returns NULL even on success,
                // which windows-rs reports as an error; whether it worked shows up as
                // WM_RENDERFORMAT arriving (or not).
                let _ = SetClipboardData(CF_UNICODETEXT, None);
                let zero = 0u32.to_le_bytes();
                for name in [
                    "ExcludeClipboardContentFromMonitorProcessing",
                    "CanIncludeInClipboardHistory",
                    "CanUploadToCloudClipboard",
                ] {
                    let fmt = register_format(name);
                    if fmt != 0 {
                        if let Ok(g) = global_from(&zero) {
                            let _ = SetClipboardData(fmt, Some(HANDLE(g.0)));
                        }
                    }
                }
            }
        }
        if let Err(error) = press() {
            RECOVERING.with(|r| r.set(true));
            let _g = owner.open_clipboard()?;
            if owner.owns_clipboard() {
                render_pending();
            }
            PENDING.with(|p| p.borrow_mut().take());
            return Err(error);
        }
        let pressed = Instant::now();
        let deadline = pressed + timeout;
        while !TARGET_READ.with(|r| r.get()) && owner.owns_clipboard() && Instant::now() < deadline
        {
            owner.pump();
            std::thread::sleep(Duration::from_millis(5));
        }
        if TARGET_READ.with(|r| r.get()) {
            // The first read is not necessarily the paste: Chromium browsers read new
            // clipboard text as soon as it appears, and once rendered later reads don't
            // reach us. Keep the text on the clipboard long enough for the real paste.
            while pressed.elapsed() < PASTE_HOLD {
                owner.pump();
                std::thread::sleep(Duration::from_millis(5));
            }
            // The target has the text. Put the old clipboard back unless something
            // else (the user, another app) has taken the clipboard since.
            // Delivered: forget it, or destroying the owner window would render it
            // again (WM_RENDERALLFORMATS) over the restored clipboard.
            PENDING.with(|p| p.borrow_mut().take());
            let _g = owner.open_clipboard()?;
            if owner.owns_clipboard() {
                // SAFETY: clipboard open by our window.
                unsafe { EmptyClipboard() }.context("emptying clipboard")?;
                write_saved_locked(previous)?;
            }
            Ok(PasteResult::Read)
        } else {
            // Nobody pasted. Leave the transcript for the user (rendered now, so it
            // survives this window), unless they already copied something newer.
            let already_rendered = RENDERED.with(|r| r.get()) && owner.owns_clipboard();
            RECOVERING.with(|r| r.set(true));
            let _g = owner.open_clipboard()?;
            if owner.owns_clipboard() {
                render_pending();
            }
            PENDING.with(|p| p.borrow_mut().take());
            Ok(if already_rendered {
                PasteResult::Unconfirmed
            } else {
                PasteResult::Ignored
            })
        }
    }

    /// Original selection identity retained privately while the review window has focus.
    #[derive(Debug, Clone)]
    pub struct SelectionSnapshot {
        pub text: String,
        target: TargetInfo,
        pid: u32,
        focused_hwnd: isize,
        caption: String,
        runtime_id: Option<Vec<i32>>,
    }

    impl SelectionSnapshot {
        /// Caption captured with the original selection, for the user's review.
        pub fn target_caption(&self) -> &str {
            &self.caption
        }
    }

    fn window_caption(hwnd: isize) -> String {
        use windows::Win32::UI::WindowsAndMessaging::GetWindowTextW;
        let mut buf = [0u16; 1024];
        let n = unsafe { GetWindowTextW(HWND(hwnd as *mut _), &mut buf) };
        String::from_utf16_lossy(&buf[..n as usize])
    }

    fn native_text_control(hwnd: isize) -> bool {
        let mut buf = [0u16; 256];
        let n = unsafe { GetClassNameW(HWND(hwnd as *mut _), &mut buf) };
        let class = String::from_utf16_lossy(&buf[..n as usize]).to_ascii_lowercase();
        class == "edit" || class.starts_with("richedit")
    }

    fn focused_runtime_id(expected_pid: u32) -> Result<Vec<i32>> {
        accessible_identity(expected_pid, None, None, None).map(|(ids, _)| ids)
    }

    fn selected_text_within_limit(text: String) -> Result<String> {
        // A maximum of 3,000 Unicode scalars occupies at most 6,000 UTF-16
        // units. The extra unit is a sentinel: never accept a truncated prefix.
        if text.encode_utf16().count() >= 6001 || text.chars().count() > 3000 {
            bail!("Select up to 3,000 characters at a time. Original text kept.");
        }
        if text.is_empty() {
            bail!("select text before using Touch up");
        }
        Ok(text)
    }

    fn menu_control_type(control_type: i32) -> bool {
        matches!(control_type, 50009..=50011)
    }

    fn retry_menu_transition(code: u32, dismissed: bool, before_deadline: bool) -> bool {
        use windows::Win32::UI::Accessibility::UIA_E_ELEMENTNOTAVAILABLE;
        dismissed && before_deadline && code == UIA_E_ELEMENTNOTAVAILABLE
    }

    fn accessible_identity(
        expected_pid: u32,
        selected_text: Option<&mut Option<String>>,
        native_hwnd: Option<isize>,
        capture_target: Option<isize>,
    ) -> Result<(Vec<i32>, isize)> {
        use windows::Win32::System::Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
            COINIT_MULTITHREADED,
        };
        use windows::Win32::System::Ole::{
            SafeArrayDestroy, SafeArrayGetElement, SafeArrayGetLBound, SafeArrayGetUBound,
        };
        use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation};
        struct Apartment(bool);
        impl Drop for Apartment {
            fn drop(&mut self) {
                if self.0 {
                    unsafe { CoUninitialize() };
                }
            }
        }
        unsafe {
            let initialized = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();

            let _apartment = Apartment(initialized);

            let automation: IUIAutomation =
                CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;

            let mut element = match native_hwnd {
                Some(hwnd) => automation.ElementFromHandle(HWND(hwnd as *mut _))?,
                None => automation.GetFocusedElement()?,
            };
            if let Some(hwnd) = native_hwnd {
                anyhow::ensure!(
                    element.CurrentNativeWindowHandle()?.0 as isize == hwnd,
                    "the selected native editor could not be identified"
                );
            }
            if let Some(target) = capture_target {
                let deadline = Instant::now() + Duration::from_millis(300);
                let mut dismissed = false;
                loop {
                    anyhow::ensure!(
                        foreground_window() == target && window_pid(target) == expected_pid,
                        "focus changed while dismissing the shortcut menu"
                    );
                    // A menu can destroy its provider as Escape is processed. Read
                    // each replacement provider and its metadata as one retry unit.
                    let current = (|| -> windows::core::Result<_> {
                        let current = if dismissed {
                            automation.GetFocusedElement()?
                        } else {
                            element.clone()
                        };
                        let pid = current.CurrentProcessId()? as u32;
                        let control_type = current.CurrentControlType()?.0;
                        Ok((current, pid, control_type))
                    })();
                    let (current, pid, control_type) = match current {
                        Ok(current) => current,
                        Err(error)
                            if retry_menu_transition(
                                error.code().0 as u32,
                                dismissed,
                                Instant::now() < deadline,
                            ) =>
                        {
                            std::thread::sleep(Duration::from_millis(10));
                            continue;
                        }
                        Err(error) => return Err(error.into()),
                    };
                    element = current;
                    anyhow::ensure!(
                        pid == expected_pid,
                        "focused accessibility element belongs to another application"
                    );
                    if !menu_control_type(control_type) {
                        break;
                    }
                    anyhow::ensure!(
                        Instant::now() < deadline,
                        "close the application menu and select text before using Touch up"
                    );
                    if !dismissed {
                        anyhow::ensure!(
                            held_modifiers().is_empty(),
                            "release the shortcut keys before using Touch up"
                        );

                        // One Escape only, to the verified original app and menu.
                        send(&[key(0x1B, false), key(0x1B, true)])?;
                        dismissed = true;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
            }

            let element_pid = element.CurrentProcessId()? as u32;

            anyhow::ensure!(
                element_pid == expected_pid,
                "focused accessibility element belongs to another application"
            );
            if let Some(selected_text) = selected_text {
                use windows::Win32::UI::Accessibility::{
                    IUIAutomationTextPattern, TextPatternRangeEndpoint_End,
                    TextPatternRangeEndpoint_Start, UIA_TextPatternId, UIA_E_NOTSUPPORTED,
                };
                // Read selection and identity from this same provider element. No
                // focus, keyboard or clipboard operation occurs on this path.
                match element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) {
                    Ok(pattern) => {
                        let ranges = pattern.GetSelection()?;
                        let count = ranges.Length()?;

                        anyhow::ensure!(
                            count == 1,
                            "select one continuous text range before using Touch up"
                        );
                        let range = ranges.GetElement(0)?;
                        anyhow::ensure!(
                            range.CompareEndpoints(
                                TextPatternRangeEndpoint_Start,
                                &range,
                                TextPatternRangeEndpoint_End
                            )? != 0,
                            "select text before using Touch up"
                        );
                        let text = selected_text_within_limit(range.GetText(6001)?.to_string())?;

                        *selected_text = Some(text);
                    }
                    Err(error)
                        if error.code().0 as u32 == UIA_E_NOTSUPPORTED
                            || error.code().0 as u32 == 0x80004002
                            || error.code().0 == 0 => {}
                    Err(error) => return Err(error.into()),
                }
            }

            let array = element.GetRuntimeId()?;

            let result = (|| -> Result<Vec<i32>> {
                let first = SafeArrayGetLBound(array, 1)?;
                let last = SafeArrayGetUBound(array, 1)?;
                anyhow::ensure!(
                    last >= first && last - first < 1024,
                    "invalid accessibility identity"
                );
                let mut ids = Vec::new();
                for index in first..=last {
                    let mut value = 0i32;
                    SafeArrayGetElement(array, &index, &mut value as *mut i32 as *mut _)?;
                    ids.push(value);
                }
                Ok(ids)
            })();
            let _ = SafeArrayDestroy(array);

            let provider_hwnd = element.CurrentNativeWindowHandle()?.0 as isize;
            result.map(|ids| (ids, provider_hwnd))
        }
    }

    #[cfg(test)]
    mod selection_limits {
        use super::{menu_control_type, retry_menu_transition, selected_text_within_limit};

        #[test]
        fn provider_retry_is_specific_to_menu_dismissal_and_existing_deadline() {
            use windows::Win32::UI::Accessibility::UIA_E_ELEMENTNOTAVAILABLE;
            assert_eq!(UIA_E_ELEMENTNOTAVAILABLE, 0x80040201);
            assert!(retry_menu_transition(UIA_E_ELEMENTNOTAVAILABLE, true, true));
            assert!(!retry_menu_transition(
                UIA_E_ELEMENTNOTAVAILABLE,
                false,
                true
            ));
            assert!(!retry_menu_transition(
                UIA_E_ELEMENTNOTAVAILABLE,
                true,
                false
            ));
            assert!(!retry_menu_transition(0x80004005, true, true));
            assert!(!retry_menu_transition(0, true, true));
        }

        #[test]
        fn only_menu_controls_allow_capture_time_escape() {
            for control in [50009, 50010, 50011] {
                assert!(menu_control_type(control));
            }
            for control in [50000, 50004, 50030, 50033, 0] {
                assert!(!menu_control_type(control));
            }
        }

        #[test]
        fn accepts_complete_astral_text_and_rejects_overlong_or_truncated_ranges() {
            let complete = "😀".repeat(3000);
            assert_eq!(
                selected_text_within_limit(complete.clone()).unwrap(),
                complete
            );
            assert!(selected_text_within_limit("😀".repeat(3001)).is_err());
            assert!(selected_text_within_limit("x".repeat(6001)).is_err());
            assert!(selected_text_within_limit(String::new()).is_err());
        }
    }

    fn focused_control(hwnd: isize) -> Result<isize> {
        use windows::Win32::UI::WindowsAndMessaging::{GetGUIThreadInfo, GUITHREADINFO};
        let thread = unsafe { GetWindowThreadProcessId(HWND(hwnd as *mut _), None) };
        let mut info = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        unsafe { GetGUIThreadInfo(thread, &mut info) }.context("reading target input focus")?;

        Ok(info.hwndFocus.0 as isize)
    }

    fn window_pid(hwnd: isize) -> u32 {
        let mut pid = 0;
        // SAFETY: Windows validates the supplied HWND; failure leaves pid zero.
        unsafe { GetWindowThreadProcessId(HWND(hwnd as *mut _), Some(&mut pid)) };
        pid
    }

    fn copy_selected_text(expected: isize) -> Result<String> {
        if foreground_window() != expected {
            bail!("focus changed before copying selection");
        }

        let previous = snapshot_clipboard()?;

        if !previous.complete {
            bail!("the clipboard cannot be preserved for selection capture");
        }
        // A fresh clipboard value distinguishes an ignored Copy from old clipboard text.
        let marker = format!(
            "Aural selection {} {:?}",
            std::process::id(),
            Instant::now()
        );

        let marker_sequence = set_clipboard_text(&marker, true)?;

        let result = (|| -> Result<(String, u32)> {
            release_modifiers()?;

            if foreground_window() != expected {
                bail!("focus changed before copying selection");
            }

            send(&[
                key(0xA2, false),
                key(0x43, false),
                key(0x43, true),
                key(0xA2, true),
            ])?;

            let deadline = Instant::now() + Duration::from_millis(750);
            loop {
                if foreground_window() != expected {
                    bail!("focus changed while copying selection");
                }
                let sequence = unsafe { GetClipboardSequenceNumber() };
                if sequence != marker_sequence {
                    // Require the copy owner to belong to the original app. An unrelated
                    // user copy cannot become the selection to replace.
                    let owner =
                        unsafe { windows::Win32::System::DataExchange::GetClipboardOwner() }.ok();

                    if owner.is_none_or(|h| window_pid(h.0 as isize) != window_pid(expected)) {
                        bail!("another application changed the clipboard");
                    }

                    let text = read_clipboard_text().context("the selection contains no text")?;

                    if text.is_empty() || text == marker {
                        bail!("select text before using Touch up");
                    }
                    return Ok((text, sequence));
                }
                if Instant::now() >= deadline {
                    bail!("select text before using Touch up");
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        })();
        let expected_sequence = result
            .as_ref()
            .map(|(_, seq)| *seq)
            .unwrap_or(marker_sequence);
        // Hold the clipboard across the comparison and restore: a newer user copy
        // can never be overwritten between the check and the write.
        {
            let _g = open_clipboard()?;
            let current_sequence = unsafe { GetClipboardSequenceNumber() };

            if current_sequence == expected_sequence {
                unsafe { EmptyClipboard() }.context("emptying clipboard")?;
                write_saved_locked(&previous)?;
            }
        }

        result.map(|(text, _)| text)
    }

    pub fn capture_selection() -> Result<SelectionSnapshot> {
        let target = foreground_target();

        if target.hwnd == 0 || (target.elevated && !self_elevated()) {
            bail!("the focused application cannot be accessed");
        }
        if is_terminal(&target).is_some() {
            bail!("select text in an editor for Touch up");
        }
        let pid = window_pid(target.hwnd);
        let focused_hwnd = focused_control(target.hwnd)?;
        let caption = window_caption(target.hwnd);

        let mut selected_text = None;
        let runtime_result = accessible_identity(
            pid,
            Some(&mut selected_text),
            native_text_control(focused_hwnd).then_some(focused_hwnd),
            Some(target.hwnd),
        );

        let (runtime_id, provider_hwnd) = runtime_result?;
        let runtime_id = Some(runtime_id);
        // Menu dismissal may have returned focus to a different native editor child.
        // Capture that fresh provider/control identity together with its selection.
        let focused_hwnd = if native_text_control(provider_hwnd) && window_pid(provider_hwnd) == pid
        {
            provider_hwnd
        } else {
            focused_control(target.hwnd)?
        };

        let text = match selected_text {
            Some(text) => text,
            None if native_text_control(focused_hwnd) =>
                selected_text_within_limit(copy_selected_text(target.hwnd)?)?,
            None => bail!("this input does not expose a safe text selection; use an editor with accessibility support"),
        };
        anyhow::ensure!(
            foreground_window() == target.hwnd,
            "focus changed while capturing selection"
        );

        Ok(SelectionSnapshot {
            text,
            target,
            pid,
            focused_hwnd,
            caption,
            runtime_id,
        })
    }

    /// Restore the reviewed target, re-copy its exact selection, and apply once.
    /// A changed target, selection, or intervening foreground window cancels delivery.
    pub fn replace_selection(
        selection: &SelectionSnapshot,
        text: &str,
        review_hwnd: isize,
    ) -> Result<InsertOutcome> {
        if review_hwnd == 0 || foreground_window() != review_hwnd {
            bail!("focus changed from the Touch up review");
        }
        if window_pid(selection.target.hwnd) != selection.pid || selection.pid == 0 {
            bail!("the original application has closed");
        }
        // SAFETY: validated target HWND; Windows may refuse focus, checked below.
        let _ = unsafe {
            windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(HWND(
                selection.target.hwnd as *mut _,
            ))
        };
        let deadline = Instant::now() + Duration::from_millis(300);
        while foreground_window() != selection.target.hwnd && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }

        if window_caption(selection.target.hwnd) != selection.caption {
            bail!("the original document changed; select the text again");
        }

        if let Some(original) = &selection.runtime_id {
            let current = focused_runtime_id(selection.pid)?;

            if current != *original {
                bail!("the original editor context changed; select the text again");
            }
        } else if !native_text_control(selection.focused_hwnd) {
            bail!("the original input cannot be safely identified");
        }

        if focused_control(selection.target.hwnd)? != selection.focused_hwnd {
            bail!("the original input control changed; select the text again");
        }

        if copy_selected_text(selection.target.hwnd)? != selection.text {
            bail!("the original selection changed; select it again");
        }

        insert(text, selection.target.hwnd).map_err(anyhow::Error::new)
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
    pub enum InsertOutcome {
        Inserted,
        /// Paste keys were sent but delivery could not be acknowledged. Do not retry.
        PasteSent,
        /// Text left on the clipboard for the user; carries why.
        CopiedOnly(Reason),
    }

    #[derive(Debug)]
    pub struct InsertError {
        /// Text normalized for the target and profile used by this insertion attempt.
        pub fallback_text: String,
        recovery_sequence: u32,
        source: anyhow::Error,
    }

    impl std::fmt::Display for InsertError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            self.source.fmt(f)
        }
    }

    impl std::error::Error for InsertError {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(self.source.as_ref())
        }
    }

    pub(super) fn preserve_fallback<T>(
        result: Result<T>,
        fallback_text: String,
    ) -> std::result::Result<T, InsertError> {
        result.map_err(|source| InsertError {
            fallback_text,
            recovery_sequence: unsafe { GetClipboardSequenceNumber() },
            source,
        })
    }

    /// Deliver `text` to the focused app. The pasted text is never left in clipboard
    /// history, and the user's previous clipboard (every format) is restored afterwards.
    pub fn insert(
        text: &str,
        release_hwnd: isize,
    ) -> std::result::Result<InsertOutcome, InsertError> {
        insert_with_paste(text, release_hwnd, None)
    }

    pub fn insert_with_paste(
        text: &str,
        release_hwnd: isize,
        paste: Option<PasteChord>,
    ) -> std::result::Result<InsertOutcome, InsertError> {
        let recovery_sequence = unsafe { GetClipboardSequenceNumber() };
        let target = foreground_target();
        let snapshot = snapshot_clipboard().unwrap_or(ClipboardSnapshot {
            items: Vec::new(),
            complete: false,
        });
        let p = plan_with_paste(
            &target,
            release_hwnd,
            snapshot.complete,
            self_elevated(),
            paste,
        );
        let text = p.apply(text);
        if text.is_empty() {
            return Ok(InsertOutcome::Inserted);
        }
        let result = (|| -> Result<InsertOutcome> {
            match p.strategy {
                Strategy::ClipboardOnly(reason) => {
                    set_clipboard_text(&text, false)?;
                    Ok(InsertOutcome::CopiedOnly(reason))
                }
                Strategy::Type => {
                    release_modifiers()?;
                    if foreground_window() != release_hwnd {
                        bail!("focus changed before typing");
                    }
                    type_text(&text)?;
                    Ok(InsertOutcome::Inserted)
                }
                Strategy::Paste(chord) => {
                    let mut press = || -> Result<()> {
                        release_modifiers()?;
                        if foreground_window() != release_hwnd {
                            bail!("focus changed before paste");
                        }
                        send(&paste_inputs(chord))
                    };
                    // Restore happens only after the target has actually read the text.
                    let mut pid = 0;
                    unsafe {
                        GetWindowThreadProcessId(HWND(release_hwnd as *mut _), Some(&mut pid))
                    };
                    match paste_for_process(&text, &snapshot, &mut press, PASTE_TIMEOUT, pid)? {
                        PasteResult::Read => Ok(InsertOutcome::Inserted),
                        PasteResult::Unconfirmed => Ok(InsertOutcome::PasteSent),
                        PasteResult::Ignored => {
                            if read_clipboard_text().as_deref() == Some(text.as_str()) {
                                Ok(InsertOutcome::CopiedOnly(Reason::PasteIgnored))
                            } else {
                                // The user's newer copy won; recovery remains in Aural.
                                Ok(InsertOutcome::PasteSent)
                            }
                        }
                    }
                }
            }
        })();
        preserve_fallback(result, text).map_err(|mut error| {
            error.recovery_sequence = recovery_sequence;
            error
        })
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
    fn modern_terminal_hosts_use_terminal_paste() {
        for host in [
            "Alacritty.exe",
            "wezterm-gui.exe",
            "ghostty.exe",
            "Warp.exe",
        ] {
            let p = plan(&target(host, "terminal"), 42, true, false);
            assert_eq!(
                p.strategy,
                Strategy::Paste(PasteChord::CtrlShiftV),
                "{host}"
            );
            assert!(p.single_line, "{host}");
        }
        let p = plan(
            &target("unknown.exe", "CASCADIA_HOSTING_WINDOW_CLASS"),
            42,
            true,
            false,
        );
        assert_eq!(p.strategy, Strategy::Paste(PasteChord::CtrlShiftV));
    }

    #[test]
    fn terminal_text_preserves_spacing_and_removes_input_controls() {
        let p = plan(&target("WindowsTerminal.exe", "terminal"), 42, true, false);
        assert_eq!(
            p.apply("echo  \"two  spaces\"\r\nnext\x1b[200~\x08\targ"),
            "echo  \"two  spaces\" next[200~ arg"
        );
        assert_eq!(
            p.apply("one\rtwo\nthree\u{2028}four\u{2029}five"),
            "one two three four five"
        );
    }

    #[test]
    fn embedded_terminal_override_keeps_focus_and_elevation_guards() {
        let t = target("Code.exe", "Chrome_WidgetWin_1");
        let p = plan_with_paste(&t, 42, true, false, Some(PasteChord::CtrlShiftV));
        assert_eq!(p.strategy, Strategy::Paste(PasteChord::CtrlShiftV));
        assert_eq!(p.apply("review  this\nplease"), "review  this please");
        assert_eq!(
            plan_with_paste(&t, 7, true, false, Some(PasteChord::CtrlShiftV)).strategy,
            Strategy::ClipboardOnly(Reason::FocusChanged)
        );
        let mut elevated = t;
        elevated.elevated = true;
        assert_eq!(
            plan_with_paste(&elevated, 42, true, false, Some(PasteChord::ShiftInsert)).strategy,
            Strategy::ClipboardOnly(Reason::Elevated)
        );
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
    fn a_terminal_never_uses_unicode_key_injection_as_a_fallback() {
        let p = plan(&target("WindowsTerminal.exe", "terminal"), 42, false, false);
        assert_eq!(p.strategy, Strategy::ClipboardOnly(Reason::PasteIgnored));
        assert_eq!(p.apply("first\nsecond"), "first second");
    }

    #[test]
    fn failed_terminal_paste_retains_the_normalized_fallback_text() {
        let p = plan(&target("WindowsTerminal.exe", "terminal"), 42, true, false);
        let raw = "echo first\nsecond\x1b\targ";
        let failed: anyhow::Result<()> = Err(anyhow::anyhow!("paste failed"));
        let error = win::preserve_fallback(failed, p.apply(raw)).unwrap_err();
        assert_eq!(error.fallback_text, "echo first second arg");
    }

    // Formats Aural saves and puts back around a paste. Typing is only for clipboards
    // that cannot be saved at all, because some apps (Windows 11 Notepad) drop most of a
    // long burst of typed characters.
    const CF_TEXT: u32 = 1;
    const CF_BITMAP: u32 = 2;
    const CF_METAFILEPICT: u32 = 3;
    const CF_DIB: u32 = 8;
    const CF_PALETTE: u32 = 9;
    const CF_UNICODETEXT: u32 = 13;
    const CF_ENHMETAFILE: u32 = 14;
    const CF_LOCALE: u32 = 16;
    const HTML: u32 = 0xC0F1; // a registered format, e.g. "HTML Format"

    #[test]
    fn rich_text_html_and_app_formats_are_saved_as_bytes() {
        let present = [CF_UNICODETEXT, HTML, CF_LOCALE, CF_TEXT];
        assert_eq!(save_kind(CF_UNICODETEXT, &present), SaveKind::Bytes);
        assert_eq!(save_kind(HTML, &present), SaveKind::Bytes);
        assert_eq!(save_kind(CF_LOCALE, &present), SaveKind::Bytes);
        assert_eq!(
            save_kind(15 /* CF_HDROP: copied files */, &[15]),
            SaveKind::Bytes
        );
    }

    #[test]
    fn formats_windows_makes_from_another_are_not_saved_twice() {
        assert_eq!(
            save_kind(CF_TEXT, &[CF_UNICODETEXT, CF_TEXT]),
            SaveKind::Synthesized
        );
        assert_eq!(
            save_kind(CF_BITMAP, &[CF_BITMAP, CF_DIB]),
            SaveKind::Synthesized
        );
        assert_eq!(
            save_kind(CF_PALETTE, &[CF_DIB, CF_PALETTE]),
            SaveKind::Synthesized
        );
        assert_eq!(
            save_kind(CF_METAFILEPICT, &[CF_ENHMETAFILE, CF_METAFILEPICT]),
            SaveKind::Synthesized
        );
        // ANSI-only text is the real data when there is no Unicode text.
        assert_eq!(save_kind(CF_TEXT, &[CF_TEXT]), SaveKind::Bytes);
    }

    #[test]
    fn drawings_are_saved_and_handles_aural_cannot_copy_are_reported() {
        assert_eq!(
            save_kind(CF_ENHMETAFILE, &[CF_ENHMETAFILE]),
            SaveKind::EnhMetafile
        );
        assert_eq!(save_kind(CF_BITMAP, &[CF_BITMAP]), SaveKind::Unsupported);
        assert_eq!(
            save_kind(0x0080 /* CF_OWNERDISPLAY */, &[0x0080]),
            SaveKind::Unsupported
        );
        assert_eq!(
            save_kind(0x0200 /* CF_PRIVATEFIRST */, &[0x0200]),
            SaveKind::Unsupported
        );
        assert_eq!(
            save_kind(0x0300 /* CF_GDIOBJFIRST */, &[0x0300]),
            SaveKind::Unsupported
        );
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

    /// Real clipboard, no keyboard: a second thread plays the target app. The clipboard
    /// is global, so these run one at a time.
    #[cfg(windows)]
    mod delayed_render {
        use super::super::*;
        use std::sync::mpsc;
        use std::sync::Mutex;
        use std::time::{Duration, Instant};

        static CLIPBOARD: Mutex<()> = Mutex::new(());

        /// Stand-in for an app handling Ctrl+V: opens the clipboard and reads the text.
        fn app_that_pastes(tx: mpsc::Sender<Option<String>>) -> impl FnMut() -> anyhow::Result<()> {
            move || {
                let tx = tx.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(50));
                    let _ = tx.send(read_clipboard_text());
                });
                Ok(())
            }
        }

        #[test]
        fn text_is_rendered_when_the_target_reads_it_and_old_text_comes_back() {
            let _g = CLIPBOARD.lock().unwrap_or_else(|p| p.into_inner());
            set_clipboard_text("previous clipboard", false).unwrap();
            let (tx, rx) = mpsc::channel();
            let mut press = app_that_pastes(tx);
            let got = paste_and_restore(
                "dictated text",
                &ClipboardSnapshot::text("previous clipboard"),
                &mut press,
                Duration::from_secs(2),
            )
            .unwrap();
            assert_eq!(got, PasteResult::Read);
            assert_eq!(
                rx.recv_timeout(Duration::from_secs(2)).unwrap().as_deref(),
                Some("dictated text")
            );
            assert_eq!(read_clipboard_text().as_deref(), Some("previous clipboard"));
        }

        /// Chromium browsers (Chrome, Edge, Brave, Opera) read new clipboard text as soon
        /// as it appears, for their "paste this link" suggestions, before the page's paste
        /// happens. That early read must not trigger the restore, or the page pastes the
        /// old clipboard (found in the ChatGPT composer in Brave).
        #[test]
        fn an_early_read_by_a_clipboard_watcher_does_not_restore_before_the_paste() {
            let _g = CLIPBOARD.lock().unwrap_or_else(|p| p.into_inner());
            set_clipboard_text("previous clipboard", false).unwrap();
            let (tx, rx) = mpsc::channel();
            let mut press = move || -> anyhow::Result<()> {
                std::thread::spawn(read_clipboard_text); // the browser's clipboard watcher
                let tx = tx.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(150)); // the page's paste
                    let _ = tx.send(read_clipboard_text());
                });
                Ok(())
            };
            let got = paste_and_restore(
                "dictated text",
                &ClipboardSnapshot::text("previous clipboard"),
                &mut press,
                Duration::from_secs(2),
            )
            .unwrap();
            assert_eq!(got, PasteResult::Read);
            assert_eq!(
                rx.recv_timeout(Duration::from_secs(2)).unwrap().as_deref(),
                Some("dictated text"),
                "the page pasted the old clipboard"
            );
            assert_eq!(read_clipboard_text().as_deref(), Some("previous clipboard"));
        }

        #[test]
        fn a_target_that_never_reads_keeps_the_transcript_on_the_clipboard() {
            let _g = CLIPBOARD.lock().unwrap_or_else(|p| p.into_inner());
            set_clipboard_text("previous clipboard", false).unwrap();
            let mut press = || -> anyhow::Result<()> { Ok(()) }; // e.g. a window that ignores Ctrl+V
            let t0 = Instant::now();
            let got = paste_and_restore(
                "dictated text",
                &ClipboardSnapshot::text("previous clipboard"),
                &mut press,
                Duration::from_millis(600),
            )
            .unwrap();
            assert_eq!(got, PasteResult::Ignored);
            assert!(t0.elapsed() < Duration::from_secs(3));
            // The old clipboard must not replace the text the user still needs to paste.
            assert_eq!(read_clipboard_text().as_deref(), Some("dictated text"));
        }

        /// Subprocess fixture: its clipboard owner window has a genuinely unrelated PID.
        #[test]
        #[ignore = "invoked by the unrelated-reader integration tests"]
        fn unrelated_reader_process() {
            assert_eq!(std::env::var("AURAL_CLIPBOARD_READER").as_deref(), Ok("1"));
            assert!(
                read_clipboard_text().is_none(),
                "an unrelated reader consumed delayed text"
            );
        }

        fn unrelated_reader() -> std::process::Child {
            std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "insert::tests::delayed_render::unrelated_reader_process",
                    "--ignored",
                    "--test-threads=1",
                ])
                .env("AURAL_CLIPBOARD_READER", "1")
                .stdout(std::process::Stdio::null())
                .spawn()
                .unwrap()
        }

        #[test]
        fn an_unrelated_process_cannot_acknowledge_an_ignored_paste() {
            let _g = CLIPBOARD.lock().unwrap_or_else(|p| p.into_inner());
            let mut child = None;
            let mut press = || -> anyhow::Result<()> {
                child = Some(unrelated_reader());
                Ok(())
            };
            let got = paste_and_restore(
                "recovery text",
                &ClipboardSnapshot::text("previous"),
                &mut press,
                Duration::from_secs(2),
            )
            .unwrap();
            assert_eq!(got, PasteResult::Ignored);
            assert!(child.unwrap().wait().unwrap().success());
            assert_eq!(read_clipboard_text().as_deref(), Some("recovery text"));
        }

        #[test]
        fn a_target_can_read_after_an_unrelated_process_was_refused() {
            let _g = CLIPBOARD.lock().unwrap_or_else(|p| p.into_inner());
            let (tx, rx) = mpsc::channel();
            let mut press = || -> anyhow::Result<()> {
                let mut child = unrelated_reader();
                let tx = tx.clone();
                std::thread::spawn(move || {
                    assert!(child.wait().unwrap().success());
                    let _ = tx.send(read_clipboard_text());
                });
                Ok(())
            };
            let got = paste_and_restore(
                "target text",
                &ClipboardSnapshot::text("previous"),
                &mut press,
                Duration::from_secs(3),
            )
            .unwrap();
            assert_eq!(got, PasteResult::Read);
            assert_eq!(
                rx.recv_timeout(Duration::from_secs(2)).unwrap().as_deref(),
                Some("target text")
            );
            assert_eq!(read_clipboard_text().as_deref(), Some("previous"));
        }

        /// Visible disposable Windows edit controls exercise actual Copy/Paste and focus.
        #[test]
        #[ignore = "desktop focus integration; run explicitly on the interactive desktop"]
        fn selected_text_round_trip_preserves_clipboard_and_rejects_changed_selection() {
            use windows::core::w;
            use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
            use windows::Win32::UI::WindowsAndMessaging::*;
            let _g = CLIPBOARD.lock().unwrap_or_else(|p| p.into_inner());
            let original = snapshot_clipboard().unwrap();
            let (tx, rx) = mpsc::channel();
            let (stop_tx, stop_rx) = mpsc::channel();
            let (focus_tx, focus_rx) = mpsc::channel::<bool>();
            let (ack_tx, ack_rx) = mpsc::channel();
            let gui = std::thread::spawn(move || unsafe {
                let make = |title| {
                    CreateWindowExW(
                        WINDOW_EX_STYLE(0),
                        w!("EDIT"),
                        title,
                        WINDOW_STYLE(0x10CF0004),
                        100,
                        100,
                        600,
                        200,
                        None,
                        None,
                        None,
                        None,
                    )
                    .unwrap()
                };
                let target = make(w!("Before selected after"));
                let review = make(w!("Aural disposable review"));
                let alternate = CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    w!("EDIT"),
                    w!("selected"),
                    WINDOW_STYLE(0x50000004),
                    20,
                    80,
                    300,
                    40,
                    Some(target),
                    None,
                    None,
                    None,
                )
                .unwrap();
                let _ = SetForegroundWindow(target);
                let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(target));
                SendMessageW(target, 0x00B1, Some(WPARAM(7)), Some(LPARAM(15)));
                tx.send((target.0 as isize, review.0 as isize)).unwrap();
                while stop_rx.try_recv().is_err() {
                    if let Ok(other) = focus_rx.try_recv() {
                        let control = if other { alternate } else { target };
                        let _ = SetForegroundWindow(target);
                        let _ =
                            windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(control));
                        SendMessageW(
                            control,
                            0x00B1,
                            Some(WPARAM(if other { 0 } else { 7 })),
                            Some(LPARAM(if other { 8 } else { 15 })),
                        );
                        ack_tx.send(()).unwrap();
                    }
                    let mut msg = MSG::default();
                    while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
                let _ = DestroyWindow(target);
                let _ = DestroyWindow(review);
            });
            let (target, review) = rx.recv().unwrap();
            let result = (|| -> anyhow::Result<()> {
                // SetForegroundWindow activation completes through the GUI pump.
                // Do not capture whichever application was foreground before it.
                let deadline = Instant::now() + Duration::from_secs(1);
                while foreground_window() != target && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(10));
                }
                anyhow::ensure!(
                    foreground_window() == target,
                    "disposable editor did not receive focus"
                );
                set_clipboard_text("preserved clipboard", false)?;
                let selection =
                    capture_selection().map_err(|e| anyhow::anyhow!("initial capture: {e}"))?;
                assert_eq!(selection.text, "selected");
                assert_eq!(
                    read_clipboard_text().as_deref(),
                    Some("preserved clipboard")
                );
                // Another field with identical selected text is still the wrong destination.
                // Keep the host active for this guard check so its top-level Edit
                // procedure cannot reset focus while activating from the review.
                focus_tx.send(true).unwrap();
                ack_rx.recv().unwrap();
                assert!(replace_selection(&selection, "WRONG FIELD", target).is_err());
                focus_tx.send(false).unwrap();
                ack_rx.recv().unwrap();
                unsafe {
                    let _ = SetForegroundWindow(HWND(review as *mut _));
                }
                let deadline = Instant::now() + Duration::from_secs(1);
                while foreground_window() != review && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(10));
                }
                assert_eq!(
                    replace_selection(&selection, "REVISED", review)
                        .map_err(|e| anyhow::anyhow!("review apply: {e}"))?,
                    InsertOutcome::Inserted
                );
                let mut buf = [0u16; 128];
                let n = unsafe { GetWindowTextW(HWND(target as *mut _), &mut buf) };
                assert_eq!(
                    String::from_utf16_lossy(&buf[..n as usize]),
                    "Before REVISED after"
                );
                assert_eq!(
                    read_clipboard_text().as_deref(),
                    Some("preserved clipboard")
                );
                // The stale snapshot cannot replace the newly selected/replaced text.
                unsafe {
                    let _ = SetForegroundWindow(HWND(review as *mut _));
                }
                let deadline = Instant::now() + Duration::from_secs(1);
                while foreground_window() != review && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(10));
                }
                assert!(replace_selection(&selection, "DUPLICATE", review).is_err());
                let n = unsafe { GetWindowTextW(HWND(target as *mut _), &mut buf) };
                assert_eq!(
                    String::from_utf16_lossy(&buf[..n as usize]),
                    "Before REVISED after"
                );
                Ok(())
            })();
            stop_tx.send(()).unwrap();
            gui.join().unwrap();
            restore_clipboard(&original).unwrap();
            result.unwrap();
        }

        #[test]
        fn failed_delivery_recovery_never_overwrites_a_newer_copy() {
            let _g = CLIPBOARD.lock().unwrap_or_else(|p| p.into_inner());
            set_clipboard_text("original", false).unwrap();
            let error = win::preserve_fallback::<()>(
                Err(anyhow::anyhow!("input failed")),
                "fallback".into(),
            )
            .unwrap_err();
            set_clipboard_text("newer user copy", false).unwrap();
            assert!(!recover_insert_error(&error).unwrap());
            assert_eq!(read_clipboard_text().as_deref(), Some("newer user copy"));
        }

        #[test]
        fn failed_paste_keys_leave_rendered_recovery_without_overwriting_newer_copy() {
            let _g = CLIPBOARD.lock().unwrap_or_else(|p| p.into_inner());
            set_clipboard_text("previous", false).unwrap();
            let mut press =
                || -> anyhow::Result<()> { anyhow::bail!("focus changed before sending keys") };
            assert!(paste_and_restore(
                "normalized recovery",
                &ClipboardSnapshot::text("previous"),
                &mut press,
                Duration::from_secs(1)
            )
            .is_err());
            assert_eq!(
                read_clipboard_text().as_deref(),
                Some("normalized recovery")
            );
            let error = win::preserve_fallback::<()>(
                Err(anyhow::anyhow!("input failed")),
                "normalized recovery".into(),
            )
            .unwrap_err();
            set_clipboard_text("newer copy", false).unwrap();
            assert!(!recover_insert_error(&error).unwrap());
            assert_eq!(read_clipboard_text().as_deref(), Some("newer copy"));
        }

        #[test]
        fn failed_delivery_recovers_normalized_text_when_clipboard_is_unchanged() {
            let _g = CLIPBOARD.lock().unwrap_or_else(|p| p.into_inner());
            set_clipboard_text("original", false).unwrap();
            let error = win::preserve_fallback::<()>(
                Err(anyhow::anyhow!("input failed")),
                "normalized fallback".into(),
            )
            .unwrap_err();
            assert!(recover_insert_error(&error).unwrap());
            assert_eq!(
                read_clipboard_text().as_deref(),
                Some("normalized fallback")
            );
        }

        /// Runs inside the disposable real Windows Terminal tab, consuming text only.
        #[test]
        #[ignore = "subprocess fixture for real Windows Terminal delivery"]
        fn windows_terminal_input_fixture() {
            #[link(name = "kernel32")]
            unsafe extern "system" {
                fn GetStdHandle(kind: u32) -> *mut std::ffi::c_void;
                fn GetConsoleMode(handle: *mut std::ffi::c_void, mode: *mut u32) -> i32;
                fn SetConsoleMode(handle: *mut std::ffi::c_void, mode: u32) -> i32;
                fn ReadConsoleW(
                    handle: *mut std::ffi::c_void,
                    buffer: *mut u16,
                    count: u32,
                    read: *mut u32,
                    control: *mut std::ffi::c_void,
                ) -> i32;
            }
            let evidence = std::env::var("AURAL_TERMINAL_EVIDENCE").unwrap();
            unsafe {
                let input = GetStdHandle((-10i32) as u32);
                let mut mode = 0;
                assert_ne!(GetConsoleMode(input, &mut mode), 0);
                // Raw character input: no command interpreter receives dictated text.
                assert_ne!(SetConsoleMode(input, mode & !0x6), 0);
                std::fs::write(format!("{evidence}.ready"), b"ready").unwrap();
                let expected = "Aural test C:\\path\\with spaces \"two  spaces\" next arg";
                let mut received = String::new();
                while received.len() < expected.len() {
                    let mut buffer = [0u16; 512];
                    let mut count = 0;
                    assert_ne!(
                        ReadConsoleW(
                            input,
                            buffer.as_mut_ptr(),
                            buffer.len() as u32,
                            &mut count,
                            std::ptr::null_mut()
                        ),
                        0
                    );
                    received.push_str(&String::from_utf16_lossy(&buffer[..count as usize]));
                }
                std::fs::write(&evidence, received).unwrap();
                let _ = SetConsoleMode(input, mode);
            }
        }

        #[test]
        #[ignore = "opens a disposable real Windows Terminal tab; run explicitly"]
        fn real_windows_terminal_receives_text_once_and_reports_unidentified_reader_honestly() {
            let _g = CLIPBOARD.lock().unwrap_or_else(|p| p.into_inner());
            let original = snapshot_clipboard().unwrap();
            let evidence =
                std::env::temp_dir().join(format!("aural-terminal-{}.txt", std::process::id()));
            let ready = format!("{}.ready", evidence.display());
            let exe = std::env::current_exe().unwrap();
            let script = format!("$env:AURAL_TERMINAL_EVIDENCE='{}'; & '{}' --exact insert::tests::delayed_render::windows_terminal_input_fixture --ignored --test-threads=1", evidence.display().to_string().replace('\'', "''"), exe.display().to_string().replace('\'', "''"));
            let script_path = evidence.with_extension("ps1");
            std::fs::write(&script_path, script).unwrap();
            let wt = std::path::PathBuf::from(std::env::var("LOCALAPPDATA").unwrap())
                .join("Microsoft/WindowsApps/wt.exe");
            let mut launcher = std::process::Command::new(wt)
                .args([
                    "-w",
                    "new",
                    "new-tab",
                    "--title",
                    "Aural delivery verification",
                    "powershell.exe",
                    "-NoProfile",
                    "-File",
                    script_path.to_str().unwrap(),
                ])
                .spawn()
                .unwrap();
            let result = (|| -> anyhow::Result<()> {
                let deadline = Instant::now() + Duration::from_secs(20);
                while !std::path::Path::new(&ready).exists() && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(25));
                }
                anyhow::ensure!(
                    std::path::Path::new(&ready).exists(),
                    "terminal fixture did not become ready"
                );
                let target = foreground_target();
                anyhow::ensure!(
                    target
                        .class
                        .eq_ignore_ascii_case("CASCADIA_HOSTING_WINDOW_CLASS"),
                    "Windows Terminal is not foreground: {target:?}"
                );
                set_clipboard_text("terminal previous clipboard", false)?;
                let input = "Aural test C:\\path\\with spaces \"two  spaces\"\nnext\targ";
                let outcome = insert(input, target.hwnd)?;
                let deadline = Instant::now() + Duration::from_secs(5);
                while !evidence.exists() && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(10));
                }
                let actual = std::fs::read_to_string(&evidence)?;
                assert_eq!(
                    actual,
                    "Aural test C:\\path\\with spaces \"two  spaces\" next arg"
                );
                // Windows Terminal opens with a NULL clipboard HWND on this desktop.
                // Content observation above proves this fixture received it, while
                // production must conservatively report the unidentifiable reader.
                assert_eq!(outcome, InsertOutcome::PasteSent);
                assert_eq!(read_clipboard_text().as_deref(), Some(actual.as_str()));
                Ok(())
            })();
            // Reap the Windows Terminal launcher after the fixture has completed.
            let launcher_status = launcher.wait();
            restore_clipboard(&original).unwrap();
            let _ = std::fs::remove_file(evidence);
            let _ = std::fs::remove_file(ready);
            let _ = std::fs::remove_file(script_path);
            assert!(
                launcher_status.unwrap().success(),
                "Windows Terminal launcher failed"
            );
            result.unwrap();
        }

        /// What a browser or Office puts on the clipboard: text plus HTML, an image and an
        /// app's own format.
        fn rich_clipboard() -> ClipboardSnapshot {
            let text: Vec<u8> = "plain copy\0"
                .encode_utf16()
                .flat_map(|u| u.to_le_bytes())
                .collect();
            let html = b"Version:0.9\r\nStartHTML:0\r\n<b>rich copy</b>\0".to_vec();
            let mut dib = Vec::new();
            for v in [40i32, 1, 1] {
                dib.extend(v.to_le_bytes());
            }
            dib.extend(1u16.to_le_bytes()); // planes
            dib.extend(32u16.to_le_bytes()); // bits per pixel
            for v in [0u32, 4, 0, 0, 0, 0] {
                dib.extend(v.to_le_bytes());
            }
            dib.extend([0x10, 0x20, 0x30, 0xFF]);
            ClipboardSnapshot::from_items(vec![
                (13, text),
                (register_format("HTML Format"), html),
                (8, dib),
                (
                    register_format("Aural Test Format"),
                    b"app data \x01\x02".to_vec(),
                ),
            ])
        }

        fn data(s: &ClipboardSnapshot, format: u32) -> Option<&[u8]> {
            s.bytes(format)
        }

        fn assert_holds(got: &ClipboardSnapshot, want: &ClipboardSnapshot) {
            for (format, bytes) in want.byte_items() {
                let g = data(got, format).unwrap_or_else(|| panic!("format {format} missing"));
                // Windows may round a block up; the extra bytes are padding.
                assert!(g.starts_with(bytes), "format {format} changed");
            }
        }

        #[test]
        fn the_whole_clipboard_is_saved_and_put_back_exactly() {
            let _g = CLIPBOARD.lock().unwrap_or_else(|p| p.into_inner());
            let rich = rich_clipboard();
            restore_clipboard(&rich).unwrap();
            let saved = snapshot_clipboard().unwrap();
            assert!(saved.complete, "a rich clipboard must not force typing");
            assert_holds(&saved, &rich);
            set_clipboard_text("something else", false).unwrap();
            restore_clipboard(&saved).unwrap();
            assert_holds(&snapshot_clipboard().unwrap(), &rich);
            assert_eq!(read_clipboard_text().as_deref(), Some("plain copy"));
        }

        #[test]
        fn a_paste_puts_back_html_images_and_app_formats_not_just_text() {
            let _g = CLIPBOARD.lock().unwrap_or_else(|p| p.into_inner());
            let rich = rich_clipboard();
            restore_clipboard(&rich).unwrap();
            let saved = snapshot_clipboard().unwrap();
            let (tx, rx) = mpsc::channel();
            let mut press = app_that_pastes(tx);
            let got =
                paste_and_restore("dictated text", &saved, &mut press, Duration::from_secs(2))
                    .unwrap();
            assert_eq!(got, PasteResult::Read);
            assert_eq!(
                rx.recv_timeout(Duration::from_secs(2)).unwrap().as_deref(),
                Some("dictated text")
            );
            assert_holds(&snapshot_clipboard().unwrap(), &rich);
        }

        #[test]
        fn markers_left_by_an_ignored_paste_do_not_force_typing_next_time() {
            let _g = CLIPBOARD.lock().unwrap_or_else(|p| p.into_inner());
            set_clipboard_text("previous clipboard", false).unwrap();
            let saved = snapshot_clipboard().unwrap();
            let mut press = || -> anyhow::Result<()> { Ok(()) };
            let got = paste_and_restore(
                "dictated text",
                &saved,
                &mut press,
                Duration::from_millis(300),
            )
            .unwrap();
            assert_eq!(got, PasteResult::Ignored);
            // The transcript now sits there with Aural's privacy markers.
            assert!(snapshot_clipboard().unwrap().complete);
        }

        #[test]
        fn a_newer_copy_by_the_user_is_never_overwritten() {
            let _g = CLIPBOARD.lock().unwrap_or_else(|p| p.into_inner());
            // Before the target gets to paste, the user copies something else.
            let mut press = || -> anyhow::Result<()> {
                set_clipboard_text("newer copy", false)?;
                Ok(())
            };
            let got = paste_and_restore(
                "dictated text",
                &ClipboardSnapshot::text("previous"),
                &mut press,
                Duration::from_millis(600),
            )
            .unwrap();
            assert_eq!(got, PasteResult::Ignored);
            assert_eq!(read_clipboard_text().as_deref(), Some("newer copy"));
        }
    }

    #[test]
    fn unicode_typing_encodes_surrogate_pairs() {
        let units = utf16_units("a👋é");
        assert_eq!(units, vec![0x61, 0xD83D, 0xDC4B, 0xE9]);
    }
}
