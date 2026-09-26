//! Command-line surface of `aural-bench` and the engine registry behind `--engine`.

use clap::Parser;
use std::path::PathBuf;

pub use aural_engines::{build_engine, Backend, Engine};

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
}
