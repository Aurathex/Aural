//! Pure dictation state machine: events in, state + side effects out. The app executes
//! the effects; nothing here touches the OS, so every transition is unit-tested.

pub use crate::error::ErrorCode;
pub use crate::settings::HotkeyMode;
use serde::Serialize;

/// Holds shorter than this are treated as accidental taps and discarded.
pub const MIN_HOLD_MS: u64 = 250;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Idle,
    Listening {
        since_ms: u64,
    },
    Processing,
    Inserting {
        text: String,
    },
    /// Showing success or an error; `Settled` returns to Idle.
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    HotkeyDown { t_ms: u64 },
    HotkeyUp { t_ms: u64 },
    Cancel,
    MaxDurationReached,
    Transcript(String),
    TranscriptEmpty,
    Inserted,
    InsertFailed(ErrorCode),
    Failed(ErrorCode),
    Settled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "state", content = "error", rename_all = "snake_case")]
pub enum PillState {
    Hidden,
    Listening,
    Processing,
    Success,
    Error(ErrorCode),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    StartCapture,
    StopCaptureDiscard,
    StopCaptureAndTranscribe,
    Insert(String),
    CopyToClipboard(String),
    RememberLast(String),
    Pill(PillState),
}

fn start(t_ms: u64) -> (State, Vec<Effect>) {
    (
        State::Listening { since_ms: t_ms },
        vec![Effect::StartCapture, Effect::Pill(PillState::Listening)],
    )
}

fn stop() -> (State, Vec<Effect>) {
    (
        State::Processing,
        vec![
            Effect::StopCaptureAndTranscribe,
            Effect::Pill(PillState::Processing),
        ],
    )
}

fn discard() -> (State, Vec<Effect>) {
    (
        State::Idle,
        vec![Effect::StopCaptureDiscard, Effect::Pill(PillState::Hidden)],
    )
}

