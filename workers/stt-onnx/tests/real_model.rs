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
    assert_eq!(c.backend(), Some("directml"));
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
