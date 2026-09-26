//! whisper.cpp (ggml) via `whisper-rs`. GPU backends are compile-time: build with
//! `--features vulkan` or `--features cuda` (both imply `ggml`).

use crate::Backend;
use crate::Transcriber;
use anyhow::{bail, Context, Result};
use std::path::Path;
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState,
};

pub fn compiled_backends() -> Vec<Backend> {
    let mut v = vec![Backend::Cpu];
    if cfg!(feature = "vulkan") {
        v.push(Backend::Vulkan);
    }
    if cfg!(feature = "cuda") {
        v.push(Backend::Cuda);
    }
    v
}

pub struct Whisper {
    // `state` borrows nothing from `ctx` in whisper-rs 0.16, but keep the context alive
    // for the lifetime of the engine regardless.
    _ctx: WhisperContext,
    state: WhisperState,
    threads: usize,
    label: String,
    backend_used: String,
}

/// GPU device whisper.cpp actually initialized, read from its init log. `None` means it
/// is running on CPU (no GPU requested, none found, or GPU init failed).
pub fn gpu_device_from_log(lines: &[String]) -> Option<String> {
    const PREFIX: &str = "whisper_backend_init_gpu: ";
    let mut device = None;
    for line in lines {
        let Some(rest) = line.trim().strip_prefix(PREFIX) else {
            continue;
        };
        if let Some(name) = rest
            .strip_prefix("using ")
            .and_then(|r| r.strip_suffix(" backend"))
        {
            device = Some(name.to_owned());
        } else if rest.starts_with("failed to initialize") || rest.starts_with("no GPU found") {
            device = None;
        }
    }
    device
}

/// Refuse to report a GPU benchmark that silently ran on the CPU.
pub fn verify_backend(requested: Backend, gpu_device: Option<&str>) -> Result<String> {
    let expected_prefix = match requested {
        Backend::Cpu => return Ok("cpu".into()),
        Backend::Vulkan => "Vulkan",
        Backend::Cuda => "CUDA",
    };
    match gpu_device {
        None => bail!("requested {requested:?} but whisper.cpp fell back to CPU"),
        Some(dev) if dev.starts_with(expected_prefix) => {
            Ok(format!("{}:{dev}", expected_prefix.to_lowercase()))
        }
        Some(dev) => bail!("requested {requested:?} but whisper.cpp initialized {dev}"),
    }
}

/// Captures native whisper.cpp/ggml log lines (routed through the `log` crate by
/// whisper-rs's `log_backend`) so the init log can be inspected after loading.
mod capture {
    use std::sync::{Mutex, Once};

    static LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());
    static INSTALL: Once = Once::new();

    struct Capture;

    impl log::Log for Capture {
        fn enabled(&self, _: &log::Metadata) -> bool {
            true
        }
        fn log(&self, record: &log::Record) {
            let line = record.args().to_string();
            if std::env::var_os("AURAL_BENCH_VERBOSE").is_some() {
                eprintln!("{line}");
            }
            if let Ok(mut v) = LINES.lock() {
                v.push(line);
            }
        }
        fn flush(&self) {}
    }

    pub fn install() {
        INSTALL.call_once(|| {
            if log::set_boxed_logger(Box::new(Capture)).is_ok() {
                log::set_max_level(log::LevelFilter::Trace);
            }
            whisper_rs::install_logging_hooks();
        });
    }

    pub fn take() -> Vec<String> {
        LINES
            .lock()
            .map(|mut v| std::mem::take(&mut *v))
            .unwrap_or_default()
    }
}

pub fn load(model: &Path, backend: Backend, threads: usize) -> Result<Box<dyn Transcriber>> {
    if !compiled_backends().contains(&backend) {
        bail!(
            "backend {backend:?} is not compiled into this build; rebuild with --features {}",
            format!("{backend:?}").to_lowercase()
        );
    }
    capture::install();
    capture::take();
    let mut params = WhisperContextParameters::default();
    params.use_gpu(backend != Backend::Cpu);
    let ctx = WhisperContext::new_with_params(model, params)
        .with_context(|| format!("loading whisper model {}", model.display()))?;
    let backend_used = verify_backend(backend, gpu_device_from_log(&capture::take()).as_deref())?;
    let state = ctx.create_state().context("creating whisper state")?;
    let name = model
        .file_stem()
        .map_or_else(|| "whisper".into(), |n| n.to_string_lossy().into_owned());
    Ok(Box::new(Whisper {
        _ctx: ctx,
        state,
        threads,
        label: name,
        backend_used,
    }))
}

impl Transcriber for Whisper {
    fn label(&self) -> String {
        self.label.clone()
    }

    fn threads(&self) -> Option<usize> {
        Some(self.threads)
    }

    fn backend_used(&self) -> String {
        self.backend_used.clone()
    }

