//! Command-line surface of `aural-bench` and the engine registry behind `--engine`.

use crate::runner::Transcriber;
use anyhow::{bail, Result};
use clap::{Parser, ValueEnum};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Engine {
    /// NVIDIA Parakeet TDT via ONNX Runtime (build with --features onnx)
    Parakeet,
    /// whisper.cpp / ggml (build with --features ggml)
    Whisper,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Backend {
    Cpu,
    Vulkan,
    Cuda,
}

#[derive(Debug, Parser)]
#[command(
    name = "aural-bench",
    about = "Benchmark a local STT engine on a WAV corpus"
)]
pub struct Cli {
    #[arg(long, value_enum)]
    pub engine: Engine,
    /// Model file (whisper .bin) or model directory (parakeet ONNX)
    #[arg(long)]
    pub model: PathBuf,
    #[arg(long, value_enum, default_value = "cpu")]
    pub backend: Backend,
    /// corpus.tsv: <id>\t<wav>\t<reference>
    #[arg(long)]
    pub corpus: PathBuf,
    /// JSON results file to write
    #[arg(long)]
    pub out: PathBuf,
    /// Untimed runs on the first clip before measuring
    #[arg(long, default_value_t = 1)]
    pub warmup: usize,
    /// CPU threads for the engine (default: physical cores - 1, max 8)
    #[arg(long)]
    pub threads: Option<usize>,
}

/// Construct the requested engine. Engines are behind mutually exclusive cargo
/// features because ONNX Runtime and whisper.cpp must not link into one binary.
#[allow(unused_variables)]
pub fn build_engine(
    engine: Engine,
    model: &Path,
    backend: Backend,
    threads: usize,
) -> Result<Box<dyn Transcriber>> {
    match engine {
        Engine::Parakeet => {
            #[cfg(feature = "onnx")]
            return crate::engines::onnx_parakeet::load(model, backend, threads);
            #[cfg(not(feature = "onnx"))]
            bail!("engine 'parakeet' is not compiled in; rebuild with --features onnx")
        }
        Engine::Whisper => {
            #[cfg(feature = "ggml")]
            return crate::engines::ggml_whisper::load(model, backend, threads);
            #[cfg(not(feature = "ggml"))]
            bail!("engine 'whisper' is not compiled in; rebuild with --features ggml")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parses_full_invocation() {
        let cli = Cli::try_parse_from([
            "aural-bench",
            "--engine",
            "whisper",
            "--model",
            "m.bin",
            "--backend",
            "vulkan",
            "--corpus",
            "c.tsv",
            "--out",
            "r.json",
            "--warmup",
            "2",
        ])
        .unwrap();
        assert_eq!(cli.engine, Engine::Whisper);
        assert_eq!(cli.backend, Backend::Vulkan);
        assert_eq!(cli.warmup, 2);
    }

    #[test]
    fn backend_defaults_to_cpu_and_warmup_to_one() {
        let cli = Cli::try_parse_from([
            "aural-bench",
            "--engine",
            "parakeet",
            "--model",
            "dir",
            "--corpus",
            "c.tsv",
            "--out",
            "r.json",
        ])
        .unwrap();
        assert_eq!(cli.backend, Backend::Cpu);
        assert_eq!(cli.warmup, 1);
    }

    #[test]
    fn unknown_engine_rejected() {
        assert!(Cli::try_parse_from([
            "aural-bench",
            "--engine",
            "vosk",
            "--model",
            "m",
            "--corpus",
            "c",
            "--out",
            "o",
        ])
        .is_err());
    }

    #[cfg(not(any(feature = "onnx", feature = "ggml")))]
    #[test]
    fn engine_not_compiled_in_says_which_feature_to_enable() {
        let err = build_engine(Engine::Parakeet, std::path::Path::new("m"), Backend::Cpu, 4)
            .err()
            .unwrap();
        assert!(err.to_string().contains("--features onnx"), "{err}");
    }
}
