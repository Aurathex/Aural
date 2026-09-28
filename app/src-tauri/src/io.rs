//! The real world behind `DictationIo`: microphone, speech worker, Win32 insertion,
//! clipboard, the pill window and timers.

use crate::dictation::{DictationIo, Input, PillView};
use crate::live::{LiveEngine, LiveFeed, LiveStatus};
use crate::state::{lock, App, Control};
use aural_audio::capture::{self, AudioCallback, CaptureError, CaptureHandle};
use aural_core::error::ErrorCode;
use aural_core::session::PillState;
use aural_platform::consent::{mic_consent, MicConsent};
use aural_platform::insert::{self, InsertOutcome, Reason};
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::time::Duration;
use tauri::{Emitter, Manager};

pub struct AppIo {
    pub app: Arc<App>,
    capture: Option<CaptureHandle>,
    /// The last pill state shown, to pick the start/stop sound.
    last_pill: PillState,
    /// The start sound played for the current recording, so it gets muted out.
    start_cue_played: bool,
    /// Live text for the current recording, when it runs.
    live: Option<LiveFeed>,
    /// A cancelled stream that may still be closing itself; joined before the next one
    /// opens (the worker cancels whichever stream is open).
    stale: Option<LiveFeed>,
    /// This session's pill has a live-text caption (until the pill hides).
    caption: bool,
    /// Facts about the current dictation, for history and statistics.
    session: crate::text::Session,
}

/// The speech engine as the live-text feeder sees it.
struct AppEngine(Arc<App>);

impl LiveEngine for AppEngine {
    fn begin(&self) -> Result<aural_engines::live::LiveMode, ErrorCode> {
        self.0.engine.live_begin()
    }
    fn push(&self, pcm: &[f32]) -> Result<aural_engines::live::LiveText, ErrorCode> {
        self.0.engine.live_push(pcm)
    }
    fn end(&self, pcm: &[f32], seconds: u64) -> Result<String, ErrorCode> {
        self.0.engine.live_end(pcm, seconds)
    }
    fn cancel(&self) {
        self.0.engine.live_cancel()
    }
}

impl AppIo {
    pub fn new(app: Arc<App>) -> Self {
        Self {
            app,
            capture: None,
            last_pill: PillState::Hidden,
            start_cue_played: false,
            live: None,
            stale: None,
            caption: false,
            session: Default::default(),
        }
    }

    fn open(
        &self,
        device: Option<&str>,
        live: Option<Sender<Vec<f32>>>,
    ) -> Result<CaptureHandle, CaptureError> {
        let handle = self.app.handle.clone();
        // The start cue is muted out of the live audio exactly as out of the recording.
        let mut mute = if self.app.settings().ui.sounds {
            crate::sounds::START_CUE_MUTE_SAMPLES
        } else {
            0
        };
        let on_audio = live.map(|tx| -> AudioCallback {
            Box::new(move |pcm: &[f32]| {
                let mut piece = pcm.to_vec();
                crate::sounds::mute_start_cue_piece(&mut piece, &mut mute);
                let _ = tx.send(piece);
            })
        });
        capture::start_live(
            device,
            Box::new(move |frame| {
                let _ = handle.emit_to("pill", "levels", frame.bands);
            }),
            on_audio,
        )
    }

    /// Start live text for a new recording when it's on and the model keeps up.
    fn start_live(&mut self) -> Option<Sender<Vec<f32>>> {
        // The app in front can turn live text off (its profile).
        let front = insert::foreground_target().process;
        if !aural_text::profile::resolve(&self.app.settings(), &front).live {
            return None;
        }
        if !matches!(
            crate::live::for_app(&self.app),
            LiveStatus::Native | LiveStatus::Phrases
        ) {
            return None;
        }
        if let Some(old) = self.stale.take() {
            old.finish();
        }
        let (tx, rx) = mpsc::channel();
        let handle = self.app.handle.clone();
        self.live = Some(LiveFeed::start(
            Arc::new(AppEngine(self.app.clone())),
            rx,
            move |text| {
                let _ = handle.emit_to("pill", "live", text);
            },
        ));
        Some(tx)
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
        let live = self.start_live();
        self.caption = live.is_some();
        self.session = crate::text::Session {
            live: live.is_some(),
            ..Default::default()
        };
        // A chosen microphone that was unplugged falls back to the Windows default.
        let opened = match self.open(device.as_deref(), live.clone()) {
            Err(CaptureError::DeviceNotFound(_)) => self.open(None, live.clone()),
            other => other,
        };
        // The capture callback now holds the only sender: the live stream ends when
        // the recording does.
        drop(live);
        match opened {
            Ok(handle) => {
                self.capture = Some(handle);
                Ok(())
            }
            Err(e) => {
                self.live_cancel();
                Err(capture_error(e))
            }
        }
    }

