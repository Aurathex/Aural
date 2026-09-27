//! The worker executables exactly as the installer ships them (staged by
//! scripts/prepare-bundle.ps1). Run with `-- --ignored` on a PC with a GPU.

use aural_engines::{Backend, Engine};
use aural_stt_protocol::client::{SttClient, WorkerSpec};
use std::path::PathBuf;
use std::time::Duration;

fn staged(worker: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("binaries")
        .join(format!("{worker}-x86_64-pc-windows-msvc.exe"))
}

/// The shipped Whisper worker can use the graphics card through Vulkan.
/// Needs AURAL_TEST_WHISPER_BASE (path to ggml-base.en-q8_0.bin).
#[test]
#[ignore]
fn bundled_ggml_worker_offers_vulkan() {
    let model = PathBuf::from(std::env::var("AURAL_TEST_WHISPER_BASE").unwrap());
    let mut c = SttClient::spawn(WorkerSpec {
        exe: staged("aural-stt-ggml"),
        args: vec![],
    })
    .unwrap();
    c.load(
        model,
        Engine::Whisper,
        Backend::Vulkan,
        4,
        Duration::from_secs(120),
    )
    .unwrap();
    let backend = c.backend().unwrap_or_default().to_owned();
    assert!(backend.starts_with("vulkan:"), "{backend}");
}
