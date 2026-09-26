//! NVIDIA Parakeet TDT (ONNX export in the onnx-asr layout) via `transcribe-rs`.
//!
//! Expected model directory: `encoder-model[.int8].onnx`, `decoder_joint-model[.int8].onnx`,
//! `nemo128.onnx`, `vocab.txt`.
//!
//! transcribe-rs 0.3 sizes the ONNX Runtime thread pool itself, so `--threads` is not
//! applied to this engine.

use crate::cli::Backend;
use crate::runner::Transcriber;
use anyhow::{bail, Context, Result};
use std::path::Path;
use transcribe_rs::onnx::parakeet::{ParakeetModel, ParakeetParams};
use transcribe_rs::onnx::Quantization;

pub struct Parakeet {
    model: ParakeetModel,
    label: String,
}

pub fn detect_quantization(model_dir: &Path) -> Quantization {
    if model_dir.join("encoder-model.int8.onnx").exists() {
        Quantization::Int8
    } else {
        Quantization::FP32
    }
}

pub fn load(model_dir: &Path, backend: Backend, _threads: usize) -> Result<Box<dyn Transcriber>> {
    if backend != Backend::Cpu {
        bail!("parakeet benchmark currently supports only the cpu backend (got {backend:?})");
    }
    let quant = detect_quantization(model_dir);
    let model = ParakeetModel::load(model_dir, &quant)
        .map_err(|e| anyhow::anyhow!("{e}"))
        .with_context(|| format!("loading parakeet from {}", model_dir.display()))?;
    let name = model_dir
        .file_name()
        .map_or_else(|| "parakeet".into(), |n| n.to_string_lossy().into_owned());
    Ok(Box::new(Parakeet {
        model,
        label: format!("{name} ({quant:?})"),
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wer::word_errors;

    #[test]
    fn gpu_backends_rejected_before_touching_the_model() {
        let err = load(std::path::Path::new("does-not-exist"), Backend::Cuda, 4)
            .err()
            .unwrap();
        assert!(err.to_string().contains("cpu"), "{err}");
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
        let dir = std::env::var("AURAL_TEST_PARAKEET_DIR").expect("AURAL_TEST_PARAKEET_DIR");
        let wav = std::env::var("AURAL_TEST_WAV").expect("AURAL_TEST_WAV");
        let reference = std::env::var("AURAL_TEST_REF").expect("AURAL_TEST_REF");
        let mut t = load(std::path::Path::new(&dir), Backend::Cpu, 4).unwrap();
        let pcm = crate::audio::load_wav_16k_mono(std::path::Path::new(&wav)).unwrap();
        let text = t.transcribe(&pcm).unwrap();
        let wer = word_errors(&reference, &text).wer();
        assert!(!text.trim().is_empty());
        assert!(wer < 0.3, "wer {wer}: {text}");
    }
}
