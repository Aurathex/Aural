//! Where Windows thinks Aural's uninstaller is (the per-user uninstall entry written by
//! the installer). Delete Aural only runs an uninstaller that matches this record.

use std::path::PathBuf;

/// Uninstall key name the Tauri NSIS installer uses (the product name).
pub const UNINSTALL_KEY: &str = "Aural";

/// `UninstallString` values look like `"C:\path\uninstall.exe"` or, unquoted,
/// `C:\path\uninstall.exe /S`. Returns the executable path.
pub fn parse_uninstall_string(value: &str) -> Option<PathBuf> {
    let v = value.trim();
    let exe = if let Some(rest) = v.strip_prefix('"') {
        &rest[..rest.find('"')?]
    } else {
        let lower = v.to_ascii_lowercase();
        let end = lower.find(".exe").map(|i| i + 4)?;
        &v[..end]
    };
    (!exe.is_empty()).then(|| PathBuf::from(exe))
}

#[cfg(windows)]
pub fn registered_uninstaller() -> Option<PathBuf> {
    use winreg::enums::HKEY_CURRENT_USER;
    let key = winreg::RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(format!(
            r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{UNINSTALL_KEY}"
        ))
        .ok()?;
    let value: String = key.get_value("UninstallString").ok()?;
    parse_uninstall_string(&value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quoted_paths_with_spaces() {
        assert_eq!(
            parse_uninstall_string(r#""C:\Users\A B\AppData\Local\Aural\uninstall.exe""#),
            Some(PathBuf::from(
                r"C:\Users\A B\AppData\Local\Aural\uninstall.exe"
            ))
        );
    }

    #[test]
    fn parses_unquoted_paths_with_arguments() {
        assert_eq!(
            parse_uninstall_string(r"C:\Apps\Aural\Uninstall.EXE /S"),
            Some(PathBuf::from(r"C:\Apps\Aural\Uninstall.EXE"))
        );
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse_uninstall_string(""), None);
        assert_eq!(parse_uninstall_string(r#""unterminated"#), None);
        assert_eq!(parse_uninstall_string("msiexec"), None);
    }
}
