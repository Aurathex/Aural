//! The global hotkey: a low-level keyboard hook (WH_KEYBOARD_LL) on its own thread.
//! Unlike RegisterHotKey it sees key-up (push-to-talk) and modifier-only chords. The
//! callback only updates the matcher and forwards events; Windows silently removes
//! hooks that take too long, so no real work happens inside it.

use crate::chord::{Chord, ChordMatcher, HotkeyEvent};
use anyhow::{bail, Result};
use std::sync::atomic::{AtomicIsize, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Mutex, OnceLock};
use windows::core::w;
use windows::Win32::Foundation::{HMODULE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, VIRTUAL_KEY,
};
use windows::Win32::UI::Input::{RegisterRawInputDevices, RAWINPUTDEVICE, RIDEV_INPUTSINK};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW,
    PostThreadMessageW, RegisterClassW, SetTimer, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, HC_ACTION, HHOOK, HWND_MESSAGE, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG,
    WH_KEYBOARD_LL, WINDOW_EX_STYLE, WINDOW_STYLE, WM_INPUT, WM_KEYDOWN, WM_QUIT, WM_SYSKEYDOWN,
    WM_TIMER, WNDCLASSW,
};

struct HookState {
    matcher: ChordMatcher,
    tx: Sender<HotkeyEvent>,
    accept_injected: bool,
    paused: bool,
}

// One hook per process: the callback has no user data pointer, so state is global.
static STATE: OnceLock<Mutex<Option<HookState>>> = OnceLock::new();

fn state() -> &'static Mutex<Option<HookState>> {
    STATE.get_or_init(|| Mutex::new(None))
}

const MASK_VK: u16 = 0xE8;

/// The installed hook handle, so the watchdog (and tests) can replace it.
static CURRENT_HOOK: AtomicIsize = AtomicIsize::new(0);
/// Keyboard events the hook callback has seen.
static HOOK_KEYS: AtomicU64 = AtomicU64::new(0);
/// Keyboard events Raw Input has seen; Windows delivers these even when it has
/// silently removed the hook.
static RAW_KEYS: AtomicU64 = AtomicU64::new(0);
static REINSTALLS: AtomicU64 = AtomicU64::new(0);

/// How many times the watchdog has had to reinstall the hook in this process.
pub fn reinstalls() -> u64 {
    REINSTALLS.load(Ordering::SeqCst)
}

/// Over one watchdog interval: keys arrived, and the hook saw none of them.
fn hook_looks_dead(raw_keys: u64, hook_keys: u64) -> bool {
    raw_keys > 0 && hook_keys == 0
}

const WATCHDOG_MS: u32 = 1000;
/// At most one reinstall per gap, so a lasting mismatch (for example keys that reach
/// Raw Input but not the hook while an elevated window is in front) can't churn.
const REINSTALL_GAP: std::time::Duration = std::time::Duration::from_secs(10);

/// Install a fresh hook, then remove the old one (a no-op if Windows already did), so
/// there is no moment without a hook. Runs on the hook thread.
unsafe fn reinstall(module: Option<HMODULE>) {
    // SAFETY: same call as the first install, on the same thread.
    if let Ok(new) =
        unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), module.map(|m| m.into()), 0) }
    {
        let old = CURRENT_HOOK.swap(new.0 as isize, Ordering::SeqCst);
        // SAFETY: old is the hook this thread installed earlier.
        let _ = unsafe { UnhookWindowsHookEx(HHOOK(old as *mut std::ffi::c_void)) };
        REINSTALLS.fetch_add(1, Ordering::SeqCst);
    }
}

unsafe extern "system" fn raw_input_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_INPUT {
        RAW_KEYS.fetch_add(1, Ordering::Relaxed);
    }
    // SAFETY: default handling (WM_INPUT needs it to free the input).
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// A message-only window that receives keyboard Raw Input even in the background.
/// Runs on the hook thread; returns None if Windows refuses.
unsafe fn watch_raw_keyboard(module: Option<HMODULE>) -> Option<HWND> {
    let class = w!("AuralHotkeyWatchdog");
    let wc = WNDCLASSW {
        lpfnWndProc: Some(raw_input_proc),
        hInstance: module.map(|m| m.into()).unwrap_or_default(),
        lpszClassName: class,
        ..Default::default()
    };
    // SAFETY: valid class description; registering twice (a second hook) just fails.
    unsafe { RegisterClassW(&wc) };
    // SAFETY: message-only window of the class above.
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            w!(""),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            module.map(|m| m.into()),
            None,
        )
    }
    .ok()?;
    let device = RAWINPUTDEVICE {
        usUsagePage: 0x01, // generic desktop
        usUsage: 0x06,     // keyboard
        dwFlags: RIDEV_INPUTSINK,
        hwndTarget: hwnd,
    };
    // SAFETY: one initialised device description.
    unsafe { RegisterRawInputDevices(&[device], std::mem::size_of::<RAWINPUTDEVICE>() as u32) }
        .ok()?;
    Some(hwnd)
}

