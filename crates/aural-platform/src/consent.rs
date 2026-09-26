//! Windows microphone privacy switches ("Microphone access" and "Let desktop apps
//! access your microphone"). When either is off, capture fails or returns silence, so
//! Aural checks first and sends the user to the right Settings page.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MicConsent {
    Allowed,
    Blocked,
}

/// `global` is ConsentStore\microphone\Value, `desktop` is ...\microphone\NonPackaged\Value.
/// Missing values mean Windows' default, which is allowed.
pub fn interpret(global: Option<&str>, desktop: Option<&str>) -> MicConsent {
    if global == Some("Deny") || desktop == Some("Deny") {
        MicConsent::Blocked
    } else {
        MicConsent::Allowed
    }
}

/// Page to open when blocked.
pub const PRIVACY_SETTINGS_URI: &str = "ms-settings:privacy-microphone";

#[cfg(windows)]
pub fn mic_consent() -> MicConsent {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;
    const KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone";
    let read = |hive, sub: &str| -> Option<String> {
        RegKey::predef(hive)
            .open_subkey(sub)
            .ok()?
            .get_value::<String, _>("Value")
            .ok()
    };
    let desktop_key = format!(r"{KEY}\NonPackaged");
    // A machine-wide Deny (policy) wins over the user setting.
    let machine = interpret(
        read(HKEY_LOCAL_MACHINE, KEY).as_deref(),
        read(HKEY_LOCAL_MACHINE, &desktop_key).as_deref(),
    );
    if machine == MicConsent::Blocked {
        return MicConsent::Blocked;
    }
    interpret(
        read(HKEY_CURRENT_USER, KEY).as_deref(),
        read(HKEY_CURRENT_USER, &desktop_key).as_deref(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interprets_consent_store_values() {
        assert_eq!(interpret(Some("Allow"), Some("Allow")), MicConsent::Allowed);
        assert_eq!(interpret(None, None), MicConsent::Allowed);
        assert_eq!(interpret(Some("Deny"), Some("Allow")), MicConsent::Blocked);
        assert_eq!(interpret(Some("Allow"), Some("Deny")), MicConsent::Blocked);
    }

    #[test]
    #[ignore]
    fn reads_the_real_consent_store() {
        // Only checks that reading works on this machine.
        let _ = mic_consent();
    }
}
