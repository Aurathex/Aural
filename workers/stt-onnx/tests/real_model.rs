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

/// Live text through the real worker: two LibriSpeech clips with a pause between, fed in
/// 320 ms pieces like the microphone. Words must show before the end, and the final text
/// must hold both sentences once. Needs AURAL_TEST_PARAKEET_DIR, AURAL_TEST_LIVE_WAVS ("a.wav;b.wav")
/// and AURAL_TEST_LIVE_REFS ("first|second"); run with `-- --ignored --nocapture`.
#[test]
#[ignore]
fn live_text_streams_two_sentences_through_the_worker() {
    let model = PathBuf::from(std::env::var("AURAL_TEST_PARAKEET_DIR").unwrap());
    let wavs = std::env::var("AURAL_TEST_LIVE_WAVS").unwrap();
    let refs = std::env::var("AURAL_TEST_LIVE_REFS").unwrap();
    let mut pcm = Vec::new();
    for (i, w) in wavs.split(';').enumerate() {
        if i > 0 {
            pcm.extend(std::iter::repeat_n(0.0f32, 16_000));
        }
        pcm.extend(aural_audio::dsp::load_wav_16k_mono(std::path::Path::new(w)).unwrap());
    }
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
        Duration::from_secs(120),
    )
    .unwrap();
    let t = Duration::from_secs(60);
    let mode = c.live_begin(t).unwrap();
    let mut shown = 0usize;
    let mut slowest = 0u128;
    let pieces: Vec<&[f32]> = pcm.chunks(5_120).collect();
    let (last, rest) = pieces.split_last().unwrap();
    for piece in rest {
        let t0 = std::time::Instant::now();
        let text = c.live_push(piece, t).unwrap();
        slowest = slowest.max(t0.elapsed().as_millis());
        shown = shown.max(
            text.stable.split_whitespace().count() + text.tentative.split_whitespace().count(),
        );
        eprintln!(
            "{:>5} ms | {} [{}]",
            t0.elapsed().as_millis(),
            text.stable,
            text.tentative
        );
    }
    let t0 = std::time::Instant::now();
    let fin = c.live_end(last, t).unwrap();
    eprintln!(
        "{mode:?}: final in {} ms, slowest push {slowest} ms: {fin}",
        t0.elapsed().as_millis()
    );
    let norm = |s: &str| -> Vec<String> {
        s.to_lowercase()
            .split(|ch: char| !ch.is_alphanumeric() && ch != '\'')
            .filter(|w| !w.is_empty())
            .map(str::to_owned)
            .collect()
    };
    let got = norm(&fin);
    let want: Vec<String> = refs.split('|').flat_map(norm).collect();
    let missed = |text: &[String]| want.iter().filter(|w| !text.contains(w)).count();
    // Live text loses nothing that reading the whole recording on the same model gets
    // (it may get more: phrases are easier for some models than one long recording).
    let whole = norm(&c.transcribe(&pcm, t).unwrap());
    eprintln!("whole recording: {}", whole.join(" "));
    let (live_missed, whole_missed) = (missed(&got), missed(&whole));
    assert!(shown >= 3, "no words shown while speaking");
    assert!(
        live_missed <= whole_missed + want.len() / 10,
        "live missed {live_missed}, whole recording {whole_missed} of {}: {fin}",
        want.len()
    );
    assert!(got.len() * 10 <= want.len() * 12, "duplicated text? {fin}");
    // And it is the right text (model accuracy, so a looser bar).
    assert!(
        live_missed * 5 <= want.len(),
        "missed {live_missed}/{}: {fin}",
        want.len()
    );
}
