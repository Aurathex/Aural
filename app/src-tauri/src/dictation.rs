//! The dictation loop: feeds hotkey and engine events through the pure session state
//! machine (`aural_core::session`) and carries out its effects through `DictationIo`,
//! which the app implements with the real microphone, worker and Win32 insertion and
//! the tests implement with a fake.

use aural_core::error::ErrorCode;
use aural_core::session::{step, Effect, Event, PillState, State};
pub use aural_core::settings::HotkeyMode;
use aural_platform::chord::HotkeyEvent;
use aural_platform::insert::{InsertOutcome, Reason};
use serde::Serialize;
use std::collections::VecDeque;

/// A forgotten toggle-mode recording stops itself after five minutes.
pub const MAX_RECORDING_MS: u64 = 5 * 60 * 1000;
const SUCCESS_MS: u64 = 700;
const ERROR_MS: u64 = 2_500;
const RATE: u32 = 16_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    Hotkey {
        event: HotkeyEvent,
        t_ms: u64,
    },
    /// End of the success/error beat for the given session.
    Settle {
        session: u64,
    },
    MaxDuration {
        session: u64,
    },
}

/// What the pill shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PillView {
    pub state: PillState,
    pub label: Option<String>,
    /// Where the pill is, when it has a live-text caption this session.
    pub caption: Option<aural_core::settings::PillPosition>,
}

/// Everything the dictation loop needs from the outside world.
pub trait DictationIo {
    fn engine_ready(&self) -> bool;
    fn start_capture(&mut self) -> Result<(), ErrorCode>;
    /// Stop recording; 16 kHz mono (empty when nothing was recording).
    fn stop_capture(&mut self) -> Vec<f32>;
    fn transcribe(&mut self, pcm: &[f32]) -> Result<String, ErrorCode>;
    /// Window that has keyboard focus right now.
    fn foreground(&self) -> isize;
    fn insert(&mut self, text: &str, release_hwnd: isize) -> InsertOutcome;
    fn copy(&mut self, text: &str);
    fn remember(&mut self, text: &str);
    fn pill(&mut self, view: PillView);
    fn schedule(&mut self, after_ms: u64, input: Input);
    /// End the live-text stream started with the recording (after `stop_capture`):
    /// its final text, or None when there was no live stream.
    fn live_finish(&mut self) -> Option<Result<String, ErrorCode>>;
    /// Drop the live-text stream, if any, without using it.
    fn live_cancel(&mut self);
}

pub struct Dictation<I: DictationIo> {
    pub io: I,
    state: State,
    mode: HotkeyMode,
    session: u64,
    release_hwnd: isize,
}

fn insert_failure(reason: Reason) -> ErrorCode {
    match reason {
        Reason::Elevated => ErrorCode::InsertBlocked,
        Reason::FocusChanged => ErrorCode::FocusChanged,
        Reason::NoTarget | Reason::PasteIgnored => ErrorCode::InsertFailed,
    }
}

impl<I: DictationIo> Dictation<I> {
    pub fn new(io: I, mode: HotkeyMode) -> Self {
        Self {
            io,
            state: State::Idle,
            mode,
            session: 0,
            release_hwnd: 0,
        }
    }

    pub fn set_mode(&mut self, mode: HotkeyMode) {
        self.mode = mode;
    }

    pub fn handle(&mut self, input: Input) {
        let event = match input {
            Input::Hotkey { event, t_ms } => match event {
                HotkeyEvent::Down => Event::HotkeyDown { t_ms },
                HotkeyEvent::Up => Event::HotkeyUp { t_ms },
                HotkeyEvent::Cancel => Event::Cancel,
            },
            Input::Settle { session } if session == self.session => Event::Settled,
            Input::MaxDuration { session } if session == self.session => Event::MaxDurationReached,
            // Timer from an earlier session: ignore.
            Input::Settle { .. } | Input::MaxDuration { .. } => return,
        };
        self.feed(event);
    }

