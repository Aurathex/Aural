//! The real worker process with the real Parakeet model, driven through the app's
//! client. Needs AURAL_TEST_PARAKEET_DIR and AURAL_TEST_WAV; run with `-- --ignored`.

use aural_engines::{Backend, Engine};
use aural_stt_protocol::client::{SttClient, WorkerSpec};
use std::path::PathBuf;
use std::time::Duration;

#[test]
#[ignore]
fn worker_transcribes_real_speech() {
    let model = PathBuf::from(std::env::var("AURAL_TEST_PARAKEET_DIR").unwrap());
    let wav = std::env::var("AURAL_TEST_WAV").unwrap();
    let pcm = aural_audio::dsp::load_wav_16k_mono(std::path::Path::new(&wav)).unwrap();
    let mut c = SttClient::spawn(WorkerSpec {
        exe: PathBuf::from(env!("CARGO_BIN_EXE_aural-stt-onnx")),
        args: vec![],
    })
    .unwrap();
    c.load(
        model,
        Engine::Parakeet,
        Backend::Cpu,
        4,
        Duration::from_secs(60),
    )
    .unwrap();
    let text = c
        .transcribe(&pcm, Duration::from_secs(30))
        .unwrap()
        .to_lowercase();
    assert!(
        text.contains("quick brown fox") && text.contains("maria"),
        "{text}"
    );
}

fn spawn() -> SttClient {
    SttClient::spawn(WorkerSpec {
        exe: PathBuf::from(env!("CARGO_BIN_EXE_aural-stt-onnx")),
        args: vec![],
    })
    .unwrap()
}

fn smoke_pcm() -> Vec<f32> {
    let wav = std::env::var("AURAL_TEST_WAV").unwrap();
    aural_audio::dsp::load_wav_16k_mono(std::path::Path::new(&wav)).unwrap()
}

/// Parakeet on the graphics card, through the real worker: the worker itself proves
/// it holds graphics-card memory. Needs AURAL_TEST_PARAKEET_DIR.
#[test]
#[ignore]
fn worker_runs_parakeet_on_directml() {
    let model = PathBuf::from(std::env::var("AURAL_TEST_PARAKEET_DIR").unwrap());
    let mut c = spawn();
    c.load(
        model,
        Engine::Parakeet,
        Backend::DirectMl,
        4,
        Duration::from_secs(120),
    )
    .unwrap();
    assert!(
        c.backend().unwrap_or_default().starts_with("directml:"),
        "{:?}",
        c.backend()
    );
    let stats = c.stats(Duration::from_secs(5)).unwrap();
    eprintln!("worker memory on DirectML: {stats:?}");
    assert!(stats.gpu_memory_mb.unwrap_or(0) > 0, "{stats:?}");
    let text = c
        .transcribe(&smoke_pcm(), Duration::from_secs(60))
        .unwrap()
        .to_lowercase();
    assert!(
        text.contains("quick brown fox") && text.contains("maria"),
        "{text}"
    );
}

/// Moonshine through the real worker. Needs AURAL_TEST_MOONSHINE_DIR.
#[test]
#[ignore]
fn worker_runs_moonshine() {
    let model = PathBuf::from(std::env::var("AURAL_TEST_MOONSHINE_DIR").unwrap());
    let mut c = spawn();
    c.load(
        model,
        Engine::Moonshine,
        Backend::Cpu,
        4,
        Duration::from_secs(60),
    )
    .unwrap();
    let text = c
        .transcribe(&smoke_pcm(), Duration::from_secs(30))
        .unwrap()
        .to_lowercase();
    assert!(
        text.contains("quick brown fox") && text.contains("maria"),
        "{text}"
    );
}

/// The hardware-test runner on real Parakeet, on the processor and the graphics card,
/// over the built-in clips. Run with `--release -- --ignored --nocapture`.
#[test]
#[ignore]
fn benchmark_parakeet_on_the_builtin_clips() {
    use aural_stt_protocol::bench::{benchmark_variant, BenchTarget, Stability};
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/eval");
    let clips = aural_eval::clips::builtin(&dir).unwrap();
    let never = std::sync::atomic::AtomicBool::new(false);
    for backend in [Backend::Cpu, Backend::DirectMl] {
        let r = benchmark_variant(
            WorkerSpec {
                exe: PathBuf::from(env!("CARGO_BIN_EXE_aural-stt-onnx")),
                args: vec![],
            },
            &BenchTarget {
                variant: format!("parakeet@{backend:?}"),
                model_path: PathBuf::from(std::env::var("AURAL_TEST_PARAKEET_DIR").unwrap()),
                engine: Engine::Parakeet,
                backend,
                threads: 4,
            },
            &clips,
            3,
            &never,
        );
        eprintln!("{r:?}");
        assert_eq!(r.error, None);
        assert_eq!(r.stability, Stability::Stable);
        let m = r.metrics.unwrap();
        assert!(m.wer < 0.08, "wer {}", m.wer);
        assert!(r.ram_mb > 100);
    }
}
