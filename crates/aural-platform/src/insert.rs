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
    let terminal = is_terminal(target);
    let strategy = if target.hwnd == 0 {
        Strategy::ClipboardOnly(Reason::NoTarget)
    } else if target.hwnd != release_hwnd {
        Strategy::ClipboardOnly(Reason::FocusChanged)
    } else if target.elevated && !self_elevated {
        Strategy::ClipboardOnly(Reason::Elevated)
    } else if !clipboard_saved {
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
    // moment the target app actually reads the clipboard (WM_RENDERFORMAT). That tells
    // us the paste happened, so the user's previous clipboard is restored only after
    // the target has the transcript, however slow the target is (RDP, VMs, busy apps).

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum PasteResult {
        /// The target read the text; the previous clipboard was restored if nothing
        /// else had taken the clipboard since.
        Read,
        /// Nothing read it in time; the transcript is left on the clipboard (unless the
        /// user copied something newer), and nothing was restored over it.
        Ignored,
    }

    thread_local! {
        static PENDING: std::cell::RefCell<Option<Vec<u8>>> = const { std::cell::RefCell::new(None) };
        static RENDERED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
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
        match msg {
            WM_RENDERFORMAT => {
                if wparam.0 as u32 == CF_UNICODETEXT {
                    render_pending();
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
                    return Ok(ClipboardGuard);
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
            PENDING.with(|p| p.borrow_mut().take());
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
        let owner = OwnerWindow::create()?;
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
        press()?;
        let pressed = Instant::now();
        let deadline = pressed + timeout;
        while !RENDERED.with(|r| r.get()) && Instant::now() < deadline {
            owner.pump();
            std::thread::sleep(Duration::from_millis(5));
        }
        if RENDERED.with(|r| r.get()) {
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
            let _g = owner.open_clipboard()?;
            if owner.owns_clipboard() {
                render_pending();
            }
            PENDING.with(|p| p.borrow_mut().take());
            Ok(PasteResult::Ignored)
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
    pub enum InsertOutcome {
        Inserted,
        /// Text left on the clipboard for the user; carries why.
        CopiedOnly(Reason),
    }

    /// Deliver `text` to the focused app. The pasted text is never left in clipboard
    /// history, and the user's previous clipboard (every format) is restored afterwards.
    pub fn insert(text: &str, release_hwnd: isize) -> Result<InsertOutcome> {
        let target = foreground_target();
        let snapshot = snapshot_clipboard().unwrap_or(ClipboardSnapshot {
            items: Vec::new(),
            complete: false,
        });
        let p = plan(&target, release_hwnd, snapshot.complete, self_elevated());
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
                let mut press = || -> Result<()> {
                    release_modifiers()?;
                    send(&paste_inputs(chord))
                };
                // Restore happens only after the target has actually read the text.
                match paste_and_restore(&text, &snapshot, &mut press, PASTE_TIMEOUT)? {
                    PasteResult::Read => Ok(InsertOutcome::Inserted),
                    PasteResult::Ignored => Ok(InsertOutcome::CopiedOnly(Reason::PasteIgnored)),
                }
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
