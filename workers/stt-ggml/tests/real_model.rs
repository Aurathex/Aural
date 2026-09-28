//! The real ggml worker process with a real Whisper model, driven through the app's
//! client. Needs AURAL_TEST_WHISPER_MODEL and AURAL_TEST_WAV; run with `-- --ignored`.

use aural_engines::{Backend, Engine};
use aural_stt_protocol::client::{SttClient, WorkerSpec};
use std::path::PathBuf;
use std::time::Duration;

#[test]
#[ignore]
fn worker_transcribes_real_speech() {
    let model = PathBuf::from(std::env::var("AURAL_TEST_WHISPER_MODEL").unwrap());
    let wav = std::env::var("AURAL_TEST_WAV").unwrap();
    let pcm = aural_audio::dsp::load_wav_16k_mono(std::path::Path::new(&wav)).unwrap();
    let mut c = SttClient::spawn(WorkerSpec {
        exe: PathBuf::from(env!("CARGO_BIN_EXE_aural-stt-ggml")),
        args: vec![],
    })
    .unwrap();
    c.load(
        model,
        Engine::Whisper,
        Backend::Cpu,
        4,
        Duration::from_secs(60),
    )
    .unwrap();
    assert_eq!(c.backend(), Some("cpu"));
    let text = c
        .transcribe(&pcm, Duration::from_secs(30))
        .unwrap()
        .to_lowercase();
    assert!(
        text.contains("quick brown fox") && text.contains("maria"),
        "{text}"
    );
}

/// Live text through the real worker: two LibriSpeech clips with a pause between, fed in
/// 320 ms pieces like the microphone. Words must show before the end, and the final text
/// must hold both sentences once. Needs AURAL_TEST_WHISPER_MODEL, AURAL_TEST_LIVE_WAVS ("a.wav;b.wav")
/// and AURAL_TEST_LIVE_REFS ("first|second"); run with `-- --ignored --nocapture`.
#[test]
#[ignore]
fn live_text_streams_two_sentences_through_the_worker() {
    let model = PathBuf::from(std::env::var("AURAL_TEST_WHISPER_MODEL").unwrap());
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
        exe: PathBuf::from(env!("CARGO_BIN_EXE_aural-stt-ggml")),
        args: vec![],
    })
    .unwrap();
    c.load(
        model,
        Engine::Whisper,
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
    // The invariant: live text loses and duplicates nothing compared with reading the
    // whole recording the ordinary way on the same model.
    let whole = norm(&c.transcribe(&pcm, t).unwrap());
    let lost = whole.iter().filter(|w| !got.contains(w)).count();
    eprintln!("whole recording: {}", whole.join(" "));
    assert!(shown >= 3, "no words shown while speaking");
    assert!(
        lost * 10 <= whole.len(),
        "live lost {lost}/{} words: {fin}",
        whole.len()
    );
    assert!(got.len() * 10 <= whole.len() * 12, "duplicated text? {fin}");
    // And it is still the right text (model accuracy, so a looser bar).
    let want: Vec<String> = refs.split('|').flat_map(norm).collect();
    let missed = want.iter().filter(|w| !got.contains(w)).count();
    assert!(
        missed * 5 <= want.len(),
        "missed {missed}/{}: {fin}",
        want.len()
    );
}
