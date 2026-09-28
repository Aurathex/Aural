//! Live text: words on screen while the user is still speaking.
//!
//! Two ways to get there, chosen by the engine:
//!
//! - **Native** (`Transcriber::stream`): the engine encodes audio as it arrives and never
//!   encodes the same audio twice (Moonshine Streaming). Its decoder still re-reads
//!   everything encoded in the stream for each update and stops at a fixed token limit,
//!   so the stream is closed and a new one opened at each pause (and after at most
//!   [`MAX_NATIVE_MS`] of speech without one): cost and length stay bounded per phrase.
//! - **Phrases**, for engines that only read whole recordings (Parakeet, Whisper): the
//!   recording is split at pauses. A finished phrase is read once and never again; only
//!   the phrase still being spoken is re-read, every [`STEP_MS`], so the cost stays bounded
//!   by the phrase length rather than growing with the recording.
//!
//! Either way the text comes in two parts: `stable` (finished phrases, plus words two
//! consecutive reads agree on) and `tentative` (words a later read may still correct).
//! Live text is only shown; the final text comes from [`LiveSession::finish`] and is
//! inserted once.

use crate::Transcriber;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

pub const RATE: usize = 16_000;
/// 20 ms analysis frames.
const FRAME: usize = RATE / 50;
/// RMS above which a frame counts as sound (about -40 dBFS, as `aural_audio::gate`).
const SOUND_RMS: f32 = 0.01;
/// Silence after speech that closes a phrase.
pub const PAUSE_MS: usize = 600;
/// A phrase with no pause is closed at its quietest moment after this long.
pub const MAX_PHRASE_MS: usize = 20_000;
/// A native stream with no pause is closed at the first quiet moment after
/// [`MAX_PHRASE_MS`], and at the latest after this long.
pub const MAX_NATIVE_MS: usize = 28_000;
/// New audio needed before the open phrase is read again.
pub const STEP_MS: usize = 300;
/// Silence kept around speech when a phrase is read.
const PAD_MS: usize = 200;

