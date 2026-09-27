//! Hardware test: run one model variant in a real worker over the built-in clips and
//! measure accuracy, speed, memory and whether it keeps working.

use crate::client::{ClientError, SttClient, WorkerSpec};
use aural_engines::{Backend, Engine};
use aural_eval::clips::Clip;
use aural_eval::metrics::{summarize, RunMetrics};
use aural_eval::wer::{word_errors, WerStats};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// A variant is checked for being far too slow after this many clips…
pub const SLOW_CHECK_CLIPS: usize = 3;
/// …and measuring stops there if it took this many times longer than the audio.
pub const TOO_SLOW_RTF: f64 = 1.2;

/// Speed that varies more than this (slowest-typical / typical) feels unreliable.
pub const MAX_SPREAD: f64 = 4.0;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Stability {
    Stable,
    /// `reason` is shown to the user, in plain words.
    Unstable {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VariantResult {
    pub variant: String,
    /// None when the run did not finish (didn't load, crashed every time, cancelled).
    pub metrics: Option<RunMetrics>,
    pub load_ms: u64,
    /// Peak memory of the worker process.
    pub ram_mb: u64,
    /// Graphics-card memory the worker used; None if Windows couldn't say.
    pub vram_mb: Option<u64>,
    /// p95 / p50 of the time per second of audio, over all passes.
    pub spread: f64,
    /// Passes over the clips that completed.
    pub passes: usize,
    pub stability: Stability,
    /// false = an estimate, not run on this PC.
    pub measured: bool,
    pub error: Option<String>,
}

/// What to load in the worker.
#[derive(Debug, Clone)]
pub struct BenchTarget {
    pub variant: String,
    pub model_path: PathBuf,
    pub engine: Engine,
    pub backend: Backend,
    pub threads: usize,
}

/// p95 / p50 of the time per second of audio: how much the speed varies from clip to
/// clip, without longer clips counting as slower.
pub fn latency_spread(ms_per_audio_second: &[f64]) -> f64 {
    let mut v = ms_per_audio_second.to_vec();
    v.sort_by(f64::total_cmp);
    let p50 = aural_eval::metrics::nearest_rank(&v, 50.0);
    let p95 = aural_eval::metrics::nearest_rank(&v, 95.0);
    if p50 > 0.0 {
        p95 / p50
    } else {
        1.0
    }
}

/// Stable unless something went wrong or the speed varied too much.
pub fn stability(failure: Option<&str>, spread: f64) -> Stability {
    match failure {
        Some(reason) => Stability::Unstable {
            reason: reason.to_owned(),
        },
        None if spread > MAX_SPREAD => Stability::Unstable {
            reason: "its speed varies too much from one sentence to the next".into(),
        },
        None => Stability::Stable,
    }
}

fn plain_failure(e: &ClientError) -> &'static str {
    match e {
        ClientError::Timeout => "took too long and was stopped",
        ClientError::WorkerDied(_) | ClientError::Stopped => "stopped working during the test",
        _ => "gave an error during the test",
    }
}

/// Keeps the larger reading of each memory figure.
fn read_stats(client: &mut SttClient, ram_mb: &mut u64, vram_mb: &mut Option<u64>) {
    if let Ok(s) = client.stats(Duration::from_secs(5)) {
        *ram_mb = (*ram_mb).max(s.peak_working_set_mb);
        if let Some(g) = s.gpu_memory_mb {
            *vram_mb = Some(vram_mb.unwrap_or(0).max(g));
        }
    }
}

/// Loads the variant (timed), then runs `passes` passes over `clips`. WER comes from
/// the first pass; speed from all passes. Stops early on a failure or when `cancel`
/// is set (checked between clips).
pub fn benchmark_variant(
    spec: WorkerSpec,
    target: &BenchTarget,
    clips: &[Clip],
    passes: usize,
    cancel: &AtomicBool,
) -> VariantResult {
    let mut r = VariantResult {
        variant: target.variant.clone(),
        metrics: None,
        load_ms: 0,
        ram_mb: 0,
        vram_mb: None,
        spread: 0.0,
        passes: 0,
        stability: Stability::Stable,
        measured: true,
        error: None,
    };
    let fail = |mut r: VariantResult, reason: &str, detail: String| {
        r.stability = stability(Some(reason), 0.0);
        r.error = Some(detail);
        r
    };

    let started = Instant::now();
    let mut client = match SttClient::spawn(spec) {
        Ok(c) => c,
        Err(e) => return fail(r, "could not be started", e.to_string()),
    };
    if let Err(e) = client.load(
        target.model_path.clone(),
        target.engine,
        target.backend,
        target.threads,
        LOAD_TIMEOUT,
    ) {
        return fail(r, "could not be loaded", e.to_string());
    }
    r.load_ms = started.elapsed().as_millis() as u64;

    let mut latencies = Vec::with_capacity(clips.len() * passes);
    let mut per_second = Vec::with_capacity(clips.len() * passes);
    let mut total_ms = 0u64;
    let mut audio_s = 0.0;
    let mut wer = WerStats::default();
    let mut failure: Option<(&'static str, String)> = None;
    let mut stopped_early = false;
    'passes: for pass in 0..passes {
        for clip in clips {
            if cancel.load(Ordering::SeqCst) {
                r.error = Some("cancelled".into());
                r.metrics = None;
                return r;
            }
            let secs = clip.pcm16k.len() as f64 / aural_eval::clips::SAMPLE_RATE as f64;
            let timeout = Duration::from_secs_f64(10.0 + 2.0 * secs);
            let t0 = Instant::now();
            match client.transcribe(&clip.pcm16k, timeout) {
                Ok(text) => {
                    let ms = t0.elapsed().as_millis() as u64;
                    latencies.push(ms);
                    per_second.push(ms as f64 / secs.max(0.001));
                    total_ms += ms;
                    audio_s += secs;
                    if pass == 0 {
                        wer = wer + word_errors(&clip.reference, &text);
                    }
                    // Clearly slower than speech: more clips won't change the verdict,
                    // only keep the PC busy for minutes.
                    if pass == 0
                        && latencies.len() == SLOW_CHECK_CLIPS
                        && total_ms as f64 / 1000.0 > TOO_SLOW_RTF * audio_s
                    {
                        stopped_early = true;
                        read_stats(&mut client, &mut r.ram_mb, &mut r.vram_mb);
                        break 'passes;
                    }
                }
                Err(e) => {
                    failure = Some((plain_failure(&e), format!("clip {}: {e}", clip.id)));
                    break 'passes;
                }
            }
        }
        r.passes = pass + 1;
        if pass == 0 || pass + 1 == passes {
            read_stats(&mut client, &mut r.ram_mb, &mut r.vram_mb);
        }
    }
    if failure.is_none() && client.restarts() > 0 {
        failure = Some((
            "stopped working during the test",
            format!(
                "the worker had to be restarted {} time(s)",
                client.restarts()
            ),
        ));
    }

    if r.passes > 0 || stopped_early {
        let m = summarize(&latencies, audio_s, total_ms, wer);
        r.spread = latency_spread(&per_second);
        r.metrics = Some(m);
    }
    match failure {
        Some((reason, detail)) => fail(r, reason, detail),
        None => {
            r.stability = stability(None, r.spread);
            r
        }
    }
}

const LOAD_TIMEOUT: Duration = Duration::from_secs(180);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn longer_clips_taking_longer_is_not_instability() {
        // 5 s and 12 s clips at the same speed per second of audio: steady.
        let per_second = [60.0, 60.0, 61.0, 59.0, 60.0];
        assert!((latency_spread(&per_second) - 1.0).abs() < 0.05);
        // Two clips in twenty 6x slower per second than the rest: unsteady.
        let mut uneven = vec![60.0; 18];
        uneven.extend([360.0, 360.0]);
        assert!(latency_spread(&uneven) > MAX_SPREAD);
        assert_eq!(latency_spread(&[]), 1.0);
    }

    #[test]
    fn steady_speed_is_stable() {
        assert_eq!(stability(None, 1.3), Stability::Stable);
        assert_eq!(stability(None, MAX_SPREAD), Stability::Stable);
    }

    #[test]
    fn speed_that_varies_too_much_is_unstable() {
        let Stability::Unstable { reason } = stability(None, 6.0) else {
            panic!()
        };
        assert!(reason.contains("speed varies"), "{reason}");
    }

    #[test]
    fn a_failure_is_the_reason() {
        assert_eq!(
            stability(Some("stopped working during the test"), 1.0),
            Stability::Unstable {
                reason: "stopped working during the test".into()
            }
        );
    }
}
