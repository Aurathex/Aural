//! Platform-independent core of Aural: paths, settings, the dictation state machine and
//! the Delete-Aural plan. No Win32 here, so everything is unit-testable.

pub mod error;
pub mod paths;
pub mod session;
pub mod settings;
pub mod uninstall;

pub const APP_ID: &str = "aural";

#[cfg(test)]
mod tests {
    #[test]
    fn app_id_is_lowercase_ascii() {
        assert!(super::APP_ID.chars().all(|c| c.is_ascii_lowercase()));
    }
}
