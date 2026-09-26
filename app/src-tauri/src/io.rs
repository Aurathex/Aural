//! The real world behind `DictationIo`: microphone, speech worker, Win32 insertion,
//! clipboard, the pill window and timers.

use crate::dictation::{DictationIo, Input, PillView};
use crate::state::{lock, App, Control};
use aural_audio::capture::{self, CaptureError, CaptureHandle};
use aural_core::error::ErrorCode;
use aural_core::session::PillState;
use aural_platform::consent::{mic_consent, MicConsent};
use aural_platform::insert::{self, InsertOutcome, Reason};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tauri::{Emitter, Manager};

pub struct AppIo {
    pub app: Arc<App>,
    capture: Option<CaptureHandle>,
}

impl AppIo {
    pub fn new(app: Arc<App>) -> Self {
        Self { app, capture: None }
    }

    fn open(&self, device: Option<&str>) -> Result<CaptureHandle, CaptureError> {
        let handle = self.app.handle.clone();
        capture::start(
            device,
            Box::new(move |frame| {
                let _ = handle.emit_to("pill", "levels", frame.bands);
            }),
        )
    }
}

fn capture_error(e: CaptureError) -> ErrorCode {
    match e {
        CaptureError::PermissionDenied => ErrorCode::MicBlocked,
        _ => ErrorCode::MicUnavailable,
    }
}

impl DictationIo for AppIo {
    fn engine_ready(&self) -> bool {
        self.app.engine.is_ready()
    }

    fn start_capture(&mut self) -> Result<(), ErrorCode> {
        if mic_consent() == MicConsent::Blocked {
            return Err(ErrorCode::MicBlocked);
        }
        let device = self.app.settings().audio.device;
        // A chosen microphone that was unplugged falls back to the Windows default.
        let handle = match self.open(device.as_deref()) {
            Err(CaptureError::DeviceNotFound(_)) => self.open(None),
            other => other,
        }
        .map_err(capture_error)?;
        self.capture = Some(handle);
        Ok(())
    }

    fn stop_capture(&mut self) -> Vec<f32> {
        self.capture.take().map(|c| c.stop()).unwrap_or_default()
    }

    fn transcribe(&mut self, pcm: &[f32]) -> Result<String, ErrorCode> {
        self.app.engine.transcribe(pcm)
    }

    fn foreground(&self) -> isize {
        insert::foreground_window()
    }

    fn insert(&mut self, text: &str, release_hwnd: isize) -> InsertOutcome {
        match insert::insert(text, release_hwnd) {
            Ok(outcome) => outcome,
            Err(_) => {
                let _ = insert::set_clipboard_text(text, false);
                InsertOutcome::CopiedOnly(Reason::NoTarget)
            }
        }
    }

    fn copy(&mut self, text: &str) {
        let _ = insert::set_clipboard_text(text, false);
    }

    fn remember(&mut self, text: &str) {
        *lock(&self.app.last_transcript) = Some(text.to_owned());
        if let Some(item) = lock(&self.app.tray_paste).as_ref() {
            let _ = item.set_enabled(true);
        }
    }

    fn pill(&mut self, view: PillView) {
        let Some(window) = self.app.handle.get_webview_window("pill") else {
            return;
        };
        let hidden = view.state == PillState::Hidden;
        self.app.pill_hidden.store(hidden, Ordering::SeqCst);
        if !hidden {
            crate::pill::show(&window, self.app.settings().ui.pill_position);
        }
        let _ = self.app.handle.emit_to("pill", "pill", &view);
        if hidden {
            // Let the fade-out play, then hide unless a new session started meanwhile.
            let app = self.app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(240));
                if app.pill_hidden.load(Ordering::SeqCst) {
                    if let Some(w) = app.handle.get_webview_window("pill") {
                        crate::pill::hide(&w);
                    }
                }
            });
        }
    }

    fn schedule(&mut self, after_ms: u64, input: Input) {
        let app = self.app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(after_ms));
            app.send(Control::Input(input));
        });
    }
}
