//! Live text in the app: whether it runs for the chosen model, and the feeder thread
//! that streams the recording to the speech worker while the user speaks.
//!
//! The feeder only ever produces text to show. The dictation loop decides what gets
//! inserted (`Dictation`, `live_finish`), so live text can never insert anything itself.

use aural_core::error::ErrorCode;
use aural_engines::live::{LiveMode, LiveText, RATE, STEP_MS};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::Arc;
use std::thread::JoinHandle;

/// Live text needs to keep up comfortably while you talk (same bar as the label).
pub const MAX_RTF: f64 = aural_models::labels::LIVE_TEXT_MAX_RTF;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveStatus {
    /// Turned off in settings.
    Off,
    NoModel,
    /// The model streams: words appear as they are heard.
    Native,
    /// The model reads whole recordings: the phrase being spoken is re-read.
    Phrases,
    /// The model is too slow on this PC to keep up.
    TooSlow,
}

/// `rtf`: the model's speed on this PC (measured, estimated, or the reference).
pub fn status(enabled: bool, model: Option<(bool, Option<f64>)>) -> LiveStatus {
    let Some((streaming, rtf)) = model else {
        return LiveStatus::NoModel;
    };
    if !enabled {
        LiveStatus::Off
    } else if rtf.is_some_and(|r| r > MAX_RTF) {
        LiveStatus::TooSlow
    } else if streaming {
        LiveStatus::Native
    } else {
        LiveStatus::Phrases
    }
}

/// Live text for the model in use, right now.
pub fn for_app(app: &crate::state::App) -> LiveStatus {
    let s = app.settings();
    let model = s
        .stt
        .active_variant
        .as_deref()
        .filter(|_| app.engine.is_ready())
        .and_then(|id| {
            let (entry, v) = app.catalog.variant(id)?;
            let rtf = crate::hwtest::evaluation(app)
                .results
                .iter()
                .find(|r| r.variant == id)
                .and_then(|r| r.metrics.as_ref().map(|m| m.rtf))
                .or_else(|| v.reference.as_ref().map(|r| r.rtf));
            Some((entry.streaming, rtf))
        });
    status(s.live.enabled, model)
}

/// What the feeder needs from the speech engine (`EngineHost` in the app).
pub trait LiveEngine: Send + Sync + 'static {
    fn begin(&self) -> Result<LiveMode, ErrorCode>;
    fn push(&self, pcm: &[f32]) -> Result<LiveText, ErrorCode>;
    fn end(&self, pcm: &[f32], seconds: u64) -> Result<String, ErrorCode>;
    fn cancel(&self);
}

pub struct LiveFeed {
    cancelled: Arc<AtomicBool>,
    thread: JoinHandle<Option<Result<String, ErrorCode>>>,
}

/// Minimum audio per push: pushing smaller pieces only adds round trips.
const MIN_PUSH: usize = STEP_MS * RATE / 1000;

impl LiveFeed {
    /// Start streaming. Audio arrives on `audio` (the capture callback's sender); the
    /// stream ends when every sender is dropped, i.e. when the recording stops.
    pub fn start(
        engine: Arc<dyn LiveEngine>,
        audio: Receiver<Vec<f32>>,
        show: impl Fn(LiveText) + Send + 'static,
    ) -> Self {
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = cancelled.clone();
        let thread = std::thread::Builder::new()
            .name("aural-live".into())
            .spawn(move || run(engine.as_ref(), &audio, &flag, &show))
            .expect("spawn live thread");
        Self { cancelled, thread }
    }

    /// After the recording stopped: the final text (Err when the stream failed).
    pub fn finish(self) -> Option<Result<String, ErrorCode>> {
        self.thread.join().ok().flatten()
    }

    /// Drop the stream without waiting for it; it shows nothing more.
    pub fn cancel(self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }
}

