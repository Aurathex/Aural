//! Engine-agnostic benchmark loop: time each clip, score it, pool the results.

use crate::audio::TARGET_RATE;
use crate::wer::{word_errors, WerStats};
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// One loaded STT engine. Input is always 16 kHz mono `f32`.
pub trait Transcriber {
    fn label(&self) -> String;
    fn transcribe(&mut self, pcm16k: &[f32]) -> Result<String>;

    /// Thread count actually applied, or `None` when the engine sizes its own pool.
    fn threads(&self) -> Option<usize> {
        None
    }

    /// Backend the engine is really running on (verified, not merely requested).
    fn backend_used(&self) -> String {
        "cpu".into()
    }
}

pub fn threads_label(threads: Option<usize>) -> String {
    threads.map_or_else(|| "engine-default".into(), |t| t.to_string())
}

/// (clip id, 16 kHz mono audio, reference transcript)
pub type CorpusClip = (String, Vec<f32>, String);

#[derive(Debug, Clone, Serialize)]
pub struct ClipResult {
    pub id: String,
    pub audio_secs: f64,
    pub latency_ms: f64,
    #[serde(skip)]
    pub wer: WerStats,
    pub hypothesis: String,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct Summary {
    pub clips: usize,
    pub p50_ms: f64,
    pub p95_ms: f64,
    /// Mean of per-clip latency / audio duration (lower is faster).
    pub mean_rtf: f64,
    /// Errors pooled over the whole corpus divided by total reference words.
    pub corpus_wer: f64,
}

pub fn run(
    t: &mut dyn Transcriber,
    corpus: &[CorpusClip],
    warmup: usize,
) -> Result<Vec<ClipResult>> {
    if let Some((id, pcm, _)) = corpus.first() {
        for _ in 0..warmup {
            t.transcribe(pcm)
                .with_context(|| format!("warmup on clip {id}"))?;
        }
    }
    corpus
        .iter()
        .map(|(id, pcm, reference)| {
            let start = Instant::now();
            let hypothesis = t
                .transcribe(pcm)
                .with_context(|| format!("transcribing clip {id}"))?;
            let latency_ms = start.elapsed().as_secs_f64() * 1000.0;
            Ok(ClipResult {
                id: id.clone(),
                audio_secs: pcm.len() as f64 / TARGET_RATE as f64,
                latency_ms,
                wer: word_errors(reference, &hypothesis),
                hypothesis,
            })
        })
        .collect()
}

fn nearest_rank(sorted: &[f64], pct: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = ((pct / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

pub fn summarize(results: &[ClipResult]) -> Summary {
    if results.is_empty() {
        return Summary::default();
    }
    let mut lat: Vec<f64> = results.iter().map(|r| r.latency_ms).collect();
    lat.sort_by(f64::total_cmp);
    let pooled = results.iter().fold(WerStats::default(), |acc, r| WerStats {
        substitutions: acc.substitutions + r.wer.substitutions,
        deletions: acc.deletions + r.wer.deletions,
        insertions: acc.insertions + r.wer.insertions,
        reference_words: acc.reference_words + r.wer.reference_words,
    });
    let mean_rtf = results
        .iter()
        .map(|r| (r.latency_ms / 1000.0) / r.audio_secs.max(f64::EPSILON))
        .sum::<f64>()
        / results.len() as f64;
    Summary {
        clips: results.len(),
        p50_ms: nearest_rank(&lat, 50.0),
        p95_ms: nearest_rank(&lat, 95.0),
        mean_rtf,
        corpus_wer: pooled.wer(),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CorpusEntry {
    pub id: String,
    pub wav: PathBuf,
    pub reference: String,
}

/// `corpus.tsv`: `<id>\t<wav path>\t<reference>` per line; `#` comments and blank lines
/// skipped; relative wav paths resolve against `base` (the TSV's directory).
pub fn parse_corpus_tsv(text: &str, base: &Path) -> Result<Vec<CorpusEntry>> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(3, '\t');
        let (Some(id), Some(wav), Some(reference)) = (parts.next(), parts.next(), parts.next())
        else {
            bail!("corpus line {}: expected <id>\\t<wav>\\t<reference>", i + 1);
        };
        let wav = PathBuf::from(wav);
        out.push(CorpusEntry {
            id: id.to_owned(),
            wav: if wav.is_absolute() {
                wav
            } else {
                base.join(wav)
            },
            reference: reference.to_owned(),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Echo(Vec<&'static str>);
    impl Transcriber for Echo {
        fn label(&self) -> String {
            "echo".into()
        }
        fn transcribe(&mut self, _: &[f32]) -> anyhow::Result<String> {
            Ok(self.0.remove(0).to_string())
        }
    }

    #[test]
    fn corpus_wer_is_pooled_not_averaged() {
        let corpus = vec![
            ("a".into(), vec![0.0; 16_000], "one two three four".into()),
            ("b".into(), vec![0.0; 16_000], "five".into()),
        ];
        let mut t = Echo(vec!["one two three four", "six"]);
        let r = run(&mut t, &corpus, 0).unwrap();
        let s = summarize(&r);
        assert!((s.corpus_wer - 0.2).abs() < 1e-9); // 1 error / 5 words
        assert!((r[0].audio_secs - 1.0).abs() < 1e-9);
    }

    #[test]
    fn engines_default_to_unknown_threads_and_cpu() {
        let t = Echo(vec![]);
        assert_eq!(t.threads(), None);
        assert_eq!(t.backend_used(), "cpu");
        assert_eq!(threads_label(None), "engine-default");
        assert_eq!(threads_label(Some(6)), "6");
    }

    #[test]
    fn warmup_runs_are_not_recorded() {
        let corpus = vec![("a".into(), vec![0.0; 1600], "x".into())];
        let mut t = Echo(vec!["x", "x"]);
        assert_eq!(run(&mut t, &corpus, 1).unwrap().len(), 1);
    }

    #[test]
    fn percentiles_use_nearest_rank() {
        let mk = |ms: f64| ClipResult {
            id: String::new(),
            audio_secs: 1.0,
            latency_ms: ms,
            wer: Default::default(),
            hypothesis: String::new(),
        };
        let r: Vec<_> = (1..=20).map(|i| mk(i as f64 * 10.0)).collect();
        let s = summarize(&r);
        assert_eq!(s.p50_ms, 100.0);
        assert_eq!(s.p95_ms, 190.0);
        assert!((s.mean_rtf - 0.105).abs() < 1e-9); // mean latency 105 ms / 1 s audio
    }

    #[test]
    fn engine_error_names_the_clip() {
        struct Fails;
        impl Transcriber for Fails {
            fn label(&self) -> String {
                "fails".into()
            }
            fn transcribe(&mut self, _: &[f32]) -> anyhow::Result<String> {
                anyhow::bail!("boom")
            }
        }
        let corpus = vec![("clip-7".into(), vec![0.0; 160], "x".into())];
        let err = run(&mut Fails, &corpus, 0).unwrap_err();
        assert!(format!("{err:#}").contains("clip-7"));
    }

    #[test]
    fn parses_corpus_tsv_relative_to_its_directory() {
        let base = std::path::Path::new("C:/corpus");
        let entries = parse_corpus_tsv(
            "# comment\nc1\twav/c1.wav\tHello there.\n\nc2\tD:/x/c2.wav\tHi\n",
            base,
        )
        .unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, "c1");
        assert_eq!(entries[0].wav, base.join("wav/c1.wav"));
        assert_eq!(entries[0].reference, "Hello there.");
        assert_eq!(entries[1].wav, std::path::PathBuf::from("D:/x/c2.wav"));
    }

    #[test]
    fn corpus_line_without_three_fields_is_an_error_with_line_number() {
        let err = parse_corpus_tsv("c1\tonly-two\n", std::path::Path::new(".")).unwrap_err();
        assert!(err.to_string().contains("line 1"));
    }
}
