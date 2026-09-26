//! whisper.cpp (ggml) via `whisper-rs`. GPU backends are compile-time: build with
//! `--features vulkan` or `--features cuda` (both imply `ggml`).

use crate::cli::Backend;
use crate::runner::Transcriber;
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
}

pub fn load(model: &Path, backend: Backend, threads: usize) -> Result<Box<dyn Transcriber>> {
    if !compiled_backends().contains(&backend) {
        bail!(
            "backend {backend:?} is not compiled into this build; rebuild with --features {}",
            format!("{backend:?}").to_lowercase()
        );
    }
    whisper_rs::install_logging_hooks();
    let mut params = WhisperContextParameters::default();
    params.use_gpu(backend != Backend::Cpu);
    let ctx = WhisperContext::new_with_params(model, params)
        .with_context(|| format!("loading whisper model {}", model.display()))?;
    let state = ctx.create_state().context("creating whisper state")?;
    let name = model
        .file_stem()
        .map_or_else(|| "whisper".into(), |n| n.to_string_lossy().into_owned());
    Ok(Box::new(Whisper {
        _ctx: ctx,
        state,
        threads,
        label: name,
    }))
}

impl Transcriber for Whisper {
    fn label(&self) -> String {
        self.label.clone()
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
    use crate::wer::word_errors;

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
        let pcm = crate::audio::load_wav_16k_mono(std::path::Path::new(&wav)).unwrap();
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
        let pcm = crate::audio::load_wav_16k_mono(std::path::Path::new(&wav)).unwrap();
        let text = t.transcribe(&pcm).unwrap();
        let wer = word_errors(&reference, &text).wer();
        assert!(wer < 0.3, "wer {wer}: {text}");
    }
}