/// Test helper: remove the hook the way Windows does, without telling the thread.
#[cfg(test)]
fn drop_hook_like_windows() -> bool {
    let h = CURRENT_HOOK.load(Ordering::SeqCst);
    // SAFETY: h is the hook this process installed.
    unsafe { UnhookWindowsHookEx(HHOOK(h as *mut std::ffi::c_void)) }.is_ok()
}

fn send_mask() {
    let k = |up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(MASK_VK),
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
    };
    // SAFETY: initialised INPUT array. Injected events come back flagged and are
    // ignored by the matcher.
    unsafe { SendInput(&[k(false), k(true)], std::mem::size_of::<INPUT>() as i32) };
}

/// Key state as Windows sees it right now (before the event being processed).
fn physically_down(vk: u16) -> bool {
    // SAFETY: no preconditions.
    (unsafe { GetAsyncKeyState(vk as i32) } as u16) & 0x8000 != 0
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        HOOK_KEYS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: for WH_KEYBOARD_LL with HC_ACTION, lparam points to KBDLLHOOKSTRUCT.
        let kb = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let down = matches!(wparam.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
        let injected = kb.flags.0 & LLKHF_INJECTED.0 != 0;
        // try_lock: never block inside the hook.
        if let Ok(mut guard) = state().try_lock() {
            if let Some(s) = guard.as_mut() {
                if !s.paused {
                    let out = s.matcher.on_key_checked(
                        kb.vkCode as u16,
                        down,
                        injected && !s.accept_injected,
                        &physically_down,
                    );
                    if let Some(ev) = out.event {
                        if ev == HotkeyEvent::Down && s.matcher.chord().needs_mask_key() {
                            // Win/Alt are still held: a mask key now stops their
                            // release from opening Start or a menu bar.
                            send_mask();
                        }
                        let _ = s.tx.send(ev);
                    }
                    if out.swallow {
                        return LRESULT(1);
                    }
                }
            }
        }
    }
    // SAFETY: forwarding the unchanged hook arguments.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

pub struct HookHandle {
    thread_id: u32,
    join: Option<std::thread::JoinHandle<()>>,
}

impl HookHandle {
    pub fn set_chord(&self, chord: Chord) {
        if let Ok(mut g) = state().lock() {
            if let Some(s) = g.as_mut() {
                s.matcher = ChordMatcher::new(chord);
            }
        }
    }

    /// While paused (e.g. the settings hotkey recorder is open) keys pass through.
    pub fn set_paused(&self, paused: bool) {
        if let Ok(mut g) = state().lock() {
            if let Some(s) = g.as_mut() {
                s.paused = paused;
            }
        }
    }
}

impl Drop for HookHandle {
    fn drop(&mut self) {
        // SAFETY: posting WM_QUIT to our own hook thread.
        let _ = unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) };
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
        if let Ok(mut g) = state().lock() {
            *g = None;
        }
    }
}

pub fn spawn(chord: Chord, tx: Sender<HotkeyEvent>) -> Result<HookHandle> {
    spawn_with(chord, tx, false)
}

