//! Cohere Transcribe INT4, using the cstr raw-audio encoder/decoder export.
//! English prompt; CPU only until this exact export is verified on DirectML.

use crate::{Backend, Transcriber};
use anyhow::{bail, Context, Result};
use std::path::Path;
use transcribe_rs::onnx::cohere::{CohereModel, CohereParams};
use transcribe_rs::onnx::Quantization;

struct Cohere {
    model: CohereModel,
}

pub fn load(dir: &Path, backend: Backend, _threads: usize) -> Result<Box<dyn Transcriber>> {
    if backend != Backend::Cpu {
        bail!("Cohere Transcribe INT4 currently supports the processor only; this export has not been verified on {backend:?}");
    }
    for name in [
        "cohere-encoder.int4.onnx",
        "cohere-encoder.int4.onnx.data",
        "cohere-decoder.int4.onnx",
        "cohere-decoder.int4.onnx.data",
        "tokens.txt",
    ] {
        if !dir.join(name).is_file() {
            bail!("Cohere Transcribe model is missing {name}");
        }
    }
    crate::onnx_accel::select(backend)?;
    let model = CohereModel::load(dir, &Quantization::Int4)
        .map_err(|e| anyhow::anyhow!("{e}"))
        .with_context(|| format!("loading Cohere Transcribe from {}", dir.display()))?;
    Ok(Box::new(Cohere { model }))
}

impl Transcriber for Cohere {
    fn label(&self) -> String {
        "Cohere Transcribe 03-2026 (INT4, English)".into()
    }
    fn backend_used(&self) -> String {
        "cpu".into()
    }
    fn transcribe(&mut self, pcm16k: &[f32]) -> Result<String> {
        // Keep encoder memory bounded and remain within the decoder token budget.
        // Decode longer recordings as successive 30-second windows.
        let mut parts = Vec::new();
        for chunk in pcm16k.chunks(30 * 16000) {
            let result = self
                .model
                .transcribe_with(chunk, &CohereParams::default())
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            if !result.text.trim().is_empty() {
                parts.push(result.text.trim().to_owned());
            }
        }
        Ok(parts.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unverified_backends_are_rejected_before_model_access() {
        for backend in [Backend::DirectMl, Backend::Vulkan, Backend::Cuda] {
            let err = load(Path::new("missing"), backend, 4).err().unwrap();
            assert!(err.to_string().contains("processor only"), "{err}");
        }
    }
    #[test]
    fn incomplete_download_names_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let err = load(dir.path(), Backend::Cpu, 4).err().unwrap();
        assert!(
            err.to_string().contains("cohere-encoder.int4.onnx"),
            "{err}"
        );
    }
}