    fn feed(&mut self, first: Event) {
        let mut queue = VecDeque::from([first]);
        while let Some(event) = queue.pop_front() {
            let was_listening = matches!(self.state, State::Listening { .. });
            let (next, effects) = step(self.state.clone(), event, self.mode);
            if !was_listening && matches!(next, State::Listening { .. }) {
                self.session += 1;
                self.io.schedule(
                    MAX_RECORDING_MS,
                    Input::MaxDuration {
                        session: self.session,
                    },
                );
            }
            self.state = next;
            for effect in effects {
                self.execute(effect, &mut queue);
            }
        }
    }

    fn execute(&mut self, effect: Effect, queue: &mut VecDeque<Event>) {
        match effect {
            Effect::StartCapture => {
                if !self.io.engine_ready() {
                    queue.push_back(Event::Failed(ErrorCode::NoModel));
                } else if let Err(code) = self.io.start_capture() {
                    queue.push_back(Event::Failed(code));
                }
            }
            Effect::StopCaptureDiscard => {
                self.io.stop_capture();
                self.io.live_cancel();
            }
            Effect::StopCaptureAndTranscribe => {
                self.release_hwnd = self.io.foreground();
                let pcm = self.io.stop_capture();
                let speech = aural_audio::gate::trim_silence(&pcm, RATE);
                let event = if speech.is_empty() {
                    self.io.live_cancel();
                    Event::TranscriptEmpty
                } else {
                    // Live text already read the recording; its final text is used
                    // unless the stream failed or heard nothing, in which case the
                    // whole recording is read the ordinary way.
                    let live = match self.io.live_finish() {
                        Some(Ok(text)) if !text.trim().is_empty() => Some(text),
                        _ => None,
                    };
                    match live.map_or_else(|| self.io.transcribe(speech), Ok) {
                        Ok(text) if text.trim().is_empty() => Event::TranscriptEmpty,
                        Ok(text) => Event::Transcript(text),
                        Err(code) => Event::Failed(code),
                    }
                };
                queue.push_back(event);
            }
            Effect::Insert(text) => {
                let event = match self.io.insert(&text, self.release_hwnd) {
                    InsertOutcome::Inserted => Event::Inserted,
                    InsertOutcome::CopiedOnly(reason) => {
                        Event::InsertFailed(insert_failure(reason))
                    }
                };
                queue.push_back(event);
            }
            Effect::CopyToClipboard(text) => self.io.copy(&text),
            Effect::RememberLast(text) => self.io.remember(&text),
            Effect::Pill(state) => {
                let label = match state {
                    PillState::Error(code) => Some(code.label().to_owned()),
                    _ => None,
                };
                self.io.pill(PillView {
                    state,
                    label,
                    caption: None,
                });
                let settle = match state {
                    PillState::Success => Some(SUCCESS_MS),
                    PillState::Error(_) => Some(ERROR_MS),
                    _ => None,
                };
                if let Some(ms) = settle {
                    self.io.schedule(
                        ms,
                        Input::Settle {
                            session: self.session,
                        },
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aural_core::error::ErrorCode;
    use aural_core::session::PillState;
    use aural_platform::chord::HotkeyEvent;
    use aural_platform::insert::{InsertOutcome, Reason};

    #[derive(Default)]
    struct Fake {
        log: Vec<String>,
        pcm: Vec<f32>,
        engine_ready: bool,
        transcript: Option<Result<String, ErrorCode>>,
        insert_result: Option<InsertOutcome>,
        mic_error: Option<ErrorCode>,
        pills: Vec<PillView>,
        scheduled: Vec<(u64, Input)>,
        copied: Vec<String>,
        last: Option<String>,
        /// What the live stream ends with (None: no live stream this time).
        live: Option<Result<String, ErrorCode>>,
    }

    impl DictationIo for Fake {
        fn engine_ready(&self) -> bool {
            self.engine_ready
        }
        fn start_capture(&mut self) -> Result<(), ErrorCode> {
            self.log.push("start".into());
            match self.mic_error {
                Some(e) => Err(e),
                None => Ok(()),
            }
        }
        fn stop_capture(&mut self) -> Vec<f32> {
            self.log.push("stop".into());
            std::mem::take(&mut self.pcm)
        }
        fn transcribe(&mut self, pcm: &[f32]) -> Result<String, ErrorCode> {
            self.log.push(format!("transcribe {}", pcm.len()));
            self.transcript.clone().unwrap_or(Ok(String::new()))
        }
        fn foreground(&self) -> isize {
            77
        }
        fn insert(&mut self, text: &str, release_hwnd: isize) -> InsertOutcome {
            self.log
                .push(format!("insert {text:?} into {release_hwnd}"));
            self.insert_result.unwrap_or(InsertOutcome::Inserted)
        }
        fn copy(&mut self, text: &str) {
            self.copied.push(text.into());
        }
        fn remember(&mut self, text: &str) {
            self.last = Some(text.into());
        }
        fn pill(&mut self, view: PillView) {
            self.pills.push(view);
        }
        fn schedule(&mut self, after_ms: u64, input: Input) {
            self.scheduled.push((after_ms, input));
        }
        fn live_finish(&mut self) -> Option<Result<String, ErrorCode>> {
            if self.live.is_some() {
                self.log.push("live finish".into());
            }
            self.live.take()
        }
        fn live_cancel(&mut self) {
            if self.live.take().is_some() {
                self.log.push("live cancel".into());
            }
        }
    }

    fn speech() -> Vec<f32> {
        (0..16_000)
            .map(|i| ((i as f32) * 0.05).sin() * 0.3)
            .collect()
    }

    fn ready() -> Fake {
        Fake {
            engine_ready: true,
            pcm: speech(),
            ..Default::default()
        }
    }

    fn states(f: &Fake) -> Vec<PillState> {
        f.pills.iter().map(|p| p.state).collect()
    }

    fn hold(d: &mut Dictation<Fake>, ms: u64) {
        d.handle(Input::Hotkey {
            event: HotkeyEvent::Down,
            t_ms: 1_000,
        });
        d.handle(Input::Hotkey {
            event: HotkeyEvent::Up,
            t_ms: 1_000 + ms,
        });
    }

    #[test]
    fn push_to_talk_cycle_transcribes_and_inserts_into_the_release_window() {
        let mut d = Dictation::new(ready(), HotkeyMode::PushToTalk);
        d.io.transcript = Some(Ok("Hello world.".into()));
        hold(&mut d, 1200);
        assert_eq!(
            d.io.log,
            vec![
                "start",
                "stop",
                "transcribe 16000",
                "insert \"Hello world.\" into 77"
            ]
        );
        assert_eq!(
            states(&d.io),
            vec![
                PillState::Listening,
                PillState::Processing,
                PillState::Success
            ]
        );
        // Success settles after a short beat and hides the pill.
        let (delay, input) = d.io.scheduled.last().cloned().unwrap();
        assert!(delay > 0 && delay <= 1000);
        d.handle(input);
        assert_eq!(states(&d.io).last(), Some(&PillState::Hidden));
    }

    #[test]
    fn silent_recording_never_reaches_the_engine() {
        let mut d = Dictation::new(
            Fake {
                engine_ready: true,
                pcm: vec![0.0; 16_000],
                ..Default::default()
            },
            HotkeyMode::PushToTalk,
        );
        hold(&mut d, 1200);
        assert!(!d.io.log.iter().any(|l| l.starts_with("transcribe")));
        assert_eq!(states(&d.io).last(), Some(&PillState::Hidden));
    }

    #[test]
    fn empty_transcript_inserts_nothing() {
        let mut d = Dictation::new(ready(), HotkeyMode::PushToTalk);
        d.io.transcript = Some(Ok("   ".into()));
        hold(&mut d, 1200);
        assert!(!d.io.log.iter().any(|l| l.starts_with("insert")));
        assert_eq!(states(&d.io).last(), Some(&PillState::Hidden));
    }

    #[test]
    fn no_model_fails_fast_without_opening_the_microphone() {
        let mut d = Dictation::new(Fake::default(), HotkeyMode::PushToTalk);
        d.handle(Input::Hotkey {
            event: HotkeyEvent::Down,
            t_ms: 0,
        });
        assert!(!d.io.log.contains(&"start".to_string()));
        let last = d.io.pills.last().unwrap();
        assert_eq!(last.state, PillState::Error(ErrorCode::NoModel));
        assert_eq!(last.label.as_deref(), Some("No model"));
    }

    #[test]
    fn microphone_failure_shows_the_error() {
        let mut d = Dictation::new(
            Fake {
                engine_ready: true,
                mic_error: Some(ErrorCode::MicBlocked),
                ..Default::default()
            },
            HotkeyMode::PushToTalk,
        );
        d.handle(Input::Hotkey {
            event: HotkeyEvent::Down,
            t_ms: 0,
        });
        assert_eq!(
            states(&d.io).last(),
            Some(&PillState::Error(ErrorCode::MicBlocked))
        );
    }

    #[test]
    fn engine_error_shows_the_error_pill() {
        let mut d = Dictation::new(ready(), HotkeyMode::PushToTalk);
        d.io.transcript = Some(Err(ErrorCode::EngineFailed));
        hold(&mut d, 1200);
        assert_eq!(
            states(&d.io).last(),
            Some(&PillState::Error(ErrorCode::EngineFailed))
        );
    }

    #[test]
    fn blocked_insert_keeps_the_text_on_the_clipboard_and_in_the_tray() {
        let mut d = Dictation::new(ready(), HotkeyMode::PushToTalk);
        d.io.transcript = Some(Ok("keep this".into()));
        d.io.insert_result = Some(InsertOutcome::CopiedOnly(Reason::Elevated));
        hold(&mut d, 1200);
        assert_eq!(d.io.copied, vec!["keep this".to_string()]);
        assert_eq!(d.io.last.as_deref(), Some("keep this"));
        let last = d.io.pills.last().unwrap();
        assert_eq!(last.state, PillState::Error(ErrorCode::InsertBlocked));
        assert_eq!(last.label.as_deref(), Some("Copied — Ctrl+V"));
    }

    #[test]
    fn a_stale_settle_timer_does_not_hide_a_new_session() {
        let mut d = Dictation::new(ready(), HotkeyMode::PushToTalk);
        d.io.transcript = Some(Ok("one".into()));
        hold(&mut d, 1200);
        let (_, stale) = d.io.scheduled.last().cloned().unwrap();
        // User starts dictating again before the success beat ends.
        d.io.pcm = speech();
        d.handle(Input::Hotkey {
            event: HotkeyEvent::Down,
            t_ms: 5_000,
        });
        d.handle(stale);
        assert_eq!(states(&d.io).last(), Some(&PillState::Listening));
    }

    #[test]
    fn escape_cancels_listening() {
        let mut d = Dictation::new(ready(), HotkeyMode::PushToTalk);
        d.handle(Input::Hotkey {
            event: HotkeyEvent::Down,
            t_ms: 0,
        });
        d.handle(Input::Hotkey {
            event: HotkeyEvent::Cancel,
            t_ms: 300,
        });
        assert!(d.io.log.contains(&"stop".to_string()));
        assert!(!d.io.log.iter().any(|l| l.starts_with("transcribe")));
        assert_eq!(states(&d.io).last(), Some(&PillState::Hidden));
    }

    #[test]
    fn toggle_mode_stops_on_the_second_press() {
        let mut d = Dictation::new(ready(), HotkeyMode::Toggle);
        d.io.transcript = Some(Ok("toggled".into()));
        d.handle(Input::Hotkey {
            event: HotkeyEvent::Down,
            t_ms: 0,
        });
        d.handle(Input::Hotkey {
            event: HotkeyEvent::Up,
            t_ms: 100,
        });
        d.handle(Input::Hotkey {
            event: HotkeyEvent::Down,
            t_ms: 3_000,
        });
        assert!(d.io.log.iter().any(|l| l.contains("toggled")));
    }

    #[test]
    fn max_duration_timer_stops_a_forgotten_recording() {
        let mut d = Dictation::new(ready(), HotkeyMode::Toggle);
        d.io.transcript = Some(Ok("long".into()));
        d.handle(Input::Hotkey {
            event: HotkeyEvent::Down,
            t_ms: 0,
        });
        let (delay, input) =
            d.io.scheduled
                .iter()
                .find(|(_, i)| matches!(i, Input::MaxDuration { .. }))
                .cloned()
                .unwrap();
        assert_eq!(delay, MAX_RECORDING_MS);
        d.handle(input);
        assert!(d.io.log.iter().any(|l| l.contains("long")));
    }

    fn inserts(f: &Fake) -> Vec<&String> {
        f.log.iter().filter(|l| l.starts_with("insert")).collect()
    }

    #[test]
    fn live_final_text_is_inserted_once_without_reading_the_recording_again() {
        let mut d = Dictation::new(ready(), HotkeyMode::PushToTalk);
        d.io.live = Some(Ok("Live words.".into()));
        d.io.transcript = Some(Ok("whole recording".into()));
        hold(&mut d, 1200);
        assert_eq!(inserts(&d.io), vec!["insert \"Live words.\" into 77"]);
        assert!(!d.io.log.iter().any(|l| l.starts_with("transcribe")));
        assert_eq!(states(&d.io).last(), Some(&PillState::Success));
    }

    #[test]
    fn a_failed_live_stream_falls_back_to_the_whole_recording() {
        let mut d = Dictation::new(ready(), HotkeyMode::PushToTalk);
        d.io.live = Some(Err(ErrorCode::EngineFailed));
        d.io.transcript = Some(Ok("whole recording".into()));
        hold(&mut d, 1200);
        assert!(d.io.log.contains(&"transcribe 16000".to_string()));
        assert_eq!(inserts(&d.io), vec!["insert \"whole recording\" into 77"]);
    }

    #[test]
    fn an_empty_live_result_for_real_speech_is_checked_against_the_whole_recording() {
        let mut d = Dictation::new(ready(), HotkeyMode::PushToTalk);
        d.io.live = Some(Ok("  ".into()));
        d.io.transcript = Some(Ok("said something".into()));
        hold(&mut d, 1200);
        assert_eq!(inserts(&d.io), vec!["insert \"said something\" into 77"]);
    }

    #[test]
    fn cancelling_or_a_tap_closes_the_live_stream_and_inserts_nothing() {
        let mut d = Dictation::new(ready(), HotkeyMode::PushToTalk);
        d.io.live = Some(Ok("never".into()));
        d.handle(Input::Hotkey {
            event: HotkeyEvent::Down,
            t_ms: 0,
        });
        d.handle(Input::Hotkey {
            event: HotkeyEvent::Cancel,
            t_ms: 900,
        });
        assert!(d.io.log.contains(&"live cancel".to_string()));
        let mut d = Dictation::new(ready(), HotkeyMode::PushToTalk);
        d.io.live = Some(Ok("never".into()));
        hold(&mut d, 100);
        assert!(d.io.log.contains(&"live cancel".to_string()));
        assert!(inserts(&d.io).is_empty());
    }

    #[test]
    fn silence_closes_the_live_stream_without_reaching_the_engine() {
        let mut d = Dictation::new(
            Fake {
                engine_ready: true,
                pcm: vec![0.0; 16_000],
                live: Some(Ok("ghost".into())),
                ..Default::default()
            },
            HotkeyMode::PushToTalk,
        );
        hold(&mut d, 1200);
        assert!(d.io.log.contains(&"live cancel".to_string()));
        assert!(!d.io.log.iter().any(|l| l.starts_with("transcribe")));
        assert!(inserts(&d.io).is_empty());
    }

    #[test]
    fn mode_change_applies_to_the_next_session() {
        let mut d = Dictation::new(ready(), HotkeyMode::PushToTalk);
        d.set_mode(HotkeyMode::Toggle);
        d.handle(Input::Hotkey {
            event: HotkeyEvent::Down,
            t_ms: 0,
        });
        d.handle(Input::Hotkey {
            event: HotkeyEvent::Up,
            t_ms: 900,
        });
        assert!(!d.io.log.contains(&"stop".to_string()));
    }
}
