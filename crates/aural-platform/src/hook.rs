//! The global hotkey: a low-level keyboard hook (WH_KEYBOARD_LL) on its own thread.
//! Unlike RegisterHotKey it sees key-up (push-to-talk) and modifier-only chords. The
//! callback only updates the matcher and forwards events; Windows silently removes
//! hooks that take too long, so no real work happens inside it.

use crate::chord::{Chord, ChordMatcher, HotkeyEvent};
use anyhow::{bail, Result};
use std::sync::mpsc::Sender;
use std::sync::{Mutex, OnceLock};
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PostThreadMessageW, SetWindowsHookExW,
    TranslateMessage, UnhookWindowsHookEx, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG,
    WH_KEYBOARD_LL, WM_KEYDOWN, WM_QUIT, WM_SYSKEYDOWN,
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

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        // SAFETY: for WH_KEYBOARD_LL with HC_ACTION, lparam points to KBDLLHOOKSTRUCT.
        let kb = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let down = matches!(wparam.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
        let injected = kb.flags.0 & LLKHF_INJECTED.0 != 0;
        // try_lock: never block inside the hook.
        if let Ok(mut guard) = state().try_lock() {
            if let Some(s) = guard.as_mut() {
                if !s.paused {
                    let out =
                        s.matcher
                            .on_key(kb.vkCode as u16, down, injected && !s.accept_injected);
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
                let _ = ready_tx.send(Ok(GetCurrentThreadId()));
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                let _ = UnhookWindowsHookEx(hook);
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