    fn stop_capture(&mut self) -> Vec<f32> {
        let mut pcm = self.capture.take().map(|c| c.stop()).unwrap_or_default();
        self.session.audio_ms = pcm.len() as u64 / 16;
        // The text goes to the app in front when the user stops (its profile applies).
        self.session.app = insert::foreground_target().process;
        if std::mem::take(&mut self.start_cue_played) {
            crate::sounds::mute_start_cue(&mut pcm);
        }
        pcm
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

    fn pill(&mut self, mut view: PillView) {
        let Some(window) = self.app.handle.get_webview_window("pill") else {
            return;
        };
        if let Some(cue) = crate::sounds::cue_for(&self.last_pill, &view.state) {
            if self.app.settings().ui.sounds {
                crate::sounds::play(cue);
                if cue == crate::sounds::Cue::Start {
                    self.start_cue_played = true;
                }
            }
        }
        self.last_pill = view.state;
        let hidden = view.state == PillState::Hidden;
        let position = self.app.settings().ui.pill_position;
        view.caption = self.caption.then_some(position);
        if hidden {
            self.caption = false;
        }
        self.app.pill_hidden.store(hidden, Ordering::SeqCst);
        if !hidden {
            crate::pill::show(&window, position, view.caption.is_some());
        }
        let _ = self.app.handle.emit_to("pill", "pill", &view);
        if hidden {
            // Let the exit animation (200 ms, Pill.svelte) play, then hide unless a new
            // session started meanwhile.
            let app = self.app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(280));
                if app.pill_hidden.load(Ordering::SeqCst) {
                    if let Some(w) = app.handle.get_webview_window("pill") {
                        crate::pill::hide(&w);
                    }
                }
            });
        }
    }

    fn live_finish(&mut self) -> Option<Result<String, ErrorCode>> {
        self.live.take().and_then(LiveFeed::finish)
    }

    fn live_cancel(&mut self) {
        if let Some(feed) = self.live.take() {
            feed.cancel();
            self.stale = Some(feed);
        }
    }

    fn polish(&mut self, raw: &str) -> String {
        let settings = self.app.settings();
        let words = lock(&self.app.words).clone();
        let engine = &self.app.text_engine;
        let handle = &self.app.handle;
        let caption = self.caption.then_some(settings.ui.pill_position);
        let p = crate::text::polish(&settings, &words, &self.session.app, raw, |t| {
            // Only reached when AI tidy-up runs: say so on the pill meanwhile.
            let _ = handle.emit_to(
                "pill",
                "pill",
                &PillView {
                    state: PillState::Processing,
                    label: Some(crate::dictation::AI_TIDYING.to_owned()),
                    caption,
                },
            );
            engine.rewrite(t)
        });
        self.session.cleaned = p.cleaned;
        self.session.history = p.effective.history;
        // A writing helper stopped after a timeout starts again for the next dictation.
        if p.effective.cleanup == aural_core::settings::CleanupMode::Ai
            && self.app.text_engine.status() == crate::text_engine::TextStatus::Off
        {
            crate::text::reload_text_engine(&self.app);
        }
        p.text
    }

    fn record(&mut self, raw: &str, text: &str, _inserted: bool) {
        crate::text::record(&self.app, raw, text, &self.session);
    }

    fn schedule(&mut self, after_ms: u64, input: Input) {
        let app = self.app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(after_ms));
            app.send(Control::Input(input));
        });
    }
}
