//! User-facing error codes. Each maps to a short pill label (at most 20 characters) and
//! the settings page that can fix it.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    MicBlocked,
    MicUnavailable,
    NoModel,
    EngineFailed,
    InsertBlocked,
    FocusChanged,
    InsertFailed,
}

impl ErrorCode {
    pub const ALL: [ErrorCode; 7] = [
        ErrorCode::MicBlocked,
        ErrorCode::MicUnavailable,
        ErrorCode::NoModel,
        ErrorCode::EngineFailed,
        ErrorCode::InsertBlocked,
        ErrorCode::FocusChanged,
        ErrorCode::InsertFailed,
    ];

    /// Short text for the pill.
    pub fn label(self) -> &'static str {
        match self {
            ErrorCode::MicBlocked => "Mic blocked",
            ErrorCode::MicUnavailable => "No microphone",
            ErrorCode::NoModel => "No model",
            ErrorCode::EngineFailed => "Engine error",
            // Text is on the clipboard in these three cases.
            ErrorCode::InsertBlocked => "Copied — Ctrl+V",
            ErrorCode::FocusChanged => "Copied — Ctrl+V",
            ErrorCode::InsertFailed => "Copied — Ctrl+V",
        }
    }

    /// Settings page that resolves the problem, if any.
    pub fn settings_page(self) -> Option<&'static str> {
        match self {
            ErrorCode::MicBlocked | ErrorCode::MicUnavailable => Some("microphone"),
            ErrorCode::NoModel | ErrorCode::EngineFailed => Some("models"),
            ErrorCode::InsertBlocked | ErrorCode::FocusChanged | ErrorCode::InsertFailed => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_label_fits_the_pill() {
        for code in ErrorCode::ALL {
            let l = code.label();
            assert!(!l.is_empty() && l.chars().count() <= 20, "{code:?}: {l:?}");
        }
    }

    #[test]
    fn codes_serialize_snake_case_for_the_ui() {
        assert_eq!(
            serde_json::to_string(&ErrorCode::MicBlocked).unwrap(),
            "\"mic_blocked\""
        );
    }

    #[test]
    fn model_problems_point_to_models_page() {
        assert_eq!(ErrorCode::NoModel.settings_page(), Some("models"));
        assert_eq!(ErrorCode::MicBlocked.settings_page(), Some("microphone"));
        assert_eq!(ErrorCode::InsertBlocked.settings_page(), None);
    }
}
