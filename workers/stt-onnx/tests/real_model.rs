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