fn run(
    engine: &dyn LiveEngine,
    audio: &Receiver<Vec<f32>>,
    cancelled: &AtomicBool,
    show: &dyn Fn(LiveText),
) -> Option<Result<String, ErrorCode>> {
    let mut failed = engine.begin().err();
    let mut buf: Vec<f32> = Vec::new();
    let mut total = 0usize;
    let mut ended = false;
    while !ended {
        // Wait for audio, then take everything that has arrived: while the engine was
        // busy the recording kept going, and it all goes in one push.
        match audio.recv() {
            Ok(chunk) => buf.extend(chunk),
            Err(_) => break,
        }
        loop {
            match audio.try_recv() {
                Ok(chunk) => buf.extend(chunk),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    ended = true;
                    break;
                }
            }
        }
        if cancelled.load(Ordering::SeqCst) || failed.is_some() || ended || buf.len() < MIN_PUSH {
            continue;
        }
        total += buf.len();
        match engine.push(&std::mem::take(&mut buf)) {
            Ok(text) if !cancelled.load(Ordering::SeqCst) => show(text),
            Ok(_) => {}
            Err(e) => failed = Some(e),
        }
    }
    if cancelled.load(Ordering::SeqCst) {
        engine.cancel();
        return None;
    }
    if let Some(e) = failed {
        engine.cancel();
        return Some(Err(e));
    }
    total += buf.len();
    Some(engine.end(&buf, (total / RATE) as u64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Fake {
        log: Mutex<Vec<String>>,
        fail_push: bool,
        fail_begin: bool,
        pushed: Mutex<usize>,
    }

    impl LiveEngine for Fake {
        fn begin(&self) -> Result<LiveMode, ErrorCode> {
            self.log.lock().unwrap().push("begin".into());
            if self.fail_begin {
                return Err(ErrorCode::EngineFailed);
            }
            Ok(LiveMode::Phrases)
        }
        fn push(&self, pcm: &[f32]) -> Result<LiveText, ErrorCode> {
            self.log.lock().unwrap().push(format!("push {}", pcm.len()));
            if self.fail_push {
                return Err(ErrorCode::EngineFailed);
            }
            let mut p = self.pushed.lock().unwrap();
            *p += pcm.len();
            Ok(LiveText {
                stable: String::new(),
                tentative: format!("{} so far", *p),
            })
        }
        fn end(&self, pcm: &[f32], _: u64) -> Result<String, ErrorCode> {
            self.log.lock().unwrap().push(format!("end {}", pcm.len()));
            Ok("final".into())
        }
        fn cancel(&self) {
            self.log.lock().unwrap().push("cancel".into());
        }
    }

    type Shown = Arc<Mutex<Vec<LiveText>>>;

    fn feed(engine: Arc<Fake>) -> (mpsc::Sender<Vec<f32>>, LiveFeed, Shown) {
        let (tx, rx) = mpsc::channel();
        let shown = Arc::new(Mutex::new(Vec::new()));
        let s = shown.clone();
        let f = LiveFeed::start(engine, rx, move |t| s.lock().unwrap().push(t));
        (tx, f, shown)
    }

    fn log(e: &Fake) -> Vec<String> {
        e.log.lock().unwrap().clone()
    }

    #[test]
    fn audio_is_streamed_shown_and_ends_with_one_final_text() {
        let e = Arc::new(Fake::default());
        let (tx, f, shown) = feed(e.clone());
        for _ in 0..10 {
            tx.send(vec![0.1; 1_600]).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        drop(tx); // recording stopped
        assert_eq!(f.finish(), Some(Ok("final".into())));
        let pushed: usize = log(&e)
            .iter()
            .filter_map(|l| l.strip_prefix("push ").map(|n| n.parse::<usize>().unwrap()))
            .sum();
        let ended: usize = log(&e)
            .iter()
            .find_map(|l| l.strip_prefix("end ").map(|n| n.parse::<usize>().unwrap()))
            .unwrap();
        assert_eq!(pushed + ended, 16_000, "every sample sent exactly once");
        assert!(!shown.lock().unwrap().is_empty());
        assert_eq!(log(&e).iter().filter(|l| l.starts_with("end")).count(), 1);
    }

    #[test]
    fn a_failed_push_stops_streaming_and_reports_the_failure() {
        let e = Arc::new(Fake {
            fail_push: true,
            ..Default::default()
        });
        let (tx, f, shown) = feed(e.clone());
        for _ in 0..5 {
            tx.send(vec![0.1; 8_000]).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        drop(tx);
        assert_eq!(f.finish(), Some(Err(ErrorCode::EngineFailed)));
        assert_eq!(log(&e).iter().filter(|l| l.starts_with("push")).count(), 1);
        assert!(!log(&e).iter().any(|l| l.starts_with("end")));
        assert!(shown.lock().unwrap().is_empty());
    }

    #[test]
    fn a_stream_that_could_not_start_reports_the_failure() {
        let e = Arc::new(Fake {
            fail_begin: true,
            ..Default::default()
        });
        let (tx, f, _) = feed(e.clone());
        tx.send(vec![0.1; 8_000]).unwrap();
        drop(tx);
        assert_eq!(f.finish(), Some(Err(ErrorCode::EngineFailed)));
        assert!(!log(&e).iter().any(|l| l.starts_with("push")));
    }

    #[test]
    fn a_cancelled_stream_shows_nothing_more_and_is_closed() {
        let e = Arc::new(Fake::default());
        let (tx, f, shown) = feed(e.clone());
        tx.send(vec![0.1; 8_000]).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
        let before = shown.lock().unwrap().len();
        f.cancel();
        tx.send(vec![0.1; 8_000]).unwrap();
        drop(tx);
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert_eq!(shown.lock().unwrap().len(), before);
        assert!(log(&e).contains(&"cancel".to_string()));
        assert!(!log(&e).iter().any(|l| l.starts_with("end")));
    }

    #[test]
    fn status_follows_the_setting_the_model_and_its_speed() {
        assert_eq!(status(true, None), LiveStatus::NoModel);
        assert_eq!(status(false, Some((true, Some(0.05)))), LiveStatus::Off);
        assert_eq!(status(true, Some((true, Some(0.05)))), LiveStatus::Native);
        assert_eq!(status(true, Some((false, Some(0.05)))), LiveStatus::Phrases);
        assert_eq!(status(true, Some((false, Some(2.9)))), LiveStatus::TooSlow);
        assert_eq!(status(true, Some((false, None))), LiveStatus::Phrases);
    }
}