    fn transcribe(&mut self, pcm16k: &[f32]) -> Result<String> {
        let mut p = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        p.set_language(Some("en"));
        p.set_n_threads(self.threads as i32);
        p.set_no_timestamps(true);
        p.set_no_context(true);
        p.set_print_progress(false);
        p.set_print_realtime(false);
        p.set_print_special(false);
        p.set_suppress_blank(true);
        self.state.full(p, pcm16k).context("whisper full()")?;
        let mut text = String::new();
        for seg in self.state.as_iter() {
            text.push_str(&seg.to_str_lossy().context("segment text")?);
        }
        Ok(text.trim().to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::missed_words;

    #[test]
    fn backend_not_compiled_in_is_rejected_before_loading() {
        let missing = if cfg!(feature = "cuda") {
            Backend::Vulkan
        } else {
            Backend::Cuda
        };
        if compiled_backends().contains(&missing) {
            return; // built with every backend; nothing to reject
        }
        let err = load(std::path::Path::new("does-not-exist.bin"), missing, 4)
            .err()
            .unwrap();
        assert!(err.to_string().contains("not compiled"), "{err}");
    }

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn gpu_device_read_from_whisper_init_log() {
        let log = lines(&[
            "whisper_backend_init_gpu: found GPU device 0: Vulkan0 (type: 1, cnt: 0)",
            "whisper_backend_init_gpu: using Vulkan0 backend",
        ]);
        assert_eq!(gpu_device_from_log(&log).as_deref(), Some("Vulkan0"));
    }

    #[test]
    fn no_gpu_or_failed_init_means_cpu() {
        assert_eq!(
            gpu_device_from_log(&lines(&["whisper_backend_init_gpu: no GPU found"])),
            None
        );
        let failed = lines(&[
            "whisper_backend_init_gpu: using CUDA0 backend",
            "whisper_backend_init_gpu: failed to initialize CUDA0 backend",
        ]);
        assert_eq!(gpu_device_from_log(&failed), None);
        // ACCEL backends (e.g. BLAS) are logged by whisper_backend_init, not the GPU init.
        let accel = lines(&["whisper_backend_init: using BLAS backend"]);
        assert_eq!(gpu_device_from_log(&accel), None);
    }

    #[test]
    fn requested_gpu_that_fell_back_to_cpu_is_an_error() {
        let err = verify_backend(Backend::Vulkan, None).unwrap_err();
        assert!(err.to_string().contains("fell back to CPU"), "{err}");
        assert!(verify_backend(Backend::Cuda, Some("Vulkan0")).is_err());
        assert_eq!(
            verify_backend(Backend::Cuda, Some("CUDA0")).unwrap(),
            "cuda:CUDA0"
        );
        assert_eq!(
            verify_backend(Backend::Vulkan, Some("Vulkan0")).unwrap(),
            "vulkan:Vulkan0"
        );
        assert_eq!(verify_backend(Backend::Cpu, None).unwrap(), "cpu");
    }

    #[test]
    fn cpu_is_always_compiled_in() {
        assert!(compiled_backends().contains(&Backend::Cpu));
    }

    /// Regression guard: whisper-rs-sys on MSVC builds whisper.cpp without /O2 unless
    /// `.cargo/config.toml` sets `CL`, which made tiny.en ~6x slower (RTF 0.47 vs 0.07).
    /// Needs the same env vars as `transcribes_real_speech`; run with
    /// `cargo test --release --features ggml -- --ignored`.
    #[test]
    #[ignore]
    fn whisper_cpp_is_built_optimized() {
        let model = std::env::var("AURAL_TEST_WHISPER_MODEL").expect("AURAL_TEST_WHISPER_MODEL");
        let wav = std::env::var("AURAL_TEST_WAV").expect("AURAL_TEST_WAV");
        let mut t = load(std::path::Path::new(&model), Backend::Cpu, 4).unwrap();
        let pcm = aural_audio::dsp::load_wav_16k_mono(std::path::Path::new(&wav)).unwrap();
        t.transcribe(&pcm).unwrap(); // warm up
        let start = std::time::Instant::now();
        t.transcribe(&pcm).unwrap();
        let rtf = start.elapsed().as_secs_f64() / (pcm.len() as f64 / 16_000.0);
        assert!(
            rtf < 0.25,
            "tiny.en RTF {rtf:.3}: whisper.cpp likely built without optimization"
        );
    }

    /// Needs AURAL_TEST_WHISPER_MODEL (ggml .bin), AURAL_TEST_WAV, AURAL_TEST_REF.
    /// Run with `--features ggml -- --ignored`.
    #[test]
    #[ignore]
    fn transcribes_real_speech() {
        let model = std::env::var("AURAL_TEST_WHISPER_MODEL").expect("AURAL_TEST_WHISPER_MODEL");
        let wav = std::env::var("AURAL_TEST_WAV").expect("AURAL_TEST_WAV");
        let reference = std::env::var("AURAL_TEST_REF").expect("AURAL_TEST_REF");
        let mut t = load(std::path::Path::new(&model), Backend::Cpu, 4).unwrap();
        let pcm = aural_audio::dsp::load_wav_16k_mono(std::path::Path::new(&wav)).unwrap();
        let text = t.transcribe(&pcm).unwrap();
        let wer = missed_words(&reference, &text);
        assert!(wer < 0.3, "wer {wer}: {text}");
    }
}
