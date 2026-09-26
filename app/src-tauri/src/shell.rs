//! Opening Settings pages and folders, and launching the uninstaller after Aural exits.

use std::path::Path;

/// Open a URI or folder with the Windows shell (no console window).
pub fn open(target: &str) {
    #[cfg(windows)]
    {
        use windows::core::{HSTRING, PCWSTR};
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let target = HSTRING::from(target);
        let verb = HSTRING::from("open");
        // SAFETY: valid NUL-terminated wide strings for the duration of the call.
        unsafe {
            ShellExecuteW(None, &verb, &target, PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
        }
    }
}

/// Start `program` about two seconds from now, detached, so it runs after Aural has
/// exited and released its files (used for the silent uninstaller).
pub fn run_after_exit(program: &Path, args: &[String]) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        let mut line = format!("ping 127.0.0.1 -n 3 >nul & \"{}\"", program.display());
        for a in args {
            line.push(' ');
            line.push_str(a);
        }
        let _ = std::process::Command::new("cmd.exe")
            .raw_arg("/C")
            .raw_arg(&line)
            .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
            .spawn();
    }
}
