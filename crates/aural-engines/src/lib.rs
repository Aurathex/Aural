//! Local STT engines behind one small trait. Each engine family sits behind a cargo
//! feature, and `onnx` and `ggml` are mutually exclusive: ONNX Runtime and whisper.cpp
//! must not link into one binary (MSVC CRT conflicts), so the app runs each family in
//! its own worker process.

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[cfg(all(feature = "onnx", feature = "ggml"))]
compile_error!("features `onnx` and `ggml` are mutually exclusive (ONNX Runtime and whisper.cpp must not link into one binary); build them separately");

#[cfg(any(feature = "onnx", feature = "ggml"))]
mod log_capture;
#[cfg(feature = "onnx")]
pub mod onnx_accel;
#[cfg(feature = "onnx")]
pub mod onnx_llm;
#[cfg(feature = "onnx")]
pub mod onnx_moonshine;
#[cfg(feature = "onnx")]
pub mod onnx_parakeet;

#[cfg(feature = "ggml")]
pub mod ggml_whisper;

/// Engine family. Serialized names are stable: they appear in the model catalog and
/// the worker protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
pub enum Engine {
    /// NVIDIA Parakeet TDT via ONNX Runtime (feature `onnx`)
    Parakeet,
    /// whisper.cpp / ggml (feature `ggml`)
    Whisper,
    /// Useful Sensors Moonshine via ONNX Runtime (feature `onnx`)
    Moonshine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
pub enum Backend {
    Cpu,
    Vulkan,
    Cuda,
    /// Windows DirectML (any DirectX 12 graphics card: NVIDIA, AMD, Intel), for ONNX models.
    #[serde(rename = "directml")]
    #[cfg_attr(feature = "clap", value(name = "directml"))]
    DirectMl,
}

/// One loaded STT engine. Input is always 16 kHz mono `f32`.
pub trait Transcriber {
    fn label(&self) -> String;
    fn transcribe(&mut self, pcm16k: &[f32]) -> Result<String>;

    /// Thread count actually applied, or `None` when the engine sizes its own pool.
    fn threads(&self) -> Option<usize> {
        None
    }

    /// Backend the engine is really running on (verified, not merely requested).
    fn backend_used(&self) -> String {
        "cpu".into()
    }

    /// True streaming: engines that encode audio as it arrives. Engines without it get
    /// live text from [`live::LiveSession`]'s phrase-by-phrase mode instead.
    fn stream(&mut self) -> Option<&mut dyn Stream> {
        None
    }
}

/// A local language model for AI cleanup (text in, text out).
pub trait TextModel {
    fn label(&self) -> String;
    /// Answer `user` following `system` and the example (input, answer) turns, with at
    /// most `max_tokens` tokens.
    fn generate(
        &mut self,
        system: &str,
        examples: &[(String, String)],
        user: &str,
        max_tokens: usize,
    ) -> Result<String>;
}

/// Load a text model folder (ONNX, feature `onnx`).
#[allow(unused_variables)]
pub fn build_text_model(dir: &std::path::Path, threads: usize) -> Result<Box<dyn TextModel>> {
    #[cfg(feature = "onnx")]
    return onnx_llm::load(dir, threads);
    #[cfg(not(feature = "onnx"))]
    anyhow::bail!("text models are not compiled in; rebuild with --features onnx")
}

/// One live stream at a time, owned by the engine.
pub trait Stream {
    fn begin(&mut self) -> Result<()>;
    /// New 16 kHz mono audio since the last push.
    fn push(&mut self, pcm16k: &[f32]) -> Result<()>;
    /// Text for the audio so far; may still change.
    fn partial(&mut self) -> Result<String>;
    /// Final text; closes the stream.
    fn finish(&mut self) -> Result<String>;
    fn cancel(&mut self);
}

pub mod live;

/// Construct the requested engine, or explain which feature is missing.
#[allow(unused_variables)]
pub fn build_engine(
    engine: Engine,
    model: &std::path::Path,
    backend: Backend,
    threads: usize,
) -> Result<Box<dyn Transcriber>> {
    match engine {
        Engine::Parakeet => {
            #[cfg(feature = "onnx")]
            return onnx_parakeet::load(model, backend, threads);
            #[cfg(not(feature = "onnx"))]
            anyhow::bail!("engine 'parakeet' is not compiled in; rebuild with --features onnx")
        }
        Engine::Moonshine => {
            #[cfg(feature = "onnx")]
            return onnx_moonshine::load(model, backend, threads);
            #[cfg(not(feature = "onnx"))]
            anyhow::bail!("engine 'moonshine' is not compiled in; rebuild with --features onnx")
        }
        Engine::Whisper => {
            #[cfg(feature = "ggml")]
            return ggml_whisper::load(model, backend, threads);
            #[cfg(not(feature = "ggml"))]
            anyhow::bail!("engine 'whisper' is not compiled in; rebuild with --features ggml")
        }
    }
}

/// Coarse accuracy check for real-model smoke tests (the benchmark's WER lives in
/// `aural-bench`): fraction of reference words missing from the hypothesis.
#[cfg(test)]
pub(crate) mod test_support {
    /// ONNX Runtime's device choice is process-wide (transcribe-rs), so tests that load
    /// ONNX models take turns; in the app each worker process loads one model at a time.
    #[cfg(feature = "onnx")]
    pub fn ort_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn words(s: &str) -> Vec<String> {
        s.to_lowercase()
            .split(|c: char| !c.is_alphanumeric() && c != '\'')
            .filter(|w| !w.is_empty())
            .map(str::to_owned)
            .collect()
    }

    pub fn missed_words(reference: &str, hypothesis: &str) -> f64 {
        let r = words(reference);
        let h = words(hypothesis);
        if r.is_empty() {
            return 0.0;
        }
        let missed = r.iter().filter(|w| !h.contains(w)).count();
        missed as f64 / r.len() as f64
    }

    #[test]
    fn missed_words_counts_absent_reference_words() {
        assert_eq!(missed_words("Hello, big world", "hello world"), 1.0 / 3.0);
        assert_eq!(missed_words("", "anything"), 0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_and_backend_serialize_lowercase() {
        assert_eq!(
            serde_json::to_string(&Engine::Parakeet).unwrap(),
            "\"parakeet\""
        );
        assert_eq!(
            serde_json::from_str::<Backend>("\"vulkan\"").unwrap(),
            Backend::Vulkan
        );
    }

    #[cfg(not(feature = "onnx"))]
    #[test]
    fn missing_engine_names_the_feature() {
        let err = build_engine(Engine::Parakeet, std::path::Path::new("m"), Backend::Cpu, 1)
            .err()
            .unwrap();
        assert!(err.to_string().contains("--features onnx"), "{err}");
    }
}