const fn samples(ms: usize) -> usize {
    ms * RATE / 1000
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveText {
    /// Will not change any more while the user keeps speaking.
    pub stable: String,
    /// May still be corrected by a later read.
    pub tentative: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveMode {
    Native,
    Phrases,
}

fn rms(frame: &[f32]) -> f32 {
    if frame.is_empty() {
        return 0.0;
    }
    (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt()
}

fn loud_frames(pcm: &[f32]) -> Vec<bool> {
    pcm.chunks(FRAME).map(|f| rms(f) >= SOUND_RMS).collect()
}

/// The speech in `pcm` with [`PAD_MS`] around it, or nothing.
fn speech(pcm: &[f32]) -> &[f32] {
    let loud = loud_frames(pcm);
    let (Some(first), Some(last)) = (loud.iter().position(|l| *l), loud.iter().rposition(|l| *l))
    else {
        return &[];
    };
    let start = (first * FRAME).saturating_sub(samples(PAD_MS));
    let end = ((last + 1) * FRAME + samples(PAD_MS)).min(pcm.len());
    &pcm[start..end]
}

fn words(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_owned).collect()
}

/// Leading words two reads share. A changed comma or capital is the same word: only a
/// different word makes a word tentative again.
fn agreed_prefix(a: &[String], b: &[String]) -> usize {
    let key = |w: &String| {
        w.chars()
            .filter(|c| c.is_alphanumeric() || *c == '\'')
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    a.iter()
        .zip(b)
        .take_while(|(x, y)| key(x) == key(y))
        .count()
}

pub struct LiveSession {
    mode: LiveMode,
    /// Phrases: audio of the phrase still being spoken (and any silence before it).
    open: Vec<f32>,
    committed: Vec<String>,
    /// Latest read of the open phrase (native: of everything), as words.
    last: Vec<String>,
    /// Leading words of `last` that the previous read agreed with.
    agreed: usize,
    since_read: usize,
    /// Native: audio in the open stream, whether it held speech, and trailing silence.
    stream_len: usize,
    heard: bool,
    quiet_run: usize,
}

impl LiveSession {
    /// Start live text on `engine`, natively if it can stream.
    pub fn begin(engine: &mut dyn Transcriber) -> Result<Self> {
        let mode = match engine.stream() {
            Some(s) => {
                s.begin()?;
                LiveMode::Native
            }
            None => LiveMode::Phrases,
        };
        Ok(Self {
            mode,
            open: Vec::new(),
            committed: Vec::new(),
            last: Vec::new(),
            agreed: 0,
            since_read: 0,
            stream_len: 0,
            heard: false,
            quiet_run: 0,
        })
    }

    pub fn mode(&self) -> LiveMode {
        self.mode
    }

    pub fn text(&self) -> LiveText {
        let mut stable = self.committed.clone();
        stable.extend_from_slice(&self.last[..self.agreed]);
        LiveText {
            stable: stable.join(" "),
            tentative: self.last[self.agreed..].join(" "),
        }
    }

    fn read(&mut self, hypothesis: &str) {
        let next = words(hypothesis);
        self.agreed = agreed_prefix(&self.last, &next);
        self.last = next;
        self.since_read = 0;
    }

    fn commit(&mut self, engine: &mut dyn Transcriber, cut: usize) -> Result<()> {
        let phrase: Vec<f32> = self.open.drain(..cut).collect();
        let voiced = speech(&phrase);
        if !voiced.is_empty() {
            let text = engine.transcribe(voiced)?;
            let text = text.trim();
            if !text.is_empty() {
                self.committed.push(text.to_owned());
            }
        }
        self.last.clear();
        self.agreed = 0;
        self.since_read = 0;
        Ok(())
    }

    /// Where the open phrase ends, if the speaker paused: just after its speech.
    fn pause_cut(&self) -> Option<usize> {
        let loud = loud_frames(&self.open);
        let last = loud.iter().rposition(|l| *l)?;
        let quiet_after = loud.len() - 1 - last;
        (quiet_after * FRAME >= samples(PAUSE_MS))
            .then(|| ((last + 1) * FRAME + samples(PAD_MS)).min(self.open.len()))
    }

    /// The quietest 100 ms in the last seconds of a long phrase: the least harmful cut.
    fn forced_cut(&self) -> usize {
        let len = self.open.len();
        let from = len.saturating_sub(samples(5_000));
        let to = len.saturating_sub(samples(500)).max(from + 1);
        let win = samples(100);
        (from..to)
            .step_by(FRAME)
            .min_by(|a, b| {
                let e = |i: usize| rms(&self.open[i..(i + win).min(len)]);
                e(*a).total_cmp(&e(*b))
            })
            .map_or(to, |i| i + win / 2)
    }

    fn native_push(&mut self, engine: &mut dyn Transcriber, pcm: &[f32]) -> Result<()> {
        let s = engine
            .stream()
            .ok_or_else(|| anyhow!("engine stopped streaming"))?;
        s.push(pcm)?;
        self.stream_len += pcm.len();
        for frame in pcm.chunks(FRAME) {
            if rms(frame) >= SOUND_RMS {
                self.heard = true;
                self.quiet_run = 0;
            } else {
                self.quiet_run += frame.len();
            }
        }
        let long = self.stream_len >= samples(MAX_PHRASE_MS);
        let close = self.heard
            && (self.quiet_run >= samples(PAUSE_MS)
                || (long && self.quiet_run > 0)
                || self.stream_len >= samples(MAX_NATIVE_MS));
        if close {
            let text = s.finish()?;
            let text = text.trim();
            if !text.is_empty() {
                self.committed.push(text.to_owned());
            }
            s.begin()?;
            self.reset_native();
        } else if !self.heard && self.stream_len >= samples(5_000) {
            // Only silence so far: start over rather than keep encoding it.
            s.cancel();
            s.begin()?;
            self.reset_native();
        } else if self.heard && self.since_read >= samples(STEP_MS) {
            let partial = s.partial()?;
            self.read(&partial);
        }
        Ok(())
    }

    fn reset_native(&mut self) {
        self.stream_len = 0;
        self.heard = false;
        self.quiet_run = 0;
        self.last.clear();
        self.agreed = 0;
        self.since_read = 0;
    }

    /// Add new audio; returns the text so far.
    pub fn push(&mut self, engine: &mut dyn Transcriber, pcm: &[f32]) -> Result<LiveText> {
        self.since_read += pcm.len();
        if self.mode == LiveMode::Native {
            self.native_push(engine, pcm)?;
            return Ok(self.text());
        }

        self.open.extend_from_slice(pcm);
        if let Some(cut) = self.pause_cut() {
            self.commit(engine, cut)?;
        } else if self.open.len() > samples(MAX_PHRASE_MS) {
            let cut = self.forced_cut();
            self.commit(engine, cut)?;
        } else if self.since_read >= samples(STEP_MS) {
            let voiced = speech(&self.open);
            if voiced.is_empty() {
                // Only silence so far: keep a little lead-in, drop the rest.
                let keep = samples(PAD_MS);
                if self.open.len() > keep {
                    self.open.drain(..self.open.len() - keep);
                }
                self.since_read = 0;
            } else {
                let partial = engine.transcribe(voiced)?;
                self.read(&partial);
            }
        }
        Ok(self.text())
    }

    /// Read what is left and return the whole text. Each phrase appears exactly once.
    pub fn finish(mut self, engine: &mut dyn Transcriber) -> Result<String> {
        if self.mode == LiveMode::Native {
            let s = engine
                .stream()
                .ok_or_else(|| anyhow!("engine stopped streaming"))?;
            let text = s.finish()?;
            let text = text.trim();
            if !text.is_empty() {
                self.committed.push(text.to_owned());
            }
            return Ok(self.committed.join(" "));
        }
        let len = self.open.len();
        self.commit(engine, len)?;
        Ok(self.committed.join(" "))
    }

    pub fn cancel(self, engine: &mut dyn Transcriber) {
        if self.mode == LiveMode::Native {
            if let Some(s) = engine.stream() {
                s.cancel();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Stream;

    /// Speech-like audio: loud for `ms`.
    fn talk(ms: usize) -> Vec<f32> {
        (0..samples(ms))
            .map(|i| ((i as f32) * 0.07).sin() * 0.3)
            .collect()
    }

    fn quiet(ms: usize) -> Vec<f32> {
        vec![0.0; samples(ms)]
    }

    /// Writes one word per 500 ms of loud audio it is given, numbering words across
    /// calls by the order they were first spoken, and records every call.
    #[derive(Default)]
    struct Words {
        calls: Vec<usize>,
        fail: bool,
        prefix: String,
    }

    impl Transcriber for Words {
        fn label(&self) -> String {
            "words".into()
        }
        fn transcribe(&mut self, pcm: &[f32]) -> Result<String> {
            if self.fail {
                anyhow::bail!("engine crashed");
            }
            self.calls.push(pcm.len());
            let loud = loud_frames(pcm).iter().filter(|l| **l).count();
            let n = loud * FRAME / samples(500);
            Ok((0..n)
                .map(|i| format!("{}w{i}", self.prefix))
                .collect::<Vec<_>>()
                .join(" "))
        }
    }

    fn feed(s: &mut LiveSession, e: &mut dyn Transcriber, pcm: &[f32]) -> LiveText {
        let mut t = LiveText::default();
        for chunk in pcm.chunks(samples(100)) {
            t = s.push(e, chunk).unwrap();
        }
        t
    }

    #[test]
    fn words_appear_while_speaking_and_settle_when_two_reads_agree() {
        let mut e = Words::default();
        let mut s = LiveSession::begin(&mut e).unwrap();
        assert_eq!(s.mode(), LiveMode::Phrases);
        let t = feed(&mut s, &mut e, &talk(1_200));
        assert!(!t.tentative.is_empty() || !t.stable.is_empty(), "{t:?}");
        // Keep talking: earlier words are confirmed by the later reads.
        let t = feed(&mut s, &mut e, &talk(1_500));
        assert!(t.stable.starts_with("w0"), "{t:?}");
    }

    #[test]
    fn punctuation_or_capital_changes_do_not_unsettle_words() {
        let mut e = Words::default();
        let mut s = LiveSession::begin(&mut e).unwrap();
        s.read("stew for dinner, turnips");
        s.read("Stew for dinner turnips and");
        assert_eq!(s.text().stable, "Stew for dinner turnips");
        assert_eq!(s.text().tentative, "and");
    }

    #[test]
    fn a_later_read_corrects_tentative_words() {
        let mut e = Words::default();
        let mut s = LiveSession::begin(&mut e).unwrap();
        s.read("the cat");
        assert_eq!(s.text().tentative, "the cat");
        s.read("the hat sat");
        assert_eq!(
            s.text(),
            LiveText {
                stable: "the".into(),
                tentative: "hat sat".into()
            }
        );
    }

    #[test]
    fn a_pause_closes_the_phrase_and_its_audio_is_never_read_again() {
        let mut e = Words::default();
        let mut s = LiveSession::begin(&mut e).unwrap();
        feed(&mut s, &mut e, &talk(2_000));
        let t = feed(&mut s, &mut e, &quiet(800));
        assert_eq!(t.stable, "w0 w1 w2 w3");
        assert!(t.tentative.is_empty());
        e.calls.clear();
        feed(&mut s, &mut e, &talk(1_000));
        // Only the new phrase (plus padding) is read from now on.
        assert!(
            e.calls
                .iter()
                .all(|n| *n <= samples(1_000 + 2 * PAD_MS + 100)),
            "{:?}",
            e.calls
        );
    }

    #[test]
    fn finish_returns_every_phrase_once_without_duplicates() {
        let mut e = Words::default();
        let mut s = LiveSession::begin(&mut e).unwrap();
        feed(&mut s, &mut e, &talk(1_000));
        feed(&mut s, &mut e, &quiet(900));
        e.prefix = "b".into();
        feed(&mut s, &mut e, &talk(1_000));
        let text = s.finish(&mut e).unwrap();
        assert_eq!(text, "w0 w1 bw0 bw1");
    }

    #[test]
    fn silence_alone_never_reaches_the_engine() {
        let mut e = Words::default();
        let mut s = LiveSession::begin(&mut e).unwrap();
        let t = feed(&mut s, &mut e, &quiet(5_000));
        assert_eq!(t, LiveText::default());
        assert!(e.calls.is_empty());
        assert_eq!(s.finish(&mut e).unwrap(), "");
        assert!(e.calls.is_empty());
        // Silence doesn't pile up while waiting for speech.
    }

    #[test]
    fn long_speech_without_a_pause_is_cut_so_reads_stay_bounded() {
        let mut e = Words::default();
        let mut s = LiveSession::begin(&mut e).unwrap();
        feed(&mut s, &mut e, &talk(45_000));
        assert!(!s.committed.is_empty());
        assert!(
            e.calls
                .iter()
                .all(|n| *n <= samples(MAX_PHRASE_MS + 2 * PAD_MS + 200)),
            "longest read {:?}",
            e.calls.iter().max()
        );
        assert!(s.finish(&mut e).unwrap().split(' ').count() >= 80);
    }

    #[test]
    fn an_engine_failure_is_reported_not_hidden() {
        let mut e = Words::default();
        let mut s = LiveSession::begin(&mut e).unwrap();
        e.fail = true;
        let err = talk(1_000)
            .chunks(samples(100))
            .map(|c| s.push(&mut e, c))
            .find(Result::is_err);
        assert!(err.is_some());
    }

    #[derive(Default)]
    struct Native {
        log: Vec<String>,
        pushed: usize,
        open: bool,
        /// finish() returns "p1", "p2", … (one per stream) instead of "final text".
        numbered: bool,
        finished: usize,
    }

    impl Transcriber for Native {
        fn label(&self) -> String {
            "native".into()
        }
        fn transcribe(&mut self, _: &[f32]) -> Result<String> {
            self.log.push("whole".into());
            Ok("whole".into())
        }
        fn stream(&mut self) -> Option<&mut dyn Stream> {
            Some(self)
        }
    }

    impl Stream for Native {
        fn begin(&mut self) -> Result<()> {
            self.open = true;
            self.log.push("begin".into());
            Ok(())
        }
        fn push(&mut self, pcm: &[f32]) -> Result<()> {
            anyhow::ensure!(self.open, "not open");
            self.pushed += pcm.len();
            Ok(())
        }
        fn partial(&mut self) -> Result<String> {
            self.log.push("partial".into());
            Ok(format!("n{}", self.pushed / samples(500)))
        }
        fn finish(&mut self) -> Result<String> {
            anyhow::ensure!(self.open, "not open");
            self.open = false;
            self.finished += 1;
            self.log.push("finish".into());
            Ok(if self.numbered {
                format!("p{}", self.finished)
            } else {
                " final text ".into()
            })
        }
        fn cancel(&mut self) {
            self.open = false;
            self.log.push("cancel".into());
        }
    }

    #[test]
    fn a_streaming_engine_is_streamed_natively() {
        let mut e = Native::default();
        let mut s = LiveSession::begin(&mut e).unwrap();
        assert_eq!(s.mode(), LiveMode::Native);
        let t = feed(&mut s, &mut e, &talk(1_000));
        // Reads at 300/600/900 ms: "n0", "n1", "n1" — the repeat confirms it.
        assert_eq!(t.stable, "n1");
        assert_eq!(e.pushed, samples(1_000));
        assert_eq!(s.finish(&mut e).unwrap(), "final text");
        assert!(!e.log.contains(&"whole".to_string()), "{:?}", e.log);
    }

    fn count(log: &[String], what: &str) -> usize {
        log.iter().filter(|l| *l == what).count()
    }

    #[test]
    fn a_native_stream_is_closed_and_reopened_at_each_pause() {
        let mut e = Native {
            numbered: true,
            ..Default::default()
        };
        let mut s = LiveSession::begin(&mut e).unwrap();
        feed(&mut s, &mut e, &talk(1_000));
        let t = feed(&mut s, &mut e, &quiet(800));
        assert_eq!(t.stable, "p1", "the finished phrase is settled");
        feed(&mut s, &mut e, &talk(1_000));
        assert_eq!(s.finish(&mut e).unwrap(), "p1 p2");
        assert_eq!(count(&e.log, "begin"), 2);
        assert_eq!(count(&e.log, "finish"), 2);
    }

    #[test]
    fn long_native_speech_is_cut_so_the_decoder_never_runs_out_of_room() {
        // Moonshine's decoder stops at 448 tokens (about 70 s of speech): one stream
        // for a long dictation would lose its end.
        let mut e = Native {
            numbered: true,
            ..Default::default()
        };
        let mut s = LiveSession::begin(&mut e).unwrap();
        feed(&mut s, &mut e, &talk(100_000));
        let text = s.finish(&mut e).unwrap();
        let phrases = count(&e.log, "finish");
        assert!(phrases >= 4, "{phrases} streams for 100 s: {text}");
        assert_eq!(text.split(' ').count(), phrases, "each phrase once: {text}");
    }

    #[test]
    fn silence_before_speech_never_closes_an_empty_native_stream() {
        let mut e = Native::default();
        let mut s = LiveSession::begin(&mut e).unwrap();
        feed(&mut s, &mut e, &quiet(3_000));
        assert_eq!(count(&e.log, "finish"), 0);
        assert_eq!(count(&e.log, "begin"), 1);
    }

    #[test]
    fn cancelling_closes_a_native_stream() {
        let mut e = Native::default();
        let s = LiveSession::begin(&mut e).unwrap();
        s.cancel(&mut e);
        assert!(!e.open);
    }
}
