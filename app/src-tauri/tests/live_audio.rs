//! Live text end to end with real audio, minus the hotkey and the pill: a LibriSpeech
//! clip is played into a virtual microphone, Aural's own capture records it, and the
//! live feeder streams it to a real speech worker as it arrives. Needs a loopback pair
//! (AURAL_E2E_SPEAKER = output device, AURAL_E2E_MIC = matching input device, e.g. the
//! Steam Streaming Microphone pair), AURAL_E2E_WORKER (aural-stt-onnx.exe),
//! AURAL_E2E_MODEL (+ AURAL_E2E_ENGINE parakeet|moonshine), AURAL_TEST_WAV and
//! AURAL_TEST_REF. Run with `--test live_audio -- --ignored --nocapture`.

use aural_app::live::{LiveEngine, LiveFeed};
use aural_core::error::ErrorCode;
use aural_engines::live::{LiveMode, LiveText};
use aural_engines::{Backend, Engine};
use aural_stt_protocol::client::{SttClient, WorkerSpec};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

struct Worker(Mutex<SttClient>);

impl LiveEngine for Worker {
    fn begin(&self) -> Result<LiveMode, ErrorCode> {
        self.0
            .lock()
            .unwrap()
            .live_begin(Duration::from_secs(10))
            .map_err(|_| ErrorCode::EngineFailed)
    }
    fn push(&self, pcm: &[f32]) -> Result<LiveText, ErrorCode> {
        self.0
            .lock()
            .unwrap()
            .live_push(pcm, Duration::from_secs(30))
            .map_err(|_| ErrorCode::EngineFailed)
    }
    fn end(&self, pcm: &[f32], seconds: u64) -> Result<String, ErrorCode> {
        self.0
            .lock()
            .unwrap()
            .live_end(pcm, Duration::from_secs(30 + 2 * seconds))
            .map_err(|_| ErrorCode::EngineFailed)
    }
    fn cancel(&self) {
        self.0.lock().unwrap().live_cancel();
    }
}

fn play(device_name: &str, pcm16k: Vec<f32>) -> cpal::Stream {
    let host = cpal::default_host();
    let dev = host
        .output_devices()
        .unwrap()
        .find(|d| d.description().map(|x| x.name().to_owned()).ok().as_deref() == Some(device_name))
        .unwrap_or_else(|| panic!("no output device {device_name:?}"));
    let cfg = dev.default_output_config().unwrap();
    let rate = cfg.sample_rate() as usize;
    let channels = cfg.channels() as usize;
    // Nearest-sample upsampling is enough for a test signal.
    let step = 16_000.0 / rate as f64;
    let mut pos = 0.0f64;
    let stream = dev
        .build_output_stream::<f32, _, _>(
            cfg.config(),
            move |out, _| {
                for frame in out.chunks_mut(channels) {
                    let s = pcm16k.get(pos as usize).copied().unwrap_or(0.0);
                    pos += step;
                    frame.fill(s);
                }
            },
            |e| eprintln!("playback error: {e}"),
            None,
        )
        .unwrap();
    stream.play().unwrap();
    stream
}

#[test]
#[ignore]
fn live_text_from_a_real_microphone_stream() {
    let var = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("{k}"));
    let wav =
        aural_audio::dsp::load_wav_16k_mono(std::path::Path::new(&var("AURAL_TEST_WAV"))).unwrap();
    let engine = match var("AURAL_E2E_ENGINE").as_str() {
        "moonshine" => Engine::Moonshine,
        _ => Engine::Parakeet,
    };
    let mut client = SttClient::spawn(WorkerSpec {
        exe: var("AURAL_E2E_WORKER").into(),
        args: vec![],
    })
    .unwrap();
    client
        .load(
            var("AURAL_E2E_MODEL").into(),
            engine,
            Backend::Cpu,
            4,
            Duration::from_secs(120),
        )
        .unwrap();
    let worker: Arc<dyn LiveEngine> = Arc::new(Worker(Mutex::new(client)));

    let (tx, rx) = mpsc::channel::<Vec<f32>>();
    let shown = Arc::new(Mutex::new(Vec::<(u128, LiveText)>::new()));
    let t0 = Instant::now();
    let s = shown.clone();
    let feed = LiveFeed::start(worker, rx, move |t| {
        s.lock().unwrap().push((t0.elapsed().as_millis(), t))
    });
    let capture = aural_audio::capture::start_live(
        Some(&var("AURAL_E2E_MIC")),
        Box::new(|_| {}),
        Some(Box::new(move |pcm: &[f32]| {
            let _ = tx.send(pcm.to_vec());
        })),
    )
    .unwrap();
    let secs = wav.len() as f64 / 16_000.0;
    let playing = play(&var("AURAL_E2E_SPEAKER"), wav);
    std::thread::sleep(Duration::from_secs_f64(secs + 1.0));
    let recorded = capture.stop();
    let released = Instant::now();
    drop(playing);
    let fin = feed.finish().expect("a live stream").expect("final text");
    let final_ms = released.elapsed().as_millis();
    for (ms, t) in shown.lock().unwrap().iter() {
        eprintln!("{ms:>6} ms | {} [{}]", t.stable, t.tentative);
    }
    let peak = recorded.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    eprintln!(
        "recorded {:.1} s (peak {peak:.2}); final {final_ms} ms after stop: {fin}",
        recorded.len() as f64 / 16_000.0
    );
    let reference = var("AURAL_TEST_REF").to_lowercase();
    let got = fin.to_lowercase();
    let words: Vec<&str> = reference.split_whitespace().collect();
    let missed = words.iter().filter(|w| !got.contains(*w)).count();
    assert!(
        shown.lock().unwrap().len() >= 3,
        "live text should update while audio plays"
    );
    assert!(
        missed * 5 <= words.len(),
        "missed {missed}/{}: {fin}",
        words.len()
    );
}
