//! NVIDIA Parakeet TDT (ONNX export in the onnx-asr layout) via `transcribe-rs`.
//!
//! Expected model directory: `encoder-model[.int8].onnx`, `decoder_joint-model[.int8].onnx`,
//! `nemo128.onnx`, `vocab.txt`.
//!
//! transcribe-rs 0.3 sizes the ONNX Runtime thread pool itself, so `--threads` is not
//! applied to this engine.

use crate::Backend;
use crate::Transcriber;
use anyhow::{Context, Result};
use std::path::Path;
use transcribe_rs::onnx::parakeet::{ParakeetModel, ParakeetParams};
use transcribe_rs::onnx::Quantization;

pub struct Parakeet {
    model: ParakeetModel,
    label: String,
    backend_used: String,
}

pub fn detect_quantization(model_dir: &Path) -> Quantization {
    if model_dir.join("encoder-model.int8.onnx").exists() {
        Quantization::Int8
    } else {
        Quantization::FP32
    }
}

pub fn load(model_dir: &Path, backend: Backend, _threads: usize) -> Result<Box<dyn Transcriber>> {
    crate::onnx_accel::select(backend)?;
    crate::log_capture::install();
    crate::log_capture::take();
    let quant = detect_quantization(model_dir);
    let model = ParakeetModel::load(model_dir, &quant)
        .map_err(|e| anyhow::anyhow!("{e}"))
        .with_context(|| format!("loading parakeet from {}", model_dir.display()))?;
    let backend_used = crate::onnx_accel::verify(
        backend,
        &crate::log_capture::take(),
        &aural_platform::gpu::process_gpu_memory_by_adapter(),
        crate::onnx_accel::card_luid(),
    )?;
    let name = model_dir
        .file_name()
        .map_or_else(|| "parakeet".into(), |n| n.to_string_lossy().into_owned());
    Ok(Box::new(Parakeet {
        model,
        label: format!("{name} ({quant:?})"),
        backend_used,
    }))
}

impl Transcriber for Parakeet {
    fn label(&self) -> String {
        self.label.clone()
    }

    fn transcribe(&mut self, pcm16k: &[f32]) -> Result<String> {
        let result = self
            .model
            .transcribe_with(pcm16k, &ParakeetParams::default())
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(result.text.trim().to_owned())
    }

    fn backend_used(&self) -> String {
        self.backend_used.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::missed_words;

    #[test]
    fn unsupported_gpu_backends_rejected_before_touching_the_model() {
        let err = load(std::path::Path::new("does-not-exist"), Backend::Cuda, 4)
            .err()
            .unwrap();
        assert!(err.to_string().contains("Cuda"), "{err}");
    }

    #[test]
    fn quantization_follows_encoder_file_present() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            detect_quantization(dir.path()),
            Quantization::FP32
        ));
        std::fs::write(dir.path().join("encoder-model.int8.onnx"), b"").unwrap();
        assert!(matches!(
            detect_quantization(dir.path()),
            Quantization::Int8
        ));
    }

    /// Smoke test against a real model. Needs AURAL_TEST_PARAKEET_DIR, AURAL_TEST_WAV and
    /// AURAL_TEST_REF (reference transcript). Run with `--features onnx -- --ignored`.
    #[test]
    #[ignore]
    fn transcribes_real_speech() {
        let _ort = crate::test_support::ort_lock();
        let dir = std::env::var("AURAL_TEST_PARAKEET_DIR").expect("AURAL_TEST_PARAKEET_DIR");
        let wav = std::env::var("AURAL_TEST_WAV").expect("AURAL_TEST_WAV");
        let reference = std::env::var("AURAL_TEST_REF").expect("AURAL_TEST_REF");
        let mut t = load(std::path::Path::new(&dir), Backend::Cpu, 4).unwrap();
        let pcm = aural_audio::dsp::load_wav_16k_mono(std::path::Path::new(&wav)).unwrap();
        let text = t.transcribe(&pcm).unwrap();
        let wer = missed_words(&reference, &text);
        assert!(!text.trim().is_empty());
        assert!(wer < 0.3, "wer {wer}: {text}");
    }

    /// Same as above on the graphics card via DirectML: it must really run there
    /// (verified), and be as accurate. Run with `--features onnx -- --ignored`.
    #[test]
    #[ignore]
    fn parakeet_runs_on_directml() {
        let _ort = crate::test_support::ort_lock();
        let dir = std::env::var("AURAL_TEST_PARAKEET_DIR").expect("AURAL_TEST_PARAKEET_DIR");
        let wav = std::env::var("AURAL_TEST_WAV").expect("AURAL_TEST_WAV");
        let reference = std::env::var("AURAL_TEST_REF").expect("AURAL_TEST_REF");
        let t0 = std::time::Instant::now();
        let mut t = load(std::path::Path::new(&dir), Backend::DirectMl, 4).unwrap();
        let load_ms = t0.elapsed().as_millis();
        assert!(
            t.backend_used().starts_with("directml:"),
            "{}",
            t.backend_used()
        );
        let pcm = aural_audio::dsp::load_wav_16k_mono(std::path::Path::new(&wav)).unwrap();
        let t1 = std::time::Instant::now();
        let text = t.transcribe(&pcm).unwrap();
        eprintln!(
            "directml: load {load_ms} ms, transcribe {} ms: {text}",
            t1.elapsed().as_millis()
        );
        assert!(missed_words(&reference, &text) < 0.3, "{text}");
    }
}
