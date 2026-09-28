//! Moonshine (Useful Sensors) via `transcribe-rs`, English models. Expected folder:
//! `encoder_model[.int8].onnx`, `decoder_model_merged[.int8].onnx`, `tokenizer.json`,
//! `config.json` (tiny/base), or `streaming_config.json` plus the streaming model's
//! files for the streaming variant.
//!
//! The streaming model also streams: audio is encoded as it arrives and the decoder
//! reads what has been encoded so far (live text, see Stream).

use crate::onnx_accel;
use crate::{Backend, Stream, Transcriber};
use anyhow::{bail, Context, Result};
use std::path::Path;
use transcribe_rs::onnx::moonshine::{
    LiveStream, MoonshineModel, MoonshineParams, MoonshineStreamingParams, MoonshineVariant,
    StreamingModel,
};
use transcribe_rs::onnx::Quantization;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Tiny,
    Base,
    Streaming,
}

/// Which Moonshine model a folder holds, from its config files. Unknown layouts are an
/// error rather than a guess, because the wrong variant decodes garbage.
pub fn kind(dir: &Path) -> Result<Kind> {
    if dir.join("streaming_config.json").exists() {
        return Ok(Kind::Streaming);
    }
    let path = dir.join("config.json");
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let config: serde_json::Value = serde_json::from_str(&text).context("parsing config.json")?;
    let layers = config
        .get("decoder_num_hidden_layers")
        .or_else(|| config.get("num_hidden_layers"))
        .and_then(serde_json::Value::as_u64);
    match layers {
        Some(6) => Ok(Kind::Tiny),
        Some(8) => Ok(Kind::Base),
        other => bail!("not a known Moonshine model (decoder layers: {other:?})"),
    }
}

fn quantization(dir: &Path) -> Quantization {
    if dir.join("encoder_model.int8.onnx").exists() || dir.join("encoder.int8.onnx").exists() {
        Quantization::Int8
    } else {
        Quantization::FP32
    }
}

enum Model {
    Whole(Box<MoonshineModel>),
    Streaming(Box<StreamingModel>),
}

pub struct Moonshine {
    model: Model,
    live: Option<LiveStream>,
    label: String,
    backend_used: String,
}

pub fn load(dir: &Path, backend: Backend, threads: usize) -> Result<Box<dyn Transcriber>> {
    if backend != Backend::Cpu {
        // On DirectML Moonshine loads and uses the graphics card but produces garbage
        // text (int8 and fp32, RTX 4070, ONNX Runtime 1.24.2): never offer it there.
        bail!("Moonshine runs only on the processor (asked for {backend:?})");
    }
    onnx_accel::select(backend)?;
    crate::log_capture::install();
    crate::log_capture::take();
    let kind = kind(dir)?;
    let quant = quantization(dir);
    let model = match kind {
        Kind::Streaming => Model::Streaming(Box::new(
            StreamingModel::load(dir, threads.max(1), &quant)
                .map_err(|e| anyhow::anyhow!("{e}"))
                .with_context(|| format!("loading Moonshine streaming from {}", dir.display()))?,
        )),
        Kind::Tiny | Kind::Base => {
            let variant = if kind == Kind::Tiny {
                MoonshineVariant::Tiny
            } else {
                MoonshineVariant::Base
            };
            Model::Whole(Box::new(
                MoonshineModel::load(dir, variant, &quant)
                    .map_err(|e| anyhow::anyhow!("{e}"))
                    .with_context(|| format!("loading Moonshine from {}", dir.display()))?,
            ))
        }
    };
    let backend_used = onnx_accel::verify(
        backend,
        &crate::log_capture::take(),
        &aural_platform::gpu::process_gpu_memory_by_adapter(),
        crate::onnx_accel::card_luid(),
    )?;
    Ok(Box::new(Moonshine {
        model,
        live: None,
        label: format!("moonshine-{kind:?} ({quant:?})").to_lowercase(),
        backend_used,
    }))
}

impl Transcriber for Moonshine {
    fn label(&self) -> String {
        self.label.clone()
    }

    fn transcribe(&mut self, pcm16k: &[f32]) -> Result<String> {
        let text = match &mut self.model {
            Model::Whole(m) => m.transcribe_with(pcm16k, &MoonshineParams::default()),
            Model::Streaming(m) => m.transcribe_with(pcm16k, &MoonshineStreamingParams::default()),
        }
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .text;
        Ok(text.trim().to_owned())
    }

    fn backend_used(&self) -> String {
        self.backend_used.clone()
    }

