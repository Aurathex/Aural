use aural_engines::{Backend, Engine};
use aural_stt_protocol::client::{ClientError, SttClient, WorkerSpec};
use std::path::PathBuf;
use std::time::Duration;

fn spec(args: &[&str], dir: &std::path::Path) -> WorkerSpec {
    WorkerSpec {
        exe: PathBuf::from(env!("CARGO_BIN_EXE_aural-fake-stt")),
        args: args
            .iter()
            .map(|s| s.to_string())
            .chain([format!("--state={}", dir.display())])
            .collect(),
    }
}

fn loaded(args: &[&str], dir: &std::path::Path) -> SttClient {
    let mut c = SttClient::spawn(spec(args, dir)).unwrap();
    c.load(
        PathBuf::from("model"),
        Engine::Parakeet,
        Backend::Cpu,
        2,
        Duration::from_secs(5),
    )
    .unwrap();
    c
}

#[test]
fn transcribes_via_worker_process() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = loaded(&[], dir.path());
    assert_eq!(c.label(), Some("fake"));
    let text = c
        .transcribe(&vec![0.1; 1600], Duration::from_secs(5))
        .unwrap();
    assert_eq!(text, "1600 samples");
    // A second request on the same worker works too.
    assert_eq!(
        c.transcribe(&[0.1; 16], Duration::from_secs(5)).unwrap(),
        "16 samples"
    );
}

#[test]
fn restarts_crashed_worker_and_retries_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = loaded(&["--crash-first"], dir.path());
    let text = c
        .transcribe(&vec![0.0; 320], Duration::from_secs(5))
        .unwrap();
    assert_eq!(text, "320 samples");
    assert_eq!(c.restarts(), 1);
}

#[test]
fn hung_worker_times_out_and_is_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = loaded(&["--hang-always"], dir.path());
    let err = c
        .transcribe(&[0.0; 10], Duration::from_millis(700))
        .unwrap_err();
    assert!(matches!(err, ClientError::Timeout), "{err}");
    assert!(c.restarts() >= 1);
}

#[test]
fn worker_that_keeps_crashing_reports_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = loaded(&["--crash-always"], dir.path());
    let err = c
        .transcribe(&[0.0; 10], Duration::from_secs(5))
        .unwrap_err();
    assert!(matches!(err, ClientError::WorkerDied(_)), "{err}");
}

#[test]
fn kill_switch_ends_a_hung_transcription_at_once_without_restarting() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = loaded(&["--hang-always"], dir.path());
    let killer = c.killer();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        killer.kill();
    });
    let t0 = std::time::Instant::now();
    let err = c
        .transcribe(&[0.0; 10], Duration::from_secs(30))
        .unwrap_err();
    assert!(matches!(err, ClientError::Stopped), "{err}");
    assert!(
        t0.elapsed() < Duration::from_secs(5),
        "took {:?}",
        t0.elapsed()
    );
    assert_eq!(c.restarts(), 0);
}

#[test]
fn a_stopped_client_refuses_further_work() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = loaded(&[], dir.path());
    c.killer().kill();
    assert!(matches!(
        c.transcribe(&[0.0; 10], Duration::from_secs(5)),
        Err(ClientError::Stopped)
    ));
}

#[test]
fn protocol_version_mismatch_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let err = SttClient::spawn(spec(&["--bad-version"], dir.path()))
        .err()
        .unwrap();
    assert!(matches!(err, ClientError::Protocol(_)), "{err}");
}

#[test]
fn engine_error_is_passed_through() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = SttClient::spawn(spec(&["--fail-load"], dir.path())).unwrap();
    let err = c
        .load(
            PathBuf::from("model"),
            Engine::Parakeet,
            Backend::Cpu,
            2,
            Duration::from_secs(5),
        )
        .unwrap_err();
    assert!(
        matches!(&err, ClientError::Engine(m) if m.contains("missing")),
        "{err}"
    );
}

#[test]
fn missing_worker_executable_is_reported() {
    let err = SttClient::spawn(WorkerSpec {
        exe: PathBuf::from("C:/nope/aural-stt-none.exe"),
        args: vec![],
    })
    .err()
    .unwrap();
    assert!(matches!(err, ClientError::Spawn(_)), "{err}");
}

#[test]
fn stats_report_worker_memory() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = loaded(&[], dir.path());
    let s = c.stats(Duration::from_secs(5)).unwrap();
    assert!(s.working_set_mb > 0, "{s:?}");
    assert!(s.peak_working_set_mb >= s.working_set_mb, "{s:?}");
}

#[test]
fn a_stopped_client_refuses_stats_too() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = loaded(&[], dir.path());
    c.killer().kill();
    assert!(matches!(
        c.stats(Duration::from_secs(5)),
        Err(ClientError::Stopped)
    ));
}

fn speech(n: usize) -> Vec<f32> {
    (0..n).map(|i| (i as f32 * 0.07).sin() * 0.3).collect()
}

#[test]
fn live_text_streams_through_the_worker_process() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = loaded(&[], dir.path());
    let t = Duration::from_secs(5);
    assert_eq!(
        c.live_begin(t).unwrap(),
        aural_engines::live::LiveMode::Phrases
    );
    let text = c.live_push(&speech(8_000), t).unwrap();
    assert!(text.tentative.ends_with("samples"), "{text:?}");
    let last = c.live_end(&speech(1_600), t).unwrap();
    assert!(last.ends_with("samples"), "{last}");
    // The stream is closed; the worker still transcribes normally.
    assert!(c.live_push(&speech(10), t).is_err());
    assert_eq!(c.transcribe(&[0.1; 16], t).unwrap(), "16 samples");
}

#[test]
fn a_worker_crash_during_live_text_is_an_error_and_transcribe_recovers() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = loaded(&["--crash-first"], dir.path());
    let t = Duration::from_secs(5);
    c.live_begin(t).unwrap();
    let err = c.live_push(&speech(8_000), t).unwrap_err();
    assert!(matches!(err, ClientError::WorkerDied(_)), "{err}");
    // The app then falls back to a normal transcription, which restarts the worker.
    assert_eq!(c.transcribe(&vec![0.0; 320], t).unwrap(), "320 samples");
    assert_eq!(c.restarts(), 1);
}

#[test]
fn a_cancelled_stream_leaves_the_worker_usable() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = loaded(&[], dir.path());
    let t = Duration::from_secs(5);
    c.live_begin(t).unwrap();
    c.live_push(&speech(1_600), t).unwrap();
    c.live_cancel();
    assert!(c.live_end(&[], t).is_err());
    assert_eq!(c.transcribe(&[0.1; 16], t).unwrap(), "16 samples");
}
