//! The hardware-test runner against the fake worker.

use aural_engines::{Backend, Engine};
use aural_eval::clips::Clip;
use aural_stt_protocol::bench::{benchmark_variant, BenchTarget, Stability};
use aural_stt_protocol::client::WorkerSpec;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

fn spec(args: &[&str], dir: &Path) -> WorkerSpec {
    WorkerSpec {
        exe: PathBuf::from(env!("CARGO_BIN_EXE_aural-fake-stt")),
        args: args
            .iter()
            .map(|s| s.to_string())
            .chain([format!("--state={}", dir.display())])
            .collect(),
    }
}

fn target() -> BenchTarget {
    BenchTarget {
        variant: "fake@cpu".into(),
        model_path: PathBuf::from("model"),
        engine: Engine::Parakeet,
        backend: Backend::Cpu,
        threads: 2,
    }
}

/// The fake worker answers "<n> samples", so these references score 0% errors.
fn clips(n: usize) -> Vec<Clip> {
    (0..n)
        .map(|i| {
            let len = 16_000 + i * 160;
            Clip {
                id: format!("c{i}"),
                reference: format!("{len} samples"),
                pcm16k: vec![0.0; len],
            }
        })
        .collect()
}

fn calls(dir: &Path) -> usize {
    std::fs::read_to_string(dir.join("calls"))
        .map(|s| s.lines().count())
        .unwrap_or(0)
}

#[test]
fn three_passes_are_run_and_spread_recorded() {
    let dir = tempfile::tempdir().unwrap();
    let never = AtomicBool::new(false);
    let r = benchmark_variant(
        spec(&["--count"], dir.path()),
        &target(),
        &clips(2),
        3,
        &never,
    );
    assert_eq!(r.error, None);
    assert_eq!(r.variant, "fake@cpu");
    assert!(r.measured);
    assert_eq!(r.passes, 3);
    assert_eq!(calls(dir.path()), 6);
    let m = r.metrics.unwrap();
    assert_eq!((m.wer, m.words), (0.0, 4));
    assert!(r.spread >= 1.0, "{}", r.spread);
    assert_eq!(r.stability, Stability::Stable);
    assert!(r.ram_mb > 0);
}

#[test]
fn a_crashing_worker_is_unstable_with_reason() {
    let dir = tempfile::tempdir().unwrap();
    let never = AtomicBool::new(false);
    let r = benchmark_variant(
        spec(&["--crash-always"], dir.path()),
        &target(),
        &clips(2),
        3,
        &never,
    );
    let Stability::Unstable { reason } = &r.stability else {
        panic!("expected unstable: {r:?}")
    };
    assert!(reason.contains("stopped working"), "{reason}");
    assert!(r.error.is_some());
    assert!(r.measured);
}

#[test]
fn one_crash_that_recovers_is_still_unstable() {
    let dir = tempfile::tempdir().unwrap();
    let never = AtomicBool::new(false);
    let r = benchmark_variant(
        spec(&["--crash-first"], dir.path()),
        &target(),
        &clips(2),
        3,
        &never,
    );
    assert!(r.metrics.is_some());
    assert!(matches!(r.stability, Stability::Unstable { .. }), "{r:?}");
}

#[test]
fn a_model_that_will_not_load_reports_why() {
    let dir = tempfile::tempdir().unwrap();
    let never = AtomicBool::new(false);
    let r = benchmark_variant(
        spec(&["--fail-load"], dir.path()),
        &target(),
        &clips(2),
        3,
        &never,
    );
    assert!(r.metrics.is_none());
    assert!(r.error.unwrap().contains("model files missing"));
    assert!(matches!(r.stability, Stability::Unstable { .. }));
}

#[test]
fn a_clearly_too_slow_variant_stops_after_a_few_clips() {
    // 1 s clips that take 1.5 s each: slower than speech. Measuring all 3 × 20 would
    // take minutes at full load and can't change the verdict.
    let dir = tempfile::tempdir().unwrap();
    let never = AtomicBool::new(false);
    let r = benchmark_variant(
        spec(&["--count", "--slow-ms=1500"], dir.path()),
        &target(),
        &clips(20),
        3,
        &never,
    );
    assert_eq!(
        calls(dir.path()),
        aural_stt_protocol::bench::SLOW_CHECK_CLIPS
    );
    let m = r.metrics.unwrap();
    assert!(m.rtf > 1.0, "{}", m.rtf);
    assert_eq!(r.error, None);
    assert_eq!(r.stability, Stability::Stable);
}

#[test]
fn cancel_stops_between_clips() {
    let dir = tempfile::tempdir().unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let t = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(900));
        flag.store(true, Ordering::SeqCst);
    });
    let r = benchmark_variant(
        spec(&["--count", "--slow-ms=100"], dir.path()),
        &target(),
        &clips(20),
        3,
        &cancel,
    );
    t.join().unwrap();
    assert_eq!(r.error.as_deref(), Some("cancelled"));
    assert!(r.metrics.is_none());
    let n = calls(dir.path());
    assert!(n > 0 && n < 60, "{n} transcriptions");
}