    fn stream(&mut self) -> Option<&mut dyn Stream> {
        match self.model {
            Model::Streaming(_) => Some(self),
            Model::Whole(_) => None,
        }
    }
}

impl Stream for Moonshine {
    fn begin(&mut self) -> Result<()> {
        let Model::Streaming(m) = &self.model else {
            bail!("this Moonshine model cannot stream");
        };
        self.live = Some(m.start_stream());
        Ok(())
    }

    fn push(&mut self, pcm16k: &[f32]) -> Result<()> {
        let (Model::Streaming(m), Some(s)) = (&mut self.model, self.live.as_mut()) else {
            bail!("no live stream is open");
        };
        m.push_audio(s, pcm16k).map_err(|e| anyhow::anyhow!("{e}"))
    }

    fn partial(&mut self) -> Result<String> {
        let (Model::Streaming(m), Some(s)) = (&mut self.model, self.live.as_mut()) else {
            bail!("no live stream is open");
        };
        m.partial_text(s).map_err(|e| anyhow::anyhow!("{e}"))
    }

    fn finish(&mut self) -> Result<String> {
        let (Model::Streaming(m), Some(s)) = (&mut self.model, self.live.take()) else {
            bail!("no live stream is open");
        };
        Ok(m.finish_stream(s)
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .trim()
            .to_owned())
    }

    fn cancel(&mut self) {
        self.live = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir_with(files: &[(&str, &str)]) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        for (name, body) in files {
            std::fs::write(d.path().join(name), body).unwrap();
        }
        d
    }

