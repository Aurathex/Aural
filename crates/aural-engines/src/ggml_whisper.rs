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

/// A graphics device ggml can run whisper.cpp on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuKind {
    Separate,
    Integrated,
}

/// whisper.cpp's `gpu_device` counts graphics devices (separate and built-in) in ggml's
/// order and uses 0 by default, which on hybrid laptops can be the built-in GPU. Prefer
/// the first separate card.
pub fn preferred_gpu_device(kinds: &[GpuKind]) -> i32 {
    kinds
        .iter()
        .position(|k| *k == GpuKind::Separate)
        .unwrap_or(0) as i32
}

/// The graphics devices ggml offers, in the order whisper.cpp counts them, with names.
fn gpu_devices() -> Vec<(GpuKind, String)> {
    use whisper_rs::whisper_rs_sys as sys;
    let mut out = Vec::new();
    // SAFETY: ggml's device registry is initialized on first use; the pointers it returns
    // live for the process, and descriptions are NUL-terminated C strings.
    unsafe {
        for i in 0..sys::ggml_backend_dev_count() {
            let dev = sys::ggml_backend_dev_get(i);
            let kind = match sys::ggml_backend_dev_type(dev) {
                sys::ggml_backend_dev_type_GGML_BACKEND_DEVICE_TYPE_GPU => GpuKind::Separate,
                sys::ggml_backend_dev_type_GGML_BACKEND_DEVICE_TYPE_IGPU => GpuKind::Integrated,
                _ => continue,
            };
            let desc = sys::ggml_backend_dev_description(dev);
            let name = if desc.is_null() {
                String::new()
            } else {
                std::ffi::CStr::from_ptr(desc)
                    .to_string_lossy()
                    .into_owned()
            };
            out.push((kind, name));
        }
    }
    out
}

/// Refuse to report a GPU benchmark that silently ran on the CPU.
pub fn verify_backend(requested: Backend, gpu_device: Option<&str>) -> Result<String> {
    let expected_prefix = match requested {
        Backend::Cpu => return Ok("cpu".into()),
        Backend::Vulkan => "Vulkan",
        Backend::Cuda => "CUDA",
        Backend::DirectMl => {
            bail!("whisper.cpp can't run on DirectML; use the processor or Vulkan")
        }
    };
    match gpu_device {
        None => bail!("requested {requested:?} but whisper.cpp fell back to CPU"),
        Some(dev) if dev.starts_with(expected_prefix) => {
            Ok(format!("{}:{dev}", expected_prefix.to_lowercase()))
        }
        Some(dev) => bail!("requested {requested:?} but whisper.cpp initialized {dev}"),
    }
}

/// whisper.cpp's own log goes through whisper-rs's `log_backend` into the shared capture.
mod capture {
    use std::sync::Once;

    static HOOKS: Once = Once::new();

    pub fn install() {
        crate::log_capture::install();
        HOOKS.call_once(whisper_rs::install_logging_hooks);
    }

    pub use crate::log_capture::take;
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
    let devices = if backend == Backend::Cpu {
        Vec::new()
    } else {
        gpu_devices()
    };
    let chosen = preferred_gpu_device(&devices.iter().map(|d| d.0).collect::<Vec<_>>());
    params.gpu_device(chosen);
    let ctx = WhisperContext::new_with_params(model, params)
        .with_context(|| format!("loading whisper model {}", model.display()))?;
    // whisper.cpp initializes the GPU backend (and logs which one) when the state is
    // created, not when the model loads, so check only after create_state.
    let state = ctx.create_state().context("creating whisper state")?;
    let mut backend_used =
        verify_backend(backend, gpu_device_from_log(&capture::take()).as_deref())?;
    // Say which card: "vulkan:Vulkan0 (NVIDIA GeForce RTX 4070 Laptop GPU)".
    if let Some((_, name)) = devices.get(chosen as usize).filter(|d| !d.1.is_empty()) {
        backend_used = format!("{backend_used} ({name})");
    }
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
    fn the_separate_graphics_card_is_preferred() {
        use GpuKind::*;
        // Hybrid laptop where the built-in GPU is enumerated first.
        assert_eq!(preferred_gpu_device(&[Integrated, Separate]), 1);
        assert_eq!(preferred_gpu_device(&[Separate, Integrated]), 0);
        // Only built-in graphics: use it (Aural only offers Vulkan with a separate card,
        // but whisper.cpp's own default is kept).
        assert_eq!(preferred_gpu_device(&[Integrated]), 0);
        assert_eq!(preferred_gpu_device(&[]), 0);
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
        assert!(
            verify_backend(Backend::DirectMl, Some("Vulkan0")).is_err(),
            "whisper.cpp has no DirectML backend"
        );
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

    /// A GPU build must report the GPU it initialized, not "fell back to CPU".
    /// Set AURAL_TEST_GPU_BACKEND=vulkan|cuda and build with that feature:
    /// `cargo test --release --features ggml,vulkan -- --ignored gpu_backend`.
    #[cfg(any(feature = "vulkan", feature = "cuda"))]
    #[test]
    #[ignore]
    fn gpu_backend_is_detected() {
        let model = std::env::var("AURAL_TEST_WHISPER_MODEL").expect("AURAL_TEST_WHISPER_MODEL");
        let backend = match std::env::var("AURAL_TEST_GPU_BACKEND").as_deref() {
            Ok("vulkan") => Backend::Vulkan,
            Ok("cuda") => Backend::Cuda,
            other => panic!("set AURAL_TEST_GPU_BACKEND to vulkan or cuda, got {other:?}"),
        };
        let t = load(std::path::Path::new(&model), backend, 4).unwrap();
        let used = t.backend_used();
        let expected = format!("{backend:?}").to_lowercase();
        assert!(used.starts_with(&expected), "backend_used = {used}");
    }
}
