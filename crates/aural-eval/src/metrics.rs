//! Speed and accuracy summary for one run over a set of clips.

use crate::wer::WerStats;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RunMetrics {
    /// Pooled word error rate (errors over all clips / reference words).
    pub wer: f64,
    /// Reference words the WER is measured over (sets its margin of error).
    pub words: usize,
    pub p50_ms: u64,
    pub p95_ms: u64,
    /// Total processing time / total audio time (lower is faster).
    pub rtf: f64,
}

/// Nearest-rank percentile of an ascending slice; 0 for an empty one.
pub fn nearest_rank<T: Copy + Default>(sorted: &[T], pct: f64) -> T {
    if sorted.is_empty() {
        return T::default();
    }
    let rank = ((pct / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

pub fn summarize(
    latencies_ms: &[u64],
    audio_s: f64,
    total_proc_ms: u64,
    wer: WerStats,
) -> RunMetrics {
    let mut lat = latencies_ms.to_vec();
    lat.sort_unstable();
    RunMetrics {
        wer: wer.wer(),
        words: wer.reference_words,
        p50_ms: nearest_rank(&lat, 50.0),
        p95_ms: nearest_rank(&lat, 95.0),
        rtf: if audio_s > 0.0 {
            total_proc_ms as f64 / 1000.0 / audio_s
        } else {
            0.0
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_use_nearest_rank() {
        let lat: Vec<u64> = (1..=20).rev().map(|i| i * 10).collect();
        let m = summarize(&lat, 100.0, 2100, WerStats::default());
        assert_eq!((m.p50_ms, m.p95_ms), (100, 190));
        assert!((m.rtf - 0.021).abs() < 1e-9);
    }

    #[test]
    fn wer_and_words_come_from_the_pooled_stats() {
        let s = WerStats {
            substitutions: 1,
            deletions: 1,
            insertions: 0,
            reference_words: 40,
        };
        let m = summarize(&[5], 1.0, 5, s);
        assert_eq!(m.words, 40);
        assert!((m.wer - 0.05).abs() < 1e-9);
    }

    #[test]
    fn empty_run_is_zeroes_not_a_panic() {
        let m = summarize(&[], 0.0, 0, WerStats::default());
        assert_eq!(m, RunMetrics::default());
    }
}