    #[test]
    fn size_is_read_from_the_decoder_layer_count() {
        let tiny = dir_with(&[("config.json", r#"{"decoder_num_hidden_layers": 6}"#)]);
        assert_eq!(kind(tiny.path()).unwrap(), Kind::Tiny);
        let base = dir_with(&[("config.json", r#"{"decoder_num_hidden_layers": 8}"#)]);
        assert_eq!(kind(base.path()).unwrap(), Kind::Base);
    }

    #[test]
    fn a_streaming_config_means_the_streaming_model() {
        let s = dir_with(&[("streaming_config.json", "{}"), ("config.json", "{}")]);
        assert_eq!(kind(s.path()).unwrap(), Kind::Streaming);
    }

    #[test]
    fn an_unknown_layout_is_an_error_not_a_guess() {
        let none = dir_with(&[]);
        assert!(kind(none.path()).is_err());
        let odd = dir_with(&[("config.json", r#"{"decoder_num_hidden_layers": 12}"#)]);
        assert!(kind(odd.path()).is_err());
    }

    #[test]
    fn moonshine_runs_only_on_the_processor() {
        // On DirectML Moonshine loads and uses the graphics card but produces garbage
        // text (seen with int8 and fp32 on an RTX 4070), so it is refused outright.
        for backend in [Backend::DirectMl, Backend::Vulkan, Backend::Cuda] {
            let err = load(std::path::Path::new("does-not-exist"), backend, 4)
                .err()
                .unwrap();
            assert!(err.to_string().contains("processor"), "{backend:?}: {err}");
        }
    }

    /// Real Moonshine Streaming model (AURAL_TEST_MOONSHINE_STREAM_DIR) fed in 320 ms
    /// pieces like the microphone: text must appear before the end and the final text
    /// must match the recording. Run with `--features onnx -- --ignored`.
    #[test]
    #[ignore]
    fn moonshine_streaming_shows_words_while_audio_arrives() {
        let _ort = crate::test_support::ort_lock();
        use crate::live::{LiveMode, LiveSession};
        use crate::test_support::missed_words;
        let dir = std::env::var("AURAL_TEST_MOONSHINE_STREAM_DIR").expect("dir");
        let wav = std::env::var("AURAL_TEST_WAV").expect("AURAL_TEST_WAV");
        let reference = std::env::var("AURAL_TEST_REF").expect("AURAL_TEST_REF");
        let pcm = aural_audio::dsp::load_wav_16k_mono(std::path::Path::new(&wav)).unwrap();
        let mut t = load(std::path::Path::new(&dir), Backend::Cpu, 4).unwrap();
        let mut s = LiveSession::begin(t.as_mut()).unwrap();
        assert_eq!(s.mode(), LiveMode::Native);
        let mut shown_before_end = String::new();
        let mut slowest = 0u128;
        for chunk in pcm.chunks(5_120) {
            let t0 = std::time::Instant::now();
            let text = s.push(t.as_mut(), chunk).unwrap();
            slowest = slowest.max(t0.elapsed().as_millis());
            shown_before_end = format!("{} {}", text.stable, text.tentative);
            eprintln!(
                "{:>5} ms | {} [{}]",
                t0.elapsed().as_millis(),
                text.stable,
                text.tentative
            );
        }
        let t0 = std::time::Instant::now();
        let text = s.finish(t.as_mut()).unwrap();
        eprintln!(
            "final in {} ms (slowest push {slowest} ms): {text}",
            t0.elapsed().as_millis()
        );
        assert!(
            shown_before_end.split_whitespace().count() >= 3,
            "{shown_before_end}"
        );
        assert!(missed_words(&reference, &text) < 0.3, "{text}");
    }

    /// A long dictation (about 90 s of LibriSpeech, AURAL_TEST_LIBRISPEECH_DIR) through
    /// Moonshine Streaming: the decoder's token limit (about 70 s of speech per stream)
    /// must not cut off the end, and memory must stay flat. Run with
    /// `--features onnx -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn a_long_streamed_dictation_keeps_its_last_sentence() {
        let _ort = crate::test_support::ort_lock();
        use crate::live::LiveSession;
        use crate::test_support::missed_words;
        let dir = std::env::var("AURAL_TEST_MOONSHINE_STREAM_DIR").expect("dir");
        let corpus = std::path::PathBuf::from(
            std::env::var("AURAL_TEST_LIBRISPEECH_DIR").expect("AURAL_TEST_LIBRISPEECH_DIR"),
        );
        let tsv = std::fs::read_to_string(corpus.join("corpus.tsv")).unwrap();
        let mut pcm = Vec::new();
        let mut last_ref = String::new();
        for line in tsv.lines().filter(|l| !l.starts_with('#')) {
            let cols: Vec<&str> = line.split('\t').collect();
            pcm.extend(aural_audio::dsp::load_wav_16k_mono(&corpus.join(cols[1])).unwrap());
            last_ref = cols[2].to_owned();
            if pcm.len() >= 90 * 16_000 {
                break;
            }
        }
        let mut t = load(std::path::Path::new(&dir), Backend::Cpu, 4).unwrap();
        let mut s = LiveSession::begin(t.as_mut()).unwrap();
        let mut slowest = 0u128;
        for chunk in pcm.chunks(5_120) {
            let t0 = std::time::Instant::now();
            s.push(t.as_mut(), chunk).unwrap();
            slowest = slowest.max(t0.elapsed().as_millis());
        }
        let t0 = std::time::Instant::now();
        let text = s.finish(t.as_mut()).unwrap();
        let tail: String = text
            .split_whitespace()
            .rev()
            .take(40)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join(" ");
        eprintln!(
            "{:.0} s streamed; slowest update {slowest} ms; final {} ms; {} words; ends: …{tail}",
            pcm.len() as f64 / 16_000.0,
            t0.elapsed().as_millis(),
            text.split_whitespace().count()
        );
        assert!(
            missed_words(&last_ref, &text) < 0.3,
            "last sentence missing: …{tail}"
        );
        assert!(
            slowest < 3_000,
            "updates must stay bounded, slowest {slowest} ms"
        );
    }

    /// Real Moonshine model (AURAL_TEST_MOONSHINE_DIR) on real speech, on the processor
    /// Run with `--features onnx -- --ignored`.
    #[test]
    #[ignore]
    fn moonshine_transcribes_real_speech() {
        let _ort = crate::test_support::ort_lock();
        use crate::test_support::missed_words;
        let dir = std::env::var("AURAL_TEST_MOONSHINE_DIR").expect("AURAL_TEST_MOONSHINE_DIR");
        let wav = std::env::var("AURAL_TEST_WAV").expect("AURAL_TEST_WAV");
        let reference = std::env::var("AURAL_TEST_REF").expect("AURAL_TEST_REF");
        let pcm = aural_audio::dsp::load_wav_16k_mono(std::path::Path::new(&wav)).unwrap();
        // Processor only: it produces garbage on DirectML (see load).
        let t0 = std::time::Instant::now();
        let mut t = load(std::path::Path::new(&dir), Backend::Cpu, 4).unwrap();
        let load_ms = t0.elapsed().as_millis();
        let t1 = std::time::Instant::now();
        let text = t.transcribe(&pcm).unwrap();
        eprintln!(
            "{}: load {load_ms} ms, transcribe {} ms: {text}",
            t.backend_used(),
            t1.elapsed().as_millis()
        );
        assert!(missed_words(&reference, &text) < 0.3, "{text}");
    }
}