/// `accept_injected` exists for integration tests that drive the hook with SendInput.
pub fn spawn_with(
    chord: Chord,
    tx: Sender<HotkeyEvent>,
    accept_injected: bool,
) -> Result<HookHandle> {
    {
        let mut g = state()
            .lock()
            .map_err(|_| anyhow::anyhow!("hook state poisoned"))?;
        if g.is_some() {
            bail!("the hotkey hook is already running");
        }
        *g = Some(HookState {
            matcher: ChordMatcher::new(chord),
            tx,
            accept_injected,
            paused: false,
        });
    }
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<u32, String>>();
    let join = std::thread::Builder::new()
        .name("aural-hotkey".into())
        .spawn(move || {
            // SAFETY: standard hook install / message loop / uninstall on one thread.
            unsafe {
                let module = GetModuleHandleW(None).ok();
                let hook = match SetWindowsHookExW(
                    WH_KEYBOARD_LL,
                    Some(hook_proc),
                    module.map(|m| m.into()),
                    0,
                ) {
                    Ok(h) => h,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e.to_string()));
                        return;
                    }
                };
                CURRENT_HOOK.store(hook.0 as isize, Ordering::SeqCst);
                // Without Raw Input the watchdog has nothing to compare against and
                // stays idle; the hook itself still works.
                let _raw_window = watch_raw_keyboard(module);
                let _ = SetTimer(None, 0, WATCHDOG_MS, None);
                let _ = ready_tx.send(Ok(GetCurrentThreadId()));
                let mut last = (
                    RAW_KEYS.load(Ordering::Relaxed),
                    HOOK_KEYS.load(Ordering::Relaxed),
                );
                let mut last_reinstall: Option<std::time::Instant> = None;
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    if msg.message == WM_TIMER && msg.hwnd.is_invalid() {
                        let now = (
                            RAW_KEYS.load(Ordering::Relaxed),
                            HOOK_KEYS.load(Ordering::Relaxed),
                        );
                        let rested = last_reinstall.is_none_or(|t| t.elapsed() >= REINSTALL_GAP);
                        if hook_looks_dead(now.0 - last.0, now.1 - last.1) && rested {
                            reinstall(module);
                            last_reinstall = Some(std::time::Instant::now());
                        }
                        last = now;
                        continue;
                    }
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                let current = CURRENT_HOOK.swap(0, Ordering::SeqCst);
                let _ = UnhookWindowsHookEx(HHOOK(current as *mut std::ffi::c_void));
            }
        })?;
    match ready_rx.recv() {
        Ok(Ok(thread_id)) => Ok(HookHandle {
            thread_id,
            join: Some(join),
        }),
        Ok(Err(e)) => {
            let _ = join.join();
            if let Ok(mut g) = state().lock() {
                *g = None;
            }
            bail!("could not install the keyboard hook: {e}")
        }
        Err(_) => bail!("keyboard hook thread exited"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn press(vk: u16, up: bool) {
        let i = INPUT {
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
        };
        // SAFETY: initialised INPUT.
        let sent = unsafe { SendInput(&[i], std::mem::size_of::<INPUT>() as i32) };
        assert_eq!(
            sent, 1,
            "Windows refused synthetic input: this test needs an unlocked, interactive desktop"
        );
    }

    #[test]
    fn hook_counts_as_dead_only_when_keys_arrive_without_it() {
        assert!(hook_looks_dead(3, 0));
        assert!(!hook_looks_dead(0, 0), "no typing: nothing to judge");
        assert!(!hook_looks_dead(3, 3));
        assert!(
            !hook_looks_dead(3, 1),
            "a partial count is a timing edge, not a dead hook"
        );
    }

    /// Windows removes a hook that is too slow without telling the app. Simulate that
    /// by unhooking behind the thread's back, type something, and expect the chord to
    /// work again once the watchdog notices. Run with `-- --ignored`.
    #[test]
    #[ignore]
    fn hook_is_reinstalled_after_windows_drops_it() {
        let (tx, rx) = std::sync::mpsc::channel();
        let chord = Chord::parse(&["F13".into()]).unwrap();
        let handle = spawn_with(chord, tx, true).unwrap();
        let before = reinstalls();
        let (raw0, hook0) = (
            RAW_KEYS.load(Ordering::SeqCst),
            HOOK_KEYS.load(Ordering::SeqCst),
        );
        assert!(drop_hook_like_windows(), "unhooking failed");
        // Keys the dead hook never sees (F14 is unused, like F13).
        for _ in 0..3 {
            press(0x7D, false);
            press(0x7D, true);
            std::thread::sleep(Duration::from_millis(100));
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while reinstalls() == before && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(
            reinstalls() > before,
            "the watchdog never reinstalled the hook (raw keys +{}, hook keys +{})",
            RAW_KEYS.load(Ordering::SeqCst) - raw0,
            HOOK_KEYS.load(Ordering::SeqCst) - hook0
        );
        while rx.try_recv().is_ok() {}
        press(0x7C, false);
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            HotkeyEvent::Down
        );
        press(0x7C, true);
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            HotkeyEvent::Up
        );
        drop(handle);
    }

    /// Drives the real Windows hook with synthetic F13 presses (F13 exists on almost
    /// no keyboards, so nothing else reacts). Run with `-- --ignored`.
    #[test]
    #[ignore]
    fn real_hook_reports_press_and_release() {
        let (tx, rx) = std::sync::mpsc::channel();
        let chord = Chord::parse(&["F13".into()]).unwrap();
        let handle = spawn_with(chord, tx, true).unwrap();
        press(0x7C, false);
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            HotkeyEvent::Down
        );
        press(0x7C, true);
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            HotkeyEvent::Up
        );
        // A second hook in the same process is refused.
        let (tx2, _rx2) = std::sync::mpsc::channel();
        assert!(spawn(Chord::parse(&["F14".into()]).unwrap(), tx2).is_err());
        drop(handle);
        // After shutdown a new hook can be installed again.
        let (tx3, _rx3) = std::sync::mpsc::channel();
        drop(spawn(Chord::parse(&["F14".into()]).unwrap(), tx3).unwrap());
    }
}