pub fn step(state: State, event: Event, mode: HotkeyMode) -> (State, Vec<Effect>) {
    use Event as E;
    match (state, event) {
        (State::Idle | State::Done, E::HotkeyDown { t_ms }) => start(t_ms),

        (State::Listening { since_ms }, E::HotkeyUp { t_ms }) if mode == HotkeyMode::PushToTalk => {
            if t_ms.saturating_sub(since_ms) < MIN_HOLD_MS {
                discard()
            } else {
                stop()
            }
        }
        (State::Listening { .. }, E::HotkeyDown { .. }) if mode == HotkeyMode::Toggle => stop(),
        (State::Listening { .. }, E::MaxDurationReached) => stop(),
        (State::Listening { .. }, E::Cancel) => discard(),
        (State::Listening { .. }, E::Failed(code)) => (
            State::Done,
            vec![
                Effect::StopCaptureDiscard,
                Effect::Pill(PillState::Error(code)),
            ],
        ),

        (State::Processing, E::Transcript(text)) => (
            State::Inserting { text: text.clone() },
            vec![Effect::Insert(text)],
        ),
        (State::Processing, E::TranscriptEmpty) => {
            (State::Idle, vec![Effect::Pill(PillState::Hidden)])
        }
        (State::Processing, E::Failed(code)) => {
            (State::Done, vec![Effect::Pill(PillState::Error(code))])
        }

        (State::Inserting { .. }, E::Inserted) => {
            (State::Done, vec![Effect::Pill(PillState::Success)])
        }
        // Never lose a transcript: it goes to the clipboard and the tray's
        // "Paste last transcript".
        (State::Inserting { text }, E::InsertFailed(code)) => (
            State::Done,
            vec![
                Effect::CopyToClipboard(text.clone()),
                Effect::RememberLast(text),
                Effect::Pill(PillState::Error(code)),
            ],
        ),

        (State::Done, E::Settled) => (State::Idle, vec![Effect::Pill(PillState::Hidden)]),

        (s, _) => (s, Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use HotkeyMode::{PushToTalk, Toggle};

    fn step_all(mode: HotkeyMode, events: Vec<Event>) -> (State, Vec<Effect>) {
        let mut state = State::Idle;
        let mut all = Vec::new();
        for e in events {
            let (s, fx) = step(state, e, mode);
            state = s;
            all.extend(fx);
        }
        (state, all)
    }

    #[test]
    fn ptt_down_up_transcribes() {
        let (s, fx) = step_all(
            PushToTalk,
            vec![Event::HotkeyDown { t_ms: 0 }, Event::HotkeyUp { t_ms: 900 }],
        );
        assert_eq!(s, State::Processing);
        assert_eq!(
            fx,
            vec![
                Effect::StartCapture,
                Effect::Pill(PillState::Listening),
                Effect::StopCaptureAndTranscribe,
                Effect::Pill(PillState::Processing),
            ]
        );
    }

    #[test]
    fn tap_under_250ms_discards() {
        let (s, fx) = step_all(
            PushToTalk,
            vec![
                Event::HotkeyDown { t_ms: 1000 },
                Event::HotkeyUp { t_ms: 1100 },
            ],
        );
        assert_eq!(s, State::Idle);
        assert!(fx.contains(&Effect::StopCaptureDiscard));
        assert_eq!(fx.last(), Some(&Effect::Pill(PillState::Hidden)));
    }

    #[test]
    fn toggle_second_press_stops_and_key_up_is_ignored() {
        let (s, fx) = step_all(
            Toggle,
            vec![
                Event::HotkeyDown { t_ms: 0 },
                Event::HotkeyUp { t_ms: 50 },
                Event::HotkeyDown { t_ms: 2000 },
            ],
        );
        assert_eq!(s, State::Processing);
        assert!(fx.contains(&Effect::StopCaptureAndTranscribe));
    }

    #[test]
    fn cancel_while_listening_discards_and_hides() {
        let (s, fx) = step_all(
            PushToTalk,
            vec![Event::HotkeyDown { t_ms: 0 }, Event::Cancel],
        );
        assert_eq!(s, State::Idle);
        assert_eq!(
            &fx[2..],
            &[Effect::StopCaptureDiscard, Effect::Pill(PillState::Hidden)]
        );
    }

    #[test]
    fn hotkey_ignored_while_processing() {
        let (s, fx) = step(State::Processing, Event::HotkeyDown { t_ms: 5 }, PushToTalk);
        assert_eq!(s, State::Processing);
        assert!(fx.is_empty());
    }

    #[test]
    fn max_duration_auto_stops() {
        let (s, fx) = step_all(
            PushToTalk,
            vec![Event::HotkeyDown { t_ms: 0 }, Event::MaxDurationReached],
        );
        assert_eq!(s, State::Processing);
        assert!(fx.contains(&Effect::StopCaptureAndTranscribe));
    }

    #[test]
    fn transcript_is_inserted_then_success_then_idle() {
        let (s, fx) = step(
            State::Processing,
            Event::Transcript("hello".into()),
            PushToTalk,
        );
        assert_eq!(
            s,
            State::Inserting {
                text: "hello".into()
            }
        );
        assert_eq!(fx, vec![Effect::Insert("hello".into())]);
        let (s, fx) = step(s, Event::Inserted, PushToTalk);
        assert_eq!(s, State::Done);
        assert_eq!(fx, vec![Effect::Pill(PillState::Success)]);
        let (s, fx) = step(s, Event::Settled, PushToTalk);
        assert_eq!(s, State::Idle);
        assert_eq!(fx, vec![Effect::Pill(PillState::Hidden)]);
    }

    #[test]
    fn empty_transcript_hides_silently() {
        let (s, fx) = step(State::Processing, Event::TranscriptEmpty, PushToTalk);
        assert_eq!(s, State::Idle);
        assert_eq!(fx, vec![Effect::Pill(PillState::Hidden)]);
    }

    #[test]
    fn insert_failure_copies_and_remembers() {
        let (s, _) = step(
            State::Processing,
            Event::Transcript("keep me".into()),
            PushToTalk,
        );
        let (s, fx) = step(s, Event::InsertFailed(ErrorCode::InsertBlocked), PushToTalk);
        assert_eq!(s, State::Done);
        assert_eq!(
            fx,
            vec![
                Effect::CopyToClipboard("keep me".into()),
                Effect::RememberLast("keep me".into()),
                Effect::Pill(PillState::Error(ErrorCode::InsertBlocked)),
            ]
        );
    }

    #[test]
    fn failure_while_processing_shows_error() {
        let (s, fx) = step(
            State::Processing,
            Event::Failed(ErrorCode::NoModel),
            PushToTalk,
        );
        assert_eq!(s, State::Done);
        assert_eq!(fx, vec![Effect::Pill(PillState::Error(ErrorCode::NoModel))]);
    }

    #[test]
    fn capture_failure_while_listening_shows_error() {
        let (s, fx) = step_all(
            PushToTalk,
            vec![
                Event::HotkeyDown { t_ms: 0 },
                Event::Failed(ErrorCode::MicUnavailable),
            ],
        );
        assert_eq!(s, State::Done);
        assert_eq!(
            fx.last(),
            Some(&Effect::Pill(PillState::Error(ErrorCode::MicUnavailable)))
        );
    }

    #[test]
    fn hotkey_during_done_starts_a_new_session() {
        let (s, fx) = step(State::Done, Event::HotkeyDown { t_ms: 10 }, PushToTalk);
        assert_eq!(s, State::Listening { since_ms: 10 });
        assert_eq!(fx[0], Effect::StartCapture);
    }
}
