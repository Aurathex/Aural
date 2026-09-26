//! "Start Aural with Windows": a value under HKCU\...\CurrentVersion\Run. Per-user, no
//! admin, and removed again by Delete Aural and by the uninstaller.

use anyhow::{Context, Result};
use std::path::Path;

pub const VALUE_NAME: &str = "Aural";
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

pub fn command_line(exe: &Path) -> String {
    format!("\"{}\" --autostart", exe.display())
}

#[cfg(windows)]
pub fn set_enabled(name: &str, exe: &Path, enabled: bool) -> Result<()> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE};
    let (key, _) = winreg::RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey_with_flags(RUN_KEY, KEY_READ | KEY_SET_VALUE)
        .context("opening the Run key")?;
    if enabled {
        key.set_value(name, &command_line(exe))
            .context("writing the startup entry")
    } else {
        match key.delete_value(name) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e).context("removing the startup entry"),
        }
    }
}

/// Enabled only when the entry exists and points at this executable.
#[cfg(windows)]
pub fn is_enabled(name: &str, exe: &Path) -> Result<bool> {
    use winreg::enums::HKEY_CURRENT_USER;
    let Ok(key) = winreg::RegKey::predef(HKEY_CURRENT_USER).open_subkey(RUN_KEY) else {
        return Ok(false);
    };
    match key.get_value::<String, _>(name) {
        Ok(v) => Ok(v == command_line(exe)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e).context("reading the startup entry"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_quotes_the_path_and_marks_autostart() {
        assert_eq!(
            command_line(std::path::Path::new(r"C:\Program Files\Aural\aural.exe")),
            r#""C:\Program Files\Aural\aural.exe" --autostart"#
        );
    }

    /// Writes and removes a uniquely named value in the real HKCU Run key.
    #[test]
    #[ignore]
    fn enable_disable_round_trip_in_the_registry() {
        let name = format!("AuralTest{}", std::process::id());
        let exe = std::path::Path::new(r"C:\Test\aural.exe");
        assert!(!is_enabled(&name, exe).unwrap());
        set_enabled(&name, exe, true).unwrap();
        assert!(is_enabled(&name, exe).unwrap());
        // Pointing at a different exe (e.g. after reinstall elsewhere) is not "enabled".
        assert!(!is_enabled(&name, std::path::Path::new(r"C:\Other\aural.exe")).unwrap());
        set_enabled(&name, exe, false).unwrap();
        assert!(!is_enabled(&name, exe).unwrap());
        // Disabling twice is fine.
        set_enabled(&name, exe, false).unwrap();
    }
}
